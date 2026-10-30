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
//! - `INDEX_CHUNK_PAUSE`: after a multi-chunk search document's first
//!   commit chunk and before its second, keyed by the document's account
//!   (mail ruling R62).
//! - `INDEX_GUARD_FAIL_ONCE`: the account's next later commit chunk (the
//!   second or after) of a guarded search document is not written and
//!   fails with `AssertValueFailed`, as FoundationDB reports a read error
//!   at the guard's assertion (mail ruling R63).
//!
//!   These two had their call sites in 0.16's `Store::index`, which 1.0.1
//!   replaced with `Store::index_account_documents`; no call site reaches
//!   them until R62's per-chunk guard and R63's confirmation read are
//!   placed again, if they are kept.
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

/// The account whose next guarded later chunk fails its guard unwritten.
pub static INDEX_GUARD_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

pub fn set_index_guard_fail_once(account_id: Option<u32>) {
    *INDEX_GUARD_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

/// Whether `INDEX_GUARD_FAIL_ONCE` names `account_id`; clears it if so.
pub fn take_index_guard_fail(account_id: u32) -> bool {
    let mut slot = INDEX_GUARD_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if *slot == Some(account_id) {
        *slot = None;
        true
    } else {
        false
    }
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
}
