/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    fields::za_thread_fields_k,
    open::{OpenFailure, ZaSearchMessageOpen, classify_open_error},
    write::write_pass,
};
use crate::{
    cache::MessageCacheFetch,
    message::{messagedata::MessageData, sortkeys::MessageSortKeys, za::view::za_has_attachment},
};
use common::{
    Server,
    za::search::{
        cache::{IndexEntry, LoadedIndex, PartKind, scanned_seed},
        error::{LoadFailure, LossClass, NoMapReason, TransientClass},
        seal::za_search_key,
        trace::{ZA_SEARCH_ABORTED, ZA_SEARCH_LOST, ZA_SEARCH_SKIPPED},
    },
    za_search_trace,
};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use store::{
    query::log::{Change, ZaChangeRecords},
    write::{SearchIndex, now},
    za_search::{
        Checkpoint, ThreadFieldsK,
        content::{ContentRecord, SortKeys},
        manifest::{FailingDocument, ManifestValue, manifest_hash},
        reservation,
        thread::{Alias, ThreadPart, ThreadRecord, Tombstone},
    },
};
use types::collection::SyncCollection;
use vault::session::SessionKeys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassOutcome {
    Current,
    Advanced {
        published: bool,
        indexed_to: Checkpoint,
    },
    Running,
    Aborted(TransientClass),
    Lost,
    NoMap(NoMapReason),
    Recovery(RecoveryCause),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryCause {
    Loss(LossClass),
    Truncated,
}

impl RecoveryCause {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Loss(class) => class.as_str(),
            Self::Truncated => "truncated",
        }
    }
}

impl From<LoadFailure> for PassOutcome {
    fn from(failure: LoadFailure) -> Self {
        match failure {
            LoadFailure::Loss(class) => Self::Recovery(RecoveryCause::Loss(class)),
            LoadFailure::Transient(class) => Self::Aborted(class),
            LoadFailure::NoMap(reason) => Self::NoMap(reason),
            LoadFailure::OverCap => unreachable!("the thread part is always admitted"),
        }
    }
}

pub(crate) struct PassLimits {
    pub cap: usize,
    pub hidden_bytes: usize,
    pub message_bytes: u64,
    pub elapsed: Duration,
    pub checkpoint: usize,
    pub skip_after: u8,
    pub content_allowed: bool,
}

pub(crate) struct Indexed {
    pub fields: ThreadFieldsK,
    pub content: Option<ContentRecord>,
    pub bytes: usize,
}

pub(crate) trait Opener {
    fn open(
        &mut self,
        document_id: u32,
    ) -> impl Future<Output = Result<Indexed, OpenFailure>> + Send;
}

#[derive(Clone, Copy)]
pub(crate) enum PassDecision {
    Nothing,
    ManifestOnly,
    NonPublishing,
    Publishing,
    ContentOnly,
    Abort {
        class: TransientClass,
        document_id: u32,
    },
}

#[derive(Clone, Copy)]
pub enum CursorStep {
    None,
    Advanced(u32),
    Exhausted,
}

#[derive(Clone, Copy)]
pub enum MarkerStep {
    Untouched,
    GapNext(u32),
    Finished,
}

pub(crate) struct Staging {
    pub revision: u32,
    pub records: Vec<ThreadRecord>,
    pub aliases: Vec<Alias>,
    pub tombstones: Vec<Tombstone>,
    pub content: Vec<(u32, ContentRecord)>,
    pub skipped: Vec<(u32, LossClass)>,
    transient_skips: Vec<(u32, TransientClass)>,
    pub checkpoint: Checkpoint,
    pub cursor: CursorStep,
    pub content_cursor: CursorStep,
    pub marker: MarkerStep,
    pub marker_bound: Option<u64>,
    pub suspend_content: bool,
    pub bookkeeping: bool,
    pub scanned_to: u64,
    pub update_records: usize,
    pub witness: Option<u32>,
    #[cfg(any(test, feature = "test_mode"))]
    pub max_item_aliases: usize,
    pub decision: PassDecision,
    pub failing: Option<FailingDocument>,
}

impl Staging {
    fn new(base: &LoadedIndex, marker_bound: Option<u64>) -> Self {
        Self {
            revision: base.manifest.revision,
            records: Vec::new(),
            aliases: Vec::new(),
            tombstones: Vec::new(),
            content: Vec::new(),
            skipped: Vec::new(),
            transient_skips: Vec::new(),
            checkpoint: base.manifest.indexed_to,
            cursor: CursorStep::None,
            content_cursor: CursorStep::None,
            marker: MarkerStep::Untouched,
            marker_bound,
            suspend_content: false,
            bookkeeping: false,
            scanned_to: base.scanned_to,
            update_records: 0,
            witness: None,
            #[cfg(any(test, feature = "test_mode"))]
            max_item_aliases: 0,
            decision: PassDecision::Nothing,
            failing: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct PassSources {
    pub cursor: Vec<u32>,
    pub content: Vec<u32>,
    pub content_runnable: bool,
    pub gap_inserts: Vec<u32>,
    pub gap_deletes: Vec<u32>,
    pub gap_stamp: Option<u64>,
    pub marker_bound: Option<u64>,
    pub finish_at: Option<u64>,
}

struct Planner<'a> {
    base: &'a LoadedIndex,
    limits: &'a PassLimits,
    started: Instant,
    thread: ThreadPart,
    staging: Staging,
    items: usize,
    reserved: usize,
    bytes: u64,
    consumed: bool,
}

impl Planner<'_> {
    fn capped(&self) -> bool {
        self.items > 0
            && (self.items >= self.limits.cap
                || self.reserved >= self.limits.hidden_bytes
                || self.bytes >= self.limits.message_bytes
                || self.started.elapsed() >= self.limits.elapsed)
    }

    // None as a creation stamp means a content backfill of an existing identity.
    async fn admit(
        &mut self,
        document_id: u32,
        created: Option<u64>,
        opener: &mut impl Opener,
    ) -> bool {
        self.items += 1;
        self.reserved += reservation();
        match opener.open(document_id).await {
            Ok(indexed) => {
                self.bytes += indexed.bytes as u64;
                if let Some(created) = created {
                    let assignment = self.thread.assign(
                        &indexed.fields,
                        document_id,
                        self.base.manifest.revision + 1,
                        created,
                    );
                    #[cfg(any(test, feature = "test_mode"))]
                    {
                        self.staging.max_item_aliases =
                            self.staging.max_item_aliases.max(assignment.aliases.len());
                    }
                    self.staging.records.push(
                        self.thread
                            .record(document_id)
                            .expect("an assigned document")
                            .clone(),
                    );
                    self.staging.aliases.extend(assignment.aliases);
                    self.staging.witness = Some(
                        self.staging
                            .witness
                            .map_or(document_id, |w| w.min(document_id)),
                    );
                }
                if let Some(content) = indexed.content {
                    self.staging.content.push((document_id, content));
                }
            }
            Err(OpenFailure::Loss(class)) => self.staging.skipped.push((document_id, class)),
            Err(OpenFailure::Transient(class)) => {
                let attempts = self
                    .base
                    .manifest
                    .failing
                    .as_ref()
                    .filter(|f| f.document_id == document_id)
                    .map_or(1, |f| f.attempts.saturating_add(1));
                if attempts >= self.limits.skip_after {
                    let loss = if class == TransientClass::AbsentWithinWindow {
                        LossClass::BlobAbsent
                    } else {
                        LossClass::Parse
                    };
                    self.staging.skipped.push((document_id, loss));
                    self.staging.transient_skips.push((document_id, class));
                } else {
                    let mut aborted = Staging::new(self.base, self.staging.marker_bound);
                    aborted.suspend_content = self.staging.suspend_content;
                    aborted.decision = PassDecision::Abort { class, document_id };
                    aborted.failing = Some(FailingDocument {
                        document_id,
                        attempts,
                    });
                    self.staging = aborted;
                    return false;
                }
            }
        }
        self.consumed = true;
        true
    }

    fn delete(&mut self, document_id: u32, deleted: u64) {
        if self.thread.exists_at(document_id, u64::MAX) == Some(true) {
            self.thread.tombstone(document_id, deleted);
            self.staging.tombstones.push(Tombstone {
                document_id,
                deleted,
            });
        }
        self.consumed = true;
    }

    async fn read_sources(&mut self, sources: &PassSources, opener: &mut impl Opener) -> bool {
        let manifest = &self.base.manifest;
        if manifest.cursor.is_some() {
            let mut remaining = sources.cursor.iter().copied().peekable();
            let mut next = None;
            while let Some(document_id) = remaining.next_if(|_| !self.capped()) {
                if !self.admit(document_id, Some(manifest.epoch), opener).await {
                    return false;
                }
                next = Some(document_id.saturating_add(1));
            }
            self.staging.cursor = if remaining.peek().is_none() {
                CursorStep::Exhausted
            } else {
                CursorStep::Advanced(next.expect("the first item is admitted"))
            };
            if matches!(self.staging.cursor, CursorStep::Advanced(_)) {
                return true;
            }
        }

        if manifest.content_to.cursor.is_some() {
            if !self.limits.content_allowed {
                self.staging.suspend_content = !manifest.content_to.suspended;
            } else if sources.content_runnable {
                let mut remaining = sources.content.iter().copied().peekable();
                let mut last = None;
                while let Some(document_id) = remaining.next_if(|_| !self.capped()) {
                    if !self.admit(document_id, None, opener).await {
                        return false;
                    }
                    last = Some(document_id);
                }
                self.staging.content_cursor = if remaining.peek().is_none() {
                    CursorStep::Exhausted
                } else if let Some(last) = last {
                    CursorStep::Advanced(last)
                } else {
                    CursorStep::None
                };
                if remaining.peek().is_some() {
                    return true;
                }
            }
        }

        // Task 14 supplies the gap view. Until then leave any existing marker untouched.
        if manifest.reconciliation.is_some()
            && let Some(stamp) = sources.gap_stamp
        {
            for &document_id in &sources.gap_inserts {
                if self.capped() {
                    return true;
                }
                if !self.admit(document_id, Some(stamp), opener).await {
                    return false;
                }
                self.staging.marker = MarkerStep::GapNext(document_id.saturating_add(1));
            }
            for &document_id in &sources.gap_deletes {
                self.delete(document_id, stamp);
            }
        }
        true
    }

    async fn read_records(&mut self, records: &ZaChangeRecords, opener: &mut impl Opener) -> bool {
        if self.staging.checkpoint.complete
            && self.base.scanned_to > self.staging.checkpoint.change_id
        {
            self.staging.checkpoint = complete_checkpoint(self.base.scanned_to);
        }
        for (change_id, changes) in &records.records {
            let change_id = *change_id;
            let checkpoint = self.staging.checkpoint;
            let mut position = (change_id == checkpoint.change_id && !checkpoint.complete)
                .then_some(checkpoint.position);
            let mut inserts = Vec::new();
            let mut deletes = Vec::new();
            for change in changes {
                match *change {
                    Change::InsertItem(id) if position.is_none_or(|p| id as u32 > p) => {
                        inserts.push(id as u32)
                    }
                    Change::DeleteItem(id) => deletes.push(id as u32),
                    _ => {}
                }
            }
            if inserts.is_empty() && deletes.is_empty() {
                self.staging.update_records += 1;
            } else {
                inserts.sort_unstable();
                for document_id in inserts {
                    if self.thread.exists_at(document_id, u64::MAX).is_some() {
                        self.consumed = true;
                    } else if self.capped() {
                        // A fresh record without a processed insert has no position
                        // yet. Position zero would incorrectly consume document 0.
                        if let Some(position) = position {
                            self.staging.checkpoint = Checkpoint {
                                change_id,
                                position,
                                complete: false,
                            };
                        }
                        return true;
                    } else if !self.admit(document_id, Some(change_id), opener).await {
                        return false;
                    }
                    position = Some(document_id);
                }
                deletes.sort_unstable();
                for document_id in deletes {
                    self.delete(document_id, change_id);
                }
            }
            self.staging.checkpoint = complete_checkpoint(change_id);
        }
        true
    }

    fn finish(mut self, sources: &PassSources) -> Staging {
        let staging = &mut self.staging;
        if self.base.manifest.reconciliation.is_some()
            && sources.gap_inserts.is_empty()
            && sources.gap_deletes.is_empty()
            && sources
                .finish_at
                .is_some_and(|finish| staging.checkpoint.change_id >= finish)
        {
            staging.marker = MarkerStep::Finished;
        }
        staging.bookkeeping = matches!(staging.cursor, CursorStep::Exhausted)
            || matches!(staging.content_cursor, CursorStep::Exhausted)
            || matches!(staging.marker, MarkerStep::Finished);
        staging.decision = if !staging.records.is_empty() {
            staging.revision += 1;
            staging.suspend_content |=
                !self.limits.content_allowed && !self.base.manifest.content_to.suspended;
            PassDecision::Publishing
        } else if !staging.tombstones.is_empty() {
            PassDecision::NonPublishing
        } else if !staging.content.is_empty() {
            PassDecision::ContentOnly
        } else if self.consumed
            || staging.bookkeeping
            || matches!(staging.cursor, CursorStep::Advanced(_))
            || matches!(staging.content_cursor, CursorStep::Advanced(_))
            || matches!(staging.marker, MarkerStep::GapNext(_))
            || staging.marker_bound.is_some()
            || staging.suspend_content
            || self.base.update_records + staging.update_records >= self.limits.checkpoint
        {
            PassDecision::ManifestOnly
        } else {
            PassDecision::Nothing
        };
        staging.scanned_to = scanned_seed(&staging.checkpoint);
        self.staging
    }
}

fn complete_checkpoint(change_id: u64) -> Checkpoint {
    Checkpoint {
        change_id,
        position: 0,
        complete: true,
    }
}

pub(crate) async fn plan_pass(
    base: &LoadedIndex,
    sources: PassSources,
    records: &ZaChangeRecords,
    limits: &PassLimits,
    started: Instant,
    opener: &mut impl Opener,
) -> Staging {
    let mut planner = Planner {
        base,
        limits,
        started,
        thread: base.thread.clone(),
        staging: Staging::new(base, sources.marker_bound),
        items: 0,
        reserved: 0,
        bytes: 0,
        consumed: false,
    };
    if !planner.read_sources(&sources, opener).await
        || (planner.items == 0 && !planner.consumed && !planner.read_records(records, opener).await)
    {
        return planner.staging;
    }
    planner.finish(&sources)
}

struct ServerOpener<'a> {
    server: &'a Server,
    account_id: u32,
    keys: &'a SessionKeys,
    entry: &'a IndexEntry,
    content_allowed: bool,
    absent_retry_secs: u64,
}

impl Opener for ServerOpener<'_> {
    async fn open(&mut self, document_id: u32) -> Result<Indexed, OpenFailure> {
        let opened = self
            .server
            .za_open_for_index(
                self.account_id,
                document_id,
                self.keys,
                self.entry,
                self.absent_retry_secs,
                now(),
            )
            .await?;
        let parsed = opened.parse().ok_or(OpenFailure::Loss(LossClass::Parse))?;
        let fields = za_thread_fields_k(&parsed);
        let content = if self.content_allowed {
            let row = opened
                .row
                .unarchive()
                .map_err(|err| classify_open_error(&err))?;
            let headers = opened
                .row
                .raw_headers()
                .map_err(|err| classify_open_error(&err))?;
            let email = &self.server.core.email;
            let index_fields = email
                .index_fields
                .get(&SearchIndex::Email)
                .ok_or(OpenFailure::Transient(TransientClass::Validation))?;
            let document = row.index_document(
                self.account_id,
                document_id,
                &headers,
                opened.raw.as_slice(),
                index_fields,
                email.default_language,
                &email.extract_limits,
            );
            Some(ContentRecord::new(
                document,
                document_id,
                SortKeys::from(&MessageSortKeys::from_message(&parsed)),
                Some(za_effective_sent_at(fields.sent_at, opened.received_at)),
                za_has_attachment(row),
            ))
        } else {
            None
        };
        Ok(Indexed {
            fields,
            content,
            bytes: opened.bytes,
        })
    }
}

impl From<&MessageSortKeys> for SortKeys {
    fn from(keys: &MessageSortKeys) -> Self {
        Self {
            from: keys.from.clone(),
            to: keys.to.clone(),
            subject: keys.subject.clone(),
        }
    }
}

pub(crate) fn za_effective_sent_at(date: Option<i64>, received_at: u64) -> i64 {
    received_at as i64 + i64::from(MessageData::sent_at_offset(date, received_at))
}

pub trait ZaSearchPass: Sync + Send {
    fn za_search_pass(
        &self,
        account_id: u32,
        keys: &SessionKeys,
    ) -> impl Future<Output = trc::Result<PassOutcome>> + Send;
}

impl ZaSearchPass for Server {
    async fn za_search_pass(
        &self,
        account_id: u32,
        keys: &SessionKeys,
    ) -> trc::Result<PassOutcome> {
        let config = self.za_search_config();
        let entry = match self.za_search_entry(account_id, keys).await {
            Ok(entry) => entry,
            Err(failure) => return Ok(failure.into()),
        };
        let Ok(_pass) = entry.pass_lock.try_lock() else {
            return Ok(PassOutcome::Running);
        };
        let cache = self.get_cached_messages(account_id).await?;
        let loaded_hash = {
            let Some(index) = entry.read_loaded().await else {
                return Ok(PassOutcome::Lost);
            };
            let manifest = &index.manifest;
            if manifest.cursor.is_none()
                && manifest.reconciliation.is_none()
                && manifest.content_to.cursor.is_none()
                && manifest.indexed_to.complete
                && cache.last_change_id <= index.scanned_to
            {
                return Ok(PassOutcome::Current);
            }
            index.manifest_bytes_hash
        };

        #[cfg(feature = "test_mode")]
        common::za::test_hooks::search_sweep_pause_point(account_id).await;

        let key = za_search_key(keys.swk());
        let mut read = self.za_search_read_manifest(account_id, &key).await;
        if let Ok(Some((bytes, ManifestValue::Index(_)))) = &read
            && manifest_hash(bytes) != loaded_hash
        {
            if let Err(failure) = self.za_search_reload_if_moved(&entry, keys).await {
                return Ok(failure.into());
            }
            read = self.za_search_read_manifest(account_id, &key).await;
        }
        let bytes = match read {
            Ok(Some((bytes, ManifestValue::Index(_)))) => bytes,
            Ok(Some((_, ManifestValue::Destroying(_)))) => {
                return Ok(PassOutcome::NoMap(NoMapReason::Destroying));
            }
            Ok(None) => {
                return Ok(PassOutcome::Recovery(RecoveryCause::Loss(
                    LossClass::ManifestMissing,
                )));
            }
            Err(failure) => return Ok(failure.into()),
        };
        drop(key);
        let hash = manifest_hash(&bytes);
        let search_cache = &self.inner.cache.za_search;
        let base = {
            let Some(index) = entry.read_loaded().await else {
                return Ok(PassOutcome::Lost);
            };
            if index.manifest_bytes_hash != hash {
                za_search_trace!(ZA_SEARCH_LOST, AccountId = account_id);
                return Ok(PassOutcome::Lost);
            }
            let thread_charge = search_cache
                .admit(PartKind::Thread, index.thread_bytes, account_id)
                .expect("a thread part is always admitted");
            LoadedIndex {
                manifest_bytes_hash: hash,
                manifest: index.manifest.clone(),
                thread: index.thread.clone(),
                thread_segments: Vec::new(),
                content: None,
                scanned_to: index.scanned_to,
                update_records: index.update_records,
                thread_bytes: index.thread_bytes,
                thread_charge,
            }
        };
        let cursor = base
            .manifest
            .cursor
            .as_ref()
            .map_or_else(Vec::new, |cursor| {
                let mut ids = cache
                    .emails
                    .document_ids()
                    .filter(|d| cursor.next <= *d && *d <= cursor.end)
                    .collect::<Vec<_>>();
                ids.sort_unstable();
                ids
            });
        let records = if cursor.is_empty() {
            let checkpoint = if base.scanned_to > base.manifest.indexed_to.change_id {
                complete_checkpoint(base.scanned_to)
            } else {
                base.manifest.indexed_to
            };
            self.store()
                .za_changes_records(
                    account_id,
                    SyncCollection::Email,
                    Some(checkpoint),
                    config.checkpoint.max(config.pass_cap.saturating_add(1)),
                )
                .await?
        } else {
            ZaChangeRecords::default()
        };
        if records.truncated {
            return Ok(PassOutcome::Recovery(RecoveryCause::Truncated));
        }
        let limits = PassLimits {
            cap: config.pass_cap,
            hidden_bytes: config.pass_hidden_kb * 1024,
            message_bytes: config.pass_mb << 20,
            elapsed: Duration::from_millis(config.pass_ms),
            checkpoint: config.checkpoint,
            skip_after: config.skip_after,
            content_allowed: !base.manifest.content_to.suspended
                && base.manifest.content.as_ref().map_or(0, |c| c.size)
                    < config.account_max_mb << 20,
        };
        let mut opener = ServerOpener {
            server: self,
            account_id,
            keys,
            entry: &entry,
            content_allowed: limits.content_allowed,
            absent_retry_secs: config.absent_retry_secs,
        };
        let staging = {
            let _overlay_charge = search_cache
                .admit(PartKind::Thread, base.thread_bytes, account_id)
                .expect("a thread part is always admitted");
            plan_pass(
                &base,
                PassSources {
                    cursor,
                    ..PassSources::default()
                },
                &records,
                &limits,
                Instant::now(),
                &mut opener,
            )
            .await
        };
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::note_za_search_item_aliases(account_id, staging.max_item_aliases);

        for (document_id, class) in &staging.skipped {
            let reason = staging
                .transient_skips
                .iter()
                .find_map(|(id, transient)| (id == document_id).then(|| transient.as_str()))
                .unwrap_or_else(|| class.as_str());
            za_search_trace!(
                ZA_SEARCH_SKIPPED,
                AccountId = account_id,
                DocumentId = *document_id,
                Reason = reason
            );
        }
        if let PassDecision::Abort { class, document_id } = staging.decision {
            debug_assert_eq!(
                staging.failing.as_ref().map(|f| f.document_id),
                Some(document_id)
            );
            za_search_trace!(
                ZA_SEARCH_ABORTED,
                AccountId = account_id,
                Reason = class.as_str()
            );
        }
        if matches!(staging.decision, PassDecision::Nothing) {
            let mut guard = entry.index.write().await;
            if let Some(index) = guard.as_mut()
                && index.manifest_bytes_hash == hash
            {
                index.scanned_to = index.scanned_to.max(staging.scanned_to);
                index.update_records += staging.update_records;
            }
            Ok(PassOutcome::Current)
        } else {
            write_pass(
                self,
                account_id,
                keys,
                &entry,
                &bytes,
                hash,
                base.manifest,
                staging,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use store::{
        search::IndexDocument,
        za_search::manifest::{ContentCursor, Manifest, RebuildCursor, ReconciliationMarker},
    };

    #[derive(Default)]
    struct FakeOpener(HashMap<u32, Result<Indexed, OpenFailure>>);

    impl Opener for FakeOpener {
        async fn open(&mut self, document_id: u32) -> Result<Indexed, OpenFailure> {
            self.0.remove(&document_id).expect("unexpected open")
        }
    }

    fn fields(document_id: u32) -> ThreadFieldsK {
        ThreadFieldsK {
            hashes: vec![document_id.into()],
            subject_hash: document_id.into(),
            sent_at: None,
        }
    }

    fn indexed(document_id: u32) -> Indexed {
        Indexed {
            fields: fields(document_id),
            content: None,
            bytes: 1000,
        }
    }

    fn opener(ids: impl IntoIterator<Item = u32>) -> FakeOpener {
        FakeOpener(ids.into_iter().map(|d| (d, Ok(indexed(d)))).collect())
    }

    fn complete(change_id: u64) -> Checkpoint {
        Checkpoint {
            change_id,
            position: 0,
            complete: true,
        }
    }

    fn partial(change_id: u64, position: u32) -> Checkpoint {
        Checkpoint {
            change_id,
            position,
            complete: false,
        }
    }

    fn manifest_at(indexed_to: Checkpoint) -> Manifest {
        Manifest {
            indexed_to,
            revision: 3,
            ..Manifest::default()
        }
    }

    fn base() -> LoadedIndex {
        LoadedIndex::empty(manifest_at(complete(10)))
    }

    fn limits() -> PassLimits {
        PassLimits {
            cap: 4,
            hidden_bytes: 32 * 1024,
            message_bytes: 32 << 20,
            elapsed: Duration::from_secs(2),
            checkpoint: 1000,
            skip_after: 8,
            content_allowed: true,
        }
    }

    fn singleton(d: u32) -> u64 {
        (u64::from(d) << 32) | u64::from(d)
    }

    fn inserts(ids: impl IntoIterator<Item = u32>) -> Vec<Change> {
        ids.into_iter()
            .map(|d| Change::InsertItem(singleton(d)))
            .collect()
    }

    fn records(records: Vec<(u64, Vec<Change>)>) -> ZaChangeRecords {
        ZaChangeRecords {
            records,
            ..ZaChangeRecords::default()
        }
    }

    async fn plan(
        base: &LoadedIndex,
        records: &ZaChangeRecords,
        limits: &PassLimits,
        opener: &mut FakeOpener,
    ) -> Staging {
        plan_pass(
            base,
            PassSources::default(),
            records,
            limits,
            Instant::now(),
            opener,
        )
        .await
    }

    fn ids(staged: &Staging) -> Vec<u32> {
        staged.records.iter().map(|r| r.document_id).collect()
    }

    #[tokio::test]
    async fn equal_public_inputs_stop_at_the_same_checkpoint_whatever_the_probe_merged() {
        let base = base();
        let limits = PassLimits {
            cap: 48,
            hidden_bytes: 38 * reservation() - 1,
            ..limits()
        };
        let records = records(
            (1..=39)
                .map(|d| (u64::from(d) + 10, inserts([d])))
                .collect(),
        );
        for joined in [true, false] {
            let mut opener = opener(1..=38);
            let sixth = &mut opener.0.get_mut(&6).unwrap().as_mut().unwrap().fields;
            sixth.subject_hash = 5;
            if joined {
                sixth.hashes.push(5);
            }
            let probe = &mut opener.0.get_mut(&38).unwrap().as_mut().unwrap().fields;
            probe.subject_hash = 5;
            probe.hashes.extend([5, 6]);

            let staged = plan(&base, &records, &limits, &mut opener).await;
            assert_eq!(staged.checkpoint, complete(48));
            assert_eq!(staged.scanned_to, 48);
            assert_eq!(staged.witness, Some(1));
            assert_eq!(staged.aliases.len(), usize::from(!joined));
        }
    }

    #[tokio::test]
    async fn a_record_larger_than_the_cap_is_left_part_way() {
        let mut base = base();
        let mut changes = inserts((1..=10).rev());
        changes.push(Change::DeleteItem(singleton(20)));
        base.thread.assign(&fields(20), 20, 3, 9);
        let records = records(vec![(11, changes)]);
        let mut opener = opener(1..=10);

        let first = plan(&base, &records, &limits(), &mut opener).await;
        assert_eq!(ids(&first), [1, 2, 3, 4]);
        assert_eq!(first.checkpoint, partial(11, 4));
        assert!(first.tombstones.is_empty());

        base.manifest.indexed_to = first.checkpoint;
        let second = plan(&base, &records, &limits(), &mut opener).await;
        assert_eq!(ids(&second), [5, 6, 7, 8]);

        // The cap can bind before the first insert of the next whole record.
        let fresh = self::base();
        let records = self::records(vec![(11, inserts([5])), (12, inserts([0]))]);
        let cap1 = PassLimits { cap: 1, ..limits() };
        let first = plan(&fresh, &records, &cap1, &mut self::opener([5])).await;
        assert_eq!(first.checkpoint, complete(11));
        let resumed = LoadedIndex::empty(manifest_at(first.checkpoint));
        let second = plan(
            &resumed,
            &self::records(vec![(12, inserts([0]))]),
            &cap1,
            &mut self::opener([0]),
        )
        .await;
        assert_eq!(ids(&second), [0]);
    }

    #[tokio::test]
    async fn a_cap_sized_unreadable_prefix_is_consumed_and_the_suffix_follows() {
        let records = records(vec![(11, inserts(1..=6))]);
        let mut opener = opener(5..=6);
        for id in 1..=4 {
            opener
                .0
                .insert(id, Err(OpenFailure::Loss(LossClass::BlobAbsent)));
        }
        let first = plan(&base(), &records, &limits(), &mut opener).await;
        assert!(first.records.is_empty());
        assert_eq!(first.skipped.len(), 4);
        assert!(matches!(first.decision, PassDecision::ManifestOnly));
        assert_eq!(first.checkpoint, partial(11, 4));
        assert_eq!(first.scanned_to, 10);
        let resumed = LoadedIndex::empty(manifest_at(first.checkpoint));
        let second = plan(&resumed, &records, &limits(), &mut opener).await;
        assert_eq!(ids(&second), [5, 6]);
        assert_eq!(second.checkpoint, complete(11));
        assert_eq!(second.scanned_to, 11);
    }

    #[test]
    fn effective_sent_at_is_upstreams() {
        assert_eq!(za_effective_sent_at(None, 100), 100);
        assert_eq!(za_effective_sent_at(Some(200), 100), 200);
        assert_eq!(
            za_effective_sent_at(Some(i64::MAX), 100),
            100 + i32::MAX as i64
        );
        assert_eq!(
            za_effective_sent_at(Some(i64::MIN), 100),
            100 + i32::MIN as i64
        );
    }

    #[test]
    fn scanned_to_never_covers_a_partial_checkpoint() {
        assert_eq!(scanned_seed(&partial(11, 4)), 10);
        assert_eq!(scanned_seed(&complete(11)), 11);
        assert_eq!(
            LoadedIndex::empty(manifest_at(partial(11, 4))).scanned_to,
            10
        );
    }

    #[tokio::test]
    async fn a_partial_record_finishes_then_applies_its_deletions() {
        let mut base = LoadedIndex::empty(manifest_at(partial(11, 3)));
        base.thread.assign(&fields(9), 9, 3, 9);
        let mut changes = inserts(1..=5);
        changes.push(Change::DeleteItem(singleton(9)));
        let staged = plan(
            &base,
            &records(vec![(11, changes)]),
            &limits(),
            &mut opener(4..=5),
        )
        .await;
        assert_eq!(ids(&staged), [4, 5]);
        assert_eq!(
            staged.tombstones,
            [Tombstone {
                document_id: 9,
                deleted: 11
            }]
        );
        assert_eq!(staged.checkpoint, complete(11));
    }

    #[tokio::test]
    async fn the_witness_is_the_lowest_indexed_id_whatever_threaded() {
        let mut base = base();
        base.thread.assign(&fields(2), 2, 3, 9);
        let records = records(vec![(11, inserts([20, 13])), (12, inserts([7]))]);
        for threaded in [true, false] {
            let mut opener = opener([7, 13, 20]);
            if threaded {
                opener.0.get_mut(&7).unwrap().as_mut().unwrap().fields = fields(2);
            }
            let staged = plan(&base, &records, &limits(), &mut opener).await;
            assert_eq!(staged.witness, Some(7));
            assert_eq!(staged.records[2].thread_id, if threaded { 2 } else { 7 });
        }
    }

    #[tokio::test]
    async fn a_logged_deletion_is_applied_unconditionally() {
        let mut base = base();
        base.thread.assign(&fields(3), 3, 3, 9);
        for document_id in [3, 4] {
            let staged = plan(
                &base,
                &records(vec![(11, vec![Change::DeleteItem(singleton(document_id))])]),
                &limits(),
                &mut opener([]),
            )
            .await;
            if document_id == 3 {
                assert_eq!(
                    staged.tombstones,
                    [Tombstone {
                        document_id: 3,
                        deleted: 11
                    }]
                );
                assert!(matches!(staged.decision, PassDecision::NonPublishing));
            } else {
                assert!(staged.tombstones.is_empty());
                assert!(matches!(staged.decision, PassDecision::ManifestOnly));
            }
        }
    }

    fn updates(from: u64, to: u64) -> ZaChangeRecords {
        records(
            (from..=to)
                .map(|id| (id, vec![Change::UpdateItem(singleton(1))]))
                .collect(),
        )
    }

    #[tokio::test]
    async fn the_decision_follows_what_changed() {
        let mut base = base();
        let limits = limits();
        let few = plan(&base, &updates(11, 15), &limits, &mut opener([])).await;
        assert!(matches!(few.decision, PassDecision::Nothing));
        assert_eq!(few.scanned_to, 15);
        assert!(matches!(few.cursor, CursorStep::None) && !few.bookkeeping);

        let many = plan(&base, &updates(11, 1010), &limits, &mut opener([])).await;
        assert!(matches!(many.decision, PassDecision::ManifestOnly));

        let records = records(vec![(11, inserts([1]))]);
        let lost = Err(OpenFailure::Loss(LossClass::Aead));
        let skip = plan(
            &base,
            &records,
            &limits,
            &mut FakeOpener(HashMap::from([(1, lost)])),
        )
        .await;
        assert!(matches!(skip.decision, PassDecision::ManifestOnly));
        assert_eq!(skip.skipped, [(1, LossClass::Aead)]);

        let publish = plan(&base, &records, &limits, &mut opener([1])).await;
        assert!(matches!(publish.decision, PassDecision::Publishing));
        assert_eq!(publish.revision, 4);

        base.manifest.cursor = Some(RebuildCursor { next: 4, end: 5 });
        let mut lost = FakeOpener(HashMap::from([
            (4, Err(OpenFailure::Loss(LossClass::StubAbsent))),
            (5, Err(OpenFailure::Loss(LossClass::StubAbsent))),
        ]));
        let cursor = plan_pass(
            &base,
            PassSources {
                cursor: vec![4, 5],
                ..Default::default()
            },
            &records,
            &limits,
            Instant::now(),
            &mut lost,
        )
        .await;
        assert!(matches!(cursor.decision, PassDecision::ManifestOnly));
        assert!(matches!(cursor.cursor, CursorStep::Exhausted) && cursor.bookkeeping);
    }

    #[tokio::test]
    async fn a_transient_open_aborts_with_the_attempt_count() {
        let mut base = base();
        let records = records(vec![(11, inserts([1, 2, 3]))]);
        let fixture = || {
            let mut opener = opener([1, 3]);
            opener
                .0
                .insert(2, Err(OpenFailure::Transient(TransientClass::BlobStore)));
            opener
        };
        let first = plan(&base, &records, &limits(), &mut fixture()).await;
        assert!(matches!(
            first.decision,
            PassDecision::Abort {
                class: TransientClass::BlobStore,
                document_id: 2
            }
        ));
        assert_eq!(
            first.failing,
            Some(FailingDocument {
                document_id: 2,
                attempts: 1
            })
        );
        assert!(first.records.is_empty());
        assert_eq!(first.checkpoint, complete(10));

        base.manifest.failing = Some(FailingDocument {
            document_id: 2,
            attempts: 7,
        });
        let last = plan(&base, &records, &limits(), &mut fixture()).await;
        assert_eq!(last.skipped, [(2, LossClass::Parse)]);
        assert_eq!(ids(&last), [1, 3]);
        assert_eq!(last.failing, None);
    }

    #[tokio::test]
    async fn cursor_items_come_first_and_take_the_epoch_as_creation() {
        let mut base = base();
        base.manifest.epoch = 100;
        base.manifest.cursor = Some(RebuildCursor { next: 3, end: 6 });
        let sources = || PassSources {
            cursor: vec![3, 5, 6],
            ..Default::default()
        };
        let records = records(vec![(11, inserts([10]))]);
        let staged = plan_pass(
            &base,
            sources(),
            &records,
            &limits(),
            Instant::now(),
            &mut opener([3, 5, 6]),
        )
        .await;
        assert_eq!(
            staged
                .records
                .iter()
                .map(|r| (r.document_id, r.created))
                .collect::<Vec<_>>(),
            [(3, 100), (5, 100), (6, 100)]
        );
        assert!(matches!(staged.cursor, CursorStep::Exhausted));
        assert_eq!(staged.checkpoint, complete(10));
        let cap2 = PassLimits { cap: 2, ..limits() };
        let capped = plan_pass(
            &base,
            sources(),
            &records,
            &cap2,
            Instant::now(),
            &mut opener([3, 5]),
        )
        .await;
        assert!(matches!(capped.cursor, CursorStep::Advanced(6)) && !capped.bookkeeping);

        let mut failing = opener([3]);
        failing
            .0
            .insert(5, Err(OpenFailure::Transient(TransientClass::BlobStore)));
        let aborted = plan_pass(
            &base,
            sources(),
            &records,
            &limits(),
            Instant::now(),
            &mut failing,
        )
        .await;
        assert!(matches!(aborted.cursor, CursorStep::None) && !aborted.bookkeeping);
        assert!(matches!(aborted.decision, PassDecision::Abort { .. }));

        base.manifest.cursor = Some(RebuildCursor { next: 49, end: 60 });
        let empty = plan(
            &base,
            &ZaChangeRecords::default(),
            &limits(),
            &mut opener([]),
        )
        .await;
        assert!(matches!(empty.cursor, CursorStep::Exhausted) && empty.bookkeeping);
        assert!(matches!(empty.decision, PassDecision::ManifestOnly));
    }

    fn content_opener(ids: impl IntoIterator<Item = u32>) -> FakeOpener {
        FakeOpener(
            ids.into_iter()
                .map(|d| {
                    let mut item = indexed(d);
                    item.content = Some(ContentRecord::new(
                        IndexDocument::new(SearchIndex::Email),
                        d,
                        SortKeys::default(),
                        None,
                        false,
                    ));
                    (d, Ok(item))
                })
                .collect(),
        )
    }

    #[tokio::test]
    async fn content_sources_stage_content_only() {
        let mut base = base();
        base.manifest.content_to.cursor = Some(ContentCursor {
            next_below: u32::MAX,
        });
        let cap2 = PassLimits { cap: 2, ..limits() };
        let records = ZaChangeRecords::default();
        let sources = |content| PassSources {
            content,
            content_runnable: true,
            ..Default::default()
        };
        let first = plan_pass(
            &base,
            sources(vec![9, 7, 4]),
            &records,
            &cap2,
            Instant::now(),
            &mut content_opener([9, 7]),
        )
        .await;
        assert_eq!(first.content.len(), 2);
        assert!(first.records.is_empty() && first.aliases.is_empty() && first.witness.is_none());
        assert!(matches!(first.decision, PassDecision::ContentOnly));
        assert!(matches!(first.content_cursor, CursorStep::Advanced(7)));

        let resumed = plan_pass(
            &base,
            sources(vec![4]),
            &records,
            &cap2,
            Instant::now(),
            &mut content_opener([4]),
        )
        .await;
        assert!(matches!(resumed.content_cursor, CursorStep::Exhausted) && resumed.bookkeeping);
        for failure in [
            OpenFailure::Loss(LossClass::Aead),
            OpenFailure::Transient(TransientClass::BlobStore),
        ] {
            let mut opener = content_opener([9]);
            opener.0.insert(7, Err(failure));
            let staged = plan_pass(
                &base,
                sources(vec![9, 7, 4]),
                &records,
                &cap2,
                Instant::now(),
                &mut opener,
            )
            .await;
            match failure {
                OpenFailure::Loss(_) => {
                    assert_eq!(staged.skipped, [(7, LossClass::Aead)]);
                    assert!(matches!(staged.content_cursor, CursorStep::Advanced(7)));
                }
                OpenFailure::Transient(_) => {
                    assert!(staged.content.is_empty());
                    assert!(matches!(staged.decision, PassDecision::Abort { .. }));
                    assert!(matches!(staged.content_cursor, CursorStep::None));
                }
            }
        }

        let over = PassLimits {
            content_allowed: false,
            ..limits()
        };
        let first = plan_pass(
            &base,
            sources(vec![9, 7, 4]),
            &records,
            &over,
            Instant::now(),
            &mut opener([]),
        )
        .await;
        assert!(matches!(first.content_cursor, CursorStep::None) && first.suspend_content);
        assert!(matches!(first.decision, PassDecision::ManifestOnly));

        base.manifest.content_to.suspended = true;
        let again = plan_pass(
            &base,
            sources(vec![9, 7, 4]),
            &records,
            &over,
            Instant::now(),
            &mut opener([]),
        )
        .await;
        assert!(!again.suspend_content && matches!(again.decision, PassDecision::Nothing));

        base.manifest.reconciliation = Some(ReconciliationMarker {
            bound: 40,
            gap_next: 0,
        });
        let gap = PassSources {
            gap_stamp: Some(39),
            gap_inserts: vec![2],
            ..sources(vec![9, 7, 4])
        };
        let staged = plan_pass(
            &base,
            gap,
            &records,
            &over,
            Instant::now(),
            &mut opener([2]),
        )
        .await;
        assert!(matches!(staged.marker, MarkerStep::GapNext(3)));
        assert!(matches!(staged.decision, PassDecision::Publishing));

        base.manifest.reconciliation = None;
        base.manifest.content_to.suspended = false;
        let staged = plan(
            &base,
            &self::records(vec![(11, inserts([2]))]),
            &limits(),
            &mut opener([2]),
        )
        .await;
        assert!(matches!(staged.content_cursor, CursorStep::None) && !staged.suspend_content);
        assert!(matches!(staged.decision, PassDecision::Publishing));
    }

    #[tokio::test]
    async fn a_finished_content_backfill_can_share_a_pass_with_gap_deletes() {
        let mut base = base();
        base.manifest.content_to.cursor = Some(ContentCursor {
            next_below: u32::MAX,
        });
        base.manifest.reconciliation = Some(ReconciliationMarker {
            bound: 40,
            gap_next: 0,
        });
        base.thread.assign(&fields(7), 7, 3, 9);
        base.thread.assign(&fields(8), 8, 3, 9);
        let staged = plan_pass(
            &base,
            PassSources {
                content: vec![7],
                content_runnable: true,
                gap_stamp: Some(39),
                gap_deletes: vec![8],
                ..Default::default()
            },
            &ZaChangeRecords::default(),
            &limits(),
            Instant::now(),
            &mut content_opener([7]),
        )
        .await;

        assert!(matches!(staged.decision, PassDecision::NonPublishing));
        assert!(matches!(staged.content_cursor, CursorStep::Exhausted));
        assert_eq!(staged.content[0].0, 7);
        assert_eq!(
            staged.tombstones,
            [Tombstone {
                document_id: 8,
                deleted: 39
            }]
        );
        assert!(staged.witness.is_none());
        assert_eq!(staged.revision, base.manifest.revision);
    }

    #[tokio::test]
    async fn gap_deletes_wait_for_the_last_gap_insert() {
        let mut base = base();
        base.manifest.reconciliation = Some(ReconciliationMarker {
            bound: 40,
            gap_next: 0,
        });
        base.thread.assign(&fields(8), 8, 3, 9);
        let cap1 = PassLimits { cap: 1, ..limits() };
        let records = ZaChangeRecords::default();
        let sources = |gap_inserts| PassSources {
            gap_stamp: Some(39),
            gap_inserts,
            gap_deletes: vec![8],
            ..Default::default()
        };
        let first = plan_pass(
            &base,
            sources(vec![2, 5]),
            &records,
            &cap1,
            Instant::now(),
            &mut opener([2]),
        )
        .await;
        assert_eq!(first.records[0].document_id, 2);
        assert_eq!(first.records[0].created, 39);
        assert!(matches!(first.marker, MarkerStep::GapNext(3)) && first.tombstones.is_empty());

        base.manifest.reconciliation.as_mut().unwrap().gap_next = 3;
        let mut reply = indexed(5);
        reply.fields = fields(8);
        let second = plan_pass(
            &base,
            sources(vec![5]),
            &records,
            &cap1,
            Instant::now(),
            &mut FakeOpener(HashMap::from([(5, Ok(reply))])),
        )
        .await;
        assert_eq!(second.records[0].document_id, 5);
        assert_eq!(second.records[0].thread_id, base.thread.current_thread(8));
        assert_eq!(
            second.tombstones,
            [Tombstone {
                document_id: 8,
                deleted: 39
            }]
        );
        assert!(matches!(second.marker, MarkerStep::GapNext(6)));

        let finish = PassSources {
            gap_stamp: Some(42),
            marker_bound: Some(43),
            finish_at: Some(60),
            ..Default::default()
        };
        let last = plan_pass(
            &base,
            finish,
            &updates(41, 60),
            &cap1,
            Instant::now(),
            &mut opener([]),
        )
        .await;
        assert_eq!(last.marker_bound, Some(43));
        assert!(matches!(last.marker, MarkerStep::Finished));
        assert!(matches!(last.decision, PassDecision::ManifestOnly));
    }

    #[tokio::test]
    async fn the_time_cap_stops_at_an_insert_boundary_with_at_least_one_item() {
        let records = records(vec![(11, inserts(1..=3))]);
        let limits = PassLimits {
            elapsed: Duration::ZERO,
            ..limits()
        };
        let staged = plan(&base(), &records, &limits, &mut opener([1])).await;
        assert_eq!(ids(&staged), [1]);
        assert_eq!(staged.checkpoint, partial(11, 1));
    }
}
