/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The index queue's key-account gates and fences at 1.0 (Task 6).
//!
//! - Plan 3 ruling R8: an index entry naming a destroyed key account drains
//!   instead of retrying forever (`test`), and a sealed archive that reaches
//!   the calendar builder is not indexed (`test_sealed_archive`).
//! - Mail spec 5.5 and ruling 64: a reindex of a key account rebuilds its
//!   Email documents with their ids only, no term, no thread or header
//!   material and no attachment text (`test_reindex`); the attachment
//!   indexer never reads a sealed blob (`test_attachment_indexer`).
//! - The un-index of a key account's destroyed stub clears its row, sort
//!   keys, blob link and search document without a failure
//!   (`test_destroyed_stub`).
//! - No terminal failure at 1.0: a failing partition retries with backoff and
//!   blocks provisioning with R54's 409 until `x:IndexQueueStatus/set` resets
//!   it (`test_index_retry`).
//! - Ruling 18: provisioning the account between two commit chunks of an
//!   index pass stops every later chunk (`test_mid_pass_provisioning`), and
//!   deleting it mid-pass leaves no orphan (`test_mid_pass_deletion`, run last
//!   in the suite: see its documentation).

use super::{
    STRONG,
    dav_seal::raw_event_records,
    mail::{message_ids, message_with, metadata_rows, stored_metadata, threading_keys},
    user_permissions,
};
use crate::utils::{
    account::Account,
    server::TestServer,
    smtp::SmtpConnection,
    webdav::DummyWebDavClient,
    za::{
        HookParked, delete_event_records, hold_index_partition, partition_status, plant_event,
        plant_plain_message, queue_entries, release_index_partition, search_keys,
        unindex_search_account, unindex_search_documents, wait_for_delivery, wait_for_index_queue,
        za_post_as, za_setup, za_setup_token,
    },
};
use common::za::test_hooks::{self, ForceCommitSplit};
use email::message::{sortkeys::MessageSortKeys, thread::ThreadSubject};
use groupware::calendar::{CalendarEventContent, seal::archived_event_is_sealed};
use hyper::StatusCode;
use registry::schema::{
    enums::TaskAccountMaintenanceType,
    prelude::ObjectType,
    structs::{Task, TaskAccountMaintenance, TaskStatus},
};
use serde_json::json;
use std::time::Duration;
use store::{
    Serialize as _, ValueKey,
    write::{
        Archiver, BatchBuilder, IndexPropertyClass, SearchIndex, SearchIndexClass, ValueClass,
        serialize::RawValue,
    },
};
use types::{
    blob::BlobClass,
    collection::Collection,
    field::{CalendarEventField, EmailField},
    id::Id,
};
use vault::record::VaultState;

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
const NAME: &str = "key9@example.com";
const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:r8-event\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:r8-canary\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

/// `test_reindex`'s and `test_attachment_indexer`'s accounts.
const REINDEX_KEY: &str = "keymail30@example.com";
const REINDEX_PLAIN: &str = "plainmail27@example.com";
const ATTACH_KEY: &str = "keymail31@example.com";
const ATTACH_PLAIN: &str = "plainmail28@example.com";
/// `test_destroyed_stub`'s account.
const STUB_KEY: &str = "keymail32@example.com";
/// `test_destroyed_stub`'s ordinary control (ruling 134).
const STUB_PLAIN: &str = "plainmail38@example.com";
/// A message whose header section starts with the stub's format byte and
/// whose thread subject is in its headers: two `Subject` headers, the last
/// one empty (`ArchivedMessageMetadata::thread_subject`).
const IN_HEADERS_MESSAGE: &str =
    "\u{1}X-Za-Stub: like\r\nSubject: zz in headers\r\nSubject:\r\n\r\nin headers body\r\n";
/// `test_index_retry`'s account (ordinary, then provisioned).
const RETRY: &str = "plainmail29@example.com";
/// `test_mid_pass_provisioning`'s account (provisioned mid-pass) and its
/// ordinary control.
const MID_PASS: &str = "plainmail30@example.com";
const MID_PASS_CONTROL: &str = "plainmail31@example.com";
/// `test_mid_pass_deletion`'s account.
const MID_PASS_DELETED: &str = "plainmail32@example.com";
/// `test_mid_pass_deletion`'s probe, created after the destruction: its id
/// is never the destroyed one (ruling 69).
const MID_PASS_PROBE: &str = "plainmail36@example.com";

/// Tokens of up to 16 bytes, which a term key holds verbatim
/// (`CheekyHash`), so their absence from a key account's index and their
/// presence in the ordinary control's are both read off the keys.
const SUBJECT_TOKEN: &str = "zzsubjectcanary";
const BODY_TOKEN: &str = "zzbodycanary";
const HEADER_TOKEN: &str = "zzheadercanary";
const ATTACH_TOKEN: &str = "zzattachcanary";

/// A message whose subject, body, a custom header and a text attachment
/// each carry a token, with an RTF attachment too, so the plaintext row has
/// an extractable attachment (`has_extractable_attachments`) and the builder
/// takes its blocking extraction path.
pub(super) fn attachment_message(n: u32) -> String {
    padded_attachment_message(n, 0)
}

/// `attachment_message` with `padding` lines of `X-Za-Padding` (80 bytes
/// each, well under SMTP's line limit), so messages differ in size.
fn padded_attachment_message(n: u32, padding: usize) -> String {
    let padding = format!("X-Za-Padding: {}\r\n", "p".repeat(64)).repeat(padding);
    format!(
        "From: Sender <sender@example.org>\r\n\
         To: <recipient@example.com>\r\n\
         Subject: {SUBJECT_TOKEN} {n}\r\n\
         Message-ID: <za-index-{n}.{:016x}@example.org>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         X-Za-Note: {HEADER_TOKEN}\r\n\
         {padding}\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"za-index\"\r\n\
         \r\n\
         --za-index\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         The body holds {BODY_TOKEN}.\r\n\
         --za-index\r\n\
         Content-Type: text/plain; name=\"notes.txt\"\r\n\
         Content-Disposition: attachment; filename=\"notes.txt\"\r\n\
         \r\n\
         The attachment holds {ATTACH_TOKEN}.\r\n\
         --za-index\r\n\
         Content-Type: text/rtf; name=\"notes.rtf\"\r\n\
         Content-Disposition: attachment; filename=\"notes.rtf\"\r\n\
         \r\n\
         {{\\rtf1\\ansi rtf {ATTACH_TOKEN}}}\r\n\
         --za-index--\r\n",
        store::rand::random::<u64>()
    )
}

async fn passwordless(admin: &Account, name: &'static str) -> Account {
    admin
        .create_passwordless_user_account(name, STRONG, "Index", &[], user_permissions())
        .await
}

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
    let (_, document_id) = raw_event_records(test, id, "default/r8.ics").await;

    // Ruling 135 (M8, mutation b009; calendar spec 9): a key account's
    // event that is not sealed (a legacy event, planted as a 0.16-era
    // writer left it) is not indexed either. Its entry drains at once
    // without a document; a build of it would carry R58's vault-absence
    // fence, fail against the account's vault record and retry, so the
    // bounded wait names that cause.
    let legacy = plant_event(
        test,
        id,
        "default",
        "legacy.ics",
        &EVENT
            .replace("UID:r8-event", "UID:r8-legacy")
            .replace("r8-canary", "r8-legacy-canary"),
        None,
    )
    .await;
    wait_for_index_queue(test, id, Duration::from_secs(30)).await;
    assert!(
        search_keys(test, id, SearchIndex::Calendar)
            .await
            .is_empty(),
        "a key account's unsealed event was indexed"
    );
    delete_event_records(test, id, legacy).await;
    test.wait_for_tasks().await;

    // The registry entry, the data, the search index and the index queue of
    // the account go.
    admin.destroy_account(account).await;
    test.wait_for_tasks().await;
    assert!(test.server.try_account(id).await.unwrap().is_none());

    // R8's state at 1.0: an index-queue entry naming the destroyed account.
    // The builder reads no event (the account's records are gone), so the
    // entry drains without a document.
    let mut batch = BatchBuilder::new();
    batch.queue_document_index(SearchIndex::Calendar, id, document_id);
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_index_queue();
    wait_for_index_queue(test, id, Duration::from_secs(60)).await;
    assert!(
        search_keys(test, id, SearchIndex::Calendar)
            .await
            .is_empty(),
        "the destroyed account was indexed"
    );
}

/// Plan 3's R8 fall-through: an index entry that reaches a sealed archive
/// after the key-account check (the account was destroyed in between) must
/// not index it. The window cannot be opened on demand, so the test plants a
/// key account's sealed content record under a plain account, which the
/// key-account check lets through, and queues its index entry. A control
/// copy without the trailing `X-ZA-KEY` is planted beside it and must be
/// indexed, so the test cannot pass with calendar indexing off. At 1.0 the
/// calendar builder reads the content record only
/// (`CalendarEventField::Content`).
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

    // A sealed content record, written by the real PUT path.
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
    let (records, _) = raw_event_records(test, key_id, "default/sealed.ics").await;
    assert!(archived_event_is_sealed(
        records.content.unarchive::<CalendarEventContent>().unwrap()
    ));
    let sealed_content = records
        .content
        .deserialize::<CalendarEventContent>()
        .unwrap();
    let sealed = Archiver::new(sealed_content.clone()).serialize().unwrap();

    // The control: the same content without the root's trailing X-ZA-KEY.
    let mut control_content = sealed_content;
    control_content.data.event.components[0].entries.pop();
    let control = Archiver::new(control_content).serialize().unwrap();
    assert!(!archived_event_is_sealed(
        <store::write::Archive<store::write::ArchiveBytes> as store::Deserialize>::deserialize(
            &control
        )
        .unwrap()
        .unarchive::<CalendarEventContent>()
        .unwrap()
    ));

    // Plant both under the plain account, which the key-account check lets
    // through, and queue their index entries.
    let class = ValueClass::Property(u8::from(CalendarEventField::Content));
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(plain_id)
        .with_collection(Collection::CalendarEvent)
        .with_document(PLANTED)
        .set(class.clone(), sealed)
        .with_document(CONTROL)
        .set(class.clone(), control);
    for document_id in [PLANTED, CONTROL] {
        batch.queue_document_index(SearchIndex::Calendar, plain_id, document_id);
    }
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_index_queue();
    wait_for_index_queue(test, plain_id, Duration::from_secs(60)).await;

    let keys = search_keys(test, plain_id, SearchIndex::Calendar).await;
    assert!(
        keys.document_ids().contains(&CONTROL),
        "the control document was not indexed: {keys:?}"
    );
    assert!(
        !keys.document_ids().contains(&PLANTED),
        "the sealed document was indexed: {keys:?}"
    );

    // Clear both plants and the control's index entries.
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(plain_id)
        .with_collection(Collection::CalendarEvent)
        .with_document(PLANTED)
        .clear(class.clone())
        .with_document(CONTROL)
        .clear(class);
    batch.queue_document_unindex(SearchIndex::Calendar, plain_id, CONTROL);
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_index_queue();
    wait_for_index_queue(test, plain_id, Duration::from_secs(60)).await;
    let keys = search_keys(test, plain_id, SearchIndex::Calendar).await;
    assert!(
        !keys.document_ids().contains(&CONTROL),
        "the control document was not unindexed: {keys:?}"
    );

    admin.destroy_account(key).await;
    admin.destroy_account(plain).await;
    test.wait_for_tasks().await;
}

/// Delivers `count` copies of `attachment_message` to each of `accounts`
/// and waits until they are delivered and indexed.
async fn deliver_attachment_messages(test: &TestServer, accounts: &[&str], count: u32) {
    for n in 0..count {
        SmtpConnection::connect()
            .await
            .ingest("sender@example.org", accounts, &attachment_message(n))
            .await;
    }
    wait_for_delivery(test).await;
}

/// Mail spec 5.5 and ruling 64: the key account's Email search records are
/// one document per message holding its ids only, and no term; it holds no
/// threading key and its sort keys are empty.
async fn assert_visible_fields_only(test: &TestServer, account_id: u32, what: &str) {
    let messages = message_ids(test, account_id).await;
    let keys = search_keys(test, account_id, SearchIndex::Email).await;
    let mut documents = keys.document_ids();
    documents.sort_unstable();
    assert_eq!(documents, messages, "{what}: one document per message");
    assert!(
        keys.documents_are_ids_only(),
        "{what}: a document holds more than its ids: {keys:?}"
    );
    assert_eq!(keys.terms, 0, "{what}: the index holds terms: {keys:?}");
    for document_id in messages {
        assert_eq!(
            threading_keys(test, account_id, document_id).await,
            0,
            "{what}: message {document_id} has a threading key"
        );
        let sort_keys = test
            .server
            .store()
            .get_value::<RawValue>(ValueKey::immutable(
                account_id,
                Collection::Email,
                document_id,
                EmailField::SortKeys,
            ))
            .await
            .unwrap()
            .map(|value| value.0);
        assert!(
            sort_keys
                .as_deref()
                .is_none_or(|value| value == MessageSortKeys::default().serialize().as_slice()),
            "{what}: message {document_id} has sort keys"
        );
    }
    let found = super::leak::scan(test, account_id).await;
    assert!(
        found.violations.is_empty(),
        "{what}: leaks:\n{}",
        found.violations.join("\n")
    );
}

/// The ordinary control's index holds every token, so the key account's
/// absence of them is not an artefact of indexing being off.
async fn assert_indexes_tokens(test: &TestServer, account_id: u32, what: &str) {
    let keys = search_keys(test, account_id, SearchIndex::Email).await;
    assert_eq!(
        keys.document_ids().len(),
        message_ids(test, account_id).await.len(),
        "{what}: one document per message: {keys:?}"
    );
    for token in [SUBJECT_TOKEN, BODY_TOKEN, ATTACH_TOKEN] {
        assert!(
            keys.holds_term(token),
            "{what}: the control's index does not hold {token}: {keys:?}"
        );
    }
}

/// `reindex_key_account_stays_visible_fields_only` (the plan's Task 6): a
/// key account's and an ordinary control's messages, with tokens in the
/// subject, the body, a header and the attachments, are indexed; their Email
/// search records are then removed (the account-wide range delete) and
/// rebuilt by the account reindex task (`AccountMaintenance`, `Reindex`: at
/// 1.0 it only queues entries, `reindex_account`). The key account's records
/// are rebuilt with their ids only (ruling 64: 1.0's Email index has no
/// arrival-time or size field), no term and no thread, header or
/// attachment material; the control's hold every token. The arrival time
/// and the size still sort the key account's messages, served from the
/// message cache.
pub async fn test_reindex(test: &mut TestServer) {
    println!("Running zero-access reindex test...");
    let admin = test.account("admin@example.com").clone();
    let key = admin
        .create_key_user_account(REINDEX_KEY, STRONG, "Reindex", &[], user_permissions())
        .await;
    let plain = admin
        .create_user_account(REINDEX_PLAIN, STRONG, "Reindex", &[], user_permissions())
        .await;
    let (key_id, plain_id) = (key.id().document_id(), plain.id().document_id());
    // The review's F7: the first message is the larger and arrives a second
    // before the second, so the two sorts below answer opposite orders, and
    // neither is the order of the ids.
    for (n, padding) in [(0, 64), (1, 0)] {
        SmtpConnection::connect()
            .await
            .ingest(
                "sender@example.org",
                &[REINDEX_KEY, REINDEX_PLAIN],
                &padded_attachment_message(n, padding),
            )
            .await;
        wait_for_delivery(test).await;
        tokio::time::sleep(Duration::from_millis(1100)).await;
    }
    assert_visible_fields_only(test, key_id, "delivered").await;
    assert_indexes_tokens(test, plain_id, "delivered").await;

    for (account, account_id) in [(&key, key_id), (&plain, plain_id)] {
        unindex_search_account(test, SearchIndex::Email, account_id).await;
        assert!(
            search_keys(test, account_id, SearchIndex::Email)
                .await
                .is_empty(),
            "the account-wide unindex left records"
        );
        admin
            .registry_create_object(Task::AccountMaintenance(TaskAccountMaintenance {
                account_id: account.id(),
                maintenance_type: TaskAccountMaintenanceType::Reindex,
                status: TaskStatus::now(),
            }))
            .await;
    }
    test.wait_for_tasks().await;
    assert_visible_fields_only(test, key_id, "reindexed").await;
    assert_indexes_tokens(test, plain_id, "reindexed").await;

    // The visible fields still answer: the arrival time and the size sort
    // the key account's messages, from the message cache. Each order is
    // checked against the values `Email/get` reports, and the two differ.
    let mut orders = Vec::new();
    for sort in ["receivedAt", "size"] {
        let reply = key
            .jmap_method_call(
                "Email/query",
                json!({
                    "accountId": key.id_string(),
                    "sort": [{ "property": sort, "isAscending": true }]
                }),
            )
            .await;
        let ids = reply.ids().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(
            ids.len(),
            2,
            "sorting by {sort} after the reindex: {:?}",
            reply.0
        );
        let reply = key
            .jmap_method_call(
                "Email/get",
                json!({
                    "accountId": key.id_string(),
                    "ids": &ids,
                    "properties": ["id", sort]
                }),
            )
            .await;
        let list = reply
            .pointer("/methodResponses/0/1/list")
            .and_then(|list| list.as_array())
            .unwrap_or_else(|| panic!("Email/get: {:?}", reply.0))
            .clone();
        let values = ids
            .iter()
            .map(|id| {
                list.iter()
                    .find(|email| email["id"] == id.as_str())
                    .map(|email| email[sort].clone())
                    .unwrap_or_else(|| panic!("{id} in Email/get: {:?}", reply.0))
            })
            .collect::<Vec<_>>();
        let ascending = match sort {
            "size" => values[0].as_u64() < values[1].as_u64(),
            _ => values[0].as_str() < values[1].as_str(),
        };
        assert!(
            ascending,
            "sorting by {sort} after the reindex is not ascending: {values:?}"
        );
        orders.push(ids);
    }
    assert_ne!(
        orders[0], orders[1],
        "the arrival-time and size orders must differ for these messages"
    );

    for account in [&key, &plain] {
        test.destroy_all_mailboxes(account).await;
    }
    test.wait_for_tasks().await;
    admin.destroy_account(key).await;
    admin.destroy_account(plain).await;
    test.wait_for_tasks().await;
}

/// `attachment_indexer_never_reads_sealed_blob` (the plan's Task 6): a key
/// account's message with an extractable attachment is indexed from its
/// stub without its blob being read. Its sealed blob is deleted from the
/// blob store and its index entry queued again: the pass completes at once
/// with an ids-only document (a blob read would fail, "Blob not found", and
/// put the partition in retry). The negative control proves the deletion
/// is detected: the same for an ordinary account's message fails its pass,
/// which is then cleared by the message's unindex (the remedy for a
/// permanent failure); the positive control, before the deletion, holds the
/// attachment's token from the attachment indexer.
pub async fn test_attachment_indexer(test: &mut TestServer) {
    println!("Running zero-access attachment indexer test...");
    let admin = test.account("admin@example.com").clone();
    let key = admin
        .create_key_user_account(ATTACH_KEY, STRONG, "Attach", &[], user_permissions())
        .await;
    let plain = admin
        .create_user_account(ATTACH_PLAIN, STRONG, "Attach", &[], user_permissions())
        .await;
    let (key_id, plain_id) = (key.id().document_id(), plain.id().document_id());
    deliver_attachment_messages(test, &[ATTACH_KEY, ATTACH_PLAIN], 1).await;
    assert_indexes_tokens(test, plain_id, "the control").await;
    assert_visible_fields_only(test, key_id, "delivered").await;

    let delete_blobs_and_requeue = async |account_id: u32| {
        let mut batch = BatchBuilder::new();
        for document_id in message_ids(test, account_id).await {
            let hash = stored_metadata(test, account_id, document_id)
                .await
                .unarchive()
                .unwrap()
                .blob_hash();
            test.server
                .blob_store()
                .delete_blob(hash.as_slice())
                .await
                .unwrap();
            batch.queue_document_index(SearchIndex::Email, account_id, document_id);
        }
        test.server.commit_batch(batch).await.unwrap();
        test.server.notify_index_queue();
    };

    // The key account: the pass completes well inside the 30 s retry delay.
    delete_blobs_and_requeue(key_id).await;
    wait_for_index_queue(test, key_id, Duration::from_secs(10)).await;
    assert_visible_fields_only(test, key_id, "re-indexed without its blob").await;

    // The ordinary control: its pass reads the blob and fails.
    delete_blobs_and_requeue(plain_id).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while partition_status(test, SearchIndex::Email, plain_id)
        .await
        .is_none()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the control's pass did not fail without its blob: the deletion is not detected"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // The remedy for a permanent failure: the message's unindex, then a
    // reset; the pass collapses the index and unindex entries into a
    // deletion and completes.
    admin.destroy_all_mailboxes_for_account(plain_id).await;
    release_index_partition(&test.server, SearchIndex::Email, plain_id).await;
    wait_for_index_queue(test, plain_id, Duration::from_secs(10)).await;

    test.destroy_all_mailboxes(&key).await;
    test.wait_for_tasks().await;
    admin.destroy_account(key).await;
    admin.destroy_account(plain).await;
    test.wait_for_tasks().await;
}

/// The un-index path at 1.0 (`delete_email_metadata` reads the stored row,
/// and its header section when the thread subject is in the headers): a key
/// account's destroyed message is a stub, and its unindex clears its row,
/// its sort keys, its blob link and its search document, and drains without
/// a failure. A stub's structure has no header entry, so its thread subject
/// resolves without reading the bundle; the arm that would read it treats a
/// header section that starts with the stub's format byte as empty
/// (`za_thread_subject_in`, unit-tested in `services::index_queue::za`).
pub async fn test_destroyed_stub(test: &mut TestServer) {
    println!("Running zero-access destroyed-stub unindex test...");
    let admin = test.account("admin@example.com").clone();
    let key = admin
        .create_key_user_account(STUB_KEY, STRONG, "Stub", &[], user_permissions())
        .await;
    let id = key.id().document_id();
    SmtpConnection::connect()
        .await
        .ingest(
            "sender@example.org",
            &[STUB_KEY],
            &message_with("stub subject", "stub body"),
        )
        .await;
    // Ruling 135 (M8, mutation b008): a key account's message is indexed
    // from its stub, so its partition drains at once. Bounded, so that a
    // build that reads the sealed blob (which fails and retries forever)
    // fails here by name instead of hanging the suite's later waits.
    tokio::time::timeout(Duration::from_secs(60), wait_for_delivery(test))
        .await
        .expect(
            "the key account's index partition did not drain: its message was not \
             indexed from its stub (mail spec 5.5)",
        );
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "{ids:?}");
    let document_id = ids[0];
    let stub = stored_metadata(test, id, document_id).await;
    assert!(super::mail::row_is_stub(&stub), "the message is not a stub");
    assert!(
        matches!(
            stub.unarchive().unwrap().thread_subject(),
            ThreadSubject::Resolved("")
        ),
        "a stub's thread subject is not resolved empty"
    );
    let hash = stub.unarchive().unwrap().blob_hash();
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert_eq!(keys.document_ids(), vec![document_id], "{keys:?}");

    // Ruling 134 (`ZaUnindexAccount::for_row`, mail rulings R29 and R47): a
    // row whose header section starts with the stub's format byte and whose
    // thread subject is in its headers (two `Subject` headers, the last one
    // empty), planted in the key account and in an ordinary control. Its
    // un-index reads the account's classification: the key account's takes
    // the stub's empty subject and leaves the threading key of the header
    // section's subject alone; the control takes upstream's subject and
    // clears it.
    let control = admin
        .create_user_account(STUB_PLAIN, STRONG, "Stub control", &[], user_permissions())
        .await;
    let control_id = control.id().document_id();
    let mut crafted = Vec::new();
    for account_id in [id, control_id] {
        let document_id = plant_plain_message(test, account_id, IN_HEADERS_MESSAGE).await;
        wait_for_index_queue(test, account_id, Duration::from_secs(10)).await;
        let row = stored_metadata(test, account_id, document_id).await;
        assert!(
            super::mail::row_is_stub(&row),
            "the row starts with the format byte"
        );
        assert_eq!(
            row.unarchive().unwrap().thread_subject(),
            ThreadSubject::InHeaders,
            "the fixture must reach the header-section arm"
        );
        assert_eq!(threading_keys(test, account_id, document_id).await, 1);
        crafted.push((account_id, document_id));
    }

    test.destroy_all_mailboxes(&key).await;
    test.destroy_all_mailboxes(&control).await;
    wait_for_index_queue(test, control_id, Duration::from_secs(10)).await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;
    let (_, key_doc) = crafted[0];
    let (_, control_doc) = crafted[1];
    assert!(metadata_rows(test, control_id).await.is_empty());
    assert_eq!(
        threading_keys(test, control_id, control_doc).await,
        0,
        "control: the un-index of an ordinary account's row clears its threading key"
    );
    assert_eq!(
        threading_keys(test, id, key_doc).await,
        1,
        "the key account's un-index read the row's header section as headers"
    );
    // The fixture's leftover: the key the header section's subject names.
    let message = mail_parser::MessageParser::new()
        .parse(IN_HEADERS_MESSAGE.as_bytes())
        .unwrap();
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Email)
        .with_document(key_doc)
        .clear(ValueClass::IndexProperty(IndexPropertyClass::Hash {
            property: EmailField::Threading.into(),
            hash: email::message::ingest::thread_hash(
                email::message::thread::ThreadFields::scan(&message).subject,
            ),
        }));
    test.server.store().write_batch(&mut batch).await.unwrap();
    assert_eq!(threading_keys(test, id, key_doc).await, 0);
    admin.destroy_account(control).await;

    assert!(
        metadata_rows(test, id).await.is_empty(),
        "the unindex left the stub's row"
    );
    assert!(
        test.server
            .store()
            .get_value::<RawValue>(ValueKey::immutable(
                id,
                Collection::Email,
                document_id,
                EmailField::SortKeys,
            ))
            .await
            .unwrap()
            .is_none(),
        "the unindex left the stub's sort keys"
    );
    assert!(
        test.server
            .store()
            .blob_link_size(
                &hash,
                BlobClass::Linked {
                    account_id: id,
                    collection: Collection::Email.into(),
                    document_id,
                },
            )
            .await
            .unwrap()
            .is_none(),
        "the unindex left the sealed blob linked"
    );
    assert!(
        search_keys(test, id, SearchIndex::Email).await.is_empty(),
        "the unindex left the search document"
    );
    admin.destroy_account(key).await;
    test.wait_for_tasks().await;
}

/// `setup-token` for the account `name` answers 409 with `reason` and
/// writes no record.
async fn assert_refused(test: &TestServer, admin: &Account, name: &str, id: u32, reason: &str) {
    let reply = za_post_as(admin, "setup-token", &json!({ "account": name })).await;
    assert_eq!(reply.expect(409)["error"], reason, "issuance of {name}");
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );
}

/// The `x:IndexQueueStatus` id of the account's partition in `index`
/// (`jmap/src/registry/mapping/index_queue.rs`: the index byte above the
/// partition).
fn queue_status_id(index: SearchIndex, account_id: u32) -> Id {
    Id::from(((index.to_u8() as u64) << 32) | account_id as u64)
}

/// The operator's remedy for a transient index failure (Task 9's CLAUDE.md
/// text): `x:IndexQueueStatus/set` with a `Running` status resets the
/// partition's retry state, so it retries at once; no entry is removed.
async fn reset_partition(admin: &Account, index: SearchIndex, account_id: u32) {
    admin
        .registry_update_object(
            ObjectType::IndexQueueStatus,
            queue_status_id(index, account_id),
            json!({ "status": { "@type": "Running" } }),
        )
        .await;
}

/// Waits (at most 10 s) until the account's partition in `index` is in
/// retry (its status key set by a failed pass).
async fn wait_for_retry_state(test: &TestServer, index: SearchIndex, account_id: u32) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while partition_status(test, index, account_id).await.is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the partition of account {account_id} did not enter its retry state"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The plan's index-queue fault test. At 1.0 a failing partition retries
/// with backoff (30 s, doubling, capped at 30 minutes) and never fails
/// terminally, so a queue entry blocks provisioning until it drains.
/// `INDEX_BUILD_FAIL_ONCE` fails the account's next document build once:
/// - the partition enters its retry state, its entry kept and nothing
///   indexed; `x:IndexQueueStatus/get` reports it failed; the reset makes it
///   retry at once (well inside the 30 s delay), and the retry builds the
///   event's document and drains the entry;
/// - with the account's events deleted while a second failure holds the
///   partition in retry, setup is refused with R54's 409 ("account has
///   unfinished tasks"), whose queue scan sees the entries and the status
///   key; after the reset the retry collapses each event's entries into a
///   deletion, the queue drains, and setup proceeds.
pub async fn test_index_retry(test: &mut TestServer) {
    println!("Running zero-access index-queue retry test...");
    test.wait_for_tasks().await;
    let admin = test.account("admin@example.com").clone();
    let account = passwordless(&admin, RETRY).await;
    let id = account.id().document_id();
    let ical = |uid: &str| {
        EVENT
            .replace("UID:r8-event", &format!("UID:{uid}"))
            .replace("r8-canary", "retry summary")
    };

    // A failed build, then the reset and a successful retry.
    services::index_queue::za::test_hooks::set_index_build_fail_once(Some(id));
    let first = plant_event(test, id, "default", "first.ics", &ical("retry-first"), None).await;
    wait_for_retry_state(test, SearchIndex::Calendar, id).await;
    assert!(
        services::index_queue::za::test_hooks::INDEX_BUILD_FAIL_ONCE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none(),
        "the injected failure was not taken"
    );
    let entries = queue_entries(test, id).await;
    assert!(
        entries
            .iter()
            .any(|entry| entry.index == SearchIndex::Calendar
                && entry.document_id == first
                && entry.insert),
        "the failed pass did not keep its entry: {entries:?}"
    );
    assert!(
        search_keys(test, id, SearchIndex::Calendar)
            .await
            .is_empty(),
        "the failed pass indexed the event"
    );
    let status = admin
        .registry_get_many(
            ObjectType::IndexQueueStatus,
            [queue_status_id(SearchIndex::Calendar, id)],
        )
        .await;
    assert_eq!(
        status.list()[0]["status"]["@type"],
        "Failed",
        "x:IndexQueueStatus/get: {:?}",
        status.0
    );
    reset_partition(&admin, SearchIndex::Calendar, id).await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;
    assert_eq!(
        search_keys(test, id, SearchIndex::Calendar)
            .await
            .document_ids(),
        vec![first],
        "the retry did not index the event"
    );

    // A second failure holds the partition in retry while both events are
    // deleted: setup is refused by R54's queue scan.
    services::index_queue::za::test_hooks::set_index_build_fail_once(Some(id));
    let second = plant_event(
        test,
        id,
        "default",
        "second.ics",
        &ical("retry-second"),
        None,
    )
    .await;
    wait_for_retry_state(test, SearchIndex::Calendar, id).await;
    for event_id in [first, second] {
        delete_event_records(test, id, event_id).await;
    }
    assert!(
        !test
            .server
            .za_has_documents(id, Collection::CalendarEvent)
            .await
            .unwrap(),
        "the events were not deleted"
    );
    assert_refused(test, &admin, RETRY, id, "account has unfinished tasks").await;

    reset_partition(&admin, SearchIndex::Calendar, id).await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;
    assert!(
        search_keys(test, id, SearchIndex::Calendar)
            .await
            .is_empty(),
        "the retry left the first event's document"
    );
    za_setup_token(&admin, RETRY).await;
    admin.destroy_account(account).await;
    test.wait_for_tasks().await;
}

/// Delivers two messages to `name` while its Email partition is held, so
/// one index pass reads both, then waits until both are committed with their
/// entries queued (`wait_for_delivery` would wait for the held partition).
/// Returns the message ids.
async fn two_messages_held(test: &TestServer, name: &str, id: u32) -> Vec<u32> {
    hold_index_partition(test, SearchIndex::Email, id).await;
    for n in 0..2 {
        SmtpConnection::connect()
            .await
            .ingest(
                "sender@example.org",
                &[name],
                &message_with(&format!("mid pass {n}"), &format!("midpassbody{n}")),
            )
            .await;
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let ids = message_ids(test, id).await;
        let queued = queue_entries(test, id)
            .await
            .iter()
            .filter(|entry| entry.index == SearchIndex::Email && entry.insert)
            .count();
        if ids.len() == 2 && queued == 2 {
            return ids;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the two messages were not delivered: {ids:?}, {queued} queued"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Starts the account's held pass with `FORCE_COMMIT_SPLIT` set, so its two
/// documents commit in two chunks: chunk 0 holds every term merge and the
/// first `Document` key, chunk 1 the second `Document` key
/// (`AccountIndexer::build_batch` calls `commit_point()` after each). The
/// pass parks at `INDEX_CHUNK_PAUSE` between them.
async fn park_between_chunks(test: &TestServer, id: u32) -> HookParked<()> {
    let server = test.server.clone();
    HookParked::start(id, test_hooks::set_index_chunk_pause, async move {
        release_index_partition(&server, SearchIndex::Email, id).await;
    })
    .await
}

/// Another node's execution of the partition (B), after A's lease ran out:
/// B reads the account's entries (each message's index and unindex, so two
/// deletions), unindexes the messages' metadata as `delete_email_metadata`
/// does, removes their stored search documents, and clears the queue
/// entries and the partition's marker. The harness has one index manager,
/// whose claimed partition no second worker can take, so B is simulated.
pub async fn emulate_superseding_node(test: &TestServer, id: u32, message_ids: &[u32]) {
    let mut batch = BatchBuilder::new();
    for (document_id, row) in metadata_rows(test, id).await {
        if !message_ids.contains(&document_id) {
            continue;
        }
        let metadata = row.unarchive().unwrap();
        let headers = row.raw_headers().unwrap();
        let subject = match metadata.thread_subject() {
            ThreadSubject::Resolved(subject) => subject.to_string(),
            ThreadSubject::InHeaders => metadata.thread_subject_in(&headers).to_string(),
        };
        batch
            .with_account_id(id)
            .with_collection(Collection::Email)
            .with_document(document_id);
        metadata.unindex(&mut batch, &subject);
        batch.commit_point();
    }
    test.server.store().write_batch(&mut batch).await.unwrap();
    unindex_search_documents(test, SearchIndex::Email, id, message_ids.to_vec()).await;
    let (from, to) = SearchIndexClass::queue_range(SearchIndex::Email, id);
    test.server
        .store()
        .delete_range(ValueKey::from(from), ValueKey::from(to))
        .await
        .unwrap();
    let mut batch = BatchBuilder::new();
    batch.clear(SearchIndexClass::QueueIndex {
        index: SearchIndex::Email,
        partition: id,
    });
    test.server.store().write_batch(&mut batch).await.unwrap();
}

/// Ruling 18 through HTTP and the real index worker: the vault-absence
/// assertion is in every commit chunk of a fenced pass, so provisioning the
/// account between two chunks stops every later one. On one node
/// provisioning cannot happen mid-pass: the destroyed messages' rows stay
/// until the parked pass's partition runs their unindex (R46's 409). On a
/// cluster a superseding node (B, simulated) can run it; its deletions
/// leave the second message's postings, written in chunk 0 under a
/// `Document` key that chunk 1 was to write, and R62's term scan refuses
/// setup on them (the lost-lease residual, part 6a); after the account-wide
/// unindex (the remedy) setup completes. When the parked pass resumes, its
/// chunk 1 fails the assertion: nothing of it lands, the partition enters
/// its retry state, and one reset drains it (no endless retry). The
/// ordinary control's same pass, released without provisioning, writes both
/// documents. Fencing chunk 0 only makes this test fail: chunk 1's
/// `Document` key would land in the key account's index.
pub async fn test_mid_pass_provisioning(test: &mut TestServer) {
    println!("Running zero-access mid-pass provisioning test...");
    test.wait_for_tasks().await;
    let admin = test.account("admin@example.com").clone();

    // The ordinary control.
    let control = passwordless(&admin, MID_PASS_CONTROL).await;
    let control_id = control.id().document_id();
    let control_messages = two_messages_held(test, MID_PASS_CONTROL, control_id).await;
    let split = ForceCommitSplit::new(control_id);
    let parked = park_between_chunks(test, control_id).await;
    let keys = search_keys(test, control_id, SearchIndex::Email).await;
    assert!(
        keys.documents.len() == 1 && keys.terms > 0,
        "the control's chunk 0 alone was not written: {keys:?}"
    );
    parked.finish().await;
    drop(split);
    wait_for_index_queue(test, control_id, Duration::from_secs(10)).await;
    let mut documents = search_keys(test, control_id, SearchIndex::Email)
        .await
        .document_ids();
    documents.sort_unstable();
    assert_eq!(documents, control_messages, "the control's two documents");

    // The account provisioned mid-pass.
    let account = passwordless(&admin, MID_PASS).await;
    let id = account.id().document_id();
    let messages = two_messages_held(test, MID_PASS, id).await;
    let split = ForceCommitSplit::new(id);
    let parked = park_between_chunks(test, id).await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.documents.len() == 1 && keys.terms > 0,
        "chunk 0 alone was not written: {keys:?}"
    );
    admin.destroy_all_mailboxes_for_account(id).await;
    assert_refused(test, &admin, MID_PASS, id, "account already holds mail").await;

    emulate_superseding_node(test, id, &messages).await;
    assert!(
        metadata_rows(test, id).await.is_empty() && queue_entries(test, id).await.is_empty(),
        "B's execution left a row or an entry"
    );
    assert_refused(test, &admin, MID_PASS, id, "account already holds mail").await;
    unindex_search_account(test, SearchIndex::Email, id).await;
    let token = za_setup_token(&admin, MID_PASS).await;
    za_setup(MID_PASS, &token, STRONG).await;
    assert_eq!(
        test.server
            .za_vault_record(id)
            .await
            .unwrap()
            .expect("the vault record")
            .record
            .state,
        VaultState::Active,
        "setup did not complete"
    );

    // A resumes: chunk 1 fails its assertion.
    parked.finish().await;
    drop(split);
    wait_for_retry_state(test, SearchIndex::Email, id).await;
    assert!(
        search_keys(test, id, SearchIndex::Email).await.is_empty(),
        "a later chunk landed in the provisioned account's index"
    );
    release_index_partition(&test.server, SearchIndex::Email, id).await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;
    assert!(
        search_keys(test, id, SearchIndex::Email).await.is_empty(),
        "the retry wrote into the provisioned account's index"
    );

    admin.destroy_all_mailboxes_for_account(control_id).await;
    test.wait_for_tasks().await;
    admin.destroy_account(control).await;
    admin.destroy_account(account).await;
    test.wait_for_tasks().await;
}

/// The plan's mid-pass deletion case, flipped by ruling 69: an ordinary
/// account destroyed while its index pass is parked between two commit
/// chunks may leave an orphan under its id (ruling 85: it depends on
/// timing, so the case accepts none), and the orphan is inert.
///
/// Account destruction runs `unindex_account` and
/// `clear_search_index_queue` outside the partition lock
/// (`destroy_account.rs`), so the parked pass's chunk 1 (its second
/// `Document` key, whose value lists the document's terms; a term of up to
/// 16 bytes is stored verbatim) commits after the range delete: the
/// destroyed account holds no vault record, so R58's assertion passes.
/// Ruling 69 accepts this residual as retention, not access: 1.0 never
/// reuses an account id (`Account` is `OBJ_SEQ_ID`, its ids come from
/// `RegistryClass::IdCounter` through `add_and_get` and only grow), so no
/// later account reads the orphan, and its partition does not retry. The
/// case asserts that any orphan is the resumed pass's document, the
/// inertness (no account holds the id, no queue entry is left, and an
/// account created afterwards gets another one), and then removes any
/// orphan so the suite's final emptiness check holds; in production it
/// stays until the index is rebuilt.
pub async fn test_mid_pass_deletion(test: &mut TestServer) {
    println!("Running zero-access mid-pass deletion test...");
    test.wait_for_tasks().await;
    let admin = test.account("admin@example.com").clone();
    let account = passwordless(&admin, MID_PASS_DELETED).await;
    let id = account.id().document_id();
    let messages = two_messages_held(test, MID_PASS_DELETED, id).await;
    let split = ForceCommitSplit::new(id);
    let parked = park_between_chunks(test, id).await;
    admin.destroy_account(account).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while test.server.try_account(id).await.unwrap().is_some()
        || !search_keys(test, id, SearchIndex::Email).await.is_empty()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the account's destruction did not complete"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    parked.finish().await;
    drop(split);
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;

    // The residual, timing-dependent (ruling 85): either no orphan, or the
    // resumed chunk's document under the destroyed id and nothing else.
    let keys = search_keys(test, id, SearchIndex::Email).await;
    let orphans = keys.document_ids();
    assert!(
        orphans
            .iter()
            .all(|document_id| messages.contains(document_id)),
        "ruling 85 allows only the resumed pass's document as an orphan under the \
         destroyed id: {keys:?}"
    );
    // Inert: no account holds the id, the partition is not retrying, and
    // an account created afterwards is given a new id.
    assert!(test.server.try_account(id).await.unwrap().is_none());
    assert!(
        queue_entries(test, id).await.is_empty(),
        "the destroyed account's partition still holds entries"
    );
    let probe = passwordless(&admin, MID_PASS_PROBE).await;
    assert!(
        probe.id().document_id() > id,
        "account id {id} was handed out again ({})",
        probe.id().document_id()
    );
    admin.destroy_account(probe).await;

    // The fixture's cleanup: the orphan would fail the suite's final
    // emptiness check.
    unindex_search_account(test, SearchIndex::Email, id).await;
    assert!(search_keys(test, id, SearchIndex::Email).await.is_empty());
    assert!(!internal_terms(test, id).await);
    test.wait_for_tasks().await;
}

/// Whether the internal search store holds any Email term of the account
/// (R62's kept scan, `has_account_search_terms`).
async fn internal_terms(test: &TestServer, account_id: u32) -> bool {
    crate::utils::za::internal_search(test)
        .has_account_search_terms(SearchIndex::Email, account_id)
        .await
        .unwrap()
}
