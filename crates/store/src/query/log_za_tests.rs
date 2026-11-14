/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::{
    Key, Subspace,
    backend::ephemeral::EphemeralStore,
    write::{AnyClass, BatchBuilder, PendingId, ValueClass},
    za_search::Checkpoint,
};
use types::collection::Collection;

const ACCOUNT: u32 = 7;
const EMAIL: SyncCollection = SyncCollection::Email;

async fn write_record(store: &Store, inserts: &[u32], deletes: &[u32]) -> u64 {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(ACCOUNT)
        .with_collection(Collection::Email);
    for &id in inserts {
        batch
            .with_document(id)
            .log_item_insert(EMAIL, Some(PendingId::Assigned(id)));
    }
    for &id in deletes {
        batch
            .with_document(id)
            .log_item_delete(EMAIL, Some(PendingId::Assigned(id)));
    }
    store
        .write_batch(&mut batch)
        .await
        .unwrap()
        .last_change_id(ACCOUNT, EMAIL)
}

#[tokio::test]
async fn za_changes_records_are_whole_and_unfolded() {
    let store = EphemeralStore::open();
    let c1 = write_record(&store, &[1, 2], &[]).await;
    let c2 = write_record(&store, &[3], &[1]).await;

    let read = store
        .za_changes_records(ACCOUNT, EMAIL, None, 10)
        .await
        .unwrap();
    assert_eq!(read.records.len(), 2);
    assert!(!read.truncated && !read.resumed_record_lost);
    assert_eq!(read.first_retained, None);
    assert_eq!(
        store.za_log_first_retained(ACCOUNT, EMAIL).await.unwrap(),
        None
    );
    let item = |id: u64| (id << 32) | id;
    assert!(c2 > c1);
    assert_eq!(
        read.records[0],
        (
            c1,
            vec![Change::InsertItem(item(1)), Change::InsertItem(item(2))]
        )
    );
    assert_eq!(
        read.records[1],
        (
            c2,
            vec![Change::InsertItem(item(3)), Change::DeleteItem(item(1))]
        )
    );

    let folded = store
        .changes(ACCOUNT, EMAIL.into(), Query::All)
        .await
        .unwrap();
    assert!(
        !folded
            .changes
            .iter()
            .any(|change| change.item_id() == Some(item(1)))
    );
}

#[tokio::test]
async fn za_changes_resume_includes_partial_and_skips_complete_records() {
    let store = EphemeralStore::open();
    let c1 = write_record(&store, &[1, 2], &[]).await;
    let c2 = write_record(&store, &[3], &[]).await;
    let partial = Checkpoint {
        change_id: c1,
        position: 1,
        complete: false,
    };
    let read = store
        .za_changes_records(ACCOUNT, EMAIL, Some(partial), 10)
        .await
        .unwrap();
    assert_eq!(read.records[0].0, c1);
    assert_eq!(read.records[0].1.len(), 2);

    let complete = Checkpoint {
        change_id: c1,
        position: u32::MAX,
        complete: true,
    };
    let read = store
        .za_changes_records(ACCOUNT, EMAIL, Some(complete), 10)
        .await
        .unwrap();
    assert_eq!(read.records[0].0, c2);
}

#[tokio::test]
async fn za_changes_reports_the_truncation_marker_and_names_a_lost_partial_record() {
    let store = EphemeralStore::open();
    let c1 = write_record(&store, &[1], &[]).await;
    let c2 = write_record(&store, &[2], &[]).await;
    let c3 = write_record(&store, &[3], &[]).await;
    let collection = LogCollection::Sync(EMAIL);
    let marker = collection.log_key(ACCOUNT, c2);
    let mut batch = BatchBuilder::new();
    batch.with_account_id(ACCOUNT).set(
        ValueClass::Any(AnyClass {
            subspace: Subspace::Logs,
            key: marker.serialize(0),
        }),
        Vec::new(),
    );
    store.write_batch(&mut batch).await.unwrap();
    store
        .delete_range(collection.log_key(ACCOUNT, 0), marker)
        .await
        .unwrap();

    let read = store
        .za_changes_records(ACCOUNT, EMAIL, None, 1)
        .await
        .unwrap();
    assert!(read.truncated && !read.resumed_record_lost);
    assert_eq!(read.first_retained, Some(c2));
    assert_eq!(
        read.records.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![c3]
    );

    let partial = Checkpoint {
        change_id: c2,
        position: 0,
        complete: false,
    };
    let read = store
        .za_changes_records(ACCOUNT, EMAIL, Some(partial), 10)
        .await
        .unwrap();
    assert!(read.truncated && read.resumed_record_lost);
    let complete = Checkpoint {
        change_id: c2,
        position: u32::MAX,
        complete: true,
    };
    let read = store
        .za_changes_records(ACCOUNT, EMAIL, Some(complete), 10)
        .await
        .unwrap();
    assert!(!read.truncated && !read.resumed_record_lost);
    assert_eq!(read.first_retained, None);

    let range = store
        .za_changes_records_range(ACCOUNT, EMAIL, c1, c2)
        .await
        .unwrap();
    assert!(range.truncated && !range.resumed_record_lost);
    assert_eq!(range.first_retained, Some(c2));
    assert!(range.records.is_empty());
    assert_eq!(
        store.za_log_first_retained(ACCOUNT, EMAIL).await.unwrap(),
        Some(c2)
    );
    assert_eq!(
        store
            .za_log_first_retained(ACCOUNT + 1, EMAIL)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn za_changes_bounds_record_count_and_includes_the_range_end() {
    let store = EphemeralStore::open();
    let c1 = write_record(&store, &[1], &[]).await;
    let c2 = write_record(&store, &[2], &[]).await;
    let c3 = write_record(&store, &[3], &[]).await;

    let read = store
        .za_changes_records(ACCOUNT, EMAIL, None, 1)
        .await
        .unwrap();
    assert_eq!(read.records.len(), 1);
    assert_eq!(read.records[0].0, c1);
    let read = store
        .za_changes_records(ACCOUNT, EMAIL, None, 0)
        .await
        .unwrap();
    assert!(read.records.is_empty());
    let range = store
        .za_changes_records_range(ACCOUNT, EMAIL, c1, c2)
        .await
        .unwrap();
    assert_eq!(
        range.records.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        vec![c2]
    );
    let range = store
        .za_changes_records_range(ACCOUNT, EMAIL, c3, c2)
        .await
        .unwrap();
    assert!(range.records.is_empty());
}
