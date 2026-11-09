/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    cache::{
        BudgetCharge, EvictReason, IndexEntry, LoadedContent, LoadedIndex, PartKind, ZaSearchCache,
        scanned_seed, trace_eviction,
    },
    config::ZaSearchConfig,
    error::{LoadFailure, LossClass, NoMapReason, TransientClass},
    seal::{parse_content_blob, parse_thread_blob, za_search_key},
    trace::ZA_SEARCH_OVER_BUDGET,
};
use crate::{Server, auth::vault::RawValue, za_search_trace};
use ahash::AHashSet;
use std::{future::Future, sync::Arc};
use store::{
    ValueKey,
    write::now,
    za_search::{
        BlobHeader, BlobKind, SegmentId, ZaSearchKey,
        blob::{HiddenSection, PublicSection},
        content::{ContentPart, ContentRecord},
        manifest::{
            ContentHead, Manifest, ManifestError, ManifestValue, ThreadBlobEntry, manifest_hash,
        },
        thread::{RevisionRow, ThreadPart},
    },
};
use trc::{EventType, Key, StoreEvent};
use types::{blob_hash::BlobHash, collection::Collection, field::PrincipalField};
use vault::{keys::Secret, session::SessionKeys};

#[cfg(test)]
#[path = "load_tests.rs"]
mod tests;

/// Recovery evidence at the failed read. Map loss carries the optional
/// store value. A content walk starts from a resident part, so its evidence
/// names the exact raw-manifest digest, epoch and head.
#[derive(Debug, PartialEq, Eq)]
pub enum ObservedLoadError {
    Other(LoadFailure),
    MapLoss(MapLoss),
    ContentLoss(ContentLoss),
}

#[derive(Debug, PartialEq, Eq)]
pub struct MapLoss {
    class: LossClass,
    raw: Option<Vec<u8>>,
    failed_thread: Option<BlobHash>,
}

impl MapLoss {
    pub fn missing() -> Self {
        Self {
            class: LossClass::ManifestMissing,
            raw: None,
            failed_thread: None,
        }
    }

    pub fn class(&self) -> LossClass {
        self.class
    }

    pub fn raw(&self) -> Option<&[u8]> {
        self.raw.as_deref()
    }

    pub fn failed_thread(&self) -> Option<&BlobHash> {
        self.failed_thread.as_ref()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ContentLoss {
    class: LossClass,
    manifest_hash: [u8; 32],
    epoch: u64,
    head: ContentHead,
}

impl ContentLoss {
    pub fn class(&self) -> LossClass {
        self.class
    }

    pub fn matches(&self, bytes: &[u8], manifest: &Manifest) -> bool {
        self.manifest_hash == manifest_hash(bytes)
            && self.epoch == manifest.epoch
            && manifest.content.as_ref() == Some(&self.head)
    }
}

impl ObservedLoadError {
    fn map_read(
        failure: LoadFailure,
        raw: impl FnOnce() -> Vec<u8>,
        failed_thread: Option<BlobHash>,
    ) -> Self {
        match failure {
            LoadFailure::Loss(class) => Self::MapLoss(MapLoss {
                class,
                raw: Some(raw()),
                failed_thread,
            }),
            other => Self::Other(other),
        }
    }

    pub fn classification(&self) -> LoadFailure {
        match self {
            Self::Other(failure) => *failure,
            Self::MapLoss(loss) => LoadFailure::Loss(loss.class()),
            Self::ContentLoss(loss) => LoadFailure::Loss(loss.class()),
        }
    }

    pub fn as_str(&self) -> &'static str {
        self.classification().as_str()
    }
}

impl From<LoadFailure> for ObservedLoadError {
    fn from(failure: LoadFailure) -> Self {
        Self::Other(failure)
    }
}

pub trait BlobSource: Sync {
    fn fetch(&self, hash: &BlobHash) -> impl Future<Output = trc::Result<Option<Vec<u8>>>> + Send;

    #[cfg(feature = "test_mode")]
    fn count_fetch(&self, _account_id: u32) {}
}

impl BlobSource for Server {
    async fn fetch(&self, hash: &BlobHash) -> trc::Result<Option<Vec<u8>>> {
        self.get_blob(hash.as_slice(), 0..usize::MAX).await
    }

    #[cfg(feature = "test_mode")]
    fn count_fetch(&self, account_id: u32) {
        crate::za::test_hooks::count_za_search_blob_read(account_id);
    }
}

pub fn classify_blob_error(err: &trc::Error) -> LoadFailure {
    if err.matches(EventType::Store(StoreEvent::DecryptError)) {
        if err.value(Key::Id).is_some() {
            LoadFailure::Transient(TransientClass::CodecKey)
        } else {
            LoadFailure::Loss(LossClass::Parse)
        }
    } else if err.matches(EventType::Store(StoreEvent::DecompressError)) {
        LoadFailure::Loss(LossClass::Parse)
    } else {
        LoadFailure::Transient(TransientClass::BlobStore)
    }
}

pub fn absence_class(first_seen: u64, now: u64, window_secs: u64) -> LoadFailure {
    if now.saturating_sub(first_seen) < window_secs {
        LoadFailure::Transient(TransientClass::AbsentWithinWindow)
    } else {
        LoadFailure::Loss(LossClass::BlobAbsent)
    }
}

pub async fn fetch_verified<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    hash: &BlobHash,
    window_secs: u64,
    now: u64,
) -> Result<Vec<u8>, LoadFailure> {
    #[cfg(feature = "test_mode")]
    source.count_fetch(entry.account_id);

    let Some(bytes) = source
        .fetch(hash)
        .await
        .map_err(|err| classify_blob_error(&err))?
    else {
        return Err(absence_class(
            entry.absent_since(hash, now),
            now,
            window_secs,
        ));
    };
    entry.seen(hash);
    if BlobHash::generate(&bytes) != *hash {
        return Err(LoadFailure::Loss(LossClass::Hash));
    }
    Ok(bytes)
}

pub fn classify_manifest(
    decoded: Result<ManifestValue, ManifestError>,
) -> Result<ManifestValue, LoadFailure> {
    decoded.map_err(|err| match err {
        ManifestError::Tag => LoadFailure::Loss(LossClass::Tag),
        ManifestError::Parse => LoadFailure::Loss(LossClass::Parse),
        ManifestError::OlderFormat(_) => LoadFailure::Loss(LossClass::OlderFormat),
        ManifestError::NewerFormat(_) => LoadFailure::NoMap(NoMapReason::NewerFormat),
    })
}

pub fn entry_outcome(value: Option<ManifestValue>) -> Result<Manifest, LoadFailure> {
    match value {
        Some(ManifestValue::Index(manifest)) => Ok(manifest),
        Some(ManifestValue::Destroying(_)) => Err(LoadFailure::NoMap(NoMapReason::Destroying)),
        None => Err(LoadFailure::Loss(LossClass::ManifestMissing)),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReloadPlan {
    Unchanged,
    Append {
        thread: Vec<ThreadBlobEntry>,
        content_head_moved: bool,
    },
    Whole,
}

pub fn plan_reload(loaded: &LoadedIndex, manifest: &Manifest) -> ReloadPlan {
    if &loaded.manifest == manifest {
        return ReloadPlan::Unchanged;
    }
    let held = &loaded.manifest.thread_blobs;
    // Reuse only a verified prefix, with its original hashes and order.
    if loaded.manifest.epoch != manifest.epoch
        || held.len() != loaded.thread_segments.len()
        || held
            .iter()
            .zip(&loaded.thread_segments)
            .any(|(descriptor, segment)| descriptor.segment != *segment)
        || !manifest.thread_blobs.starts_with(held)
    {
        return ReloadPlan::Whole;
    }
    ReloadPlan::Append {
        thread: manifest.thread_blobs[held.len()..].to_vec(),
        content_head_moved: loaded.manifest.content != manifest.content,
    }
}

struct ThreadDelta {
    blobs: Vec<(SegmentId, Option<HiddenSection>, PublicSection)>,
}

impl ThreadDelta {
    async fn read<S: BlobSource>(
        source: &S,
        entry: &IndexEntry,
        key: &Secret,
        descriptors: &[ThreadBlobEntry],
        window_secs: u64,
        now: u64,
    ) -> Result<Self, (LoadFailure, BlobHash)> {
        let mut blobs = Vec::with_capacity(descriptors.len());
        for descriptor in descriptors {
            let header = BlobHeader {
                kind: BlobKind::Thread,
                tier: 0,
                segment: descriptor.segment,
                predecessor: None,
            };
            let bytes = fetch_verified(source, entry, &descriptor.hash, window_secs, now)
                .await
                .map_err(|failure| (failure, descriptor.hash.clone()))?;
            let (hidden, public) = parse_thread_blob(key, entry.account_id, &header, &bytes)
                .map_err(|class| (LoadFailure::Loss(class), descriptor.hash.clone()))?;
            blobs.push((descriptor.segment, hidden, public));
        }
        Ok(Self { blobs })
    }

    fn apply(self, loaded: &mut LoadedIndex, row: RevisionRow) {
        for (segment, hidden, public) in self.blobs {
            let hidden = hidden.unwrap_or_default();
            loaded.thread.extend(
                hidden.records,
                hidden.aliases,
                public.rows,
                public.tombstones,
            );
            loaded.thread_segments.push(segment);
        }
        loaded.thread.extend([], [], [row], []);
        loaded.thread_bytes = loaded.thread.loaded_bytes() as u64;
    }
}

pub async fn load_thread_part<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    key: &Secret,
    manifest_bytes: &[u8],
    manifest: Manifest,
    window_secs: u64,
    now: u64,
) -> Result<LoadedIndex, ObservedLoadError> {
    let delta = ThreadDelta::read(source, entry, key, &manifest.thread_blobs, window_secs, now)
        .await
        .map_err(|(failure, hash)| {
            ObservedLoadError::map_read(failure, || manifest_bytes.to_vec(), Some(hash))
        })?;
    let row = manifest.newest_row;
    let mut loaded = LoadedIndex::empty(manifest);
    loaded.manifest_bytes_hash = manifest_hash(manifest_bytes);
    delta.apply(&mut loaded, row);
    Ok(loaded)
}

struct ContentWalk {
    nodes: Vec<(SegmentId, Vec<ContentRecord>)>,
    joined: bool,
}

impl ContentWalk {
    async fn read<S: BlobSource>(
        source: &S,
        entry: &IndexEntry,
        key: &Secret,
        head: Option<&ContentHead>,
        stop_at: impl Fn(&BlobHash, SegmentId) -> bool,
        window_secs: u64,
        now: u64,
    ) -> Result<Self, LoadFailure> {
        let mut nodes = Vec::new();
        let mut visited = AHashSet::new();
        let mut next = head.map(|head| (head.hash.clone(), head.segment));
        while let Some((hash, segment)) = next {
            if !visited.insert(segment) {
                return Err(LoadFailure::Loss(LossClass::Parse));
            }
            if stop_at(&hash, segment) {
                return Ok(Self {
                    nodes,
                    joined: true,
                });
            }
            let bytes = fetch_verified(source, entry, &hash, window_secs, now).await?;
            let (header, records) =
                parse_content_blob(key, entry.account_id, Some(segment), None, &bytes)
                    .map_err(LoadFailure::Loss)?;
            nodes.push((segment, records));
            next = header.predecessor;
        }
        Ok(Self {
            nodes,
            joined: false,
        })
    }

    fn apply(self, content: &mut LoadedContent) {
        if !self.joined {
            content.part = ContentPart::default();
            content.segments.clear();
        }
        let mut segments = self
            .nodes
            .iter()
            .map(|(segment, _)| *segment)
            .collect::<Vec<_>>();
        segments.append(&mut content.segments);
        content.segments = segments;
        // The chain is stored head first. Later records replace earlier ones.
        for (_, records) in self.nodes.into_iter().rev() {
            content.part.extend(records);
        }
        content.bytes = content.part.loaded_bytes() as u64;
    }
}

fn empty_content() -> LoadedContent {
    LoadedContent {
        part: ContentPart::default(),
        segments: Vec::new(),
        bytes: 0,
        charge: BudgetCharge::detached(),
    }
}

fn apply_tombstones(content: &mut LoadedContent, thread: &ThreadPart) {
    for record in thread.records().filter(|record| record.deleted.is_some()) {
        content.part.remove(record.document_id);
    }
    content.bytes = content.part.loaded_bytes() as u64;
}

pub async fn load_content<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    key: &Secret,
    manifest: &Manifest,
    stop_at: Option<&[SegmentId]>,
    window_secs: u64,
    now: u64,
) -> Result<LoadedContent, LoadFailure> {
    let walk = ContentWalk::read(
        source,
        entry,
        key,
        manifest.content.as_ref(),
        |_, segment| stop_at.is_some_and(|held| held.contains(&segment)),
        window_secs,
        now,
    )
    .await?;
    let mut content = empty_content();
    walk.apply(&mut content);
    if let Some(index) = entry.index.read().await.as_ref() {
        apply_tombstones(&mut content, &index.thread);
    }
    Ok(content)
}

pub fn thread_budget_outcome(fits: bool) -> Option<&'static str> {
    (!fits).then_some(ZA_SEARCH_OVER_BUDGET)
}

fn reserve_thread(
    cache: &ZaSearchCache,
    descriptors: &[ThreadBlobEntry],
    account_id: u32,
) -> BudgetCharge {
    let bytes = descriptors
        .iter()
        .map(|descriptor| u64::from(descriptor.size))
        .sum::<u64>()
        * 3;
    let charge = cache
        .admit(PartKind::Thread, bytes, account_id)
        .expect("thread budget is soft");
    let budget = cache.budget(PartKind::Thread);
    let used = cache.thread_used();
    if let Some(reason) = thread_budget_outcome(used <= budget) {
        za_search_trace!(reason, AccountId = account_id, Limit = budget, Size = used);
    }
    charge
}

async fn load_entry<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    key: &Secret,
    config: &ZaSearchConfig,
    read_manifest: impl Future<Output = Result<(Vec<u8>, Manifest), ObservedLoadError>>,
) -> Result<(), ObservedLoadError> {
    // A resident thread part is available even while content is loading.
    if entry.index.read().await.is_some() {
        return Ok(());
    }
    let _flight = entry.load_flight.lock().await;
    if entry.index.read().await.is_some() {
        return Ok(());
    }

    let cache = entry
        .cache()
        .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
    let (bytes, manifest) = read_manifest.await?;
    let charge = reserve_thread(&cache, &manifest.thread_blobs, entry.account_id);
    let mut index = load_thread_part(
        source,
        entry,
        key,
        &bytes,
        manifest,
        config.absent_retry_secs,
        now(),
    )
    .await?;
    index.thread_charge = charge;
    cache.grow(
        &mut index.thread_charge,
        PartKind::Thread,
        index.thread_bytes,
        entry.account_id,
    );
    *entry.index.write().await = Some(index);
    Ok(())
}

fn content_reservation(head: &ContentHead, config: &ZaSearchConfig) -> Result<u64, LoadFailure> {
    if head.size > config.account_max_mb.saturating_mul(1 << 20) {
        Err(LoadFailure::OverCap)
    } else {
        Ok(head.size.saturating_mul(3))
    }
}

fn content_head_is_resident(index: &LoadedIndex) -> bool {
    index.content.as_ref().is_some_and(|content| {
        content.segments.first() == index.manifest.content.as_ref().map(|head| &head.segment)
    })
}

fn matches_head(head: Option<&ContentHead>, hash: &BlobHash, segment: SegmentId) -> bool {
    head.is_some_and(|head| head.segment == segment && head.hash == *hash)
}

async fn load_current_content<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    key: &Secret,
    config: &ZaSearchConfig,
) -> Result<(), ObservedLoadError> {
    let _flight = entry.load_flight.lock().await;
    let cache = entry
        .cache()
        .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
    let budget = LoadFailure::Transient(TransientClass::Budget);
    let invalid = LoadFailure::Transient(TransientClass::Validation);
    let mut staged = empty_content();
    let mut staged_head: Option<(u64, ContentHead)> = None;

    loop {
        let (version, epoch, head) = {
            let guard = entry.index.read().await;
            let index = guard.as_ref().ok_or(invalid)?;
            if let Some(head) = &index.manifest.content {
                content_reservation(head, config)?;
            }
            if content_head_is_resident(index) {
                return Ok(());
            }
            (
                index.manifest_bytes_hash,
                index.manifest.epoch,
                index.manifest.content.clone(),
            )
        };

        if head.is_none()
            || staged_head
                .as_ref()
                .is_some_and(|(old_epoch, _)| *old_epoch != epoch)
        {
            staged = empty_content();
            staged_head = None;
        }

        let result = async {
            let Some(head) = head.as_ref() else {
                return Ok(());
            };
            let reserved = content_reservation(head, config)?;
            if staged_head.is_none() {
                staged.charge = cache
                    .admit(PartKind::Content, reserved, entry.account_id)
                    .ok_or(budget)?;
            } else if !cache.grow(
                &mut staged.charge,
                PartKind::Content,
                staged.bytes.saturating_add(reserved),
                entry.account_id,
            ) {
                return Err(budget);
            }
            let stop = staged_head.as_ref().map(|(_, head)| head);
            ContentWalk::read(
                source,
                entry,
                key,
                Some(head),
                |hash, segment| matches_head(stop, hash, segment),
                config.absent_retry_secs,
                now(),
            )
            .await?
            .apply(&mut staged);
            if !cache.grow(
                &mut staged.charge,
                PartKind::Content,
                staged.bytes,
                entry.account_id,
            ) {
                return Err(budget);
            }
            Ok(())
        }
        .await;
        if result.is_err() {
            staged = empty_content();
        }

        let mut guard = entry.index.write().await;
        let index = guard.as_mut().ok_or(invalid)?;
        if index.manifest.epoch != epoch || index.manifest.content != head {
            // A pass can advance the manifest while this load is reading blobs.
            // Keep a verified suffix only for the same epoch and exact descriptor.
            staged_head = if result.is_ok() {
                head.map(|head| (epoch, head))
            } else {
                None
            };
            drop(guard);
            tokio::task::yield_now().await;
            continue;
        }
        result.map_err(|failure| match failure {
            LoadFailure::Loss(class) => ObservedLoadError::ContentLoss(ContentLoss {
                class,
                manifest_hash: version,
                epoch,
                head: head.expect("content loss requires a named head"),
            }),
            other => ObservedLoadError::Other(other),
        })?;
        apply_tombstones(&mut staged, &index.thread);
        cache.grow(
            &mut staged.charge,
            PartKind::Content,
            staged.bytes,
            entry.account_id,
        );
        index.content = Some(staged);
        return Ok(());
    }
}

pub async fn apply_reload<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    key: &Secret,
    config: &ZaSearchConfig,
    manifest_bytes: &[u8],
    manifest: Manifest,
    now: u64,
) -> Result<(), LoadFailure> {
    let _flight = entry.load_flight.lock().await;
    let mut guard = entry.index.write().await;
    apply_reload_locked(
        source,
        entry,
        &mut guard,
        key,
        config,
        manifest_bytes,
        manifest,
        now,
    )
    .await
    .map(|_| ())
    .map_err(|failure| failure.classification())
}

#[allow(clippy::too_many_arguments)]
async fn apply_reload_locked<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    loaded: &mut Option<LoadedIndex>,
    key: &Secret,
    config: &ZaSearchConfig,
    manifest_bytes: &[u8],
    manifest: Manifest,
    now: u64,
) -> Result<bool, ObservedLoadError> {
    let cache = entry
        .cache()
        .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
    let old = loaded
        .as_mut()
        .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
    let hash = manifest_hash(manifest_bytes);
    if old.manifest_bytes_hash == hash {
        return Ok(false);
    }

    let account_id = entry.account_id;
    let window = config.absent_retry_secs;
    let plan = plan_reload(old, &manifest);
    let (thread, whole, content_head_moved) = match &plan {
        ReloadPlan::Unchanged => (&[][..], false, false),
        ReloadPlan::Append {
            thread,
            content_head_moved,
        } => (thread.as_slice(), false, *content_head_moved),
        ReloadPlan::Whole => (manifest.thread_blobs.as_slice(), true, true),
    };
    let thread_charge = if whole || !thread.is_empty() {
        Some(reserve_thread(&cache, thread, account_id))
    } else {
        None
    };
    let delta = ThreadDelta::read(source, entry, key, thread, window, now)
        .await
        .map_err(|(failure, hash)| {
            ObservedLoadError::map_read(failure, || manifest_bytes.to_vec(), Some(hash))
        })?;

    let mut new_content = None;
    if let Some(head) = manifest.content.as_ref()
        && old.content.is_some()
        && (content_head_moved || !content_head_is_resident(old))
    {
        let stop = if !whole && content_head_is_resident(old) {
            old.manifest.content.as_ref()
        } else {
            // Free the previous part before the whole-chain reservation.
            old.content = None;
            None
        };
        let content = async {
            let bytes = content_reservation(head, config)?;
            let charge = cache
                .admit(PartKind::Content, bytes, account_id)
                .ok_or(LoadFailure::Transient(TransientClass::Budget))?;
            let walk = ContentWalk::read(
                source,
                entry,
                key,
                Some(head),
                |hash, segment| matches_head(stop, hash, segment),
                window,
                now,
            )
            .await?;
            Ok((walk, charge))
        }
        .await;
        match content {
            Ok(staged) => new_content = Some(staged),
            Err(LoadFailure::Transient(TransientClass::Budget) | LoadFailure::OverCap) => {
                old.content = None;
                trace_eviction(account_id, EvictReason::Budget);
            }
            // Keep content-only loss out of the thread load contract. An
            // explicit content load retries this head and reports its loss.
            Err(LoadFailure::Loss(_)) => old.content = None,
            Err(err) => return Err(err.into()),
        }
    }

    let scanned_to = old.scanned_to.max(scanned_seed(&manifest.indexed_to));
    let row = manifest.newest_row;
    if whole {
        *old = LoadedIndex::empty(manifest);
    } else {
        old.manifest = manifest;
    }
    if let Some(charge) = thread_charge {
        old.thread_charge.absorb(charge);
    }
    delta.apply(old, row);
    cache.grow(
        &mut old.thread_charge,
        PartKind::Thread,
        old.thread_bytes,
        account_id,
    );
    old.manifest_bytes_hash = hash;
    old.scanned_to = scanned_to;
    if old.manifest.content.is_none() {
        old.content = None;
    }

    if let Some((walk, charge)) = new_content {
        if walk.joined {
            let content = old.content.as_mut().expect("joined resident suffix");
            content.charge.absorb(charge);
            walk.apply(content);
            apply_tombstones(content, &old.thread);
        } else {
            let mut content = empty_content();
            content.charge = charge;
            walk.apply(&mut content);
            apply_tombstones(&mut content, &old.thread);
            old.content = Some(content);
        }
    } else if let Some(content) = old.content.as_mut() {
        apply_tombstones(content, &old.thread);
    }
    cache.admit_content_growth(old, account_id);
    Ok(true)
}

impl Server {
    pub async fn za_search_read_manifest_raw(
        &self,
        account_id: u32,
    ) -> trc::Result<Option<Vec<u8>>> {
        self.store()
            .get_value::<RawValue>(ValueKey::property(
                account_id,
                Collection::Principal,
                0,
                PrincipalField::ZeroAccessSearch,
            ))
            .await
            .map(|value| value.map(|raw| raw.0))
    }

    pub async fn za_search_read_manifest(
        &self,
        account_id: u32,
        key: &ZaSearchKey,
    ) -> Result<Option<(Vec<u8>, ManifestValue)>, ObservedLoadError> {
        #[cfg(feature = "test_mode")]
        if crate::za::test_hooks::take_search_manifest_fail(account_id) {
            return Err(LoadFailure::Transient(TransientClass::Store).into());
        }
        self.za_search_read_manifest_raw(account_id)
            .await
            .map_err(|_| LoadFailure::Transient(TransientClass::Store))?
            .map(
                |bytes| match classify_manifest(ManifestValue::decode(&bytes, key)) {
                    Ok(value) => Ok((bytes, value)),
                    Err(failure) => Err(ObservedLoadError::map_read(failure, || bytes, None)),
                },
            )
            .transpose()
    }

    async fn read_search_index_manifest(
        &self,
        account_id: u32,
        key: &Secret,
    ) -> Result<(Vec<u8>, Manifest), ObservedLoadError> {
        let Some((bytes, value)) = self
            .za_search_read_manifest(account_id, &za_search_key(key))
            .await?
        else {
            return Err(ObservedLoadError::MapLoss(MapLoss::missing()));
        };
        Ok((bytes, entry_outcome(Some(value))?))
    }

    pub async fn za_search_entry(
        &self,
        account_id: u32,
        keys: &SessionKeys,
    ) -> Result<Arc<IndexEntry>, ObservedLoadError> {
        let entry = self.inner.cache.za_search.entry(account_id);
        self.za_search_load_entry(&entry, keys).await?;
        Ok(entry)
    }

    /// Load the chosen entry without switching to another cache entry after
    /// the caller has acquired this entry's pass lock.
    pub async fn za_search_load_entry(
        &self,
        entry: &IndexEntry,
        keys: &SessionKeys,
    ) -> Result<(), ObservedLoadError> {
        let key = keys.swk();
        load_entry(
            self,
            entry,
            key,
            &self.za_search_config(),
            self.read_search_index_manifest(entry.account_id, key),
        )
        .await
    }

    pub async fn za_search_reload_if_moved(
        &self,
        entry: &IndexEntry,
        keys: &SessionKeys,
    ) -> Result<bool, ObservedLoadError> {
        let key = keys.swk();
        let _flight = entry.load_flight.lock().await;
        // Serialize the manifest read with applying it. A delayed reload can
        // otherwise overwrite a newer local swap with an older manifest.
        let mut guard = entry.index.write().await;
        let (bytes, manifest) = self
            .read_search_index_manifest(entry.account_id, key)
            .await?;
        apply_reload_locked(
            self,
            entry,
            &mut guard,
            key,
            &self.za_search_config(),
            &bytes,
            manifest,
            now(),
        )
        .await
    }

    pub async fn za_search_load_content(
        &self,
        entry: &IndexEntry,
        keys: &SessionKeys,
    ) -> Result<(), ObservedLoadError> {
        load_current_content(self, entry, keys.swk(), &self.za_search_config()).await
    }
}
