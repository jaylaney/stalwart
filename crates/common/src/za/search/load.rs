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
    seal::{open_content_blob, parse_thread_blob, za_search_key},
    trace::ZA_SEARCH_OVER_BUDGET,
};
#[cfg(feature = "test_mode")]
use crate::za::test_hooks::{SearchStage, note_search_count, observe_search_wait, search_span};
use crate::{Server, auth::vault::RawValue, za_search_trace};
use ahash::AHashSet;
use std::{future::Future, sync::Arc};
use store::{
    ValueKey,
    write::now,
    za_search::{
        BlobHeader, BlobKind, SegmentId, ZaSearchKey,
        blob::{ContentFramesError, HiddenSection, PublicSection, decode_content_frames},
        content::{ContentPart, OwnedContentFrame},
        manifest::{
            ContentHead, Manifest, ManifestError, ManifestValue, ThreadBlobEntry, manifest_hash,
        },
        thread::{RevisionRow, ThreadPart},
    },
};
use trc::{EventType, Key, StoreEvent};
use types::{blob_hash::BlobHash, collection::Collection, field::PrincipalField};
use vault::{Zeroizing, keys::Secret, session::SessionKeys};

// Preserve the original wait and caller-owned guard. Only the test build
// wraps the future, so cancellation still drops the same pending operation.
macro_rules! observe_wait {
    ($account:expr, $stage:ident, $future:expr) => {{
        #[cfg(feature = "test_mode")]
        let future = observe_search_wait($account, SearchStage::$stage, $future);
        #[cfg(not(feature = "test_mode"))]
        let future = $future;
        future.await
    }};
}

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
    let _span = search_span(entry.account_id, SearchStage::VerifiedBlobFetch);
    #[cfg(feature = "test_mode")]
    source.count_fetch(entry.account_id);

    let fetched = source.fetch(hash).await;
    #[cfg(feature = "test_mode")]
    let fetched = if crate::za::test_hooks::take_search_blob_timeout(entry.account_id, hash) {
        // Match the S3 adapter's mapping of an exhausted HTTP RequestTimeout.
        Err(StoreEvent::S3Error
            .reason("S3 returned status 408 RequestTimeout")
            .ctx(Key::Code, 408u16))
    } else {
        fetched
    };

    let Some(bytes) = fetched.map_err(|err| classify_blob_error(&err))? else {
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
            #[cfg(feature = "test_mode")]
            let _span = search_span(entry.account_id, SearchStage::ThreadOpenDecode);
            let (hidden, public) = parse_thread_blob(key, entry.account_id, &header, &bytes)
                .map_err(|class| (LoadFailure::Loss(class), descriptor.hash.clone()))?;
            blobs.push((descriptor.segment, hidden, public));
        }
        Ok(Self { blobs })
    }

    fn has_deletions(&self) -> bool {
        self.blobs.iter().any(|(_, hidden, public)| {
            !public.tombstones.is_empty()
                || hidden.as_ref().is_some_and(|hidden| {
                    hidden.records.iter().any(|record| record.deleted.is_some())
                })
        })
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
    #[cfg(feature = "test_mode")]
    let _span = search_span(entry.account_id, SearchStage::ThreadModelBuild);
    delta.apply(&mut loaded, row);
    Ok(loaded)
}

/// The verified prefix of the exact committed content chain a pass is replacing.
/// The cursor starts at the manifest's authenticated head and advances only
/// through authenticated predecessor pointers.
pub struct ContentChainReader<'a> {
    base: &'a LoadedIndex,
    next: Option<(BlobHash, SegmentId)>,
    remaining_len: u32,
    remaining_size: u64,
    visited: AHashSet<SegmentId>,
}

pub struct VerifiedContentNode {
    hash: BlobHash,
    header: BlobHeader,
    size: u64,
    base_hash: [u8; 32],
    account_id: u32,
}

impl VerifiedContentNode {
    pub fn hash(&self) -> &BlobHash {
        &self.hash
    }

    pub fn header(&self) -> &BlobHeader {
        &self.header
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

impl<'a> ContentChainReader<'a> {
    pub fn new(base: &'a LoadedIndex) -> Option<Self> {
        let head = base.manifest.content.as_ref()?;
        Some(Self {
            base,
            next: Some((head.hash.clone(), head.segment)),
            remaining_len: head.length,
            remaining_size: head.size,
            visited: AHashSet::new(),
        })
    }

    pub fn next_input(&self) -> Option<(&BlobHash, u64)> {
        self.next
            .as_ref()
            .map(|(hash, _)| (hash, self.remaining_size))
    }

    pub async fn next<S: BlobSource>(
        &mut self,
        source: &S,
        entry: &IndexEntry,
        window_secs: u64,
    ) -> Result<Option<VerifiedContentNode>, ObservedLoadError> {
        let Some((hash, segment)) = self.next.take() else {
            return Ok(None);
        };
        self.read(source, entry, window_secs, hash, segment)
            .await
            .map(Some)
            .map_err(|failure| self.observed(failure))
    }

    async fn read<S: BlobSource>(
        &mut self,
        source: &S,
        entry: &IndexEntry,
        window_secs: u64,
        hash: BlobHash,
        segment: SegmentId,
    ) -> Result<VerifiedContentNode, LoadFailure> {
        let invalid = LoadFailure::Loss(LossClass::Parse);
        if !self.visited.insert(segment) {
            return Err(invalid);
        }
        let bytes = fetch_verified(source, entry, &hash, window_secs, now()).await?;
        let size = bytes.len() as u64;
        self.remaining_size = self.remaining_size.checked_sub(size).ok_or(invalid)?;
        self.remaining_len = self.remaining_len.checked_sub(1).ok_or(invalid)?;
        // The expected digest covers the header. Authenticate the AEAD and
        // decode the frames later, only for selected inputs.
        let (header, _) = BlobHeader::decode(&bytes).map_err(|_| invalid)?;
        if header.kind != BlobKind::Content
            || header.segment != segment
            || header.predecessor.is_none() != (self.remaining_len == 0)
            || header.predecessor.is_none() != (self.remaining_size == 0)
        {
            return Err(invalid);
        }
        self.next = header.predecessor.clone();
        Ok(VerifiedContentNode {
            hash,
            header,
            size,
            base_hash: self.base.manifest_bytes_hash,
            account_id: entry.account_id,
        })
    }

    pub async fn records<S: BlobSource>(
        &self,
        source: &S,
        entry: &IndexEntry,
        key: &Secret,
        window_secs: u64,
        node: &VerifiedContentNode,
        mut reserve: impl FnMut(u64) -> bool,
    ) -> Result<Vec<OwnedContentFrame>, ObservedLoadError> {
        if node.base_hash != self.base.manifest_bytes_hash || node.account_id != entry.account_id {
            return Err(LoadFailure::Transient(TransientClass::Validation).into());
        }
        if !reserve(content_read_bytes(node.size)) {
            return Err(LoadFailure::Transient(TransientClass::Budget).into());
        }
        let bytes = fetch_verified(source, entry, &node.hash, window_secs, now())
            .await
            .map_err(|failure| self.observed(failure))?;
        open_frames(key, entry.account_id, node.header.segment, bytes, reserve)
            .map(|(_, records)| records)
            .map_err(|failure| self.observed(failure))
    }

    fn observed(&self, failure: LoadFailure) -> ObservedLoadError {
        match failure {
            LoadFailure::Loss(class) => ObservedLoadError::ContentLoss(ContentLoss {
                class,
                manifest_hash: self.base.manifest_bytes_hash,
                epoch: self.base.manifest.epoch,
                head: self.base.manifest.content.clone().expect("named chain"),
            }),
            other => ObservedLoadError::Other(other),
        }
    }
}

// Sealed bytes and the opened section coexist only while authenticating.
fn content_read_bytes(sealed_size: u64) -> u64 {
    sealed_size.saturating_mul(2).saturating_add(128)
}

fn open_frames(
    key: &Secret,
    account_id: u32,
    segment: SegmentId,
    bytes: Vec<u8>,
    mut reserve: impl FnMut(u64) -> bool,
) -> Result<(BlobHeader, Vec<OwnedContentFrame>), LoadFailure> {
    #[cfg(feature = "test_mode")]
    let opening = search_span(account_id, SearchStage::ContentOpen);
    let (header, plaintext) = open_content_blob(key, account_id, Some(segment), None, &bytes)
        .map_err(LoadFailure::Loss)?;
    #[cfg(feature = "test_mode")]
    drop(opening);
    drop(bytes);
    #[cfg(feature = "test_mode")]
    let _decoding = search_span(account_id, SearchStage::ContentDecode);
    let records = decode_content_frames(&plaintext, |additional| {
        reserve((plaintext.capacity() as u64).saturating_add(additional as u64))
    })
    .map_err(|error| match error {
        ContentFramesError::Format(_) => LoadFailure::Loss(LossClass::Parse),
        ContentFramesError::Budget => LoadFailure::Transient(TransientClass::Budget),
    })?;
    Ok((header, records))
}

pub fn content_frame_bytes(records: &[OwnedContentFrame]) -> u64 {
    records
        .iter()
        .map(|record| record.bytes().len() as u64)
        .sum::<u64>()
}

struct ContentWalk {
    nodes: Vec<(SegmentId, Vec<OwnedContentFrame>)>,
    joined: bool,
    memory: u64,
}

impl ContentWalk {
    #[allow(clippy::too_many_arguments)]
    async fn read<S: BlobSource>(
        source: &S,
        entry: &IndexEntry,
        key: &Secret,
        head: Option<&ContentHead>,
        stop_at: impl Fn(&BlobHash, SegmentId) -> bool,
        window_secs: u64,
        now: u64,
        mut reserve: impl FnMut(u64) -> bool,
    ) -> Result<Self, LoadFailure> {
        let budget = LoadFailure::Transient(TransientClass::Budget);
        let mut nodes = Vec::new();
        let mut visited = AHashSet::new();
        let mut next = head.map(|head| (head.hash.clone(), head.segment));
        let mut remaining = head.map_or(0, |head| head.size);
        let mut memory = 0u64;
        while let Some((hash, segment)) = next {
            // Leave room for the node/segment containers and their growth.
            memory = memory.saturating_add(
                4 * (size_of::<(SegmentId, Vec<OwnedContentFrame>)>()
                    + 3 * size_of::<SegmentId>()
                    + 1) as u64,
            );
            if !reserve(memory) {
                return Err(budget);
            }
            if !visited.insert(segment) {
                return Err(LoadFailure::Loss(LossClass::Parse));
            }
            if stop_at(&hash, segment) {
                return Ok(Self {
                    nodes,
                    joined: true,
                    memory,
                });
            }
            if !reserve(memory.saturating_add(content_read_bytes(remaining))) {
                return Err(budget);
            }
            let bytes = fetch_verified(source, entry, &hash, window_secs, now).await?;
            remaining = remaining
                .checked_sub(bytes.len() as u64)
                .ok_or(LoadFailure::Loss(LossClass::Parse))?;
            let (header, records) = open_frames(key, entry.account_id, segment, bytes, |needed| {
                reserve(memory.saturating_add(needed))
            })?;
            memory = memory
                .saturating_add(content_frame_bytes(&records))
                .saturating_add((records.capacity() * size_of::<OwnedContentFrame>()) as u64);
            nodes.push((segment, records));
            next = header.predecessor;
        }
        Ok(Self {
            nodes,
            joined: false,
            memory,
        })
    }

    fn apply(
        self,
        content: &mut LoadedContent,
        cache: &ZaSearchCache,
        account_id: u32,
    ) -> Result<(), LoadFailure> {
        #[cfg(feature = "test_mode")]
        let _span = search_span(account_id, SearchStage::Postings);
        let budget = LoadFailure::Transient(TransientClass::Budget);
        if !self.joined {
            content.part = ContentPart::default();
            content.segments.clear();
        }
        let mut segments = Vec::with_capacity(self.nodes.len() + content.segments.len());
        segments.extend(self.nodes.iter().map(|(segment, _)| *segment));
        segments.append(&mut content.segments);
        content.segments = segments;
        let mut memory = self.memory;
        // The chain is stored head first. Later records replace earlier ones.
        for (_, records) in self.nodes.into_iter().rev() {
            let metadata = (records.capacity() * size_of::<OwnedContentFrame>()) as u64;
            for record in records {
                let frame_bytes = record.bytes().len() as u64;
                let before = memory.saturating_add(content.part.loaded_bytes() as u64);
                if !content.part.insert_frame(record, |extra| {
                    cache.grow(
                        &mut content.charge,
                        PartKind::Content,
                        before.saturating_add(extra as u64),
                        account_id,
                    )
                }) {
                    return Err(budget);
                }
                memory -= frame_bytes;
            }
            memory -= metadata;
        }
        content.bytes = content.part.loaded_bytes() as u64;
        Ok(())
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

fn apply_tombstones<I: Iterator<Item = u32>>(
    content: &mut LoadedContent,
    deletions: impl Fn() -> I,
    cache: &ZaSearchCache,
    account_id: u32,
) -> Result<(), LoadFailure> {
    let scratch_size = deletions()
        .map(|document_id| content.part.removal_scratch(document_id))
        .max()
        .unwrap_or_default();
    if !cache.grow(
        &mut content.charge,
        PartKind::Content,
        (content.part.loaded_bytes() as u64).saturating_add(scratch_size as u64),
        account_id,
    ) {
        return Err(LoadFailure::Transient(TransientClass::Budget));
    }
    let mut scratch = Zeroizing::new(Vec::with_capacity(scratch_size));
    for document_id in deletions() {
        content.part.remove_with_scratch(document_id, &mut scratch);
    }
    drop(scratch);
    content.bytes = content.part.loaded_bytes() as u64;
    cache.grow(
        &mut content.charge,
        PartKind::Content,
        content.bytes,
        account_id,
    );
    Ok(())
}

#[cfg(test)]
pub async fn load_content<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    key: &Secret,
    manifest: &Manifest,
    stop_at: Option<&[SegmentId]>,
    window_secs: u64,
    now: u64,
) -> Result<LoadedContent, LoadFailure> {
    let cache = entry
        .cache()
        .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
    let mut content = empty_content();
    let walk = ContentWalk::read(
        source,
        entry,
        key,
        manifest.content.as_ref(),
        |_, segment| stop_at.is_some_and(|held| held.contains(&segment)),
        window_secs,
        now,
        |bytes| {
            cache.grow(
                &mut content.charge,
                PartKind::Content,
                bytes,
                entry.account_id,
            )
        },
    )
    .await?;
    walk.apply(&mut content, &cache, entry.account_id)?;
    if let Some(index) = entry.index.read().await.as_ref() {
        apply_tombstones(
            &mut content,
            || {
                index
                    .thread
                    .records()
                    .filter_map(|record| record.deleted.map(|_| record.document_id))
            },
            &cache,
            entry.account_id,
        )?;
    } else {
        cache.grow(
            &mut content.charge,
            PartKind::Content,
            content.bytes,
            entry.account_id,
        );
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
    if observe_wait!(entry.account_id, MapIndexWait, entry.index.read()).is_some() {
        return Ok(());
    }
    let _flight = observe_wait!(entry.account_id, MapFlightWait, entry.load_flight.lock());
    if observe_wait!(entry.account_id, MapIndexWait, entry.index.read()).is_some() {
        return Ok(());
    }

    let cache = entry
        .cache()
        .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
    let (bytes, manifest) = read_manifest.await?;
    let index = load_observed_thread(source, entry, key, config, &bytes, manifest, &cache).await?;
    *observe_wait!(entry.account_id, MapIndexWait, entry.index.write()) = Some(index);
    Ok(())
}

async fn load_observed_thread<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    key: &Secret,
    config: &ZaSearchConfig,
    bytes: &[u8],
    manifest: Manifest,
    cache: &ZaSearchCache,
) -> Result<LoadedIndex, ObservedLoadError> {
    let charge = reserve_thread(cache, &manifest.thread_blobs, entry.account_id);
    let mut index = load_thread_part(
        source,
        entry,
        key,
        bytes,
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
    Ok(index)
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

fn same_content_target(index: &LoadedIndex, epoch: u64, head: Option<&ContentHead>) -> bool {
    index.manifest.epoch == epoch && index.manifest.content.as_ref() == head
}

struct ContentTombstones {
    version: [u8; 32],
    ids: Vec<u32>,
    _charge: Option<BudgetCharge>,
}

fn matching_tombstones<'a>(
    thread: &'a ThreadPart,
    content: &'a ContentPart,
) -> impl Iterator<Item = u32> + 'a {
    thread
        .records()
        .filter(|record| record.deleted.is_some() && content.contains(record.document_id))
        .map(|record| record.document_id)
}

async fn snapshot_content_tombstones(
    entry: &IndexEntry,
    cache: &ZaSearchCache,
    epoch: u64,
    head: Option<&ContentHead>,
    content: &ContentPart,
) -> Result<Option<ContentTombstones>, LoadFailure> {
    let invalid = LoadFailure::Transient(TransientClass::Validation);
    let (version, count) = {
        let guard = observe_wait!(entry.account_id, ContentIndexWait, entry.index.read());
        let index = guard.as_ref().ok_or(invalid)?;
        if !same_content_target(index, epoch, head) {
            return Ok(None);
        }
        (
            index.manifest_bytes_hash,
            matching_tombstones(&index.thread, content).count(),
        )
    };
    if count == 0 {
        return Ok(Some(ContentTombstones {
            version,
            ids: Vec::new(),
            _charge: None,
        }));
    }

    let charge = cache.admit(
        PartKind::Content,
        (count as u64).saturating_mul(size_of::<u32>() as u64),
        entry.account_id,
    );
    let guard = observe_wait!(entry.account_id, ContentIndexWait, entry.index.read());
    let index = guard.as_ref().ok_or(invalid)?;
    if index.manifest_bytes_hash != version || !same_content_target(index, epoch, head) {
        return Ok(None);
    }
    let charge = charge.ok_or(LoadFailure::Transient(TransientClass::Budget))?;
    let mut ids = Vec::with_capacity(count);
    ids.extend(matching_tombstones(&index.thread, content));
    Ok(Some(ContentTombstones {
        version,
        ids,
        _charge: Some(charge),
    }))
}

async fn install_content(
    entry: &IndexEntry,
    version: [u8; 32],
    epoch: u64,
    head: Option<&ContentHead>,
    staged: &mut LoadedContent,
    prepared: Result<(), LoadFailure>,
) -> Result<bool, LoadFailure> {
    #[cfg(feature = "test_mode")]
    let _span = search_span(entry.account_id, SearchStage::ExactGenerationInstall);
    let mut guard = observe_wait!(entry.account_id, ContentIndexWait, entry.index.write());
    let index = guard
        .as_mut()
        .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
    if index.manifest_bytes_hash != version || !same_content_target(index, epoch, head) {
        return Ok(false);
    }
    prepared?;
    let retired = index
        .content
        .replace(std::mem::replace(staged, empty_content()));
    drop(guard);
    drop(retired);
    Ok(true)
}

async fn load_current_content<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    key: &Secret,
    config: &ZaSearchConfig,
) -> Result<(), ObservedLoadError> {
    let _flight = observe_wait!(
        entry.account_id,
        ContentFlightWait,
        entry.content_load_flight.lock()
    );
    let cache = entry
        .cache()
        .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
    let budget = LoadFailure::Transient(TransientClass::Budget);
    let invalid = LoadFailure::Transient(TransientClass::Validation);
    let mut staged = empty_content();
    let mut staged_head: Option<(u64, ContentHead)> = None;

    loop {
        let (version, epoch, head, has_deferred) = {
            let guard = observe_wait!(entry.account_id, ContentIndexWait, entry.index.read());
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
                index.deferred_content.is_some(),
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

        let mut untouched_deferred = false;
        if head.is_some() && staged_head.is_none() && has_deferred {
            let mut guard = observe_wait!(entry.account_id, ContentIndexWait, entry.index.write());
            let index = guard.as_mut().ok_or(invalid)?;
            if index.manifest_bytes_hash != version {
                drop(guard);
                #[cfg(feature = "test_mode")]
                note_search_count(entry.account_id, SearchStage::ContentRetry);
                tokio::task::yield_now().await;
                continue;
            }
            if let Some((previous, content)) = index.deferred_content.take() {
                staged = content;
                staged_head = Some((epoch, previous));
                untouched_deferred = true;
            }
        }

        let had_suffix = staged_head.is_some();
        let result = async {
            let Some(head) = head.as_ref() else {
                return Ok(());
            };
            let stop = staged_head.as_ref().map(|(_, head)| head);
            // An unchanged head with newer Thread tombstones needs only pruning.
            if stop == Some(head) {
                return Ok(());
            }
            let reserved = content_reservation(head, config)?;
            if !had_suffix {
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
            let previous_bytes = staged.bytes;
            let walk = ContentWalk::read(
                source,
                entry,
                key,
                Some(head),
                |hash, segment| matches_head(stop, hash, segment),
                config.absent_retry_secs,
                now(),
                |bytes| {
                    cache.grow(
                        &mut staged.charge,
                        PartKind::Content,
                        previous_bytes.saturating_add(bytes.max(reserved)),
                        entry.account_id,
                    )
                },
            )
            .await?;
            untouched_deferred = false;
            walk.apply(&mut staged, &cache, entry.account_id)?;
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

        if let Err(failure) = result {
            if had_suffix && failure == budget {
                // A fresh load may fit once the old suffix is released.
                staged = empty_content();
                staged_head = None;
                trace_eviction(entry.account_id, EvictReason::Budget);
                #[cfg(feature = "test_mode")]
                note_search_count(entry.account_id, SearchStage::ContentRetry);
                continue;
            }

            let restore = untouched_deferred && matches!(failure, LoadFailure::Transient(_));
            if restore {
                cache.grow(
                    &mut staged.charge,
                    PartKind::Content,
                    staged.bytes,
                    entry.account_id,
                );
            }
            let mut guard = observe_wait!(entry.account_id, ContentIndexWait, entry.index.write());
            let index = guard.as_mut().ok_or(invalid)?;
            if !same_content_target(index, epoch, head.as_ref()) {
                drop(guard);
                staged = empty_content();
                staged_head = None;
                #[cfg(feature = "test_mode")]
                note_search_count(entry.account_id, SearchStage::ContentRetry);
                tokio::task::yield_now().await;
                continue;
            }
            if restore && index.manifest_bytes_hash == version {
                let (_, previous) = staged_head.expect("deferred suffix head");
                debug_assert!(index.deferred_content.is_none());
                index.deferred_content = Some((previous, staged));
            }
            drop(guard);
            return Err(match failure {
                LoadFailure::Loss(class) => ObservedLoadError::ContentLoss(ContentLoss {
                    class,
                    manifest_hash: version,
                    epoch,
                    head: head.expect("content loss requires a named head"),
                }),
                other => ObservedLoadError::Other(other),
            });
        }

        staged_head = head.clone().map(|head| (epoch, head));
        #[cfg(feature = "test_mode")]
        let tombstone_span = search_span(entry.account_id, SearchStage::TombstonePreparation);
        if let Some(tombstones) =
            snapshot_content_tombstones(entry, &cache, epoch, head.as_ref(), &staged.part).await?
        {
            let prepared = if tombstones.ids.is_empty() {
                Ok(())
            } else {
                apply_tombstones(
                    &mut staged,
                    || tombstones.ids.iter().copied(),
                    &cache,
                    entry.account_id,
                )
            };
            let version = tombstones.version;
            drop(tombstones);
            #[cfg(feature = "test_mode")]
            drop(tombstone_span);
            if install_content(entry, version, epoch, head.as_ref(), &mut staged, prepared).await? {
                return Ok(());
            }
        } else {
            #[cfg(feature = "test_mode")]
            drop(tombstone_span);
        }
        // A newer committed generation arrived during preparation. A same-head
        // retry just refreshes tombstones. An append can use the verified stage.
        #[cfg(feature = "test_mode")]
        note_search_count(entry.account_id, SearchStage::ContentRetry);
        tokio::task::yield_now().await;
    }
}

#[cfg(test)]
pub async fn apply_reload<S: BlobSource>(
    source: &S,
    entry: &IndexEntry,
    key: &Secret,
    config: &ZaSearchConfig,
    manifest_bytes: &[u8],
    manifest: Manifest,
    now: u64,
) -> Result<(), LoadFailure> {
    let flight = observe_wait!(entry.account_id, MapFlightWait, entry.load_flight.lock());
    let mut guard = observe_wait!(entry.account_id, MapIndexWait, entry.index.write());
    let result = apply_reload_locked(
        source,
        entry,
        &mut guard,
        key,
        config,
        manifest_bytes,
        manifest,
        now,
    )
    .await;
    drop(guard);
    drop(flight);
    result
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
) -> Result<(bool, Option<LoadedContent>), ObservedLoadError> {
    let cache = entry
        .cache()
        .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
    let old = loaded
        .as_mut()
        .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
    let hash = manifest_hash(manifest_bytes);
    if old.manifest_bytes_hash == hash {
        return Ok((false, None));
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

    // Move optional state by ownership only. It cannot participate in the
    // required reload or keep the previous Map installed.
    let content_allowed = manifest
        .content
        .as_ref()
        .is_some_and(|head| content_reservation(head, config).is_ok());
    let retired = if whole || !content_allowed {
        old.take_optional_content()
    } else if (content_head_moved || delta.has_deletions())
        && let Some(content) = old.content.take()
    {
        if let Some(head) = old.manifest.content.clone() {
            debug_assert!(old.deferred_content.is_none());
            old.deferred_content = Some((head, content));
            None
        } else {
            Some(content)
        }
    } else {
        None
    };

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
    Ok((true, retired))
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
        crate::za::test_hooks::count_search_pass_work(
            account_id,
            crate::za::test_hooks::SearchPassWork::Manifest,
        );
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
        let flight = observe_wait!(entry.account_id, MapFlightWait, entry.load_flight.lock());
        // Serialize the manifest read with applying it. A delayed reload can
        // otherwise overwrite a newer local swap with an older manifest.
        let mut guard = observe_wait!(entry.account_id, MapIndexWait, entry.index.write());
        let (bytes, manifest) = self
            .read_search_index_manifest(entry.account_id, key)
            .await?;
        let result = apply_reload_locked(
            self,
            entry,
            &mut guard,
            key,
            &self.za_search_config(),
            &bytes,
            manifest,
            now(),
        )
        .await;
        drop(guard);
        drop(flight);
        result.map(|(changed, _retired)| changed)
    }

    /// The account pass owns pass_lock. An unchanged authenticated resident head
    /// needs only shared access, so a served view can stay pinned through commit.
    /// Cold/moved heads reauthenticate under reload's load/index serialization.
    /// Every actual target is retained before Thread I/O; service APIs are unchanged.
    pub async fn za_search_load_for_pass(
        &self,
        entry: &IndexEntry,
        keys: &SessionKeys,
        observed: &mut Option<Manifest>,
    ) -> Result<(Vec<u8>, Manifest), ObservedLoadError> {
        {
            let guard = observe_wait!(entry.account_id, MapIndexWait, entry.index.read());
            if let Some(index) = guard.as_ref() {
                let (bytes, manifest) = self
                    .read_search_index_manifest(entry.account_id, keys.swk())
                    .await?;
                *observed = Some(manifest.clone());
                entry.defer_open_retry(&manifest, std::time::Instant::now());
                if index.manifest_bytes_hash == manifest_hash(&bytes) {
                    return Ok((bytes, manifest));
                }
            }
        }
        // Never apply the snapshot obtained before waiting for exclusive access:
        // a service reload or completed swap may have installed a newer head.
        let flight = observe_wait!(entry.account_id, MapFlightWait, entry.load_flight.lock());
        let mut guard = observe_wait!(entry.account_id, MapIndexWait, entry.index.write());
        let (bytes, manifest) = self
            .read_search_index_manifest(entry.account_id, keys.swk())
            .await?;
        *observed = Some(manifest.clone());
        // A successful observation resolves an unknown episode or meaningful
        // change before a later Thread failure can arm a new episode.
        entry.defer_open_retry(&manifest, std::time::Instant::now());
        let config = self.za_search_config();
        let retired = if guard.is_some() {
            apply_reload_locked(
                self,
                entry,
                &mut guard,
                keys.swk(),
                &config,
                &bytes,
                manifest.clone(),
                now(),
            )
            .await?
            .1
        } else {
            let cache = entry
                .cache()
                .ok_or(LoadFailure::Transient(TransientClass::Validation))?;
            *guard = Some(
                load_observed_thread(
                    self,
                    entry,
                    keys.swk(),
                    &config,
                    &bytes,
                    manifest.clone(),
                    &cache,
                )
                .await?,
            );
            None
        };
        drop(guard);
        drop(flight);
        drop(retired);
        Ok((bytes, manifest))
    }

    pub async fn za_search_load_content(
        &self,
        entry: &IndexEntry,
        keys: &SessionKeys,
    ) -> Result<(), ObservedLoadError> {
        load_current_content(self, entry, keys.swk(), &self.za_search_config()).await
    }
}
