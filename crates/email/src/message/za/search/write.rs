/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    compact::{CompactError, CompactedContent, CompactedThreads, compact_content, compact_threads},
    pass::{CursorStep, MarkerStep, PassDecision, PassOutcome, Staging},
};
use crate::cache::MessageCacheFetch;
#[cfg(feature = "test_mode")]
use common::za::test_hooks::{SearchContentWrite, note_search_content_write};
use common::{
    Server,
    storage::blob::BlobHold,
    za::search::{
        cache::{
            BudgetCharge, EvictReason, IndexEntry, LoadedContent, LoadedIndex, PartKind,
            ZaSearchCache, scanned_seed,
        },
        error::{LoadFailure, TransientClass},
        load::ObservedLoadError,
        seal::{build_content_blob, build_thread_blob, za_search_key},
        trace::{
            ZA_SEARCH_ABORTED, ZA_SEARCH_EVICTED, ZA_SEARCH_LOST, ZA_SEARCH_PASS,
            ZA_SEARCH_SUSPENDED,
        },
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
        content::{ContentPart, ContentRecord, OwnedContentFrame},
        manifest::{
            ContentCursor, ContentHead, Manifest, ManifestFill, ManifestValue, ManifestWrite,
            ThreadBlobEntry, manifest_assertion, manifest_hash,
        },
        thread::{RevisionRow, ThreadPart},
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

#[derive(Default)]
struct PreparedWrite {
    blobs: Vec<StagedBlob>,
    unlinked: Vec<BlobHash>,
    threads: Option<CompactedThreads>,
    content: Option<ContentSwap>,
    staged_content_charge: Option<BudgetCharge>,
    staged_content_bytes: u64,
    content_written: u64,
    content_merged: u64,
    #[cfg(feature = "test_mode")]
    content_tier: Option<u8>,
}

// Declaration order keeps the charge alive while the staged frames are dropped,
// including cancellation before the guarded swap is entered.
struct ChargedStaging {
    staging: Staging,
    content_charge: Option<BudgetCharge>,
}

struct ContentSwap {
    replaced: usize,
    full: bool,
    removed_missing: bool,
}

async fn stage_blobs(
    server: &Server,
    entry: &IndexEntry,
    keys: &SessionKeys,
    base: LoadedIndex,
    new: &mut Manifest,
    staging: &Staging,
) -> Result<PreparedWrite, CompactError> {
    let account_id = entry.account_id;
    let config = server.za_search_config();
    let cache = &server.inner.cache.za_search;
    let mut prepared = PreparedWrite::default();
    let thread_delta = matches!(
        staging.decision,
        PassDecision::Publishing | PassDecision::NonPublishing
    );
    let compact_thread = thread_delta && base.manifest.thread_blobs.len() + 1 >= config.segments;
    let compact_content_due = !staging.content.is_empty()
        && base
            .manifest
            .content
            .as_ref()
            .is_some_and(|head| u64::from(head.length) + 1 >= config.segments as u64);
    let cursor_pending =
        new.cursor.is_some() || new.content_to.cursor.is_some() || new.reconciliation.is_some();
    let content = if compact_content_due {
        compact_content(server, entry, keys, &base, staging, cursor_pending, &config).await?
    } else {
        None
    };
    let threads = if compact_thread {
        compact_threads(server, account_id, keys, &base.thread, &config)
            .await
            .map_err(CompactError::Transient)?
    } else {
        None
    };

    let thread = if let Some((header, bytes, replacement)) = threads {
        prepared.threads = Some(replacement);
        prepared
            .unlinked
            .extend(new.thread_blobs.drain(..).map(|blob| blob.hash));
        Some((header, bytes))
    } else {
        if thread_delta && new.thread_blobs.len() == usize::from(u8::MAX) {
            return Err(CompactError::ThreadInventory);
        }
        seal_thread_delta(
            keys.swk(),
            account_id,
            config.pad_min,
            base.manifest.newest_row,
            staging,
        )
        .map_err(CompactError::Transient)?
    };
    drop(base);

    if let Some((header, bytes)) = thread {
        let blob = hold(server, account_id, bytes)
            .await
            .map_err(CompactError::Transient)?;
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::search_pass_pause_point(account_id).await;
        new.thread_blobs.push(ThreadBlobEntry {
            segment: header.segment,
            hash: blob.hash.clone(),
            size: u32::try_from(blob.len)
                .map_err(|_| CompactError::Transient(TransientClass::Validation))?,
        });
        if let Some(threads) = &mut prepared.threads {
            cache.grow(
                &mut threads.charge,
                PartKind::Thread,
                threads.part.loaded_bytes() as u64,
                account_id,
            );
        }
        prepared.blobs.push(blob);
    }

    if let Some(mut compacted) = content {
        let old_head = new
            .content
            .as_ref()
            .expect("compaction replaces a named chain");
        let replaced_count = u32::try_from(compacted.replaced.len())
            .map_err(|_| CompactError::Transient(TransientClass::Validation))?;
        let length = old_head
            .length
            .checked_sub(replaced_count)
            .and_then(|len| len.checked_add(1));
        let size = old_head
            .size
            .checked_sub(compacted.replaced_size)
            .and_then(|size| size.checked_add(compacted.bytes.len() as u64));
        let (Some(length), Some(size)) = (length, size) else {
            return Err(CompactError::Transient(TransientClass::Validation));
        };
        // Keep the retirement vector and its charge together during upload.
        let blob = hold(server, account_id, std::mem::take(&mut compacted.bytes))
            .await
            .map_err(CompactError::Transient)?;
        let CompactedContent {
            header,
            replaced,
            rewritten,
            full,
            removed_missing,
            mut charge,
            staged_bytes,
            retirement_bytes,
            ..
        } = compacted;
        cache.grow(
            &mut charge,
            PartKind::Content,
            staged_bytes.saturating_add(retirement_bytes),
            account_id,
        );
        prepared.staged_content_charge = Some(charge);
        prepared.staged_content_bytes = staged_bytes;
        new.content = Some(ContentHead {
            hash: blob.hash.clone(),
            segment: header.segment,
            length,
            size,
        });
        prepared.unlinked.extend(replaced);
        prepared.content = Some(ContentSwap {
            replaced: replaced_count as usize,
            full,
            removed_missing,
        });
        prepared.content_written = blob.len as u64;
        #[cfg(feature = "test_mode")]
        {
            note_search_content_write(account_id, SearchContentWrite::StagedMemory(staged_bytes));
            note_search_content_write(
                account_id,
                SearchContentWrite::RetirementMemory(retirement_bytes),
            );
            note_search_content_write(
                account_id,
                SearchContentWrite::Uploaded {
                    tier: header.tier,
                    bytes: blob.len as u64,
                },
            );
            prepared.content_tier = Some(header.tier);
        }
        prepared.content_merged = rewritten;
        prepared.blobs.push(blob);
    } else if let Some((header, bytes)) =
        seal_content_delta(keys.swk(), account_id, new.content.as_ref(), staging)
    {
        let length = new
            .content
            .as_ref()
            .map_or(Some(1), |head| head.length.checked_add(1));
        let size = new
            .content
            .as_ref()
            .map_or(0, |head| head.size)
            .checked_add(bytes.len() as u64);
        let (Some(length), Some(size)) = (length, size) else {
            return Err(CompactError::Transient(TransientClass::Validation));
        };
        #[cfg(feature = "test_mode")]
        {
            note_search_content_write(account_id, SearchContentWrite::Original(bytes.len() as u64));
            note_search_content_write(
                account_id,
                SearchContentWrite::Encoded {
                    tier: header.tier,
                    bytes: bytes.len() as u64,
                },
            );
        }
        let blob = hold(server, account_id, bytes)
            .await
            .map_err(CompactError::Transient)?;
        prepared.content_written = blob.len as u64;
        #[cfg(feature = "test_mode")]
        {
            note_search_content_write(
                account_id,
                SearchContentWrite::Uploaded {
                    tier: header.tier,
                    bytes: blob.len as u64,
                },
            );
            prepared.content_tier = Some(header.tier);
        }
        new.content = Some(ContentHead {
            hash: blob.hash.clone(),
            segment: header.segment,
            length,
            size,
        });
        prepared.blobs.push(blob);
    }
    Ok(prepared)
}

fn build_batch(
    account_id: u32,
    previous_bytes: &[u8],
    new: &Manifest,
    key: &ZaSearchKey,
    blobs: Vec<StagedBlob>,
    unlinked: Vec<BlobHash>,
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
    for hash in unlinked {
        batch.clear(BlobOp::Link {
            hash,
            to: BlobLink::Document,
        });
    }
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
    base: LoadedIndex,
    staging: Staging,
) -> trc::Result<PassOutcome> {
    let mut new = next_manifest(&base.manifest, &staging);
    if let PassDecision::Abort { class, .. } = staging.decision
        && new == base.manifest
    {
        return Ok(PassOutcome::Aborted(class));
    }
    let base_hash = base.manifest_bytes_hash;
    let base_manifest = base.manifest.clone();
    let prepared = match stage_blobs(server, entry, keys, base, &mut new, &staging).await {
        Ok(prepared) => prepared,
        Err(CompactError::Read(error)) => {
            return Ok(match error {
                ObservedLoadError::ContentLoss(loss) => PassOutcome::ContentLoss(loss),
                other => match other.classification() {
                    LoadFailure::Transient(class) => trace_abort(account_id, class),
                    _ => trace_abort(account_id, TransientClass::Validation),
                },
            });
        }
        Err(CompactError::Transient(class)) => return Ok(trace_abort(account_id, class)),
        Err(error @ (CompactError::TierOverflow | CompactError::ThreadInventory)) => {
            let reason = if matches!(error, CompactError::TierOverflow) {
                "tier-overflow"
            } else {
                "segments"
            };
            za_search_trace!(ZA_SEARCH_ABORTED, AccountId = account_id, Reason = reason);
            return Ok(PassOutcome::Aborted(TransientClass::Validation));
        }
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
        prepared,
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
    prepared: PreparedWrite,
    staging: Staging,
) -> trc::Result<PassOutcome> {
    // The retirement vector and batch locals drop before the charge,
    // including cancellation during mailbox lookup and commit.
    let mut charged = ChargedStaging {
        staging,
        content_charge: None,
    };
    let PreparedWrite {
        blobs,
        unlinked,
        threads,
        content,
        staged_content_charge,
        staged_content_bytes,
        content_written,
        content_merged,
        #[cfg(feature = "test_mode")]
        content_tier,
    } = prepared;
    charged.content_charge = staged_content_charge;
    let staging = &charged.staging;
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
        unlinked,
        witness,
        mailboxes,
    );

    #[cfg(feature = "test_mode")]
    common::za::test_hooks::search_commit_pause_point(account_id).await;
    if expires_by.is_some_and(|deadline| deadline <= now()) {
        return Ok(trace_abort(account_id, TransientClass::BlobStore));
    }
    #[cfg(feature = "test_mode")]
    let fail_abort = matches!(staging.decision, PassDecision::Abort { .. })
        && entry.take_abort_write_failure_for_test();
    #[cfg(not(feature = "test_mode"))]
    let fail_abort = false;
    let result = if fail_abort {
        Err(trc::StoreEvent::UnexpectedError.into_err())
    } else {
        server.commit_batch(batch).await
    };
    // Only now has the transaction disposed of the retirement operations.
    if let Some(charge) = &mut charged.content_charge {
        server.inner.cache.za_search.grow(
            charge,
            PartKind::Content,
            staged_content_bytes,
            account_id,
        );
    }
    match result {
        Ok(assigned) => {
            if matches!(staging.decision, PassDecision::Abort { .. }) {
                entry.commit_open_retry(base, &new);
            }
            if publishing {
                new.newest_row.change_id = assigned
                    .change_id(account_id, SyncCollection::Email.change_group())
                    .expect("the witness update allocated a change id");
            }
        }
        Err(err) if err.is_assertion_failure() => {
            za_search_trace!(ZA_SEARCH_LOST, AccountId = account_id);
            let retry = entry.take_open_retry();
            drop(charged);
            drop(threads);
            match server.za_search_reload_if_moved(entry, keys).await {
                Ok(_) => {
                    let current = entry.index.read().await;
                    if let Some(current) = current.as_ref() {
                        entry.restore_open_retry(retry, &new, &current.manifest);
                    }
                }
                Err(failure) => {
                    za_search_trace!(
                        ZA_SEARCH_ABORTED,
                        AccountId = account_id,
                        Reason = failure.as_str()
                    );
                }
            }
            return Ok(PassOutcome::Lost);
        }
        Err(_) => return Ok(trace_abort(account_id, TransientClass::Store)),
    }
    #[cfg(feature = "test_mode")]
    if let Some(tier) = content_tier {
        note_search_content_write(
            account_id,
            SearchContentWrite::Linked {
                tier,
                bytes: content_written,
            },
        );
    }
    #[cfg(feature = "test_mode")]
    note_search_content_write(
        account_id,
        SearchContentWrite::Committed {
            chain_length: new.content.as_ref().map_or(0, |head| head.length),
            full_merge: content.as_ref().is_some_and(|swap| swap.full),
            thread_compaction: threads.is_some(),
        },
    );
    let new_hash = manifest_hash(&ManifestValue::Index(new.clone()).encode(&key));
    drop(key);

    let outcome = match staging.decision {
        PassDecision::Abort { class, .. } => PassOutcome::Aborted(class),
        _ => PassOutcome::Advanced {
            published: publishing,
            indexed_to: new.indexed_to,
        },
    };
    staging.trace_skipped(account_id);
    za_search_trace!(
        ZA_SEARCH_PASS,
        AccountId = account_id,
        Total = staging.records.len(),
        TotalFailures = staging.tombstones.len(),
        Result = publishing,
        Version = new.revision,
        ChangeId = new.indexed_to.change_id,
        Size = content_written,
        // Encoded work includes cascade outputs consumed before upload.
        Used = content_merged,
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
    swap(
        server, entry, base_hash, base, new, new_hash, charged, threads, content,
    )
    .await;
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
async fn swap(
    server: &Server,
    entry: &IndexEntry,
    base_hash: [u8; 32],
    base: &Manifest,
    new: Manifest,
    new_hash: [u8; 32],
    charged: ChargedStaging,
    threads: Option<CompactedThreads>,
    content_swap: Option<ContentSwap>,
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
    let mut charged = charged;
    let staged_charge = charged.content_charge.take();
    let staging = charged.staging;
    if matches!(
        staging.decision,
        PassDecision::Publishing | PassDecision::NonPublishing | PassDecision::ContentOnly
    ) {
        if index.content.as_ref().is_some_and(|c| {
            c.segments.first() != base.content.as_ref().map(|b| &b.segment)
                || content_swap.as_ref().is_some_and(|update| {
                    update.replaced > c.segments.len() || (!update.full && update.removed_missing)
                })
        }) {
            index.content = None;
        }

        let thread_delta = matches!(
            staging.decision,
            PassDecision::Publishing | PassDecision::NonPublishing
        );
        let staged_bytes = staging.content_owned_bytes();
        let mut staged_charge = staged_charge.unwrap_or_else(BudgetCharge::detached);
        // The Vec (or its iterator) must drop before its reservation.
        let staged_content = staging.content;
        if let Some(content) = index.content.as_mut() {
            let staged_admitted = search_cache.grow(
                &mut staged_charge,
                PartKind::Content,
                staged_bytes,
                account_id,
            );
            let keep_content = if staged_admitted && thread_delta && !staging.tombstones.is_empty()
            {
                let scratch_size = staging
                    .tombstones
                    .iter()
                    .map(|tombstone| content.part.removal_scratch(tombstone.document_id))
                    .max()
                    .unwrap_or_default();
                remove_warm_content(
                    search_cache,
                    account_id,
                    content,
                    scratch_size,
                    |part, scratch| {
                        for tombstone in &staging.tombstones {
                            part.remove_with_scratch(tombstone.document_id, scratch);
                        }
                    },
                )
            } else {
                staged_admitted
            };
            if !keep_content {
                evict_warm_content(index, account_id);
            }
        }

        if thread_delta {
            let row =
                matches!(staging.decision, PassDecision::Publishing).then_some(new.newest_row);
            if let Some(CompactedThreads { mut part, charge }) = threads {
                part.extend([], [], row, []);
                index.thread = part;
                index.thread_charge = charge;
            } else {
                index
                    .thread
                    .extend(staging.records, staging.aliases, row, staging.tombstones);
            }
            index.thread_segments = new.thread_blobs.iter().map(|e| e.segment).collect();
            index.thread_bytes = index.thread.loaded_bytes() as u64;
            search_cache.grow(
                &mut index.thread_charge,
                PartKind::Thread,
                index.thread_bytes,
                account_id,
            );
        }

        if let Some(content) = index.content.as_mut() {
            let inserted = insert_warm_records(
                search_cache,
                account_id,
                content,
                &index.thread,
                staged_content,
                &mut staged_charge,
                staged_bytes,
            );
            drop(staged_charge);
            if !inserted
                || !advance_warm_content(
                    search_cache,
                    account_id,
                    content,
                    &index.thread,
                    new.content.as_ref(),
                    content_swap,
                )
            {
                evict_warm_content(index, account_id);
            }
        } else {
            drop(staged_content);
        }
    }
    if !matches!(staging.decision, PassDecision::Abort { .. }) {
        index.scanned_to = staging.scanned_to.max(scanned_seed(&new.indexed_to));
        index.update_records = 0;
    }
    index.manifest = new;
    index.manifest_bytes_hash = new_hash;
}

fn insert_warm_records(
    cache: &ZaSearchCache,
    account_id: u32,
    content: &mut LoadedContent,
    thread: &ThreadPart,
    records: Vec<(u32, ContentRecord)>,
    staged_charge: &mut BudgetCharge,
    mut staged_bytes: u64,
) -> bool {
    for (_, record) in records {
        let owned = record.owned_buffer_bytes() as u64;
        staged_bytes -= owned;
        if !thread.is_present(record.document_id()) {
            drop(record);
            cache.grow(staged_charge, PartKind::Content, staged_bytes, account_id);
            continue;
        }

        let with_frame = content.bytes.saturating_add(record.encoded_len() as u64);
        if !cache.grow(
            &mut content.charge,
            PartKind::Content,
            with_frame,
            account_id,
        ) {
            return false;
        }
        let frame = OwnedContentFrame::from_record(&record);
        drop(record);
        // Reuse the released buffer's reservation for scratch without giving
        // another admission a chance to consume the credit between phases.
        staged_charge.transfer_to(&mut content.charge, owned);
        if !content.part.insert_frame(frame, |extra| {
            cache.grow(
                &mut content.charge,
                PartKind::Content,
                with_frame.saturating_add(extra as u64),
                account_id,
            )
        }) || !reconcile_warm_content(cache, account_id, content)
        {
            return false;
        }
    }
    true
}

fn advance_warm_content(
    cache: &ZaSearchCache,
    account_id: u32,
    content: &mut LoadedContent,
    thread: &ThreadPart,
    head: Option<&ContentHead>,
    update: Option<ContentSwap>,
) -> bool {
    let Some(head) = head else {
        return true;
    };
    if let Some(update) = update {
        if update.full
            && let Some(scratch_size) = content
                .part
                .document_ids()
                .iter()
                .filter(|id| !thread.is_present(*id))
                .map(|id| content.part.removal_scratch(id))
                .max()
        {
            let removed =
                remove_warm_content(cache, account_id, content, scratch_size, |part, scratch| {
                    let mut next = 0;
                    while let Some(id) = part
                        .document_ids()
                        .range(next..)
                        .find(|id| !thread.is_present(*id))
                    {
                        part.remove_with_scratch(id, scratch);
                        let Some(after) = id.checked_add(1) else {
                            break;
                        };
                        next = after;
                    }
                });
            if !removed {
                return false;
            }
        }
        content.segments.splice(..update.replaced, [head.segment]);
    } else if content.segments.first() != Some(&head.segment) {
        content.segments.insert(0, head.segment);
    }
    true
}

fn remove_warm_content(
    cache: &ZaSearchCache,
    account_id: u32,
    content: &mut LoadedContent,
    scratch_size: usize,
    remove: impl FnOnce(&mut ContentPart, &mut Vec<u8>),
) -> bool {
    if !cache.grow(
        &mut content.charge,
        PartKind::Content,
        content.bytes.saturating_add(scratch_size as u64),
        account_id,
    ) {
        return false;
    }
    let mut scratch = Zeroizing::new(Vec::with_capacity(scratch_size));
    remove(&mut content.part, &mut scratch);
    drop(scratch);
    reconcile_warm_content(cache, account_id, content)
}

fn reconcile_warm_content(
    cache: &ZaSearchCache,
    account_id: u32,
    content: &mut LoadedContent,
) -> bool {
    let bytes = content.part.loaded_bytes() as u64;
    if !cache.grow(&mut content.charge, PartKind::Content, bytes, account_id) {
        return false;
    }
    content.bytes = bytes;
    true
}

fn evict_warm_content(index: &mut LoadedIndex, account_id: u32) {
    index.content = None;
    za_search_trace!(
        ZA_SEARCH_EVICTED,
        AccountId = account_id,
        Reason = EvictReason::Budget.as_str()
    );
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
            &mut base,
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
    async fn aborts_preserve_changed_suspension_and_reconciliation_bounds() {
        let (mut manifest, mut staging) = mixed_staging(Vec::new(), vec![8], false).await;
        let failing = store::za_search::manifest::FailingDocument {
            document_id: 7,
            attempts: u8::MAX,
        };
        manifest.failing = Some(failing.clone());
        staging.decision = PassDecision::Abort {
            class: TransientClass::Store,
            document_id: 7,
        };
        staging.failing = Some(failing);
        staging.marker = MarkerStep::Untouched;
        staging.marker_bound = None;
        staging.suspend_content = false;
        assert_eq!(next_manifest(&manifest, &staging), manifest);
        staging.suspend_content = true;
        let suspended = next_manifest(&manifest, &staging);
        assert!(suspended.content_to.suspended);
        assert_eq!(suspended.indexed_to, manifest.indexed_to);
        assert_eq!(suspended.failing, manifest.failing);
        staging.suspend_content = false;
        staging.marker_bound = Some(41);
        let reconciled = next_manifest(&manifest, &staging);
        assert_eq!(reconciled.reconciliation.as_ref().unwrap().bound, 41);
        assert_ne!(reconciled, manifest);
        assert_eq!(reconciled.indexed_to, manifest.indexed_to);
        assert_eq!(reconciled.failing, manifest.failing);
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
