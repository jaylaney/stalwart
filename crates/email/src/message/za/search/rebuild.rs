/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::message::messagedata::EmailMessageData;
use common::{
    Server,
    za::search::{
        ZaSearchConfig,
        cache::LoadedIndex,
        load::{ContentLoss, MapLoss},
        seal::za_search_key,
        trace::{ZA_SEARCH_LOST, ZA_SEARCH_REBUILD},
    },
    za_search_trace,
};
use std::future::Future;
use store::{
    query::log::{Change, ZaChangeRecords},
    roaring::RoaringBitmap,
    write::{BatchBuilder, BlobLink, BlobOp, PendingId, SetValue, assert::AssertValue},
    za_search::{
        Checkpoint, ZaSearchKey,
        manifest::{
            ContentCursor, ContentTo, Inventory, Manifest, ManifestFill, ManifestValue,
            ManifestWrite, RebuildCursor, ReconciliationMarker, manifest_assertion,
        },
        thread::RevisionRow,
    },
};
use tokio::time::{Instant, timeout_at};
use trc::{EventType, Key, NetworkEvent, StoreEvent};
use types::{
    blob_hash::BlobHash,
    collection::{Collection, SyncCollection},
    field::{Field, PrincipalField},
};
use vault::session::SessionKeys;

#[derive(Debug, PartialEq, Eq)]
pub enum RebuildStarted {
    Written,
    LostToAnotherNode,
    Destroying,
}

#[derive(Debug)]
pub enum ContentRebuildReason {
    ContentLoss(ContentLoss),
    CapRaised,
    Behind,
}

impl ContentRebuildReason {
    fn as_str(&self) -> &'static str {
        match self {
            Self::ContentLoss(loss) => loss.class().as_str(),
            Self::CapRaised => "cap_raised",
            Self::Behind => "behind",
        }
    }
}

pub fn za_search_content_needs_rebuild(
    manifest: &Manifest,
    config: &ZaSearchConfig,
) -> Option<ContentRebuildReason> {
    if manifest.content.as_ref().map_or(0, |head| head.size) >= config.account_max_mb << 20 {
        None
    } else if manifest.content_to.suspended {
        Some(ContentRebuildReason::CapRaised)
    } else if manifest.content_to.cursor.is_none()
        && manifest.content_to.checkpoint.change_id < manifest.indexed_to.change_id
        && manifest.cursor.is_none()
    {
        Some(ContentRebuildReason::Behind)
    } else {
        None
    }
}

pub(super) fn cursor_items(
    bounds: &RebuildCursor,
    present: impl IntoIterator<Item = u32>,
) -> Vec<u32> {
    let mut ids: Vec<_> = present
        .into_iter()
        .filter(|id| (bounds.next..=bounds.end).contains(id))
        .collect();
    ids.sort_unstable();
    ids
}

pub(crate) fn content_cursor_items(
    cursor: &ContentCursor,
    index: &LoadedIndex,
) -> Option<Vec<u32>> {
    let indexed = if let Some(head) = &index.manifest.content {
        let content = index.content.as_ref()?;
        if content.segments.first() != Some(&head.segment) {
            return None;
        }
        Some(content.part.document_ids())
    } else {
        None
    };
    let mut ids: Vec<_> = index
        .thread
        .records()
        .filter(|record| record.deleted.is_none())
        .map(|record| record.document_id)
        .filter(|id| *id < cursor.next_below && indexed.is_none_or(|set| !set.contains(*id)))
        .collect();
    ids.sort_unstable_by(|left, right| right.cmp(left));
    Some(ids)
}

pub(crate) struct Difference {
    pub gap_inserts: Vec<u32>,
    pub gap_deletes: Vec<u32>,
    pub retained_from: Option<Checkpoint>,
}

pub(crate) fn difference(
    index: &LoadedIndex,
    present: impl IntoIterator<Item = u32>,
    retained: &ZaChangeRecords,
    marker: &ReconciliationMarker,
) -> Difference {
    let present: RoaringBitmap = present.into_iter().collect();
    let mut inserted = RoaringBitmap::new();
    let mut deleted = RoaringBitmap::new();
    // Attribution uses all retained records, even records the pass previously consumed.
    for (_, changes) in &retained.records {
        for &change in changes {
            match change {
                Change::InsertItem(id) => {
                    inserted.insert(id as u32);
                }
                Change::DeleteItem(id) => {
                    deleted.insert(id as u32);
                }
                _ => {}
            }
        }
    }
    let gap_inserts = present
        .range(marker.gap_next..)
        .filter(|id| !index.thread.is_present(*id) && !inserted.contains(*id))
        .collect();
    let gap_deletes = index
        .thread
        .records()
        .filter(|record| record.deleted.is_none())
        .map(|record| record.document_id)
        .filter(|id| !present.contains(*id) && !deleted.contains(*id))
        .collect();
    let indexed_to = index.manifest.indexed_to;
    Difference {
        gap_inserts,
        gap_deletes,
        retained_from: retained
            .first_retained
            .is_none_or(|first| indexed_to.change_id > first)
            .then_some(indexed_to),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SecondTruncation {
    No,
    BoundMoved,
    Restart,
}

pub(crate) fn second_truncation(
    marker_bound: u64,
    bound_now: u64,
    indexed_to: &Checkpoint,
) -> SecondTruncation {
    if bound_now <= marker_bound {
        SecondTruncation::No
    } else if bound_now > indexed_to.change_id
        || (bound_now == indexed_to.change_id && !indexed_to.complete)
    {
        SecondTruncation::Restart
    } else {
        SecondTruncation::BoundMoved
    }
}

pub(super) struct Present {
    pub state: u64,
    pub documents: RoaringBitmap,
}

pub(super) async fn observe_present(
    server: &Server,
    account_id: u32,
) -> trc::Result<Option<Present>> {
    let durable_state = || async {
        server
            .store()
            .get_last_change_id(account_id, SyncCollection::Email.into())
            .await
            .map(Option::unwrap_or_default)
    };
    let state = durable_state().await?;
    let mut documents = RoaringBitmap::new();
    server
        .message_datas(account_id, &(), |id, _| {
            documents.insert(id);
            Ok(true)
        })
        .await?;
    // The two store reads are separate. Retry in another pass if their
    // observations cross an Email commit.
    Ok((state == durable_state().await?).then_some(Present { state, documents }))
}

fn rebuild_manifest(old: Option<&Manifest>, state: u64, highest: Option<u32>) -> Manifest {
    let revision = old.map_or(1, |manifest| manifest.revision + 1);
    let indexed_to = Checkpoint {
        change_id: state,
        position: 0,
        complete: true,
    };
    Manifest {
        revision,
        newest_row: RevisionRow {
            revision,
            change_id: 0,
        },
        indexed_to,
        content_to: ContentTo {
            checkpoint: indexed_to,
            ..Default::default()
        },
        cursor: highest.map(|end| RebuildCursor { next: 0, end }),
        ..Default::default()
    }
}

fn principal_batch(account_id: u32, previous: Option<&[u8]>) -> BatchBuilder {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .assert_value(
            PrincipalField::ZeroAccessSearch,
            manifest_assertion(previous),
        );
    batch
}

fn set_manifest(
    batch: &mut BatchBuilder,
    account_id: u32,
    manifest: Manifest,
    key: ZaSearchKey,
    fill: ManifestFill,
) {
    batch.set(
        PrincipalField::ZeroAccessSearch,
        SetValue::serializable(ManifestWrite::for_account(
            ManifestValue::Index(manifest),
            key,
            fill,
            account_id,
        )),
    );
}

fn unlink(batch: &mut BatchBuilder, hashes: impl IntoIterator<Item = BlobHash>) {
    for hash in hashes {
        batch.clear(BlobOp::Link {
            hash,
            to: BlobLink::Document,
        });
    }
}

const RECOVERY_DEADLINE: &str = "Search recovery deadline";

pub(super) fn recovery_timed_out(error: &trc::Error) -> bool {
    error.matches(EventType::Network(NetworkEvent::Timeout))
        && error.value_as_str(Key::Details) == Some(RECOVERY_DEADLINE)
}

async fn prepare_before<T>(
    deadline: Option<Instant>,
    prepare: impl Future<Output = trc::Result<T>>,
) -> trc::Result<T> {
    if let Some(deadline) = deadline {
        let expired = || NetworkEvent::Timeout.into_err().details(RECOVERY_DEADLINE);
        if Instant::now() >= deadline {
            return Err(expired());
        }
        let prepared = timeout_at(deadline, prepare).await.map_err(|_| expired())?;
        if Instant::now() >= deadline {
            return Err(expired());
        }
        prepared
    } else {
        prepare.await
    }
}

enum RebuildAdmission {
    Ready {
        batch: BatchBuilder,
        cursor_size: u64,
        source: &'static str,
    },
    Settled(RebuildStarted),
}

pub trait ZaSearchRebuild: Sync + Send {
    fn za_search_rebuild_start(
        &self,
        account_id: u32,
        keys: &SessionKeys,
        loss: MapLoss,
        deadline: Option<Instant>,
    ) -> impl Future<Output = trc::Result<RebuildStarted>> + Send;

    fn za_search_content_rebuild_start(
        &self,
        account_id: u32,
        keys: &SessionKeys,
        reason: ContentRebuildReason,
        deadline: Option<Instant>,
    ) -> impl Future<Output = trc::Result<()>> + Send;
}

impl ZaSearchRebuild for Server {
    async fn za_search_rebuild_start(
        &self,
        account_id: u32,
        keys: &SessionKeys,
        loss: MapLoss,
        deadline: Option<Instant>,
    ) -> trc::Result<RebuildStarted> {
        let prepared = prepare_before(deadline, async {
            let key = za_search_key(keys.swk());
            let raw = loss.raw();
            let current = self.za_search_read_manifest_raw(account_id).await?;
            if current.as_deref() != raw {
                za_search_trace!(ZA_SEARCH_LOST, AccountId = account_id);
                return Ok(RebuildAdmission::Settled(
                    if current.as_ref().is_some_and(|bytes| {
                        matches!(
                            ManifestValue::decode(bytes, &key),
                            Ok(ManifestValue::Destroying(_))
                        )
                    }) {
                        RebuildStarted::Destroying
                    } else {
                        RebuildStarted::LostToAnotherNode
                    },
                ));
            }
            let old = match raw.map(|bytes| ManifestValue::decode(bytes, &key)) {
                Some(Ok(ManifestValue::Index(old)))
                    if loss.failed_thread().is_some_and(|failed| {
                        old.thread_blobs.iter().any(|blob| &blob.hash == failed)
                    }) =>
                {
                    Some(old)
                }
                Some(Ok(ManifestValue::Destroying(_))) => {
                    return Ok(RebuildAdmission::Settled(RebuildStarted::Destroying));
                }
                Some(Ok(ManifestValue::Index(_))) => {
                    return Err(StoreEvent::UnexpectedError
                        .into_err()
                        .details("Search recovery has no failed thread input"));
                }
                _ => None,
            };
            let mut batch = principal_batch(account_id, raw);
            if raw.is_none() {
                let vault = self.za_vault_record(account_id).await?.ok_or_else(|| {
                    StoreEvent::UnexpectedError
                        .into_err()
                        .details("Search recovery has no vault record")
                })?;
                batch.assert_value(
                    PrincipalField::ZeroAccessVault,
                    AssertValue::Hash(vault.cas),
                );
            }
            let Some(present) = observe_present(self, account_id).await? else {
                za_search_trace!(ZA_SEARCH_LOST, AccountId = account_id);
                return Ok(RebuildAdmission::Settled(RebuildStarted::LostToAnotherNode));
            };
            let manifest = rebuild_manifest(old.as_ref(), present.state, present.documents.max());
            let (hashes, source) = self
                .za_search_inventory_links(account_id, old.as_ref().map(Manifest::inventory))
                .await?;
            let cursor_size = manifest
                .cursor
                .as_ref()
                .map_or(0, |cursor| u64::from(cursor.end) + 1);
            set_manifest(
                &mut batch,
                account_id,
                manifest,
                key,
                ManifestFill {
                    newest_row: true,
                    epoch: true,
                },
            );
            unlink(&mut batch, hashes);

            let mut witnessed = false;
            if let Some(witness) = present.documents.min() {
                batch
                    .with_collection(Collection::Email)
                    .with_document(witness)
                    .log_item_update(SyncCollection::Email, Some(PendingId::Assigned(witness)))
                    .log_container_property_change(
                        SyncCollection::Thread,
                        PendingId::Assigned(witness),
                    );
                witnessed = true;
            }
            let mut mailboxes = Vec::new();
            self.archives(
                account_id,
                Collection::Mailbox,
                Field::ARCHIVE,
                &(),
                |id, _| {
                    mailboxes.push(id);
                    Ok(true)
                },
            )
            .await?;
            for mailbox in mailboxes {
                batch.log_container_property_change(
                    SyncCollection::Email,
                    PendingId::Assigned(mailbox),
                );
                witnessed = true;
            }
            if !witnessed {
                batch.za_log_empty_email();
            }
            Ok(RebuildAdmission::Ready {
                batch,
                cursor_size,
                source,
            })
        })
        .await?;
        let (batch, cursor_size, source) = match prepared {
            RebuildAdmission::Ready {
                batch,
                cursor_size,
                source,
            } => (batch, cursor_size, source),
            RebuildAdmission::Settled(result) => return Ok(result),
        };
        // A deadline may cancel preparation. Once admitted, the owning
        // request runs commit and its cache notifications to completion.
        za_search_trace!(
            ZA_SEARCH_REBUILD,
            AccountId = account_id,
            Type = "rebuild",
            Reason = loss.class().as_str(),
            Total = cursor_size,
            Source = source
        );
        match self.commit_batch(batch).await {
            Ok(ids) => {
                let epoch = ids
                    .change_id(account_id, SyncCollection::Email.change_group())
                    .expect("the boundary allocated the account's Email group");
                za_search_trace!(
                    ZA_SEARCH_REBUILD,
                    AccountId = account_id,
                    Type = "rebuild",
                    Reason = "boundary",
                    ChangeId = epoch
                );
                Ok(RebuildStarted::Written)
            }
            Err(err) if err.is_assertion_failure() => {
                za_search_trace!(ZA_SEARCH_LOST, AccountId = account_id);
                Ok(RebuildStarted::LostToAnotherNode)
            }
            Err(err) => Err(err),
        }
    }

    async fn za_search_content_rebuild_start(
        &self,
        account_id: u32,
        keys: &SessionKeys,
        reason: ContentRebuildReason,
        deadline: Option<Instant>,
    ) -> trc::Result<()> {
        let (batch, epoch, source) = prepare_before(deadline, async {
            let key = za_search_key(keys.swk());
            let bytes = self
                .za_search_read_manifest_raw(account_id)
                .await?
                .ok_or_else(|| StoreEvent::AssertValueFailed.into_err())?;
            let Ok(ManifestValue::Index(mut manifest)) = ManifestValue::decode(&bytes, &key) else {
                return Err(StoreEvent::AssertValueFailed.into_err());
            };
            let mut batch = principal_batch(account_id, Some(&bytes));
            let mut source = "inventory";
            match &reason {
                ContentRebuildReason::ContentLoss(loss) => {
                    if !loss.matches(&bytes, &manifest) {
                        return Err(StoreEvent::AssertValueFailed.into_err());
                    }
                    let (hashes, unlink_source) = self
                        .za_search_inventory_links(
                            account_id,
                            Some(Inventory {
                                thread_blobs: Vec::new(),
                                content: manifest.content.take(),
                            }),
                        )
                        .await?;
                    source = unlink_source;
                    // The account-wide scan also found the thread descriptors.
                    unlink(
                        &mut batch,
                        hashes.into_iter().filter(|hash| {
                            !manifest.thread_blobs.iter().any(|blob| &blob.hash == hash)
                        }),
                    );
                    manifest.content_to.checkpoint = Checkpoint {
                        change_id: 0,
                        position: 0,
                        complete: true,
                    };
                }
                other => {
                    if za_search_content_needs_rebuild(&manifest, &self.za_search_config())
                        .is_none_or(|due| {
                            std::mem::discriminant(&due) != std::mem::discriminant(other)
                        })
                    {
                        return Err(StoreEvent::AssertValueFailed.into_err());
                    }
                }
            }
            manifest.content_to.suspended = false;
            manifest.content_to.cursor = Some(ContentCursor {
                next_below: u32::MAX,
            });
            let epoch = manifest.epoch;
            set_manifest(
                &mut batch,
                account_id,
                manifest,
                key,
                ManifestFill::default(),
            );
            Ok((batch, epoch, source))
        })
        .await?;
        self.commit_batch(batch).await?;
        za_search_trace!(
            ZA_SEARCH_REBUILD,
            AccountId = account_id,
            Type = "content",
            Reason = reason.as_str(),
            ChangeId = epoch,
            Source = source
        );
        Ok(())
    }
}

pub(super) async fn za_search_reconcile_start(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    bytes: &[u8],
    mut manifest: Manifest,
    bound: u64,
) -> trc::Result<()> {
    let mut batch = principal_batch(account_id, Some(bytes));
    manifest.reconciliation = Some(ReconciliationMarker { bound, gap_next: 0 });
    manifest.cursor = None;
    set_manifest(
        &mut batch,
        account_id,
        manifest,
        za_search_key(keys.swk()),
        ManifestFill::default(),
    );
    server.commit_batch(batch).await?;
    za_search_trace!(
        ZA_SEARCH_REBUILD,
        AccountId = account_id,
        Type = "reconciliation",
        Reason = "truncated",
        ChangeId = bound
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use common::za::search::cache::{BudgetCharge, LoadedContent};
    use store::za_search::{ThreadFieldsK, content::ContentRecord, manifest::ContentHead};

    fn checkpoint(change_id: u64) -> Checkpoint {
        Checkpoint {
            change_id,
            position: 0,
            complete: true,
        }
    }

    fn index_with(ids: &[u32]) -> LoadedIndex {
        let mut index = LoadedIndex::empty(Manifest::default());
        for &id in ids {
            index.thread.assign(
                &ThreadFieldsK {
                    hashes: vec![id.into()],
                    subject_hash: id.into(),
                    sent_at: None,
                },
                id,
                1,
                1,
            );
        }
        index
    }

    #[test]
    fn cursor_items_are_the_snapshot_between_next_and_end() {
        assert_eq!(
            cursor_items(&RebuildCursor { next: 5, end: 9 }, [12, 9, 3, 5]),
            [5, 9]
        );
    }

    #[test]
    fn content_cursor_items_run_newest_first_and_skip_content_present() {
        let mut index = index_with(&[1, 2, 3]);
        let cursor = ContentCursor {
            next_below: u32::MAX,
        };
        let head = ContentHead::default();
        let mut content = LoadedContent {
            part: Default::default(),
            segments: vec![head.segment],
            bytes: 0,
            charge: BudgetCharge::detached(),
        };
        content.part.insert(ContentRecord::new(
            store::search::IndexDocument::new(store::write::SearchIndex::Email),
            2,
            Default::default(),
            None,
            false,
        ));
        index.manifest.content = Some(head);
        assert_eq!(content_cursor_items(&cursor, &index), None);
        index.content = Some(content);
        assert_eq!(content_cursor_items(&cursor, &index), Some(vec![3, 1]));
        index.content.as_mut().unwrap().segments[0] = store::za_search::SegmentId([1; 16]);
        assert_eq!(content_cursor_items(&cursor, &index), None);
        index.manifest.content = None;
        assert_eq!(content_cursor_items(&cursor, &index), Some(vec![3, 2, 1]));
    }

    #[test]
    fn difference_excludes_what_the_retained_log_shows() {
        let retained = ZaChangeRecords {
            records: vec![(
                3,
                vec![
                    Change::InsertItem((9 << 32) | 4),
                    Change::DeleteItem((9 << 32) | 3),
                ],
            )],
            ..Default::default()
        };
        let index = index_with(&[1, 3]);
        let diff = difference(
            &index,
            [1, 2, 4],
            &retained,
            &ReconciliationMarker::default(),
        );
        assert_eq!(diff.gap_inserts, [2]);
        assert!(diff.gap_deletes.is_empty());
    }

    #[test]
    fn difference_orders_gap_inserts_ascending() {
        let diff = difference(
            &index_with(&[]),
            [7, 2, 5],
            &ZaChangeRecords::default(),
            &ReconciliationMarker::default(),
        );
        assert_eq!(diff.gap_inserts, [2, 5, 7]);
    }

    #[test]
    fn difference_resumes_gap_inserts_at_the_cursor() {
        let index = index_with(&[]);
        let retained = ZaChangeRecords::default();
        for (gap_next, expected) in [(5, &[5, 7][..]), (8, &[][..])] {
            let diff = difference(
                &index,
                [2, 5, 7],
                &retained,
                &ReconciliationMarker {
                    bound: 40,
                    gap_next,
                },
            );
            assert_eq!(diff.gap_inserts, expected);
        }
    }

    #[test]
    fn difference_resumes_retained_records_from_indexed_to() {
        let mut index = index_with(&[]);
        let retained = ZaChangeRecords {
            first_retained: Some(40),
            ..Default::default()
        };
        let marker = ReconciliationMarker {
            bound: 40,
            gap_next: 0,
        };
        for (change_id, position, complete) in
            [(30, 0, true), (40, 2, false), (40, 0, true), (44, 2, false)]
        {
            index.manifest.indexed_to = Checkpoint {
                change_id,
                position,
                complete,
            };
            let diff = difference(&index, [], &retained, &marker);
            assert_eq!(
                diff.retained_from,
                (change_id > 40).then_some(index.manifest.indexed_to)
            );
        }
    }

    #[test]
    fn second_truncation_is_detected_by_the_bound() {
        let complete = checkpoint(44);
        let partial = Checkpoint {
            position: 2,
            complete: false,
            ..complete
        };
        assert_eq!(second_truncation(40, 40, &complete), SecondTruncation::No);
        assert_eq!(
            second_truncation(40, 41, &complete),
            SecondTruncation::BoundMoved
        );
        assert_eq!(
            second_truncation(40, 45, &complete),
            SecondTruncation::Restart
        );
        assert_eq!(
            second_truncation(40, 44, &partial),
            SecondTruncation::Restart
        );
        assert_eq!(
            second_truncation(40, 44, &complete),
            SecondTruncation::BoundMoved
        );
    }

    #[test]
    fn difference_attributes_over_every_retained_record() {
        let mut index = index_with(&[46, 47, 49]);
        // At marker creation its own record is empty. Here retained processing
        // has already consumed records 46 through 49, skipping 48 as loss.
        index.manifest.indexed_to = checkpoint(49);
        let retained = ZaChangeRecords {
            records: (46..=50)
                .map(|id| (id, vec![Change::InsertItem(id)]))
                .collect(),
            first_retained: Some(45),
            ..Default::default()
        };
        let diff = difference(
            &index,
            46..=50,
            &retained,
            &ReconciliationMarker {
                bound: 45,
                gap_next: 0,
            },
        );
        assert!(diff.gap_inserts.is_empty());
        assert_eq!(diff.retained_from, Some(checkpoint(49)));
    }

    #[test]
    fn first_batch_fields() {
        let new = rebuild_manifest(None, 40, Some(7));
        assert_eq!(new.revision, 1);
        assert_eq!(
            new.newest_row,
            RevisionRow {
                revision: 1,
                change_id: 0
            }
        );
        assert_eq!(new.indexed_to, checkpoint(40));
        assert_eq!(new.cursor, Some(RebuildCursor { next: 0, end: 7 }));
        assert_eq!(new.content_to.checkpoint, new.indexed_to);
        assert!(new.content.is_none());
        let old = Manifest { revision: 6, ..new };
        let next = rebuild_manifest(Some(&old), 40, None);
        assert_eq!(next.revision, 7);
        assert_eq!(next.newest_row.revision, 7);
        assert!(next.cursor.is_none());
    }

    #[test]
    fn content_needs_rebuild_rules() {
        let config = ZaSearchConfig {
            account_max_mb: 1,
            ..Default::default()
        };
        let mut manifest = Manifest::default();
        manifest.content_to.suspended = true;
        assert!(matches!(
            za_search_content_needs_rebuild(&manifest, &config),
            Some(ContentRebuildReason::CapRaised)
        ));
        manifest.content_to.suspended = false;
        manifest.indexed_to = checkpoint(40);
        assert!(matches!(
            za_search_content_needs_rebuild(&manifest, &config),
            Some(ContentRebuildReason::Behind)
        ));
        manifest.cursor = Some(RebuildCursor { next: 0, end: 7 });
        assert!(za_search_content_needs_rebuild(&manifest, &config).is_none());
        manifest.cursor = None;
        for size in [1 << 20, (1 << 20) + 1] {
            manifest.content = Some(ContentHead {
                size,
                ..Default::default()
            });
            for suspended in [true, false] {
                manifest.content_to.suspended = suspended;
                assert!(za_search_content_needs_rebuild(&manifest, &config).is_none());
            }
        }
    }

    struct ReplyOpener(u32);

    impl crate::message::za::search::pass::Opener for ReplyOpener {
        async fn open(
            &mut self,
            document_id: u32,
        ) -> Result<
            crate::message::za::search::pass::Indexed,
            crate::message::za::search::open::OpenFailure,
        > {
            assert_eq!(document_id, self.0);
            Ok(crate::message::za::search::pass::Indexed {
                fields: ThreadFieldsK {
                    hashes: vec![8, u128::from(document_id)],
                    subject_hash: 8,
                    sent_at: None,
                },
                content: None,
                bytes: 100,
            })
        }
    }

    async fn plan_difference(
        index: &LoadedIndex,
        present: &[u32],
        retained: &ZaChangeRecords,
        expected_reply: u32,
    ) -> crate::message::za::search::pass::Staging {
        use crate::message::za::search::pass::{PassLimits, PassSources, plan_pass};
        let marker = index.manifest.reconciliation.as_ref().unwrap();
        let diff = difference(index, present.iter().copied(), retained, marker);
        plan_pass(
            index,
            PassSources {
                gap_inserts: diff.gap_inserts,
                gap_deletes: diff.gap_deletes,
                gap_stamp: Some(marker.bound - 1),
                ..Default::default()
            },
            retained,
            &PassLimits {
                cap: 48,
                hidden_bytes: 32 * 1024,
                message_bytes: 32 << 20,
                elapsed: std::time::Duration::from_secs(2),
                checkpoint: 1000,
                skip_after: 8,
                content_allowed: true,
            },
            std::time::Instant::now(),
            &mut ReplyOpener(expected_reply),
        )
        .await
    }

    #[tokio::test]
    async fn reconciliation_h3_gap_insert_threads_with_gap_deleted() {
        let mut index = index_with(&[8]);
        index.manifest.reconciliation = Some(ReconciliationMarker {
            bound: 40,
            gap_next: 0,
        });
        let retained = ZaChangeRecords::default();
        let staged = plan_difference(&index, &[9], &retained, 9).await;
        assert_eq!(staged.records[0].thread_id, index.thread.current_thread(8));
        assert_eq!(staged.records[0].created, 39);
        assert_eq!(staged.tombstones[0].document_id, 8);
        assert_eq!(staged.tombstones[0].deleted, 39);
        // If the gap delete ran first, its threading evidence would be gone.
        index.thread.tombstone(8, 39);
        let reordered = plan_difference(&index, &[9], &retained, 9).await;
        assert_eq!(reordered.records[0].thread_id, 9);
    }

    #[tokio::test]
    async fn reconciliation_i1_retained_insert_does_not_see_gap_deleted() {
        let mut index = index_with(&[8]);
        index.manifest.reconciliation = Some(ReconciliationMarker {
            bound: 40,
            gap_next: 0,
        });
        let retained = ZaChangeRecords {
            records: vec![(41, vec![Change::InsertItem((9 << 32) | 9)])],
            first_retained: Some(40),
            ..Default::default()
        };
        let first = plan_difference(&index, &[9], &retained, 9).await;
        assert!(first.records.is_empty());
        assert_eq!(first.tombstones[0].document_id, 8);
        index.thread.tombstone(8, first.tombstones[0].deleted);
        let next = plan_difference(&index, &[9], &retained, 9).await;
        assert_eq!(next.records[0].created, 41);
        assert_eq!(next.records[0].thread_id, 9);
    }

    #[tokio::test]
    async fn recovery_preparation_uses_its_supplied_deadline() {
        let deadline = Instant::now() + std::time::Duration::from_millis(10);
        let result = prepare_before::<()>(Some(deadline), std::future::pending()).await;
        assert!(recovery_timed_out(&result.unwrap_err()));
        let result = prepare_before(Some(deadline), async {
            panic!("expired preparation was polled");
            #[allow(unreachable_code)]
            Ok(())
        })
        .await;
        assert!(recovery_timed_out(&result.unwrap_err()));
    }
}
