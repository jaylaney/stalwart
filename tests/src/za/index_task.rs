/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Plan 3 ruling R8: an index task naming a destroyed key account leaves the
//! queue instead of retrying forever.

use super::{STRONG, user_permissions};
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use groupware::cache::GroupwareCache;
use hyper::StatusCode;
use registry::schema::{
    enums::IndexDocumentType,
    structs::{Task, TaskIndexDocument, TaskStatus},
};
use std::time::Duration;
use store::write::BatchBuilder;
use types::{collection::SyncCollection, id::Id};

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
