/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::pass::{CursorStep, MarkerStep, PassDecision, PassOutcome, Staging};
use crate::cache::MessageCacheFetch;
use common::{
    Server,
    storage::blob::BlobHold,
    za::search::{
        cache::{IndexEntry, PartKind, scanned_seed},
        error::TransientClass,
        seal::{build_content_blob, build_thread_blob, za_search_key},
        trace::{ZA_SEARCH_ABORTED, ZA_SEARCH_LOST, ZA_SEARCH_PASS, ZA_SEARCH_SUSPENDED},
    },
    za_search_trace,
};
use registry::schema::enums::CompressionAlgo;
use std::ptr;
use store::{
    write::{
        BatchBuilder, BlobLink, BlobOp, PendingId, SetValue, blob::BlobLinkValue,
        blob_codec::BlobOpts, now,
    },
    za_search::{
        BlobHeader, BlobKind, PadTarget, SegmentId, ZaSearchKey,
        blob::{HiddenSection, PublicSection},
        manifest::{
            ContentCursor, ContentHead, Manifest, ManifestFill, ManifestValue, ManifestWrite,
            ThreadBlobEntry, manifest_assertion, manifest_hash,
        },
        thread::RevisionRow,
    },
};
use types::{
    blob_hash::BlobHash,
    collection::{Collection, SyncCollection},
    field::{Field, PrincipalField},
};
use vault::{Zeroizing, keys::Secret, session::SessionKeys};

pub(crate) fn apply_marker_arms(
    mut manifest: Manifest,
    marker: MarkerStep,
    marker_bound: Option<u64>,
) -> Manifest {
    match marker {
        MarkerStep::Untouched => {}
        MarkerStep::GapNext(next) => {
            manifest
                .reconciliation
                .as_mut()
                .expect("a marker is set")
                .gap_next = next;
        }
        MarkerStep::Finished => manifest.reconciliation = None,
    }
    if let (Some(bound), Some(marker)) = (marker_bound, manifest.reconciliation.as_mut()) {
        marker.bound = bound;
    }
    manifest
}

fn next_manifest(base: &Manifest, staging: &Staging) -> Manifest {
    let mut new = apply_marker_arms(base.clone(), staging.marker, staging.marker_bound);
    if matches!(staging.decision, PassDecision::Abort { .. }) {
        new.failing = staging.failing.clone();
    } else {
        new.indexed_to = staging.checkpoint;
        match staging.cursor {
            CursorStep::None => {}
            CursorStep::Advanced(next) => new.cursor.as_mut().expect("a cursor is set").next = next,
            CursorStep::Exhausted => new.cursor = None,
        }
        match staging.content_cursor {
            CursorStep::None => {}
            CursorStep::Advanced(next_below) => {
                new.content_to.cursor = Some(ContentCursor { next_below })
            }
            CursorStep::Exhausted => {
                new.content_to.cursor = None;
                new.content_to.checkpoint = new.indexed_to;
            }
        }
        if base.content_to.checkpoint == base.indexed_to && covers_new_records(staging) {
            new.content_to.checkpoint = new.indexed_to;
        }
        if let Some(failing) = &base.failing
            && (staging
                .records
                .iter()
                .any(|r| r.document_id == failing.document_id)
                || staging
                    .content
                    .iter()
                    .any(|(d, _)| *d == failing.document_id)
                || staging
                    .skipped
                    .iter()
                    .any(|(d, _)| *d == failing.document_id))
        {
            new.failing = None;
        }
    }
    new.content_to.suspended |= staging.suspend_content;
    if matches!(staging.decision, PassDecision::Publishing) {
        new.revision = staging.revision;
        new.newest_row = RevisionRow {
            revision: staging.revision,
            change_id: 0,
        };
    }
    new
}

// Backfilled content cannot stand in for a different, newly indexed message.
fn covers_new_records(staging: &Staging) -> bool {
    staging.records.iter().all(|record| {
        staging
            .content
            .iter()
            .any(|(id, _)| *id == record.document_id)
    })
}

struct StagedBlob {
    hash: BlobHash,
    len: usize,
    hold: BlobHold,
    expires_by: u64,
}

async fn hold(
    server: &Server,
    account_id: u32,
    bytes: Vec<u8>,
) -> Result<StagedBlob, TransientClass> {
    let hold_for = server.core.jmap.upload_tmp_ttl;
    #[cfg(feature = "test_mode")]
    let hold_for = {
        use common::za::test_hooks::{search_blob_slow, search_hold_expire};
        if let Some(delay) = search_blob_slow(account_id) {
            tokio::time::sleep(delay).await;
        }
        if search_hold_expire(account_id) {
            0
        } else {
            hold_for
        }
    };
    // hold_blob computes its own expiry when called. This is a conservative
    // lower bound, checked again after staging and before entering the batch.
    let expires_by = now().saturating_add(hold_for);
    let (hash, hold) = server
        .hold_blob(
            account_id,
            BlobHash::generate(&bytes),
            &bytes,
            hold_for,
            &BlobOpts::new(
                CompressionAlgo::None,
                server.core.blob.email.encryption.clone(),
            ),
        )
        .await
        .map_err(|_| TransientClass::BlobStore)?;
    Ok(StagedBlob {
        hash,
        len: bytes.len(),
        hold,
        expires_by,
    })
}

fn seal_thread_delta(
    swk: &Secret,
    account_id: u32,
    pad_min: usize,
    previous_row: RevisionRow,
    staging: &Staging,
) -> Result<Option<(BlobHeader, Vec<u8>)>, TransientClass> {
    let publishing = matches!(staging.decision, PassDecision::Publishing);
    if !publishing && !matches!(staging.decision, PassDecision::NonPublishing) {
        return Ok(None);
    }
    let header = BlobHeader {
        kind: BlobKind::Thread,
        tier: 0,
        segment: SegmentId::random(),
        predecessor: None,
    };
    let hidden = publishing.then(|| HiddenSection {
        records: staging.records.clone(),
        aliases: staging.aliases.clone(),
    });
    let public = PublicSection {
        tombstones: staging.tombstones.clone(),
        rows: publishing.then_some(previous_row).into_iter().collect(),
    };
    let bytes = build_thread_blob(
        swk,
        account_id,
        &header,
        hidden
            .as_ref()
            .map(|hidden| (hidden, PadTarget::Publishing)),
        &public,
        pad_min,
    )
    .map_err(|_| TransientClass::Validation)?;
    Ok(Some((header, bytes)))
}

fn seal_content_delta(
    swk: &Secret,
    account_id: u32,
    predecessor: Option<&ContentHead>,
    staging: &Staging,
) -> Option<(BlobHeader, Vec<u8>)> {
    if staging.content.is_empty() {
        return None;
    }
    let header = BlobHeader {
        kind: BlobKind::Content,
        tier: 0,
        segment: SegmentId::random(),
        predecessor: predecessor.map(|h| (h.hash.clone(), h.segment)),
    };
    let encoded = staging
        .content
        .iter()
        .map(|(_, record)| Zeroizing::new(record.encode()))
        .collect::<Vec<_>>();
    let refs = encoded
        .iter()
        .map(|bytes| bytes.as_slice())
        .collect::<Vec<_>>();
    let bytes = build_content_blob(swk, account_id, &header, &refs);
    Some((header, bytes))
}

async fn stage_blobs(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    base: &Manifest,
    new: &mut Manifest,
    staging: &Staging,
) -> Result<Vec<StagedBlob>, TransientClass> {
    let mut blobs = Vec::new();
    if let Some((header, bytes)) = seal_thread_delta(
        keys.swk(),
        account_id,
        server.za_search_config().pad_min,
        base.newest_row,
        staging,
    )? {
        let blob = hold(server, account_id, bytes).await?;
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::search_pass_pause_point(account_id).await;
        new.thread_blobs.push(ThreadBlobEntry {
            segment: header.segment,
            hash: blob.hash.clone(),
            size: u32::try_from(blob.len).map_err(|_| TransientClass::Validation)?,
        });
        blobs.push(blob);
    }

    if let Some((header, bytes)) =
        seal_content_delta(keys.swk(), account_id, new.content.as_ref(), staging)
    {
        let blob = hold(server, account_id, bytes).await?;
        let length = new
            .content
            .as_ref()
            .map_or(Some(1), |h| h.length.checked_add(1));
        new.content = Some(ContentHead {
            hash: blob.hash.clone(),
            segment: header.segment,
            length: length.ok_or(TransientClass::Validation)?,
            size: new.content.as_ref().map_or(0, |h| h.size) + blob.len as u64,
        });
        blobs.push(blob);
    }
    Ok(blobs)
}

fn build_batch(
    account_id: u32,
    previous_bytes: &[u8],
    new: &Manifest,
    key: &ZaSearchKey,
    blobs: Vec<StagedBlob>,
    witness: Option<u32>,
    mailboxes: impl IntoIterator<Item = u32>,
) -> BatchBuilder {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .assert_value(
            PrincipalField::ZeroAccessSearch,
            manifest_assertion(Some(previous_bytes)),
        )
        .set(
            PrincipalField::ZeroAccessSearch,
            SetValue::serializable(ManifestWrite::for_account(
                ManifestValue::Index(new.clone()),
                key.clone(),
                ManifestFill {
                    newest_row: witness.is_some(),
                    epoch: false,
                },
                account_id,
            )),
        );
    for blob in blobs {
        blob.hold.release(&mut batch);
        batch
            .with_account_id(account_id)
            .with_collection(Collection::Principal)
            .with_document(0)
            .set(
                BlobOp::Link {
                    hash: blob.hash,
                    to: BlobLink::Document,
                },
                BlobLinkValue::new(blob.len),
            );
    }
    if let Some(witness) = witness {
        batch
            .with_collection(Collection::Email)
            .with_document(witness)
            .log_item_update(SyncCollection::Email, Some(PendingId::Assigned(witness)))
            .log_container_property_change(SyncCollection::Thread, PendingId::Assigned(witness));
        for mailbox in mailboxes {
            batch
                .log_container_property_change(SyncCollection::Email, PendingId::Assigned(mailbox));
        }
    }
    batch
}

pub(crate) async fn write_pass(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    entry: &IndexEntry,
    previous_bytes: &[u8],
    base_hash: [u8; 32],
    base_manifest: Manifest,
    staging: Staging,
) -> trc::Result<PassOutcome> {
    if base_manifest.thread_blobs.len() >= usize::from(u8::MAX)
        && matches!(
            staging.decision,
            PassDecision::Publishing | PassDecision::NonPublishing
        )
    {
        za_search_trace!(
            ZA_SEARCH_ABORTED,
            AccountId = account_id,
            Reason = "segments"
        );
        return Ok(PassOutcome::Aborted(TransientClass::Validation));
    }
    let mut new = next_manifest(&base_manifest, &staging);
    let blobs =
        match stage_blobs(server, account_id, keys, &base_manifest, &mut new, &staging).await {
            Ok(blobs) => blobs,
            Err(class) => return Ok(trace_abort(account_id, class)),
        };
    commit_and_swap(
        server,
        account_id,
        keys,
        entry,
        previous_bytes,
        base_hash,
        &base_manifest,
        new,
        blobs,
        staging,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn commit_and_swap(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    entry: &IndexEntry,
    previous_bytes: &[u8],
    base_hash: [u8; 32],
    base: &Manifest,
    mut new: Manifest,
    blobs: Vec<StagedBlob>,
    staging: Staging,
) -> trc::Result<PassOutcome> {
    let publishing = matches!(staging.decision, PassDecision::Publishing);
    let witness = publishing.then(|| staging.witness.expect("publishing indexed a message"));
    let mut mailboxes = Vec::new();
    if publishing {
        // Witness only existing mailboxes. Loading the ordinary message
        // cache on a cold account can provision default mailboxes.
        if let Some(cache) = server.fresh_messages(account_id) {
            mailboxes.extend(
                cache
                    .mailboxes
                    .items
                    .iter()
                    .map(|mailbox| mailbox.document_id),
            );
        } else {
            server
                .archives(
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
        }
    }
    let expires_by = blobs.iter().map(|blob| blob.expires_by).min();
    let key = za_search_key(keys.swk());
    let batch = build_batch(
        account_id,
        previous_bytes,
        &new,
        &key,
        blobs,
        witness,
        mailboxes,
    );

    #[cfg(feature = "test_mode")]
    common::za::test_hooks::search_commit_pause_point(account_id).await;
    if expires_by.is_some_and(|deadline| deadline <= now()) {
        return Ok(trace_abort(account_id, TransientClass::BlobStore));
    }
    match server.commit_batch(batch).await {
        Ok(assigned) => {
            if publishing {
                new.newest_row.change_id = assigned
                    .change_id(account_id, SyncCollection::Email.change_group())
                    .expect("the witness update allocated a change id");
            }
        }
        Err(err) if err.is_assertion_failure() => {
            za_search_trace!(ZA_SEARCH_LOST, AccountId = account_id);
            drop(staging);
            if let Err(failure) = server.za_search_reload_if_moved(entry, keys).await {
                za_search_trace!(
                    ZA_SEARCH_ABORTED,
                    AccountId = account_id,
                    Reason = failure.as_str()
                );
            }
            return Ok(PassOutcome::Lost);
        }
        Err(_) => return Ok(trace_abort(account_id, TransientClass::Store)),
    }
    let new_hash = manifest_hash(&ManifestValue::Index(new.clone()).encode(&key));
    drop(key);

    let outcome = match staging.decision {
        PassDecision::Abort { class, .. } => PassOutcome::Aborted(class),
        _ => PassOutcome::Advanced {
            published: publishing,
            indexed_to: new.indexed_to,
        },
    };
    za_search_trace!(
        ZA_SEARCH_PASS,
        AccountId = account_id,
        Total = staging.records.len(),
        TotalFailures = staging.tombstones.len(),
        Result = publishing,
        Version = new.revision,
        ChangeId = new.indexed_to.change_id,
        Size = new.content.as_ref().map_or(0, |h| h.size)
            - base.content.as_ref().map_or(0, |h| h.size),
        Used = 0u64,
    );
    if !base.content_to.suspended && new.content_to.suspended {
        za_search_trace!(
            ZA_SEARCH_SUSPENDED,
            AccountId = account_id,
            Size = new.content.as_ref().map_or(0, |h| h.size)
        );
    }

    #[cfg(feature = "test_mode")]
    {
        use common::za::test_hooks::{search_swap_pause_point, search_swap_skip};
        search_swap_pause_point(account_id).await;
        if search_swap_skip(account_id) {
            return Ok(outcome);
        }
    }
    swap(server, entry, base_hash, base, new, new_hash, staging).await;
    Ok(outcome)
}

async fn swap(
    server: &Server,
    entry: &IndexEntry,
    base_hash: [u8; 32],
    base: &Manifest,
    new: Manifest,
    new_hash: [u8; 32],
    staging: Staging,
) {
    let search_cache = &server.inner.cache.za_search;
    let account_id = entry.account_id;
    let mut guard = entry.index.write().await;
    if !search_cache
        .get(account_id)
        .is_some_and(|current| ptr::eq(current.as_ref(), entry))
    {
        return;
    }
    let Some(index) = guard
        .as_mut()
        .filter(|i| i.manifest_bytes_hash == base_hash)
    else {
        return;
    };
    if matches!(
        staging.decision,
        PassDecision::Publishing | PassDecision::NonPublishing | PassDecision::ContentOnly
    ) {
        if matches!(
            staging.decision,
            PassDecision::Publishing | PassDecision::NonPublishing
        ) {
            for tombstone in &staging.tombstones {
                if let Some(content) = index.content.as_mut() {
                    content.part.remove(tombstone.document_id);
                }
            }
            let row =
                matches!(staging.decision, PassDecision::Publishing).then_some(new.newest_row);
            index
                .thread
                .extend(staging.records, staging.aliases, row, staging.tombstones);
            index.thread_segments = new.thread_blobs.iter().map(|e| e.segment).collect();
            index.thread_bytes = index.thread.loaded_bytes() as u64;
            search_cache.grow(
                &mut index.thread_charge,
                PartKind::Thread,
                index.thread_bytes,
                account_id,
            );
        }
        if index
            .content
            .as_ref()
            .is_some_and(|c| c.segments.first() != base.content.as_ref().map(|b| &b.segment))
        {
            index.content = None;
        }
        if let Some(content) = index.content.as_mut() {
            for (_, record) in staging.content {
                if index.thread.is_present(record.document_id()) {
                    content.part.insert(record);
                }
            }
            if let Some(head) = &new.content
                && content.segments.first() != Some(&head.segment)
            {
                content.segments.insert(0, head.segment);
            }
        }
        search_cache.admit_content_growth(index, account_id);
    }
    if !matches!(staging.decision, PassDecision::Abort { .. }) {
        index.scanned_to = staging.scanned_to.max(scanned_seed(&new.indexed_to));
        index.update_records = 0;
    }
    index.manifest = new;
    index.manifest_bytes_hash = new_hash;
}

fn trace_abort(account_id: u32, class: TransientClass) -> PassOutcome {
    za_search_trace!(
        ZA_SEARCH_ABORTED,
        AccountId = account_id,
        Reason = class.as_str()
    );
    PassOutcome::Aborted(class)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::za::search::{
        open::OpenFailure,
        pass::{Indexed, Opener, PassLimits, PassSources, plan_pass},
    };
    use common::za::search::{
        ZaSearchConfig,
        cache::LoadedIndex,
        seal::{parse_content_blob, parse_thread_blob},
    };
    use std::time::{Duration, Instant};
    use store::{
        query::log::ZaChangeRecords,
        search::IndexDocument,
        write::SearchIndex,
        za_search::{
            Checkpoint, ThreadFieldsK,
            content::{ContentRecord, SortKeys},
            manifest::ReconciliationMarker,
            thread::Tombstone,
        },
    };

    struct BackfillOpener {
        include_insert_content: bool,
    }

    impl Opener for BackfillOpener {
        async fn open(&mut self, id: u32) -> Result<Indexed, OpenFailure> {
            assert!(id == 7 || id == 11);
            Ok(Indexed {
                fields: ThreadFieldsK {
                    hashes: vec![id.into()],
                    subject_hash: id.into(),
                    sent_at: None,
                },
                content: (id == 7 || self.include_insert_content).then(|| {
                    ContentRecord::new(
                        IndexDocument::new(SearchIndex::Email),
                        id,
                        SortKeys::default(),
                        None,
                        false,
                    )
                }),
                bytes: 1000,
            })
        }
    }

    async fn mixed_staging(
        gap_inserts: Vec<u32>,
        gap_deletes: Vec<u32>,
        include_insert_content: bool,
    ) -> (Manifest, Staging) {
        let mut manifest = manifest_with_marker(40, 0);
        manifest.revision = 3;
        manifest.newest_row = RevisionRow {
            revision: 3,
            change_id: 41,
        };
        manifest.indexed_to = Checkpoint {
            change_id: 42,
            position: 0,
            complete: true,
        };
        manifest.content_to.cursor = Some(ContentCursor {
            next_below: u32::MAX,
        });
        let mut base = LoadedIndex::empty(manifest.clone());
        for id in [7, 8] {
            base.thread.assign(
                &ThreadFieldsK {
                    hashes: vec![id.into()],
                    subject_hash: id.into(),
                    sent_at: None,
                },
                id,
                2,
                11,
            );
        }
        let staging = plan_pass(
            &base,
            PassSources {
                content: vec![7],
                content_runnable: true,
                gap_stamp: Some(39),
                gap_inserts,
                gap_deletes,
                ..PassSources::default()
            },
            &ZaChangeRecords::default(),
            &PassLimits {
                cap: 4,
                hidden_bytes: 32 * 1024,
                message_bytes: 32 << 20,
                elapsed: Duration::from_secs(2),
                checkpoint: 1000,
                skip_after: 8,
                content_allowed: true,
            },
            Instant::now(),
            &mut BackfillOpener {
                include_insert_content,
            },
        )
        .await;
        (manifest, staging)
    }

    #[tokio::test]
    async fn mixed_backfill_and_gap_delete_seals_both_deltas_without_revision() {
        let (manifest, staging) = mixed_staging(Vec::new(), vec![8], false).await;
        let swk = Secret::random();
        let new = next_manifest(&manifest, &staging);
        assert_eq!(new.revision, manifest.revision);
        assert_eq!(new.newest_row, manifest.newest_row);
        assert!(new.content_to.cursor.is_none());
        assert!(staging.witness.is_none());

        let (thread_header, thread_bytes) = seal_thread_delta(
            &swk,
            5,
            ZaSearchConfig::default().pad_min,
            manifest.newest_row,
            &staging,
        )
        .unwrap()
        .unwrap();
        let (hidden, public) = parse_thread_blob(&swk, 5, &thread_header, &thread_bytes).unwrap();
        assert!(hidden.is_none());
        assert!(public.rows.is_empty());
        assert_eq!(
            public.tombstones,
            [Tombstone {
                document_id: 8,
                deleted: 39
            }]
        );

        let (content_header, content_bytes) = seal_content_delta(&swk, 5, None, &staging).unwrap();
        let (_, records) =
            parse_content_blob(&swk, 5, Some(content_header.segment), None, &content_bytes)
                .unwrap();
        assert_eq!(
            records
                .iter()
                .map(ContentRecord::document_id)
                .collect::<Vec<_>>(),
            [7]
        );
    }

    #[tokio::test]
    async fn backfill_content_cannot_cover_a_different_new_insert() {
        for complete in [false, true] {
            let (_, staging) = mixed_staging(vec![11], Vec::new(), complete).await;
            assert!(matches!(staging.decision, PassDecision::Publishing));
            assert_eq!(staging.records[0].document_id, 11);
            assert_eq!(staging.witness, Some(11));
            assert_eq!(staging.content[0].0, 7);
            assert_eq!(covers_new_records(&staging), complete);
        }
    }

    fn manifest_with_marker(bound: u64, gap_next: u32) -> Manifest {
        Manifest {
            reconciliation: Some(ReconciliationMarker { bound, gap_next }),
            ..Manifest::default()
        }
    }

    #[test]
    fn a_finishing_bound_move_writes_no_bound() {
        let new = apply_marker_arms(manifest_with_marker(40, 9), MarkerStep::Finished, Some(43));
        assert!(new.reconciliation.is_none());
        let new = apply_marker_arms(
            manifest_with_marker(40, 9),
            MarkerStep::GapNext(12),
            Some(43),
        );
        assert_eq!(
            new.reconciliation,
            Some(ReconciliationMarker {
                bound: 43,
                gap_next: 12
            })
        );
    }
}
