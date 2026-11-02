/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access fork: the test-mode hooks of the store crate, compiled only
//! under `test_mode`, with the `Pause` shape of `common::za::test_hooks`
//! (which re-exports them). None changes behaviour while unset. A pause is
//! one-shot and keyed by account id: only the first request for that
//! account reaching the point takes it; requests for other accounts pass
//! and leave it set.
//!
//! - `INDEX_CHUNK_PAUSE`: in `Store::index_account_documents`, after an
//!   index pass's first commit chunk is written and before its second,
//!   keyed by the pass's account (`write_index_pass`). A pass reaches a
//!   second chunk when it is large or under `FORCE_COMMIT_SPLIT`. Used to
//!   provision or delete the account mid-pass (Task 6, ruling 18's
//!   per-chunk vault-absence assertion).
//!
//!   0.16's `INDEX_GUARD_FAIL_ONCE` is retired with R62's per-chunk guard
//!   and R63's confirmation read (Task 6, ruling 19): 1.0's index queue runs
//!   an account's index and unindex entries in order on one partition
//!   worker, so the guard has no subject.
//! - `FORCE_COMMIT_SPLIT`: while it names an account, every
//!   `BatchBuilder::commit_point()` adds a commit point, as if the batch were
//!   large, once the chunk holds an operation, in two kinds of batch: one
//!   whose current account is that account, and one with no current
//!   account. The second kind covers a search-index pass:
//!   `AccountIndexer::build_batch` never sets an account, and calls
//!   `commit_point()` after each `Document` key, so under the hook a pass
//!   with two documents commits in at least two chunks. Fixture-sized
//!   batches then reach their later commit chunks, so rules that apply per
//!   chunk can be tested. It stays set until cleared; batches whose current
//!   account is another account are not affected.
//!
//!   It governs `commit_point()` only. It does not govern
//!   `bulk_commit_point()`, nor the `add_commit_point()` that `build_batch`
//!   calls after every 1000 term merges (`search/term.rs`, by its own
//!   count): an index pass splits between its term merges only when it holds
//!   more than 1000 of them, as a document with more than 1000 distinct terms
//!   does.

use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub struct Pause {
    pub account_id: u32,
    pub arrived: Notify,
    pub release: Notify,
}

impl Pause {
    pub fn new(account_id: u32) -> Self {
        Pause {
            account_id,
            arrived: Notify::new(),
            release: Notify::new(),
        }
    }
}

pub static INDEX_CHUNK_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub fn set_index_chunk_pause(pause: Option<Arc<Pause>>) {
    *INDEX_CHUNK_PAUSE.lock().unwrap_or_else(|e| e.into_inner()) = pause;
}

fn take_for(slot: &Mutex<Option<Arc<Pause>>>, account_id: u32) -> Option<Arc<Pause>> {
    let mut pause = slot.lock().unwrap_or_else(|e| e.into_inner());
    if pause.as_ref().is_some_and(|p| p.account_id == account_id) {
        pause.take()
    } else {
        None
    }
}

/// The guard is gone before this await: `take_for` returns the pause.
async fn wait(pause: Option<Arc<Pause>>) {
    if let Some(pause) = pause {
        pause.arrived.notify_one();
        pause.release.notified().await;
    }
}

pub async fn index_chunk_pause_point(account_id: u32) {
    wait(take_for(&INDEX_CHUNK_PAUSE, account_id)).await;
}

/// `Store::write_batch` for an index pass, with `INDEX_CHUNK_PAUSE` between
/// its first and second commit chunk: the same chunks, each written as its
/// own transaction, in the same order.
pub(crate) async fn write_index_pass(
    store: &crate::Store,
    batch: &mut crate::write::BatchBuilder,
    account_id: u32,
) -> trc::Result<()> {
    let mut assigned_ids = crate::write::AssignedIds::default();
    let mut commit_points = batch.commit_points();
    for (chunk, commit_point) in commit_points.iter().enumerate() {
        if chunk == 1 {
            index_chunk_pause_point(account_id).await;
        }
        store
            .write(batch.build_one(commit_point), &mut assigned_ids)
            .await?;
    }
    Ok(())
}

/// The account whose batches, and every batch with no current account, add
/// a commit point at every `commit_point()`.
pub static FORCE_COMMIT_SPLIT: Mutex<Option<u32>> = Mutex::new(None);

pub fn set_force_commit_split(account_id: Option<u32>) {
    *FORCE_COMMIT_SPLIT.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

/// Whether `commit_point()` splits a batch whose current account is
/// `account_id`: `FORCE_COMMIT_SPLIT` is set, and it names that account or
/// the batch has none.
pub fn force_commit_split(account_id: Option<u32>) -> bool {
    match *FORCE_COMMIT_SPLIT.lock().unwrap_or_else(|e| e.into_inner()) {
        Some(forced) => account_id.is_none_or(|account_id| account_id == forced),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        search::{EmailSearchField, IndexDocument, index::account_batch},
        write::{BatchBuilder, SearchIndex, ValueClass},
    };
    use nlp::language::Language;
    use types::collection::Collection;

    fn batch_of(account_id: u32) -> BatchBuilder {
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(account_id)
            .with_collection(Collection::Email);
        for document_id in 0..3 {
            batch
                .with_document(document_id)
                .set(ValueClass::Property(1), vec![1u8])
                .commit_point();
        }
        // A commit point on a chunk that holds no operation adds nothing.
        batch.commit_point();
        batch
    }

    /// A search-index pass of two documents, built as the internal search
    /// store builds one: `account_batch` returns `AccountIndexer::build_batch`'s
    /// batch, which has no current account.
    fn index_pass_of(account_id: u32) -> BatchBuilder {
        let documents = [1u32, 2]
            .into_iter()
            .map(|document_id| {
                let mut document = IndexDocument::new(SearchIndex::Email)
                    .with_account_id(account_id)
                    .with_document_id(document_id);
                document.index_text(
                    EmailSearchField::Subject,
                    "quarterly planning notes",
                    Language::English,
                );
                (document_id, document)
            })
            .collect();
        account_batch(SearchIndex::Email, account_id, documents, &[], &[], false).unwrap()
    }

    // One test, as the hook is a process-wide setting.
    #[test]
    fn force_commit_split_splits_the_named_account_and_index_passes() {
        // Account ids no other test of this crate uses.
        const SPLIT: u32 = 0x5a5a_0001;
        const OTHER: u32 = 0x5a5a_0002;

        assert_eq!(batch_of(SPLIT).commit_point_count(), 0);
        assert_eq!(index_pass_of(SPLIT).commit_point_count(), 0);

        set_force_commit_split(Some(SPLIT));
        let split = batch_of(SPLIT).commit_point_count();
        let other = batch_of(OTHER).commit_point_count();
        let index_pass = index_pass_of(SPLIT).commit_point_count();
        set_force_commit_split(None);

        assert_eq!(split, 3);
        assert_eq!(other, 0);
        // One commit point after each document's `Document` key.
        assert_eq!(index_pass, 2);
        assert_eq!(batch_of(SPLIT).commit_point_count(), 0);
        assert_eq!(index_pass_of(SPLIT).commit_point_count(), 0);
    }

    /// Ruling 18 at runtime: a vault record written between a fenced pass's
    /// first and second commit chunk stops every later chunk. The pass's
    /// `Document` keys come last (`build_batch`), so none is written.
    #[tokio::test]
    async fn a_vault_written_mid_pass_stops_the_later_chunks() {
        use crate::{Store, backend::ephemeral::EphemeralStore};
        use types::field::PrincipalField;

        // An account id no other test of this crate uses.
        const MID_PASS: u32 = 0x5a5a_0201;
        let vault: ValueClass = PrincipalField::ZeroAccessVault.into();
        let text = (0..2500)
            .map(|n| format!("term{n:05}"))
            .collect::<Vec<_>>()
            .join(" ");
        let mut document = IndexDocument::new(SearchIndex::Email)
            .with_account_id(MID_PASS)
            .with_document_id(1);
        document.index_text(EmailSearchField::Body, &text, Language::English);
        let document =
            document.with_assert_absent(MID_PASS, Collection::Principal, 0, vault.clone());

        let store: Store = EphemeralStore::open();
        let pause = Arc::new(Pause::new(MID_PASS));
        set_index_chunk_pause(Some(pause.clone()));
        let arrived = pause.arrived.notified();
        let writer = {
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .index_account_documents(MID_PASS, SearchIndex::Email, vec![document], vec![])
                    .await
            })
        };
        arrived.await;

        // Chunk 0 is written: some terms, no `Document` key.
        assert!(
            store
                .has_account_search_terms(SearchIndex::Email, MID_PASS)
                .await
                .unwrap()
        );
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(MID_PASS)
            .with_collection(Collection::Principal)
            .with_document(0)
            .set(vault, vec![1u8]);
        store.write_batch(&mut batch).await.unwrap();
        pause.release.notify_one();

        let err = writer.await.unwrap().unwrap_err();
        assert!(err.is_assertion_failure());
        let document = crate::ValueKey::from(ValueClass::SearchIndex(
            crate::write::SearchIndexClass::Document {
                index: SearchIndex::Email,
                account_id: MID_PASS,
                document_id: 1,
            },
        ));
        assert!(
            store
                .get_value::<crate::write::serialize::RawValue>(document)
                .await
                .unwrap()
                .is_none()
        );
    }
}
