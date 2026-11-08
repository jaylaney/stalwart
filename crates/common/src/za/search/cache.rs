/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{config::ZaSearchConfig, trace::ZA_SEARCH_EVICTED};
use crate::{Server, za_search_trace};
use ahash::AHashMap;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicU64, Ordering},
};
use store::{
    write::now,
    za_search::{
        Checkpoint, SegmentId, content::ContentPart, manifest::Manifest, thread::ThreadPart,
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
        let entry = {
            let mut entries = self.inner.entries.lock().unwrap();
            let entry = entries.entry(account_id).or_insert_with(|| {
                Arc::new(IndexEntry {
                    account_id,
                    index: Arc::default(),
                    pass_lock: AsyncMutex::default(),
                    load_flight: AsyncMutex::default(),
                    last_used: AtomicU64::new(now()),
                    absent: Mutex::default(),
                    cache: Arc::downgrade(&self.inner),
                })
            });
            entry.touch();
            entry.clone()
        };
        self.sweep_idle();
        entry
    }

    pub fn get(&self, account_id: u32) -> Option<Arc<IndexEntry>> {
        self.inner.entries.lock().unwrap().get(&account_id).cloned()
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

    pub fn admit_content_growth(&self, index: &mut LoadedIndex, account_id: u32) -> bool {
        let Some(content) = index.content.as_mut() else {
            return true;
        };
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

    pub(super) fn budget(&self, part: PartKind) -> u64 {
        let mb = match part {
            PartKind::Thread => self.config().thread_mem_mb,
            PartKind::Content => self.config().mem_mb,
        };
        mb.saturating_mul(1 << 20)
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
                                if index
                                    .as_ref()
                                    .is_some_and(|index| index.content.is_some() == has_content)
                                {
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
    pub fn set_content_bytes_for_test(&self, account_id: u32, bytes: u64) {
        let entry = self.entry(account_id);
        let mut guard = entry.index.try_write().expect("test index has no readers");
        let index = guard.get_or_insert_with(|| LoadedIndex::empty(Manifest::default()));
        index.content = None;
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
    pub load_flight: AsyncMutex<()>,
    pub(super) last_used: AtomicU64,
    absent: Mutex<AHashMap<BlobHash, u64>>,
    cache: Weak<CacheState>,
}

pub type LoadedGuard = OwnedRwLockReadGuard<Option<LoadedIndex>, LoadedIndex>;

impl IndexEntry {
    pub(super) fn cache(&self) -> Option<ZaSearchCache> {
        self.cache.upgrade().map(|inner| ZaSearchCache { inner })
    }

    pub async fn read_loaded(self: &Arc<Self>) -> Option<LoadedGuard> {
        let guard = self.index.clone().read_owned().await;
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
        let mut index = self.index.try_write().ok()?;
        Some(evict(&mut index))
    }

    fn try_drop_content(&self, reason: EvictReason) {
        self.try_eviction(|index| {
            if let Some(index) = index.as_mut()
                && index.content.take().is_some()
            {
                trace_eviction(self.account_id, reason);
            }
        });
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
    pub scanned_to: u64,
    pub update_records: usize,
    pub thread_bytes: u64,
    pub thread_charge: BudgetCharge,
}

impl LoadedIndex {
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
