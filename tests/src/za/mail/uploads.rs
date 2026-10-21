/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 5.7 and 9: a key account's uploads are sealed under the
//! session's mail wrapping key and read back through `blob_download`; an
//! upload or download without the account's keys is refused (403, or
//! `forbidden` in a method call) before anything is reserved; and no raw
//! consumer of a sealed source (an upload, a message or a section of one)
//! reads it, whichever account's session asks (rulings R30, R32).

use super::{
    assert_sealed, assert_sealed_upload, deliver, key_mail_account, message_ids, message_with,
    new_message, open_message, stored_metadata,
};
use crate::{
    utils::{
        account::Account,
        jmap::JmapResponse,
        server::TestServer,
        za::{bearer_token, wait_for_delivery},
    },
    za::{
        STRONG,
        tracing::{ws_call, ws_close, ws_connect},
        user_permissions,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::za::ZA_MAIL_KEYLESS;
use email::{mailbox::INBOX_ID, message::metadata::MessageMetadata};
use groupware::calendar::seal::{WRAP_MK, WRAP_PK};
use registry::schema::{enums::Permission, prelude::ObjectType};
use serde_json::{Value, json};
use std::{str::FromStr, time::Duration};
use store::{IterateParams, SUBSPACE_BLOB_LINK, U32_LEN, U64_LEN, write::AnyKey};
use tokio::sync::mpsc;
use trc::{
    Collector, EventType, Key, SecurityEvent,
    ipc::subscriber::{EventBatch, Interests, SubscriberBuilder},
};
use types::{
    blob::BlobId,
    blob_hash::{BLOB_HASH_LEN, BlobHash},
    id::Id,
};

const KEY: &str = "keymail6@example.com";
const OTHER_KEY: &str = "keymail7@example.com";
const PLAIN: &str = "plainmail5@example.com";
const OTHER_PLAIN: &str = "plainmail6@example.com";
const SENDER: &str = "sender@example.org";
const SUBSCRIBER_ID: &str = "za-mail-uploads-test";
/// In every upload and message of the key account; no trace carries it.
const CANARY: &str = "upload canary";
/// The key account's uploaded attachment.
const ATTACHMENT: &[u8] = b"upload canary attachment \x00\x01\x02\xfe\xff bytes";
/// The attachment of the key account's delivered message.
const DELIVERED_ATTACHMENT: &[u8] = b"upload canary delivered \x00\xff attachment";
const RAW_REFUSED: &str = "zero-access: raw read of a sealed source";
const COPY_REFUSED: &str = "zero-access: mail copy across accounts";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mail upload tests...");
    let admin = test.account("admin@example.com").clone();
    // Ruling R32: the spam-sample case needs `sysSpamTrainingSampleCreate`.
    let mut permissions = user_permissions();
    permissions.push(Permission::SysSpamTrainingSampleCreate);
    let key = admin
        .create_key_user_account(KEY, STRONG, "Key mail", &[], permissions.clone())
        .await;
    test.insert_account(key.clone());
    let other_key = key_mail_account(test, OTHER_KEY, STRONG).await;
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain mail", &[], permissions)
        .await;
    let other_plain = admin
        .create_user_account(OTHER_PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;

    // The key account's delivered message, with an attachment: its blob
    // and the attachment's section are sealed sources.
    deliver(test, SENDER, KEY, &message_with_attachment()).await;
    let delivered = *message_ids(test, key.id().document_id())
        .await
        .first()
        .unwrap();
    assert_sealed(test, key.id().document_id(), delivered, WRAP_PK).await;
    let (message_blob, section_blob) = message_blobs(&key).await;

    let upload = upload_with_the_credential_is_sealed(test, &key).await;
    upload_without_keys_is_refused(test, &key, &other_key, &plain, &message_blob, &upload).await;
    sealed_upload_imported_and_attached(test, &key, &section_blob).await;
    raw_consumers_are_refused(
        test,
        &key,
        &plain,
        &other_plain,
        [&message_blob, &section_blob, &upload],
    )
    .await;

    for account in [&key, &other_key, &plain, &other_plain] {
        test.destroy_all_mailboxes(account).await;
    }
    admin
        .registry_destroy_all(ObjectType::SpamTrainingSample)
        .await;
    // Uploads and copies are temporary links: expired here, as the purge
    // would.
    test.blob_expire_all().await;
    wait_for_delivery(test).await;
}

/// A multipart/mixed message to the key account: a text part and a base64
/// attachment.
fn message_with_attachment() -> String {
    format!(
        "From: Sender <{SENDER}>\r\n\
         To: <{KEY}>\r\n\
         Subject: {CANARY} delivered\r\n\
         Message-ID: <{:016x}.za-uploads@example.org>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"za\"\r\n\
         \r\n\
         --za\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         {CANARY} body\r\n\
         --za\r\n\
         Content-Type: application/octet-stream; name=\"delivered.bin\"\r\n\
         Content-Disposition: attachment; filename=\"delivered.bin\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         {}\r\n\
         --za--\r\n",
        store::rand::random::<u64>(),
        STANDARD.encode(DELIVERED_ATTACHMENT)
    )
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The account's messages: id, blob id and attachments.
async fn emails(account: &Account, ids: Option<Vec<String>>) -> Vec<Value> {
    account
        .jmap_method_call(
            "Email/get",
            json!({
                "accountId": account.id_string(),
                "ids": ids,
                "properties": ["id", "blobId", "attachments"],
            }),
        )
        .await
        .list()
        .to_vec()
}

/// The blob id of the account's one message and of its first attachment.
async fn message_blobs(account: &Account) -> (String, String) {
    let list = emails(account, None).await;
    assert_eq!(list.len(), 1, "{list:?}");
    (
        list[0]["blobId"].as_str().unwrap().to_string(),
        list[0]["attachments"][0]["blobId"]
            .as_str()
            .unwrap_or_else(|| panic!("no attachment: {}", list[0]))
            .to_string(),
    )
}

/// `/jmap/download` of `blob_id` with the account's own credentials.
async fn download(account: &Account, blob_id: &str) -> (u16, Vec<u8>) {
    let response = account
        .http_get_raw(&download_url(account, blob_id), None)
        .await;
    (response.status, response.body)
}

fn download_url(account: &Account, blob_id: &str) -> String {
    format!(
        "{}/jmap/download/{}/{blob_id}/blob",
        account.base_url(),
        account.id_string()
    )
}

fn upload_url(base: &Account, target: &Account) -> String {
    format!("{}/jmap/upload/{}/", base.base_url(), target.id_string())
}

/// `Blob/get` of `blob_id`, as bytes.
async fn blob_get(account: &Account, blob_id: &str) -> Vec<u8> {
    let reply = account
        .jmap_method_call(
            "Blob/get",
            json!({
                "accountId": account.id_string(),
                "ids": [blob_id],
                "properties": ["data:asBase64"],
            }),
        )
        .await;
    let data = reply.list()[0]["data:asBase64"]
        .as_str()
        .unwrap_or_else(|| panic!("Blob/get of {blob_id}: {:?}", reply.0));
    STANDARD.decode(data).unwrap()
}

/// `Blob/upload` of one blob from `data`, its data source objects; the id.
async fn blob_upload(account: &Account, data: Value) -> String {
    let reply = account
        .jmap_method_call(
            "Blob/upload",
            json!({
                "accountId": account.id_string(),
                "create": { "b": { "data": data } },
            }),
        )
        .await;
    reply
        .pointer("/methodResponses/0/1/created/b/id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("Blob/upload: {:?}", reply.0))
        .to_string()
}

/// An `/jmap/upload` with the account's own Basic credentials; the id.
async fn upload(account: &Account, bytes: &[u8]) -> String {
    account
        .jmap_client()
        .await
        .upload(None, bytes.to_vec(), None)
        .await
        .unwrap()
        .take_blob_id()
}

/// The 403 of ruling R30 and of a keyless `/jmap/upload`: the keyless text
/// and no content.
fn assert_forbidden_http(status: u16, body: &str, what: &str) {
    assert_eq!(status, 403, "{what}: {body}");
    let body: Value = serde_json::from_str(body).unwrap_or_else(|_| panic!("{what}: {body}"));
    assert_eq!(body["detail"], ZA_MAIL_KEYLESS, "{what}: {body}");
    assert!(!body.to_string().contains("canary"), "{what}: {body}");
}

/// The method call answered the keyless refusal (ruling R22).
fn assert_refused(reply: &JmapResponse, what: &str) {
    assert_eq!(
        reply.0["methodResponses"][0],
        json!(["error", { "type": "forbidden", "description": ZA_MAIL_KEYLESS }, "0"]),
        "{what}: {:?}",
        reply.0
    );
}

/// `pointer` of the first method response, which must exist (an update's
/// entry may be `null`).
fn accepted<'x>(reply: &'x JmapResponse, pointer: &str, what: &str) -> &'x Value {
    reply
        .pointer(&format!("/methodResponses/0/1{pointer}"))
        .unwrap_or_else(|| panic!("{what}: {:?}", reply.0))
}

/// The temporary blob links of `account_id` (an upload's reservation):
/// keys of `hash || account id || until` in the link subspace, as
/// `store_blob_expire_all` reads them.
async fn temporary_links(test: &TestServer, account_id: u32) -> usize {
    let mut found = 0;
    test.server
        .store()
        .iterate(
            IterateParams::new(
                AnyKey {
                    subspace: SUBSPACE_BLOB_LINK,
                    key: vec![0u8],
                },
                AnyKey {
                    subspace: SUBSPACE_BLOB_LINK,
                    key: vec![u8::MAX; 64],
                },
            )
            .no_values(),
            |key, _| {
                if key.len() == BLOB_HASH_LEN + U32_LEN + U64_LEN
                    && key[BLOB_HASH_LEN..BLOB_HASH_LEN + U32_LEN] == account_id.to_be_bytes()
                {
                    found += 1;
                }
                Ok(true)
            },
        )
        .await
        .unwrap();
    found
}

/// Mail spec 5.7: an upload with the password (Basic, the session's keys)
/// is stored as a sealed blob, and `/jmap/download` and `Blob/get` read it
/// back through `blob_download`. Returns the upload's blob id.
async fn upload_with_the_credential_is_sealed(test: &TestServer, key: &Account) -> String {
    println!("upload_with_the_credential_is_sealed");
    let id = key.id().document_id();
    let client = key.jmap_client().await;
    let bytes = message_with(CANARY, "upload body canary").into_bytes();
    let blob_id = client
        .upload(None, bytes.clone(), None)
        .await
        .unwrap()
        .take_blob_id();
    assert_sealed_upload(test, id, STRONG, &blob_id, &bytes).await;
    assert_eq!(
        client.download(&blob_id).await.unwrap(),
        bytes,
        "the download"
    );
    assert_eq!(blob_get(key, &blob_id).await, bytes, "Blob/get");

    let text = "upload canary as text";
    let text_id = blob_upload(key, json!([{ "data:asText": text }])).await;
    assert_sealed_upload(test, id, STRONG, &text_id, text.as_bytes()).await;
    assert_eq!(
        client.download(&text_id).await.unwrap(),
        text.as_bytes(),
        "the download of a Blob/upload"
    );
    assert_eq!(blob_get(key, &text_id).await, text.as_bytes());
    blob_id
}

/// Mail spec 4, 9, ruling R30: without the target account's keys an upload
/// answers 403 (`/jmap/upload`) or `forbidden` (`Blob/upload`) and reserves
/// nothing; a download of the account's message or upload answers 403.
async fn upload_without_keys_is_refused(
    test: &TestServer,
    key: &Account,
    other_key: &Account,
    plain: &Account,
    message_blob: &str,
    upload: &str,
) {
    println!("upload_without_keys_is_refused");
    let id = key.id().document_id();
    let other_id = other_key.id().document_id();
    let links = temporary_links(test, id).await;
    let other_links = temporary_links(test, other_id).await;

    let token = bearer_token(KEY, STRONG).await;
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let response = client
        .post(upload_url(key, key))
        .bearer_auth(&token)
        .header("Content-Type", "application/octet-stream")
        .body(CANARY.as_bytes().to_vec())
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    assert_forbidden_http(status, &response.text().await.unwrap(), "a Bearer upload");
    for (blob_id, what) in [
        (message_blob, "a Bearer download of a message"),
        (upload, "a Bearer download of an upload"),
    ] {
        let response = client
            .get(download_url(key, blob_id))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        assert_forbidden_http(status, &response.text().await.unwrap(), what);
    }

    // A WebSocket never holds keys (spec 3.3).
    let mut socket = ws_connect(KEY, STRONG).await;
    let reply = ws_call(
        &mut socket,
        &json!({
            "@type": "Request",
            "id": "1",
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:blob"],
            "methodCalls": [["Blob/upload", {
                "accountId": key.id_string(),
                "create": { "b": { "data": [{ "data:asText": CANARY }] } }
            }, "c0"]]
        })
        .to_string(),
    )
    .await;
    ws_close(socket).await;
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(
        reply["methodResponses"][0],
        json!(["error", { "type": "forbidden", "description": ZA_MAIL_KEYLESS }, "c0"]),
        "Blob/upload over the WebSocket: {reply}"
    );

    // Another account's session holds no keys of the target.
    for (session, target, what) in [
        (plain, key, "a plain session into a key account"),
        (key, other_key, "a key session into another key account"),
    ] {
        let response = session
            .http_post_raw(
                &upload_url(session, target),
                "application/octet-stream",
                CANARY,
            )
            .await;
        assert_forbidden_http(response.status, &response.text(), what);
    }

    assert_eq!(temporary_links(test, id).await, links, "a reservation");
    assert_eq!(
        temporary_links(test, other_id).await,
        other_links,
        "a reservation in the other key account"
    );
}

/// Mail spec 5.7, 6.2: an imported upload is resealed into a fresh blob
/// under `mk`; an uploaded attachment, and a forwarded section of the
/// account's own earlier message, are read through `blob_download` into
/// the new message.
async fn sealed_upload_imported_and_attached(test: &TestServer, key: &Account, section: &str) {
    println!("sealed_upload_imported_and_attached");
    let id = key.id().document_id();
    let mailbox = Id::from(INBOX_ID).to_string();

    let before = message_ids(test, id).await;
    let message = message_with(CANARY, "imported upload canary");
    let blob_id = upload(key, message.as_bytes()).await;
    let reply = key
        .jmap_method_call(
            "Email/import",
            json!({
                "accountId": key.id_string(),
                "emails": { "i": { "blobId": &blob_id, "mailboxIds": { &mailbox: true } } }
            }),
        )
        .await;
    let email_id = accepted(&reply, "/created/i/id", "Email/import")
        .as_str()
        .unwrap()
        .to_string();
    let document_id = new_message(test, id, &before).await;
    assert_eq!(Id::from_str(&email_id).unwrap().document_id(), document_id);
    assert_sealed(test, id, document_id, WRAP_MK).await;
    let stored_hash = BlobHash::from(
        &stored_metadata(test, id, document_id)
            .await
            .unarchive::<MessageMetadata>()
            .unwrap()
            .blob_hash,
    );
    assert_ne!(
        stored_hash,
        BlobId::from_base32(&blob_id).unwrap().hash,
        "the upload's blob was reused"
    );
    let (_, raw) = open_message(test, id, STRONG, document_id).await;
    assert_eq!(raw, message.as_bytes(), "the imported message");

    for (blob_id, expected, what) in [
        (
            upload(key, ATTACHMENT).await,
            ATTACHMENT,
            "an uploaded attachment",
        ),
        (
            section.to_string(),
            DELIVERED_ATTACHMENT,
            "a forwarded attachment",
        ),
    ] {
        let before = message_ids(test, id).await;
        let reply = key
            .jmap_method_call(
                "Email/set",
                json!({
                    "accountId": key.id_string(),
                    "create": { "e": {
                        "mailboxIds": { &mailbox: true },
                        "from": [{ "email": KEY }],
                        "to": [{ "email": "recipient@example.com" }],
                        "subject": format!("{CANARY} attached"),
                        "textBody": [{ "partId": "t", "type": "text/plain" }],
                        "bodyValues": { "t": { "value": "see the attachment" } },
                        "attachments": [{
                            "blobId": &blob_id,
                            "type": "application/octet-stream",
                            "name": "attached.bin"
                        }]
                    } }
                }),
            )
            .await;
        let email_id = accepted(&reply, "/created/e/id", what)
            .as_str()
            .unwrap()
            .to_string();
        let document_id = new_message(test, id, &before).await;
        assert_sealed(test, id, document_id, WRAP_MK).await;
        let (_, raw) = open_message(test, id, STRONG, document_id).await;
        assert!(
            contains(&raw, STANDARD.encode(expected).as_bytes()),
            "{what}: {}",
            String::from_utf8_lossy(&raw)
        );
        let list = emails(key, Some(vec![email_id])).await;
        let attachment = list[0]["attachments"][0]["blobId"]
            .as_str()
            .unwrap_or_else(|| panic!("{what}: {list:?}"))
            .to_string();
        let (status, bytes) = download(key, &attachment).await;
        assert_eq!(status, 200, "{what}");
        assert_eq!(bytes, expected, "{what}: the attachment's bytes");
    }
}

/// One captured event.
struct TracedEvent {
    details: String,
    account_id: Option<u64>,
    /// Every value, for the content check.
    text: String,
}

/// The raw consumers of mail spec 5.7 and ruling R32, as `session` with
/// `blob_id`: `Blob/upload` from a data source, `FileNode/set` create (in
/// the session's own account) and update (of `update`, an account and its
/// file node), `FileNode/copy` of `copy_from` (the session's own node)
/// into `shared` (an account and its folder), `x:SpamTrainingSample/set`
/// and `SieveScript/set` creates.
async fn raw_consumers(
    session: &Account,
    blob_id: &str,
    update: (&str, &str),
    copy_from: &str,
    shared: (&str, &str),
) -> Vec<(&'static str, JmapResponse)> {
    let own = session.id_string();
    vec![
        (
            "Blob/upload",
            session
                .jmap_method_call(
                    "Blob/upload",
                    json!({
                        "accountId": own,
                        "create": { "b": { "data": [{ "blobId": blob_id }] } }
                    }),
                )
                .await,
        ),
        (
            "FileNode/set create",
            session
                .jmap_method_call(
                    "FileNode/set",
                    json!({
                        "accountId": own,
                        "create": { "f": { "name": "za-raw.bin", "blobId": blob_id } }
                    }),
                )
                .await,
        ),
        (
            "FileNode/set update",
            session
                .jmap_method_call(
                    "FileNode/set",
                    json!({
                        "accountId": update.0,
                        "update": { update.1: { "blobId": blob_id } }
                    }),
                )
                .await,
        ),
        (
            "FileNode/copy",
            session
                .jmap_method_call(
                    "FileNode/copy",
                    json!({
                        "fromAccountId": own,
                        "accountId": shared.0,
                        "create": { copy_from: {
                            "parentId": shared.1,
                            "name": "za-raw-copy.bin",
                            "blobId": blob_id
                        } }
                    }),
                )
                .await,
        ),
        (
            "x:SpamTrainingSample/set",
            session
                .jmap_method_call(
                    "x:SpamTrainingSample/set",
                    json!({
                        "accountId": own,
                        "create": { "s": { "blobId": blob_id, "isSpam": true } }
                    }),
                )
                .await,
        ),
        (
            "SieveScript/set",
            session
                .jmap_method_call(
                    "SieveScript/set",
                    json!({
                        "accountId": own,
                        "create": { "v": { "name": "za-raw", "blobId": blob_id } }
                    }),
                )
                .await,
        ),
    ]
}

/// A `Blob/copy` as `session`.
async fn blob_copy(
    session: &Account,
    from: &Account,
    to: &Account,
    blob_ids: &[&str],
) -> JmapResponse {
    session
        .jmap_method_call(
            "Blob/copy",
            json!({
                "fromAccountId": from.id_string(),
                "accountId": to.id_string(),
                "blobIds": blob_ids,
            }),
        )
        .await
}

/// A `FileNode/set` create of one node in `account`; its id.
async fn file_node(session: &Account, account: &Account, node: Value) -> String {
    let reply = session
        .jmap_method_call(
            "FileNode/set",
            json!({ "accountId": account.id_string(), "create": { "n": node } }),
        )
        .await;
    accepted(&reply, "/created/n/id", "FileNode/set create")
        .as_str()
        .unwrap()
        .to_string()
}

/// Mail spec 5.7, rulings R30, R32: a key account's message, section and
/// upload ids (`sources`) are refused by every raw consumer, from the key
/// account's own session and from a plain account's, and so is a blob copy
/// into or out of the key account; a plain account's own blobs pass every
/// one of them. No trace carries content.
async fn raw_consumers_are_refused(
    test: &TestServer,
    key: &Account,
    plain: &Account,
    other_plain: &Account,
    sources: [&str; 3],
) {
    println!("raw_consumers_are_refused");
    let key_id = key.id().document_id();

    // The plain account's own blobs: its message and three uploads (a
    // message, the replacement content, a Sieve script).
    deliver(
        test,
        SENDER,
        PLAIN,
        &message_with("plain control", "plain control body"),
    )
    .await;
    let plain_message = emails(plain, None).await[0]["blobId"]
        .as_str()
        .unwrap()
        .to_string();
    let plain_upload = upload(
        plain,
        message_with("plain control", "plain control upload").as_bytes(),
    )
    .await;
    let plain_replacement = upload(plain, b"plain control replacement").await;
    let plain_sieve = upload(plain, b"keep;\r\n").await;

    // `plainmail6`'s folder, shared with both sessions, holding a file.
    let shared_folder = file_node(other_plain, other_plain, json!({ "name": "za-uploads" })).await;
    let reply = other_plain
        .jmap_method_call(
            "FileNode/set",
            json!({
                "accountId": other_plain.id_string(),
                "update": { &shared_folder: { "shareWith": {
                    key.id_string(): { "mayRead": true, "mayAddChildren": true },
                    plain.id_string(): { "mayRead": true, "mayAddChildren": true }
                } } }
            }),
        )
        .await;
    accepted(&reply, &format!("/updated/{shared_folder}"), "the share");
    let shared_upload = upload(other_plain, b"plain control shared").await;
    let shared_file = file_node(
        other_plain,
        other_plain,
        json!({ "name": "shared.bin", "parentId": &shared_folder, "blobId": &shared_upload }),
    )
    .await;
    // The sessions' own nodes: a folder of the key account (none of its
    // uploads can become a file's blob), a file of the plain account (the
    // `FileNode/set` create control).
    let key_folder = file_node(key, key, json!({ "name": "za-key-folder" })).await;
    let plain_file = file_node(
        plain,
        plain,
        json!({ "name": "plain.bin", "blobId": &plain_upload }),
    )
    .await;
    let shared = (other_plain.id_string(), shared_folder.as_str());

    let traced = Traced::start(&[
        EventType::Security(SecurityEvent::Unauthorized),
        EventType::Jmap(trc::JmapEvent::Forbidden),
        EventType::Http(trc::HttpEvent::RequestBody),
        EventType::Http(trc::HttpEvent::ResponseBody),
        EventType::Store(trc::StoreEvent::DataCorruption),
    ]);
    let mut raw_refusals = 0;
    for source in sources {
        for (what, reply) in raw_consumers(
            key,
            source,
            (other_plain.id_string(), &shared_file),
            &key_folder,
            shared,
        )
        .await
        {
            assert_refused(&reply, &format!("{what} of {source} by the key account"));
            raw_refusals += 1;
        }
        // The rule is keyed by the blob's owning account and class, not by
        // the session asking.
        for (what, reply) in raw_consumers(
            plain,
            source,
            (plain.id_string(), &plain_file),
            &plain_file,
            shared,
        )
        .await
        {
            assert_refused(&reply, &format!("{what} of {source} by a plain account"));
            raw_refusals += 1;
        }
        // A plain `fromAccountId` does not hide the blob's owner.
        assert_refused(
            &blob_copy(plain, other_plain, plain, &[source]).await,
            &format!("Blob/copy of {source} from a plain account"),
        );
        raw_refusals += 1;
        // Spec 9: no blob copy out of a key account.
        assert_refused(
            &blob_copy(plain, key, plain, &[source]).await,
            &format!("Blob/copy of {source} out of the key account"),
        );
    }
    // Spec 9: no blob copy into a key account, even of a plain blob.
    assert_refused(
        &blob_copy(key, plain, key, &[&plain_upload]).await,
        "Blob/copy into the key account",
    );

    // The controls: the plain account's own blobs pass every consumer.
    let reply = plain
        .jmap_method_call(
            "Blob/upload",
            json!({
                "accountId": plain.id_string(),
                "create": { "b": { "data": [{ "blobId": &plain_upload }, { "blobId": &plain_message }] } }
            }),
        )
        .await;
    accepted(&reply, "/created/b/id", "control: Blob/upload data sources");
    let reply = plain
        .jmap_method_call(
            "FileNode/set",
            json!({
                "accountId": plain.id_string(),
                "update": { &plain_file: { "blobId": &plain_replacement } }
            }),
        )
        .await;
    accepted(
        &reply,
        &format!("/updated/{plain_file}"),
        "control: FileNode/set update",
    );
    let reply = plain
        .jmap_method_call(
            "FileNode/copy",
            json!({
                "fromAccountId": plain.id_string(),
                "accountId": other_plain.id_string(),
                "create": { &plain_file: {
                    "parentId": &shared_folder,
                    "name": "copied.bin",
                    "blobId": &plain_upload
                } }
            }),
        )
        .await;
    accepted(
        &reply,
        &format!("/created/{plain_file}/id"),
        "control: FileNode/copy",
    );
    // `Blob/copy` writes into an account the session is a member of; the
    // source account is the plain `plainmail6`.
    let reply = blob_copy(plain, other_plain, plain, &[&plain_upload, &plain_message]).await;
    for blob_id in [&plain_upload, &plain_message] {
        accepted(&reply, &format!("/copied/{blob_id}"), "control: Blob/copy");
    }
    let reply = plain
        .jmap_method_call(
            "x:SpamTrainingSample/set",
            json!({
                "accountId": plain.id_string(),
                "create": { "s": { "blobId": &plain_upload, "isSpam": true } }
            }),
        )
        .await;
    accepted(&reply, "/created/s/id", "control: x:SpamTrainingSample/set");
    let reply = plain
        .jmap_method_call(
            "SieveScript/set",
            json!({
                "accountId": plain.id_string(),
                "create": { "v": { "name": "za-plain", "blobId": &plain_sieve } }
            }),
        )
        .await;
    let script = accepted(&reply, "/created/v/id", "control: SieveScript/set")
        .as_str()
        .unwrap()
        .to_string();

    let events = traced.finish().await;
    for event in &events {
        assert!(
            !event.text.contains("canary"),
            "a trace carries content: {}",
            event.text
        );
    }
    let refused = |details: &str| {
        events
            .iter()
            .filter(|event| event.details == details)
            .map(|event| event.account_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        refused(RAW_REFUSED),
        vec![Some(u64::from(key_id)); raw_refusals],
        "one trace per refused raw read, naming the blob's owner"
    );
    assert_eq!(
        refused(COPY_REFUSED),
        vec![Some(u64::from(key_id)); sources.len() + 1],
        "one trace per refused blob copy"
    );

    // Cleanup: the file nodes and the script; the uploads expire.
    let reply = other_plain
        .jmap_method_call(
            "FileNode/set",
            json!({
                "accountId": other_plain.id_string(),
                "destroy": [&shared_folder],
                "onDestroyRemoveChildren": true
            }),
        )
        .await;
    assert_eq!(reply.destroyed().count(), 3, "{:?}", reply.0);
    for (account, node) in [(plain, &plain_file), (key, &key_folder)] {
        assert_eq!(
            account
                .jmap_destroy("FileNode", [node], Vec::<(&str, &str)>::new())
                .await
                .destroyed()
                .count(),
            1,
            "{}",
            account.name()
        );
    }
    assert_eq!(
        plain
            .jmap_destroy("SieveScript", [&script], Vec::<(&str, &str)>::new())
            .await
            .destroyed()
            .count(),
        1
    );
}

/// Captures the named event types (the test server configures no tracer,
/// so nothing is emitted until something declares interest).
struct Traced {
    _tx: mpsc::Sender<EventBatch>,
    rx: mpsc::Receiver<EventBatch>,
}

impl Traced {
    fn start(types: &[EventType]) -> Self {
        let mut interests = Interests::default();
        for event_type in types {
            interests.set(*event_type);
        }
        let (_tx, rx) = SubscriberBuilder::new(SUBSCRIBER_ID.into())
            .set_interests(types.iter().copied())
            .with_lossy(false)
            .register();
        Collector::union_interests(interests);
        Collector::reload();
        Traced { _tx, rx }
    }

    async fn finish(mut self) -> Vec<TracedEvent> {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let mut captured = Vec::new();
        while let Ok(batch) = self.rx.try_recv() {
            for event in batch {
                let mut traced = TracedEvent {
                    details: String::new(),
                    account_id: None,
                    text: String::new(),
                };
                for (key, value) in event.keys.iter() {
                    match (key, value) {
                        (Key::AccountId, trc::Value::UInt(id)) => traced.account_id = Some(*id),
                        (Key::Details, trc::Value::String(text)) => {
                            traced.details = text.to_string()
                        }
                        _ => {}
                    }
                    traced.text.push_str(&format!("{key:?}={value:?} "));
                }
                captured.push(traced);
            }
        }
        Collector::remove_subscriber(SUBSCRIBER_ID.into());
        Collector::reload();
        captured
    }
}
