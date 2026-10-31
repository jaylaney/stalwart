/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access fork: mail ruling R58 in every commit chunk of an index pass
//! (Task 1's ruling 18), and the move route's input-byte count.

use super::*;
use crate::{
    backend::ephemeral::EphemeralStore,
    search::EmailSearchField,
    write::{Operation, PendingId, ValueOp, assert::AssertValue},
};
use nlp::language::Language;
use types::{collection::SyncCollection, field::PrincipalField};

// Account ids no other test of this crate uses.
const FENCED: u32 = 0x5a5a_0101;
const PLAIN: u32 = 0x5a5a_0102;
const CONTROL: u32 = 0x5a5a_0103;

fn vault_class() -> ValueClass {
    PrincipalField::ZeroAccessVault.into()
}

/// A document with more distinct terms than `build_batch` merges in one
/// commit chunk (1000), so its pass splits without a test hook.
fn large_document(account_id: u32, document_id: u32, fenced: bool) -> IndexDocument {
    let text = (0..2500)
        .map(|n| format!("term{n:05}"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut document = IndexDocument::new(SearchIndex::Email)
        .with_account_id(account_id)
        .with_document_id(document_id);
    document.index_text(EmailSearchField::Body, &text, Language::English);
    if fenced {
        document.with_assert_absent(account_id, Collection::Principal, 0, vault_class())
    } else {
        document
    }
}

fn pass(account_id: u32, fenced: bool) -> BatchBuilder {
    account_batch(
        SearchIndex::Email,
        account_id,
        vec![
            (1, large_document(account_id, 1, fenced)),
            (2, large_document(account_id, 2, fenced)),
        ],
        &[],
        &[],
        false,
    )
    .unwrap()
}

fn is_fence(ops: &[Operation], account_id: u32) -> bool {
    matches!(
        ops,
        [
            Operation::AccountId { account_id: a },
            Operation::Collection {
                collection: Collection::Principal
            },
            Operation::DocumentId {
                document_id: PendingId::Assigned(0)
            },
            Operation::AssertValue {
                class,
                assert_value: AssertValue::None,
            },
            ..
        ] if *a == account_id && *class == vault_class()
    )
}

#[test]
fn every_chunk_of_a_fenced_pass_opens_with_the_assertion() {
    let mut batch = pass(FENCED, true);
    assert!(
        batch.commit_point_count() >= 2,
        "the fixture must split into several chunks"
    );
    let mut commit_points = batch.commit_points();
    let mut chunks = 0;
    for commit_point in commit_points.iter() {
        let built = batch.build_one(commit_point);
        if built.ops.is_empty() {
            continue;
        }
        chunks += 1;
        assert!(is_fence(built.ops, FENCED), "chunk {chunks} is not fenced");
        assert_eq!(
            built
                .ops
                .iter()
                .filter(|op| matches!(op, Operation::AssertValue { .. }))
                .count(),
            1
        );
        // Every other operation is the pass's own, self-keyed.
        assert!(built.ops[4..].iter().all(|op| matches!(
            op,
            Operation::Value {
                class: ValueClass::SearchIndex(_),
                ..
            }
        )));
    }
    assert!(chunks >= 3);
}

#[test]
fn an_unfenced_pass_carries_no_assertion() {
    let batch = pass(PLAIN, false);
    assert!(batch.commit_point_count() >= 2);
    assert!(
        !batch
            .ops()
            .iter()
            .any(|op| matches!(op, Operation::AssertValue { .. }))
    );
}

#[test]
fn chunk_assertions_refuse_a_batch_that_is_not_a_search_pass() {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(FENCED)
        .with_collection(Collection::Email)
        .with_document(1)
        .set(ValueClass::Property(1), vec![1u8]);
    let before = batch.ops().len();
    assert!(
        batch
            .za_assert_absent_in_every_chunk(&[(FENCED, Collection::Principal, 0, vault_class())])
            .is_err()
    );
    assert_eq!(batch.ops().len(), before);
}

async fn write_vault(store: &Store, account_id: u32) {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .set(vault_class(), vec![1u8]);
    store.write_batch(&mut batch).await.unwrap();
}

#[tokio::test]
async fn a_fenced_pass_writes_no_chunk_once_the_vault_exists() {
    let store = EphemeralStore::open();

    // Ordinary: the pass is written whole.
    store
        .index_account_documents(
            PLAIN,
            SearchIndex::Email,
            vec![large_document(PLAIN, 1, true)],
            vec![],
        )
        .await
        .unwrap();
    assert!(
        store
            .has_account_search_terms(SearchIndex::Email, PLAIN)
            .await
            .unwrap()
    );

    // Provisioned: the first chunk fails, and no chunk is written.
    write_vault(&store, FENCED).await;
    let err = store
        .index_account_documents(
            FENCED,
            SearchIndex::Email,
            vec![large_document(FENCED, 1, true)],
            vec![],
        )
        .await
        .unwrap_err();
    assert!(err.is_assertion_failure());
    assert!(
        !store
            .has_account_search_terms(SearchIndex::Email, FENCED)
            .await
            .unwrap()
    );

    // Control: the same pass without the fence is written, so the
    // assertion is what refused it.
    write_vault(&store, CONTROL).await;
    store
        .index_account_documents(
            CONTROL,
            SearchIndex::Email,
            vec![large_document(CONTROL, 1, false)],
            vec![],
        )
        .await
        .unwrap();
    assert!(
        store
            .has_account_search_terms(SearchIndex::Email, CONTROL)
            .await
            .unwrap()
    );
}

#[test]
fn accounted_input_bytes_counts_values_logs_and_merges() {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(PLAIN)
        .with_collection(Collection::Calendar)
        .with_document(7)
        .set(ValueClass::Property(1), vec![0u8; 1000])
        .clear(ValueClass::Property(2))
        .clear_if_equals(ValueClass::Property(3), 7u32)
        .log_container_insert(SyncCollection::Calendar);
    let bytes = batch.accounted_input_bytes();

    // The change log is materialized, as `commit_points()` would.
    let logs = batch
        .ops()
        .iter()
        .filter(|op| matches!(op, Operation::Log { .. }))
        .count();
    assert_eq!(logs, 1);
    let merges = batch
        .ops()
        .iter()
        .filter(|op| {
            matches!(
                op,
                Operation::Value {
                    op: ValueOp::MergeFnc(_),
                    ..
                }
            )
        })
        .count();
    assert_eq!(merges, 1);

    let operations = batch.ops().len();
    let key = ValueClass::Property(1).key_len_hint();
    assert!(bytes >= operations * ZA_OVERHEAD + 1000 + key);
    // Nothing beyond the overhead, the keys, the value and the log.
    assert!(bytes <= operations * ZA_OVERHEAD + 1000 + 3 * key + 128);
    // Idempotent: nothing is left to materialize.
    assert_eq!(batch.accounted_input_bytes(), bytes);
}

const ZA_OVERHEAD: usize = 64;
