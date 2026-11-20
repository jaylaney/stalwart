/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access fork (PR #90 review C4617611506): the property a calendar
//! delete's change-id fence relies on, on every store backend the unit tests
//! build. A batch's change ids are allocated before its merge functions run,
//! so a merge function sees the id assigned to the batch, and an error from a
//! merge function aborts the whole transaction: the counter, the log record
//! and every write before it.

use crate::{
    Store, ValueKey,
    write::{BatchBuilder, MergeResult, PendingId, ValueClass, serialize::RawValue},
};
use std::sync::{Arc, Mutex};
use types::collection::{Collection, SyncCollection};

const ACCOUNT: u32 = 0x5a5a_0501;

fn change_counter() -> ValueKey<ValueClass> {
    ValueKey {
        account_id: ACCOUNT,
        collection: 0,
        document_id: 0,
        class: ValueClass::ChangeId(SyncCollection::Calendar.into()),
    }
}

fn written(document_id: u32) -> ValueKey<ValueClass> {
    ValueKey {
        account_id: ACCOUNT,
        collection: Collection::CalendarEvent.into(),
        document_id,
        class: ValueClass::Property(1),
    }
}

/// A batch that logs one Calendar change of `document_id`, writes one value
/// of it, then runs a merge function that checks the batch's assigned change
/// id against `expected`.
fn fenced_batch(document_id: u32, expected: u64, seen: Arc<Mutex<Option<u64>>>) -> BatchBuilder {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(ACCOUNT)
        .with_collection(Collection::CalendarEvent)
        .with_document(document_id)
        .set(ValueClass::Property(1), vec![1u8])
        .log_item_insert(
            SyncCollection::Calendar,
            Some(PendingId::Assigned(document_id)),
        );
    batch
        .with_collection(Collection::Principal)
        .with_document(0)
        .merge_fnc(ValueClass::Property(2), move |ids, _| {
            let assigned = ids.change_id(ACCOUNT, SyncCollection::Calendar);
            *seen.lock().unwrap() = assigned;
            if assigned != Some(expected) {
                return Err(trc::StoreEvent::AssertValueFailed.into_err());
            }
            Ok(MergeResult::Skip)
        });
    batch
}

/// Whether the backend rolls back a transaction whose merge function fails.
async fn check(store: &Store, rolls_back: bool) {
    let mut first = BatchBuilder::new();
    first
        .with_account_id(ACCOUNT)
        .with_collection(Collection::CalendarEvent)
        .with_document(1)
        .log_item_insert(SyncCollection::Calendar, Some(PendingId::Assigned(1)));
    store.write_batch(&mut first).await.unwrap();
    let tip = store.get_counter(change_counter()).await.unwrap() as u64;
    assert!(tip > 0);

    // The id assigned to the batch is visible to its merge function: tip + 1.
    let seen = Arc::new(Mutex::new(None));
    let ids = store
        .write_batch(&mut fenced_batch(2, tip + 1, seen.clone()))
        .await
        .expect("the fence holds for the next change id");
    assert_eq!(*seen.lock().unwrap(), Some(tip + 1));
    assert_eq!(
        ids.change_id(ACCOUNT, SyncCollection::Calendar),
        Some(tip + 1)
    );
    assert!(
        store
            .get_value::<RawValue>(written(2))
            .await
            .unwrap()
            .is_some()
    );

    // A stale expectation fails the batch with an assertion failure.
    let seen = Arc::new(Mutex::new(None));
    let error = store
        .write_batch(&mut fenced_batch(3, tip + 1, seen.clone()))
        .await
        .expect_err("a stale fence fails the commit");
    assert!(error.is_assertion_failure(), "{error:?}");
    assert_eq!(*seen.lock().unwrap(), Some(tip + 2));
    let counter = store.get_counter(change_counter()).await.unwrap() as u64;
    let last_log = store
        .get_last_change_id(ACCOUNT, SyncCollection::Calendar.into())
        .await
        .unwrap();
    let value = store.get_value::<RawValue>(written(3)).await.unwrap();
    if rolls_back {
        assert_eq!(counter, tip + 1, "the failed batch's change id was kept");
        assert_eq!(last_log, Some(tip + 1), "the failed batch's log record");
        assert!(value.is_none(), "a write before the failed merge function");
    } else {
        // The ephemeral store applies each operation as it goes, for a failed
        // assertion as well: pinned so that a change is seen. It is the data
        // store only in bootstrap mode, where no key account exists, and a
        // key calendar's delete takes no fence on it (PR #90 review RA-2, RA-7).
        assert_eq!(counter, tip + 2);
        assert!(value.is_some());
    }
}

#[tokio::test]
async fn ephemeral_merge_sees_the_assigned_change_id() {
    check(&crate::backend::ephemeral::EphemeralStore::open(), false).await;
}

#[cfg(all(feature = "sqlite", feature = "test_mode"))]
#[tokio::test]
async fn sqlite_merge_sees_the_assigned_change_id_and_aborts() {
    let store = Store::SQLite(Arc::new(
        crate::backend::sqlite::SqliteStore::open_memory().unwrap(),
    ));
    check(&store, true).await;
}

#[cfg(feature = "rocks")]
#[tokio::test]
async fn rocksdb_merge_sees_the_assigned_change_id_and_aborts() {
    let path = std::env::temp_dir().join(format!(
        "za-change-id-fence-{}-{}",
        std::process::id(),
        utils::snowflake::SnowflakeIdGenerator::new().generate()
    ));
    let store =
        crate::backend::rocksdb::RocksDbStore::open(registry::schema::structs::RocksDbStore {
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    check(&store, true).await;
    drop(store);
    let _ = std::fs::remove_dir_all(&path);
}
