/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Plan 3 ruling R8: an index task naming a destroyed key account leaves the
//! queue instead of retrying forever.

use super::{STRONG, user_permissions};
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use groupware::{
    cache::GroupwareCache,
    calendar::{CalendarEvent, seal::archived_event_is_sealed},
};
use hyper::StatusCode;
use registry::schema::{
    enums::IndexDocumentType,
    structs::{Task, TaskIndexDocument, TaskStatus},
};
use std::time::Duration;
use store::{
    Deserialize, Serialize, ValueKey,
    write::{AlignedBytes, Archive, Archiver, BatchBuilder},
};
use types::{
    collection::{Collection, SyncCollection},
    id::Id,
};

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
const NAME: &str = "key9@example.com";
const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:r8-event\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:r8-canary\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access destroyed-account index task test...");
    let admin = test.account("admin@example.com").clone();
    let account = admin
        .create_key_user_account(NAME, STRONG, "Key Nine", &[], user_permissions())
        .await;
    let id = account.id().document_id();
    let client = DummyWebDavClient::new(id, NAME, STRONG, NAME);
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/key9%40example.com/default/r8.ics",
            [CONTENT_TYPE],
            EVENT,
        )
        .await
        .with_status(StatusCode::CREATED);
    test.wait_for_tasks().await;
    let document_id = test
        .server
        .fetch_dav_resources(id, id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("default/r8.ics")
        .unwrap()
        .document_id();

    // The registry entry, the data and the search index of the account go.
    admin.destroy_account(account).await;
    test.wait_for_tasks().await;
    assert!(test.server.try_account(id).await.unwrap().is_none());

    // R8's state: an index task naming the destroyed account.
    let mut batch = BatchBuilder::new();
    batch.schedule_task(Task::IndexDocument(TaskIndexDocument {
        account_id: Id::from(id),
        document_id: Id::from(document_id),
        document_type: IndexDocumentType::Calendar,
        status: TaskStatus::now(),
    }));
    test.server.store().write(batch.build_all()).await.unwrap();
    test.server.notify_task_queue();

    // About 15 s: three missing-document retries five seconds apart.
    tokio::time::timeout(Duration::from_secs(60), test.wait_for_tasks())
        .await
        .expect("an index task for a destroyed account must drain");
}

/// Plan 3's R8 fall-through: an index task that reaches a sealed archive
/// after the key-account check (the account was destroyed in between)
/// must not index it. The window cannot be opened on demand, so the test
/// plants a key account's sealed archive under a plain account, which the
/// key-account check lets through, and schedules its index task. A
/// control copy without the trailing `X-ZA-KEY` is planted beside it and
/// must be indexed, so the test cannot pass with calendar indexing off.
pub async fn test_sealed_archive(test: &mut TestServer) {
    println!("Running zero-access sealed-archive index test...");
    const KEY: &str = "key10@example.com";
    const PLAIN: &str = "plain10@example.com";
    const PLANTED: u32 = u32::MAX - 3;
    const CONTROL: u32 = u32::MAX - 4;
    let admin = test.account("admin@example.com").clone();
    let key = admin
        .create_key_user_account(KEY, STRONG, "Key Ten", &[], user_permissions())
        .await;
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain Ten", &[], user_permissions())
        .await;
    let key_id = key.id().document_id();
    let plain_id = plain.id().document_id();

    // A sealed archive, written by the real PUT path.
    DummyWebDavClient::new(key_id, KEY, STRONG, KEY)
        .request_with_headers(
            "PUT",
            "/dav/cal/key10%40example.com/default/sealed.ics",
            [CONTENT_TYPE],
            EVENT,
        )
        .await
        .with_status(StatusCode::CREATED);
    test.wait_for_tasks().await;
    let document_id = test
        .server
        .fetch_dav_resources(key_id, key_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("default/sealed.ics")
        .unwrap()
        .document_id();
    let sealed = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            key_id,
            Collection::CalendarEvent,
            document_id,
        ))
        .await
        .unwrap()
        .unwrap();
    assert!(archived_event_is_sealed(
        sealed.unarchive::<CalendarEvent>().unwrap()
    ));
    let sealed_event = sealed.deserialize::<CalendarEvent>().unwrap();
    let sealed = Archiver::new(sealed_event.clone()).serialize().unwrap();

    // The control: the same event without the root's trailing X-ZA-KEY.
    let mut control_event = sealed_event;
    control_event.data.event.components[0].entries.pop();
    let control = Archiver::new(control_event).serialize().unwrap();
    assert!(!archived_event_is_sealed(
        <Archive<AlignedBytes> as Deserialize>::deserialize(&control)
            .unwrap()
            .unarchive::<CalendarEvent>()
            .unwrap()
    ));

    // Plant both under the plain account, which the key-account check lets
    // through, and index them.
    let class = ValueKey::archive(plain_id, Collection::CalendarEvent, PLANTED).class;
    let control_class = ValueKey::archive(plain_id, Collection::CalendarEvent, CONTROL).class;
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(plain_id)
        .with_collection(Collection::CalendarEvent)
        .with_document(PLANTED)
        .set(class.clone(), sealed)
        .with_document(CONTROL)
        .set(control_class.clone(), control);
    for document_id in [PLANTED, CONTROL] {
        batch.schedule_task(Task::IndexDocument(TaskIndexDocument {
            account_id: Id::from(plain_id),
            document_id: Id::from(document_id),
            document_type: IndexDocumentType::Calendar,
            status: TaskStatus::now(),
        }));
    }
    test.server.store().write(batch.build_all()).await.unwrap();
    test.server.notify_task_queue();
    tokio::time::timeout(Duration::from_secs(60), test.wait_for_tasks())
        .await
        .expect("the planted index tasks must drain");

    let violations = super::leak::scan(test, plain_id).await.violations;
    assert!(
        calendar_entries_of(&violations, CONTROL) > 0,
        "the control document was not indexed: {violations:?}"
    );
    assert_eq!(
        calendar_entries_of(&violations, PLANTED),
        0,
        "the sealed document was indexed: {violations:?}"
    );

    // Clear both plants and the control's index entries.
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(plain_id)
        .with_collection(Collection::CalendarEvent)
        .with_document(PLANTED)
        .clear(class)
        .with_document(CONTROL)
        .clear(control_class);
    batch.schedule_task(Task::UnindexDocument(TaskIndexDocument {
        account_id: Id::from(plain_id),
        document_id: Id::from(CONTROL),
        document_type: IndexDocumentType::Calendar,
        status: TaskStatus::now(),
    }));
    test.server.store().write(batch.build_all()).await.unwrap();
    test.server.notify_task_queue();
    tokio::time::timeout(Duration::from_secs(60), test.wait_for_tasks())
        .await
        .expect("the control unindex task must drain");
    let violations = super::leak::scan(test, plain_id).await.violations;
    assert_eq!(
        calendar_entries_of(&violations, CONTROL),
        0,
        "the control document was not unindexed: {violations:?}"
    );

    admin.destroy_account(key).await;
    admin.destroy_account(plain).await;
    test.wait_for_tasks().await;
}

/// Calendar search-index entries that `leak::scan` flagged for one document.
/// Every account-scoped search-index key ends with the document id (u32 BE,
/// `store/src/write/key.rs`), and the violation text prints the key as a
/// byte list followed by the reason.
fn calendar_entries_of(violations: &[String], document_id: u32) -> usize {
    let [a, b, c, d] = document_id.to_be_bytes();
    let suffix = format!(", {a}, {b}, {c}, {d}]: calendar search index entry of the account");
    violations.iter().filter(|v| v.ends_with(&suffix)).count()
}
