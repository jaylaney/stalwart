/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{config::ZaSearchConfig, error::TransientClass, trace::ZA_SEARCH_EVICTED};
use crate::{Server, za_search_trace};
use ahash::AHashMap;
use std::{
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use store::{
    write::now,
    za_search::{
        Checkpoint, SegmentId,
        content::ContentPart,
        manifest::{ContentHead, Manifest},
        thread::ThreadPart,
    },
};
use tokio::sync::{Mutex as AsyncMutex, OwnedRwLockReadGuard, RwLock};
use types::blob_hash::BlobHash;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Thread,
    Content,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvictReason {
    Idle,
    Budget,
    Invalidation,
}

impl EvictReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Budget => "budget",
            Self::Invalidation => "invalidation",
        }
    }
}

pub struct ZaSearchCache {
    inner: Arc<CacheState>,
}

struct CacheState {
    config: ZaSearchConfig,
    refusal: Option<String>,
    entries: Mutex<AHashMap<u32, Arc<IndexEntry>>>,
    admission: Mutex<()>,
    thread_used: Arc<AtomicU64>,
    content_used: Arc<AtomicU64>,
    #[cfg(feature = "test_mode")]
    test_content_budget: AtomicU64,
}

#[cfg(feature = "test_mode")]
#[must_use = "dropping the guard restores the content budget"]
pub struct SearchContentBudgetGuard {
    cache: Arc<CacheState>,
    previous: u64,
}

#[cfg(feature = "test_mode")]
impl Drop for SearchContentBudgetGuard {
    fn drop(&mut self) {
        self.cache
            .test_content_budget
            .store(self.previous, Ordering::Relaxed);
    }
}

impl ZaSearchCache {
    pub fn new(config: Result<ZaSearchConfig, String>) -> Self {
        let (config, refusal) = match config {
            Ok(config) => (config, None),
            Err(reason) => (ZaSearchConfig::default(), Some(reason)),
        };
        Self {
            inner: Arc::new(CacheState {
                config,
                refusal,
                entries: Mutex::default(),
                admission: Mutex::default(),
                thread_used: Arc::default(),
                content_used: Arc::default(),
                #[cfg(feature = "test_mode")]
                test_content_budget: AtomicU64::default(),
            }),
        }
    }

    pub fn config(&self) -> &ZaSearchConfig {
        &self.inner.config
    }

    pub fn refusal(&self) -> Option<&str> {
        self.inner.refusal.as_deref()
    }

    fn effective_config(&self) -> ZaSearchConfig {
        #[cfg(feature = "test_mode")]
        if let Some(config) = crate::za::test_hooks::search_config_override() {
            return config;
        }

        self.config().clone()
    }

    pub fn entry(&self, account_id: u32) -> Arc<IndexEntry> {
        let mut entries = self.inner.entries.lock().unwrap();
        let entry = entries.entry(account_id).or_insert_with(|| {
            Arc::new(IndexEntry {
                account_id,
                index: Arc::default(),
                pass_lock: AsyncMutex::default(),
                load_flight: AsyncMutex::default(),
                content_load_flight: AsyncMutex::default(),
                last_used: AtomicU64::new(now()),
                absent: Mutex::default(),
                open_retry: Mutex::default(),
                #[cfg(feature = "test_mode")]
                fail_abort_write: std::sync::atomic::AtomicBool::new(false),
                cache: Arc::downgrade(&self.inner),
            })
        });
        entry.touch();
        entry.clone()
    }

    pub fn get(&self, account_id: u32) -> Option<Arc<IndexEntry>> {
        self.inner.entries.lock().unwrap().get(&account_id).cloned()
    }

    /// Peek at the entry table without blocking or touching idle state.
    #[cfg(feature = "test_mode")]
    pub fn try_get_for_test(&self, account_id: u32) -> Option<Option<Arc<IndexEntry>>> {
        Some(
            self.inner
                .entries
                .try_lock()
                .ok()?
                .get(&account_id)
                .cloned(),
        )
    }

    pub fn remove_account(&self, account_id: u32, reason: EvictReason) {
        if self
            .inner
            .entries
            .lock()
            .unwrap()
            .remove(&account_id)
            .is_some()
        {
            trace_eviction(account_id, reason);
        }
    }

    #[cfg(test)]
    pub fn drop_content(&self, account_id: u32, reason: EvictReason) {
        if let Some(entry) = self.inner.entries.lock().unwrap().get(&account_id) {
            entry.try_drop_content(reason);
        }
    }

    pub fn admit(&self, part: PartKind, bytes: u64, keep: u32) -> Option<BudgetCharge> {
        let _admission = self.inner.admission.lock().unwrap();
        if !self.make_room(part, bytes, keep) && part == PartKind::Content {
            return None;
        }
        let counter = self.counter(part).clone();
        counter.fetch_add(bytes, Ordering::Relaxed);
        Some(BudgetCharge { counter, bytes })
    }

    pub fn grow(
        &self,
        charge: &mut BudgetCharge,
        part: PartKind,
        new_bytes: u64,
        keep: u32,
    ) -> bool {
        let _admission = self.inner.admission.lock().unwrap();
        if new_bytes > charge.bytes {
            let delta = new_bytes - charge.bytes;
            if !self.make_room(part, delta, keep) && part == PartKind::Content {
                return false;
            }
            if charge.bytes == 0 {
                charge.counter = self.counter(part).clone();
            }
            debug_assert!(Arc::ptr_eq(&charge.counter, self.counter(part)));
            charge.counter.fetch_add(delta, Ordering::Relaxed);
        } else {
            charge
                .counter
                .fetch_sub(charge.bytes - new_bytes, Ordering::Relaxed);
        }
        charge.bytes = new_bytes;
        true
    }

    #[cfg(test)]
    pub fn admit_content_growth(&self, index: &mut LoadedIndex, account_id: u32) -> bool {
        self.admit_content_growth_with_staged(index, account_id, None)
    }

    #[cfg(test)]
    pub fn admit_content_growth_with_staged(
        &self,
        index: &mut LoadedIndex,
        account_id: u32,
        staged: Option<BudgetCharge>,
    ) -> bool {
        let Some(content) = index.content.as_mut() else {
            return true;
        };
        if let Some(staged) = staged {
            content.charge.absorb(staged);
        }
        let bytes = content.part.loaded_bytes() as u64;
        if self.grow(&mut content.charge, PartKind::Content, bytes, account_id) {
            content.bytes = bytes;
            true
        } else {
            index.content = None;
            trace_eviction(account_id, EvictReason::Budget);
            false
        }
    }

    pub fn sweep_idle(&self) {
        let current = now();
        let expired = |entry: &IndexEntry| {
            current.saturating_sub(entry.last_used.load(Ordering::Relaxed))
                > self.config().idle_secs
        };
        let mut entries = self.inner.entries.lock().unwrap();
        let candidates = entries
            .values()
            .filter(|entry| expired(entry))
            .map(|entry| entry.account_id)
            .collect::<Vec<_>>();
        for account_id in candidates {
            let entry = entries[&account_id].clone();
            entry.try_eviction(|_| {
                if expired(&entry) {
                    entries.remove(&account_id);
                    trace_eviction(account_id, EvictReason::Idle);
                }
            });
        }
    }

    pub fn thread_used(&self) -> u64 {
        self.inner.thread_used.load(Ordering::Relaxed)
    }

    pub fn content_used(&self) -> u64 {
        self.inner.content_used.load(Ordering::Relaxed)
    }

    fn counter(&self, part: PartKind) -> &Arc<AtomicU64> {
        match part {
            PartKind::Thread => &self.inner.thread_used,
            PartKind::Content => &self.inner.content_used,
        }
    }

    pub fn content_budget(&self) -> u64 {
        #[cfg(feature = "test_mode")]
        {
            let bytes = self.inner.test_content_budget.load(Ordering::Relaxed);
            if bytes != 0 {
                return bytes;
            }
        }
        self.config().mem_mb.saturating_mul(1 << 20)
    }

    #[cfg(feature = "test_mode")]
    pub fn override_content_budget_for_test(&self, bytes: u64) -> SearchContentBudgetGuard {
        assert!(bytes != 0);
        SearchContentBudgetGuard {
            cache: self.inner.clone(),
            previous: self
                .inner
                .test_content_budget
                .swap(bytes, Ordering::Relaxed),
        }
    }

    pub(super) fn budget(&self, part: PartKind) -> u64 {
        match part {
            PartKind::Thread => self.config().thread_mem_mb.saturating_mul(1 << 20),
            PartKind::Content => self.content_budget(),
        }
    }

    // Called only under admission. Dropping a charge can decrease the counters at
    // any time. The map and individual index locks are never waited on together.
    fn make_room(&self, part: PartKind, bytes: u64, keep: u32) -> bool {
        let budget = self.budget(part);
        let fits = || {
            self.counter(part)
                .load(Ordering::Relaxed)
                .checked_add(bytes)
                .is_some_and(|total| total <= budget)
        };
        if fits() {
            return true;
        }
        // Even evicting everything cannot satisfy an oversized content request.
        if part == PartKind::Content && bytes > budget {
            return false;
        }

        let mut entries = self.inner.entries.lock().unwrap();
        let mut candidates = entries
            .values()
            .filter(|entry| entry.account_id != keep)
            .map(|entry| (entry.last_used.load(Ordering::Relaxed), entry.account_id))
            .collect::<Vec<_>>();
        candidates.sort_unstable();

        match part {
            PartKind::Content => {
                for (_, account_id) in candidates {
                    entries[&account_id].try_drop_content(EvictReason::Budget);
                    if fits() {
                        return true;
                    }
                }
            }
            PartKind::Thread => {
                // When a thread entry is evicted its optional content goes with
                // it. First prefer entries whose content has already gone.
                for has_content in [false, true] {
                    for &(_, account_id) in &candidates {
                        if let Some(entry) = entries.get(&account_id).cloned() {
                            entry.try_eviction(|index| {
                                if index.as_ref().is_some_and(|index| {
                                    index.has_optional_content() == has_content
                                }) {
                                    entries.remove(&account_id);
                                    trace_eviction(account_id, EvictReason::Budget);
                                }
                            });
                            drop(entry);
                            if fits() {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        fits()
    }

    #[cfg(feature = "test_mode")]
    pub fn expire_idle_for_test(&self, account_id: u32) {
        self.inner.entries.lock().unwrap()[&account_id]
            .last_used
            .store(
                now().saturating_sub(self.config().idle_secs.saturating_add(1)),
                Ordering::Relaxed,
            );
    }

    #[cfg(feature = "test_mode")]
    pub fn set_content_bytes_for_test(&self, account_id: u32, bytes: u64) {
        let entry = self.entry(account_id);
        let mut guard = entry.index.try_write().expect("test index has no readers");
        let index = guard.get_or_insert_with(|| LoadedIndex::empty(Manifest::default()));
        drop(index.take_optional_content());
        index.content = self
            .admit(PartKind::Content, bytes, account_id)
            .map(|charge| LoadedContent {
                part: ContentPart::default(),
                segments: Vec::new(),
                bytes,
                charge,
            });
    }
}

pub struct IndexEntry {
    pub account_id: u32,
    pub index: Arc<RwLock<Option<LoadedIndex>>>,
    pub pass_lock: AsyncMutex<()>,
    /// Required Thread loading and validated Map reloads.
    pub load_flight: AsyncMutex<()>,
    /// Optional Content fetch and detached preparation.
    pub content_load_flight: AsyncMutex<()>,
    pub(super) last_used: AtomicU64,
    absent: Mutex<AHashMap<BlobHash, u64>>,
    open_retry: Mutex<Option<OpenRetry>>,
    #[cfg(feature = "test_mode")]
    fail_abort_write: std::sync::atomic::AtomicBool,
    cache: Weak<CacheState>,
}

/// One resident entry's consecutive failed account probes. Never serialized.
/// Only the pass-lock owner changes admission, observations and outcomes.
pub struct OpenRetry {
    // None means no manifest has ever authenticated in this episode. A failed
    // observation retains a historical Some basis without claiming it is current.
    basis: Option<Manifest>,
    streak: u8,
    class: TransientClass,
    deadline: Instant,
    observe_at: Instant,
}

fn same_retry_basis(left: &Manifest, right: &Manifest) -> bool {
    if left.failing.is_some() && right.failing.is_none() {
        return false;
    }
    let mut normalized = right.clone();
    normalized.failing = left.failing.clone();
    left == &normalized
}

pub type LoadedGuard = OwnedRwLockReadGuard<Option<LoadedIndex>, LoadedIndex>;

impl IndexEntry {
    /// Read-only scheduling information. The pass lock owns the decision to
    /// observe; a waiter uses this only to choose its next bounded wakeup.
    pub fn retry_schedule(&self) -> Option<(TransientClass, Instant, Instant)> {
        self.open_retry
            .lock()
            .unwrap()
            .as_ref()
            .map(|retry| (retry.class, retry.deadline, retry.observe_at))
    }

    /// Reserve the next shared observation before awaiting I/O. Cancellation
    /// and failed reads consume this observation slot, never a failed probe.
    pub fn begin_retry_observation(&self, now: Instant) -> bool {
        if let Some(retry) = self.open_retry.lock().unwrap().as_mut()
            && now >= retry.observe_at
        {
            retry.observe_at = now + Duration::from_secs(1);
            true
        } else {
            false
        }
    }

    /// Called under pass_lock with an authenticated actual head. Diagnostic
    /// replacement/creation is not progress; explicit clearing is progress.
    pub fn defer_open_retry(
        &self,
        manifest: &Manifest,
        now: Instant,
    ) -> Option<(TransientClass, Instant)> {
        let mut slot = self.open_retry.lock().unwrap();
        let retry = slot.as_mut()?;
        if !retry
            .basis
            .as_ref()
            .is_some_and(|basis| same_retry_basis(basis, manifest))
        {
            *slot = None;
            return None;
        }
        retry.basis = Some(manifest.clone());
        (now < retry.deadline).then_some((retry.class, retry.deadline))
    }

    /// Record the first transient of an admitted probe synchronously. Unknown
    /// reads may retain a historical basis, but cannot create a generation.
    pub fn note_pass_failure(
        &self,
        observed: Option<&Manifest>,
        class: TransientClass,
        now: Instant,
    ) {
        let mut slot = self.open_retry.lock().unwrap();
        let prior = slot.as_ref().filter(|retry| match observed {
            Some(observed) => retry
                .basis
                .as_ref()
                .is_some_and(|basis| same_retry_basis(basis, observed)),
            None => true,
        });
        let streak = prior.map_or(1, |retry| retry.streak.saturating_add(1));
        let basis = observed
            .cloned()
            .or_else(|| prior.and_then(|retry| retry.basis.clone()));
        let delay = match streak {
            1 => 1,
            2 => 2,
            3 => 4,
            4 => 8,
            5 => 16,
            _ => 30,
        };
        *slot = Some(OpenRetry {
            basis,
            streak,
            class,
            deadline: now + Duration::from_secs(delay),
            observe_at: now + Duration::from_secs(1),
        });
    }

    /// Settle an acknowledged batch before any resident-swap await. Compare
    /// the actual committed manifest, including all meaningful progress.
    pub fn commit_open_retry(&self, before: &Manifest, after: &Manifest) {
        let mut slot = self.open_retry.lock().unwrap();
        if let Some(retry) = slot.as_mut() {
            if retry
                .basis
                .as_ref()
                .is_some_and(|basis| same_retry_basis(basis, before))
                && same_retry_basis(before, after)
            {
                retry.basis = Some(after.clone());
            } else {
                *slot = None;
            }
        }
    }

    /// Settle this probe's acknowledged abort. Its discarded prefix made no
    /// progress, even when suspension or reconciliation bookkeeping changed.
    /// The pass owner must have recorded its primary failure before committing.
    pub fn commit_aborted_retry(&self, before: &Manifest, after: &Manifest) {
        let mut slot = self.open_retry.lock().unwrap();
        if let Some(retry) = slot.as_mut() {
            if retry
                .basis
                .as_ref()
                .is_some_and(|basis| same_retry_basis(basis, before))
            {
                retry.basis = Some(after.clone());
            } else {
                // Never reconstruct a superseded or cleared episode.
                *slot = None;
            }
        }
    }

    pub fn clear_open_retry(&self) {
        self.open_retry.lock().unwrap().take();
    }

    /// Explicit clock seam: advance only this entry's deadline, retaining the
    /// probe streak and identity. No production bypass is compiled.
    #[cfg(feature = "test_mode")]
    pub fn set_open_retry_deadline_for_test(&self, deadline: Instant) {
        self.open_retry
            .lock()
            .unwrap()
            .as_mut()
            .expect("an open retry is armed")
            .deadline = deadline;
    }

    #[cfg(feature = "test_mode")]
    pub fn set_retry_observation_for_test(&self, observe_at: Instant) {
        self.open_retry
            .lock()
            .unwrap()
            .as_mut()
            .expect("a retry is armed")
            .observe_at = observe_at;
    }

    #[cfg(feature = "test_mode")]
    pub fn fail_abort_write_once_for_test(&self) {
        self.fail_abort_write.store(true, Ordering::Relaxed);
    }

    #[cfg(feature = "test_mode")]
    pub fn take_abort_write_failure_for_test(&self) -> bool {
        self.fail_abort_write.swap(false, Ordering::Relaxed)
    }

    pub(super) fn cache(&self) -> Option<ZaSearchCache> {
        self.cache.upgrade().map(|inner| ZaSearchCache { inner })
    }

    pub async fn read_loaded(self: &Arc<Self>) -> Option<LoadedGuard> {
        let read = self.index.clone().read_owned();
        #[cfg(feature = "test_mode")]
        let read = crate::za::test_hooks::observe_search_wait(
            self.account_id,
            crate::za::test_hooks::SearchStage::ResidentIndexWait,
            read,
        );
        let guard = read.await;
        let guard = OwnedRwLockReadGuard::try_map(guard, Option::as_ref).ok()?;
        self.touch();
        Some(guard)
    }

    pub fn touch(&self) {
        self.last_used.store(now(), Ordering::Relaxed);
    }

    pub fn in_use(&self) -> bool {
        self.try_eviction(|_| ()).is_none()
    }

    fn try_eviction<T>(&self, evict: impl FnOnce(&mut Option<LoadedIndex>) -> T) -> Option<T> {
        let _pass = self.pass_lock.try_lock().ok()?;
        let _load = self.load_flight.try_lock().ok()?;
        let _content = self.content_load_flight.try_lock().ok()?;
        let mut index = self.index.try_write().ok()?;
        Some(evict(&mut index))
    }

    fn try_drop_content(&self, reason: EvictReason) {
        let retired = self
            .try_eviction(|index| index.as_mut().and_then(LoadedIndex::take_optional_content))
            .flatten();
        if retired.is_some() {
            trace_eviction(self.account_id, reason);
        }
    }

    /// The current pass owns pass_lock. Budget pressure may release its
    /// content only after a successful preflight, with no reader or load in
    /// flight and only while this exact base remains resident.
    pub fn try_evict_content_for_compaction(&self, base_hash: [u8; 32]) -> bool {
        let Ok(load) = self.load_flight.try_lock() else {
            return false;
        };
        let Ok(content_load) = self.content_load_flight.try_lock() else {
            return false;
        };
        let Ok(mut guard) = self.index.try_write() else {
            return false;
        };
        let retired = guard
            .as_mut()
            .filter(|index| index.manifest_bytes_hash == base_hash)
            .and_then(LoadedIndex::take_optional_content);
        drop(guard);
        drop(content_load);
        drop(load);
        if retired.is_some() {
            trace_eviction(self.account_id, EvictReason::Budget);
            true
        } else {
            false
        }
    }

    pub fn absent_since(&self, hash: &BlobHash, now: u64) -> u64 {
        *self
            .absent
            .lock()
            .unwrap()
            .entry(hash.clone())
            .or_insert(now)
    }

    pub fn seen(&self, hash: &BlobHash) {
        self.absent.lock().unwrap().remove(hash);
    }
}

pub struct LoadedIndex {
    pub manifest_bytes_hash: [u8; 32],
    pub manifest: Manifest,
    pub thread: ThreadPart,
    pub thread_segments: Vec<SegmentId>,
    pub content: Option<LoadedContent>,
    // One previously verified suffix, never served as the current Content.
    pub(super) deferred_content: Option<(ContentHead, LoadedContent)>,
    pub scanned_to: u64,
    pub update_records: usize,
    pub thread_bytes: u64,
    pub thread_charge: BudgetCharge,
}

impl LoadedIndex {
    /// Expose the resident optional part's position without allowing the
    /// diagnostic to take ownership of deferred Content or promote it.
    #[cfg(feature = "test_mode")]
    pub fn content_for_test(&self) -> Option<(bool, Option<&ContentHead>, &LoadedContent)> {
        self.content
            .as_ref()
            .map(|content| (true, self.manifest.content.as_ref(), content))
            .or_else(|| {
                self.deferred_content
                    .as_ref()
                    .map(|(head, part)| (false, Some(head), part))
            })
    }

    fn has_optional_content(&self) -> bool {
        self.content.is_some() || self.deferred_content.is_some()
    }

    pub(super) fn take_optional_content(&mut self) -> Option<LoadedContent> {
        debug_assert!(self.content.is_none() || self.deferred_content.is_none());
        self.content
            .take()
            .or_else(|| self.deferred_content.take().map(|(_, content)| content))
    }

    pub fn identity_version(&self) -> (u64, u32) {
        self.manifest.identity_version()
    }

    pub fn empty(manifest: Manifest) -> Self {
        Self {
            manifest_bytes_hash: [0; 32],
            scanned_to: scanned_seed(&manifest.indexed_to),
            manifest,
            thread: ThreadPart::default(),
            thread_segments: Vec::new(),
            content: None,
            deferred_content: None,
            update_records: 0,
            thread_bytes: 0,
            thread_charge: BudgetCharge::detached(),
        }
    }
}

pub fn scanned_seed(indexed_to: &Checkpoint) -> u64 {
    if indexed_to.complete {
        indexed_to.change_id
    } else {
        indexed_to.change_id.saturating_sub(1)
    }
}

pub struct BudgetCharge {
    counter: Arc<AtomicU64>,
    bytes: u64,
}

impl BudgetCharge {
    // A staged append becomes part of the resident allocation. Transfer its
    // reservation without releasing or double-counting bytes in the process.
    pub(super) fn absorb(&mut self, mut staged: Self) {
        if self.bytes == 0 {
            *self = staged;
        } else if staged.bytes != 0 {
            debug_assert!(Arc::ptr_eq(&self.counter, &staged.counter));
            self.bytes += staged.bytes;
            staged.bytes = 0;
        }
    }

    /// Move part of this reservation without changing the shared usage counter.
    pub fn transfer_to(&mut self, target: &mut Self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        assert!(bytes <= self.bytes);
        assert!(target.bytes == 0 || Arc::ptr_eq(&self.counter, &target.counter));
        self.bytes -= bytes;
        target.absorb(Self {
            counter: self.counter.clone(),
            bytes,
        });
    }

    pub fn detached() -> Self {
        Self {
            counter: Arc::default(),
            bytes: 0,
        }
    }
}

impl Drop for BudgetCharge {
    fn drop(&mut self) {
        self.counter.fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

pub struct LoadedContent {
    pub part: ContentPart,
    pub segments: Vec<SegmentId>,
    pub bytes: u64,
    pub charge: BudgetCharge,
}

pub(super) fn trace_eviction(account_id: u32, reason: EvictReason) {
    za_search_trace!(
        ZA_SEARCH_EVICTED,
        AccountId = account_id,
        Reason = reason.as_str()
    );
}

impl Server {
    pub fn za_search_config(&self) -> ZaSearchConfig {
        self.inner.cache.za_search.effective_config()
    }
}
#[cfg(all(test, feature = "test_mode"))]
mod tests {
    use super::*;
    use crate::za::test_hooks::set_search_config_override;

    #[test]
    fn staged_content_charge_is_reconciled_with_resident_estimate() {
        use store::{
            search::IndexDocument,
            write::SearchIndex,
            za_search::content::{ContentRecord, SortKeys},
        };

        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let mut index = LoadedIndex::empty(Manifest::default());
        let mut part = ContentPart::default();
        part.insert(ContentRecord::new(
            IndexDocument::new(SearchIndex::Email),
            1,
            SortKeys::default(),
            None,
            false,
        ));
        let estimate = part.loaded_bytes() as u64;
        let staged = cache.admit(PartKind::Content, estimate + 10, 0).unwrap();
        index.content = Some(LoadedContent {
            part,
            segments: Vec::new(),
            bytes: 0,
            charge: BudgetCharge::detached(),
        });
        assert!(cache.admit_content_growth_with_staged(&mut index, 0, Some(staged)));
        assert_eq!(cache.content_used(), estimate);
        assert_eq!(index.content.take().unwrap().bytes, estimate);
        assert_eq!(cache.content_used(), 0);

        let staged = cache.admit(PartKind::Content, estimate, 0).unwrap();
        assert!(cache.admit_content_growth_with_staged(&mut index, 0, Some(staged)));
        assert_eq!(cache.content_used(), 0);
    }

    #[test]
    fn partial_transfer_keeps_remaining_staging_and_resident_credit_separate() {
        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let _budget = cache.override_content_budget_for_test(120);
        let mut staged = cache.admit(PartKind::Content, 100, 0).unwrap();
        let mut resident = BudgetCharge::detached();
        let existing = cache.admit(PartKind::Content, 20, 0).unwrap();

        staged.transfer_to(&mut resident, 60);
        staged.transfer_to(&mut resident, 20);
        assert_eq!(cache.content_used(), 120);
        assert!(cache.admit(PartKind::Content, 1, 1).is_none());
        assert!(cache.grow(&mut resident, PartKind::Content, 75, 0));
        assert_eq!(cache.content_used(), 115);
        drop(resident);
        assert_eq!(cache.content_used(), 40);
        drop(staged);
        assert_eq!(cache.content_used(), 20);
        drop(existing);
        assert_eq!(cache.content_used(), 0);
    }

    #[test]
    fn partial_transfer_rejects_different_nonempty_pools() {
        use std::panic::{AssertUnwindSafe, catch_unwind};

        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let mut staged = cache.admit(PartKind::Content, 10, 0).unwrap();
        let mut thread = cache.admit(PartKind::Thread, 20, 0).unwrap();
        assert!(catch_unwind(AssertUnwindSafe(|| staged.transfer_to(&mut thread, 5))).is_err());
        assert_eq!(cache.content_used(), 10);
        assert_eq!(cache.thread_used(), 20);
        drop(staged);
        drop(thread);
        assert_eq!((cache.content_used(), cache.thread_used()), (0, 0));
    }

    #[test]
    fn content_budget_override_is_scoped_to_one_instance_and_part() {
        let boot = ZaSearchConfig::default();
        let cache = ZaSearchCache::new(Ok(boot.clone()));
        let separate = ZaSearchCache::new(Ok(boot));
        let original = cache.content_budget();
        let thread = cache.budget(PartKind::Thread);
        {
            let _guard = cache.override_content_budget_for_test(4096);
            assert_eq!(cache.content_budget(), 4096);
            assert_eq!(cache.budget(PartKind::Thread), thread);
            assert_eq!(separate.content_budget(), original);
            assert!(cache.admit(PartKind::Content, 4097, 0).is_none());
            assert!(cache.admit(PartKind::Content, 4096, 0).is_some());
        }
        assert_eq!(cache.content_budget(), original);
    }

    #[test]
    fn the_override_replaces_the_boot_value_in_test_mode() {
        let boot = ZaSearchConfig {
            pass_cap: 3,
            ..ZaSearchConfig::default()
        };
        let cache = ZaSearchCache::new(Ok(boot.clone()));
        let overridden = ZaSearchConfig {
            pass_cap: 5,
            ..boot.clone()
        };

        assert_eq!(cache.effective_config(), boot);
        set_search_config_override(Some(overridden.clone()));
        let effective = cache.effective_config();
        set_search_config_override(None);

        assert_eq!(effective, overridden);
        assert_eq!(cache.effective_config(), boot);
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use store::za_search::manifest::{ContentCursor, FailingDocument, ReconciliationMarker};

    fn failing(attempts: u8) -> Manifest {
        Manifest {
            epoch: 9,
            failing: Some(FailingDocument {
                document_id: 7,
                attempts,
            }),
            ..Manifest::default()
        }
    }

    #[test]
    fn retry_delays_follow_actual_probes_and_cap_without_sleeping() {
        for class in [
            TransientClass::BlobStore,
            TransientClass::AbsentWithinWindow,
        ] {
            let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
            let entry = cache.entry(1);
            let mut basis = failing(0);
            let mut now = Instant::now();
            for delay in [1, 2, 4, 8, 16, 30, 30, 30] {
                entry.note_pass_failure(Some(&basis), class, now);
                let mut committed = basis.clone();
                if class != TransientClass::AbsentWithinWindow {
                    committed.failing.as_mut().unwrap().attempts += 1;
                }
                entry.commit_open_retry(&basis, &committed);
                let deadline = now + Duration::from_secs(delay);
                assert_eq!(
                    entry.defer_open_retry(&committed, now),
                    Some((class, deadline))
                );
                assert_eq!(
                    entry.defer_open_retry(&committed, deadline - Duration::from_nanos(1)),
                    Some((class, deadline))
                );
                assert_eq!(entry.defer_open_retry(&committed, deadline), None);
                assert_eq!(
                    entry.defer_open_retry(&committed, deadline + Duration::from_nanos(1)),
                    None
                );
                // A second node's diagnostic write cannot reset this node's
                // next delay. Absence keeps zero persisted attempts throughout.
                if class != TransientClass::AbsentWithinWindow {
                    committed.failing.as_mut().unwrap().attempts += 3;
                }
                assert_eq!(
                    entry.defer_open_retry(&committed, now),
                    Some((class, deadline))
                );
                basis = committed;
                now = deadline;
            }
            if class == TransientClass::AbsentWithinWindow {
                assert_eq!(basis.failing.unwrap().attempts, 0);
            }
        }
    }

    #[test]
    fn account_retry_streak_survives_alternating_documents_and_fault_classes() {
        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let entry = cache.entry(1);
        let mut observed = failing(0);
        let mut now = Instant::now();
        for (index, delay) in [1, 2, 4, 8, 16, 30, 30].into_iter().enumerate() {
            let document_id = if index % 2 == 0 { 7 } else { 8 };
            let class = if index % 2 == 0 {
                TransientClass::Store
            } else {
                TransientClass::BlobStore
            };
            entry.note_pass_failure(Some(&observed), class, now);
            let mut committed = observed.clone();
            committed.failing = Some(FailingDocument {
                document_id,
                attempts: 1,
            });
            entry.commit_open_retry(&observed, &committed);
            let expected = now + Duration::from_secs(delay);
            assert_eq!(
                entry.defer_open_retry(&committed, now),
                Some((class, expected)),
                "a diagnostic-only document change cannot reset the account streak"
            );
            assert_eq!(entry.defer_open_retry(&committed, expected), None);
            observed = committed;
            now = expected;
        }
    }

    #[test]
    fn progress_and_lifecycle_changes_clear_retry() {
        let original = failing(4);
        let mut changes = Vec::new();
        let mut changed = original.clone();
        changed.failing = None;
        changes.push(changed);
        let mut changed = original.clone();
        changed.epoch += 1;
        changes.push(changed);
        let mut changed = original.clone();
        changed.revision += 1;
        changes.push(changed);
        let mut changed = original.clone();
        changed.indexed_to.change_id += 1;
        changes.push(changed);
        let mut changed = original.clone();
        changed.content_to.suspended = true;
        changes.push(changed);
        let mut changed = original.clone();
        changed.content_to.cursor = Some(ContentCursor { next_below: 2 });
        changes.push(changed);
        let mut changed = original.clone();
        changed.reconciliation = Some(ReconciliationMarker {
            bound: 3,
            gap_next: 0,
        });
        changes.push(changed);
        for changed in changes {
            let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
            let entry = cache.entry(1);
            let now = Instant::now();
            entry.note_pass_failure(Some(&original), TransientClass::Store, now);
            assert!(entry.defer_open_retry(&original, now).is_some());
            assert_eq!(entry.defer_open_retry(&changed, now), None);
            entry.note_pass_failure(Some(&changed), TransientClass::Store, now);
            assert_eq!(
                entry.defer_open_retry(&changed, now),
                Some((TransientClass::Store, now + Duration::from_secs(1)))
            );
            entry.clear_open_retry();
            assert_eq!(entry.defer_open_retry(&changed, now), None);
        }
    }

    #[test]
    fn failed_writes_and_assertion_losers_keep_only_their_validated_basis() {
        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let entry = cache.entry(1);
        let observed = Manifest {
            failing: None,
            ..failing(0)
        };
        let now = Instant::now();
        entry.note_pass_failure(Some(&observed), TransientClass::Store, now);
        // A failed write retains the unchanged observation, even with no
        // persisted failure record yet. A winner with the same None is not progress.
        assert!(entry.defer_open_retry(&observed, now).is_some());
        entry.defer_open_retry(&observed, now);
        assert!(entry.defer_open_retry(&observed, now).is_some());
        entry.clear_open_retry();
        entry.note_pass_failure(Some(&observed), TransientClass::Store, now);
        let remote = failing(9);
        entry.defer_open_retry(&remote, now);
        assert_eq!(
            entry.defer_open_retry(&remote, now),
            Some((TransientClass::Store, now + Duration::from_secs(1)))
        );
        entry.note_pass_failure(
            Some(&remote),
            TransientClass::Store,
            now + Duration::from_secs(1),
        );
        assert_eq!(
            entry.defer_open_retry(&remote, now),
            Some((TransientClass::Store, now + Duration::from_secs(3)))
        );
        let replacement = Manifest {
            epoch: 10,
            ..remote
        };
        entry.defer_open_retry(&replacement, now);
        assert_eq!(entry.defer_open_retry(&replacement, now), None);
    }

    fn assert_abort_settlement_retry(assertion_winner: bool, suspension: bool) {
        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let entry = cache.entry(1);
        let mut before = failing(4);
        before.reconciliation = Some(ReconciliationMarker {
            bound: 10,
            gap_next: 0,
        });
        let mut now = Instant::now();
        for delay in [1, 2, 4, 8, 16] {
            entry.note_pass_failure(Some(&before), TransientClass::Store, now);
            now += Duration::from_secs(delay);
        }
        entry.note_pass_failure(Some(&before), TransientClass::Store, now);
        assert_eq!(
            entry.defer_open_retry(&before, now),
            Some((TransientClass::Store, now + Duration::from_secs(30)))
        );

        let mut after = before.clone();
        after.failing.as_mut().unwrap().attempts = 5;
        if suspension {
            after.content_to.suspended = true;
        } else {
            after.reconciliation.as_mut().unwrap().bound = 11;
        }
        let failed_schedule = entry.retry_schedule();
        if assertion_winner {
            entry.defer_open_retry(&after, now);
            assert_eq!(entry.retry_schedule(), None);
            entry.note_pass_failure(Some(&after), TransientClass::Store, now);
            assert_eq!(
                entry.defer_open_retry(&after, now),
                Some((TransientClass::Store, now + Duration::from_secs(1))),
                "an independently authenticated meaningful winner clears the old episode"
            );
        } else {
            entry.commit_aborted_retry(&before, &after);
            assert_eq!(
                entry.retry_schedule(),
                failed_schedule,
                "own abort bookkeeping must retain the current failed probe exactly"
            );
            assert_eq!(
                entry.defer_open_retry(&after, now),
                Some((TransientClass::Store, now + Duration::from_secs(30))),
                "the recorded failure must bind to the acknowledged abort manifest"
            );
        }
    }

    #[test]
    fn local_suspension_abort_commit_preserves_retry() {
        assert_abort_settlement_retry(false, true);
    }

    #[test]
    fn local_reconciliation_abort_commit_preserves_retry() {
        assert_abort_settlement_retry(false, false);
    }

    #[test]
    fn winning_suspension_abort_commit_clears_retry() {
        assert_abort_settlement_retry(true, true);
    }

    #[test]
    fn winning_reconciliation_abort_commit_clears_retry() {
        assert_abort_settlement_retry(true, false);
    }

    #[test]
    fn own_abort_rebase_preserves_unsaturated_streak_and_never_restores_a_cleared_slot() {
        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let entry = cache.entry(1);
        let before = failing(4);
        let mut now = Instant::now();
        for delay in [1, 2] {
            entry.note_pass_failure(Some(&before), TransientClass::Store, now);
            now += Duration::from_secs(delay);
        }
        entry.note_pass_failure(Some(&before), TransientClass::BlobStore, now);
        let schedule = entry.retry_schedule();
        let mut after = before.clone();
        after.content_to.suspended = true;
        entry.commit_aborted_retry(&before, &after);
        assert_eq!(entry.retry_schedule(), schedule);
        assert_eq!(
            entry.defer_open_retry(&after, now),
            Some((TransientClass::BlobStore, now + Duration::from_secs(4)))
        );
        now += Duration::from_secs(4);
        entry.note_pass_failure(Some(&after), TransientClass::Store, now);
        assert_eq!(
            entry.defer_open_retry(&after, now),
            Some((TransientClass::Store, now + Duration::from_secs(8)))
        );
        entry.clear_open_retry();
        entry.commit_aborted_retry(&before, &after);
        assert_eq!(entry.retry_schedule(), None);
    }

    #[test]
    fn diagnostic_creation_and_count_only_commits_preserve_retry() {
        for assertion_winner in [false, true] {
            for prior_attempts in [None, Some(4)] {
                let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
                let entry = cache.entry(1);
                let mut before = failing(0);
                before.failing = prior_attempts.map(|attempts| FailingDocument {
                    document_id: 7,
                    attempts,
                });
                let now = Instant::now();
                entry.note_pass_failure(Some(&before), TransientClass::Store, now);
                let after = failing(prior_attempts.unwrap_or_default() + 1);
                if assertion_winner {
                    entry.defer_open_retry(&after, now);
                } else {
                    entry.commit_open_retry(&before, &after);
                }
                assert_eq!(
                    entry.defer_open_retry(&after, now),
                    Some((TransientClass::Store, now + Duration::from_secs(1)))
                );
                let remote = failing(20);
                assert_eq!(
                    entry.defer_open_retry(&remote, now),
                    Some((TransientClass::Store, now + Duration::from_secs(1)))
                );
                entry.note_pass_failure(
                    Some(&remote),
                    TransientClass::Store,
                    now + Duration::from_secs(1),
                );
                assert_eq!(
                    entry.defer_open_retry(&remote, now),
                    Some((TransientClass::Store, now + Duration::from_secs(3)))
                );
            }
        }
    }

    #[test]
    fn a_new_actual_failing_document_preserves_the_streak_and_binds_its_diagnostic() {
        for assertion_winner in [false, true] {
            let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
            let entry = cache.entry(1);
            let before = failing(4);
            let now = Instant::now();
            entry.note_pass_failure(Some(&before), TransientClass::Store, now);
            entry.note_pass_failure(
                Some(&before),
                TransientClass::Store,
                now + Duration::from_secs(1),
            );
            entry.note_pass_failure(
                Some(&before),
                TransientClass::Store,
                now + Duration::from_secs(3),
            );
            let mut after = before.clone();
            after.failing = Some(FailingDocument {
                document_id: 8,
                attempts: 1,
            });
            if assertion_winner {
                entry.defer_open_retry(&after, now);
            } else {
                entry.commit_open_retry(&before, &after);
            }
            assert_eq!(
                entry.defer_open_retry(&after, now + Duration::from_secs(3)),
                Some((TransientClass::Store, now + Duration::from_secs(7)))
            );
        }
    }

    #[test]
    fn diagnostic_transitions_preserve_only_the_same_meaningful_basis() {
        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let entry = cache.entry(1);
        let now = Instant::now();
        let mut none = failing(0);
        none.failing = None;
        entry.note_pass_failure(Some(&none), TransientClass::Budget, now);
        let schedule = entry.retry_schedule().unwrap();
        assert!(entry.defer_open_retry(&none, now).is_some());
        let mut diagnostic = failing(9);
        diagnostic.failing.as_mut().unwrap().document_id = 99;
        assert!(entry.defer_open_retry(&diagnostic, now).is_some());
        diagnostic.failing.as_mut().unwrap().document_id = 17;
        diagnostic.failing.as_mut().unwrap().attempts = 255;
        assert!(entry.defer_open_retry(&diagnostic, now).is_some());
        assert_eq!(entry.retry_schedule(), Some(schedule));
        assert_eq!(entry.defer_open_retry(&none, now), None);
        assert!(entry.retry_schedule().is_none());
    }

    #[test]
    fn unknown_observations_keep_schedule_without_claiming_a_generation() {
        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let entry = cache.entry(1);
        let now = Instant::now();
        entry.note_pass_failure(None, TransientClass::Store, now);
        let initial = entry.retry_schedule().unwrap();
        assert!(!entry.begin_retry_observation(now));
        assert!(entry.begin_retry_observation(initial.2));
        assert!(!entry.begin_retry_observation(initial.2));
        // A failed observation changes only the shared cadence. It is not a
        // probe and cannot change the deadline or grow the failure streak.
        assert_eq!(
            entry.retry_schedule(),
            Some((initial.0, initial.1, initial.2 + Duration::from_secs(1)))
        );
        assert_eq!(entry.defer_open_retry(&failing(2), now), None);
        entry.note_pass_failure(Some(&failing(2)), TransientClass::BlobStore, now);
        assert_eq!(
            entry.retry_schedule().unwrap().1,
            now + Duration::from_secs(1)
        );
        entry.note_pass_failure(None, TransientClass::Store, now + Duration::from_secs(1));
        assert_eq!(
            entry.retry_schedule().unwrap().1,
            now + Duration::from_secs(3)
        );
        assert!(entry.defer_open_retry(&failing(9), now).is_some());
        let replacement = Manifest {
            epoch: 10,
            ..failing(9)
        };
        assert_eq!(entry.defer_open_retry(&replacement, now), None);
    }

    #[test]
    fn actual_winner_generation_change_clears_historical_retry() {
        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let entry = cache.entry(1);
        let now = Instant::now();
        let before = failing(4);
        entry.note_pass_failure(Some(&before), TransientClass::Store, now);
        let actual = Manifest {
            epoch: 10,
            ..before.clone()
        };
        entry.defer_open_retry(&actual, now);
        assert!(entry.retry_schedule().is_none());
    }

    #[test]
    fn replacing_a_resident_entry_starts_without_retry_state() {
        let cache = ZaSearchCache::new(Ok(ZaSearchConfig::default()));
        let old = cache.entry(1);
        let basis = failing(1);
        let now = Instant::now();
        old.note_pass_failure(Some(&basis), TransientClass::Store, now);
        cache.remove_account(1, EvictReason::Invalidation);
        let new = cache.entry(1);
        assert!(!Arc::ptr_eq(&old, &new));
        assert_eq!(new.defer_open_retry(&basis, now), None);
    }
}
