/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 6.2: IMAP APPEND, JMAP Email/set create and Email/import by a
//! key account's session are sealed under the session's mail wrapping key
//! (`mk`); no spam sample is written for them (6.1 step 4); a key account's
//! APPEND into a plain account's shared mailbox stores plaintext the owner
//! reads (review focus 3).

use super::{
    assert_sealed, key_mail_account, message_ids, message_with, new_message, open_message,
    spam_samples, stored_metadata,
};
use crate::{
    utils::{
        account::Account,
        imap::{ImapConnection, Type},
        server::TestServer,
        za::wait_for_delivery,
    },
    za::{STRONG, user_permissions},
};
use email::{
    mailbox::{INBOX_ID, JUNK_ID},
    message::{metadata::MessageMetadata, za::is_stub},
};
use groupware::calendar::seal::WRAP_MK;
use imap_proto::ResponseType;
use registry::schema::prelude::ObjectType;
use serde_json::{Map, Value, json};
use std::str::FromStr;
use types::{blob_hash::BlobHash, id::Id};

const NAME: &str = "keymail4@example.com";
const PLAIN: &str = "plainmail2@example.com";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mail session-write tests...");
    let admin = test.account("admin@example.com").clone();
    let key = key_mail_account(test, NAME, STRONG).await;
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    let id = key.id().document_id();
    let plain_id = plain.id().document_id();

    let appended = append_is_sealed_under_mk(test, id).await;
    email_set_create_is_sealed_under_mk(test, &key, id).await;
    email_import_reseals_the_upload(test, &key, id).await;
    no_sample_on_session_writes(test, &key, id, appended).await;
    rf_3_append_into_plain_share(test, plain_id).await;

    test.destroy_all_mailboxes(&key).await;
    test.destroy_all_mailboxes(&plain).await;
    admin
        .registry_destroy_all(ObjectType::SpamTrainingSample)
        .await;
    wait_for_delivery(test).await;
}

/// `{"<mailbox id>": true}` for JMAP's `mailboxIds`.
fn mailbox_ids(mailbox_id: u32) -> Value {
    let mut ids = Map::new();
    ids.insert(Id::from(mailbox_id).to_string(), Value::Bool(true));
    Value::Object(ids)
}

/// An Email/set create of a text message into `mailbox_id`.
fn created_email(mailbox_id: u32, subject: &str, body: &str) -> Value {
    json!({
        "mailboxIds": mailbox_ids(mailbox_id),
        "keywords": { "$flagged": true },
        "from": [{ "email": NAME }],
        "to": [{ "email": "recipient@example.com" }],
        "subject": subject,
        "textBody": [{ "partId": "t", "type": "text/plain" }],
        "bodyValues": { "t": { "value": body } }
    })
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

/// The stub's blob hash.
async fn blob_hash_of(test: &TestServer, id: u32, document_id: u32) -> BlobHash {
    BlobHash::from(
        &stored_metadata(test, id, document_id)
            .await
            .unarchive::<MessageMetadata>()
            .unwrap()
            .blob_hash,
    )
}

/// The session's token carries the keys (`refresh_access_token`), so the
/// message is sealed under `mk`. Returns its document id.
async fn append_is_sealed_under_mk(test: &TestServer, id: u32) -> u32 {
    println!("append_is_sealed_under_mk");
    let before = message_ids(test, id).await;
    let message = message_with("appended", "a");
    let mut imap = ImapConnection::connect(b"_w ").await;
    imap.authenticate(NAME, STRONG).await;
    imap.send(&format!("APPEND INBOX {{{}+}}\r\n{message}", message.len()))
        .await;
    let lines = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    assert!(lines.last().unwrap().contains("[APPENDUID "), "{lines:?}");
    imap.send_ok("LOGOUT").await;

    let document_id = new_message(test, id, &before).await;
    assert_sealed(test, id, document_id, WRAP_MK).await;
    let (_, raw) = open_message(test, id, STRONG, document_id).await;
    assert_eq!(raw, message.as_bytes(), "the appended bytes");
    document_id
}

async fn email_set_create_is_sealed_under_mk(test: &TestServer, key: &Account, id: u32) {
    println!("email_set_create_is_sealed_under_mk");
    let before = message_ids(test, id).await;
    let created = key
        .jmap_create(
            "Email",
            [created_email(INBOX_ID, "created", "created body")],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created_id(0);
    let document_id = new_message(test, id, &before).await;
    assert_eq!(created.document_id(), document_id);
    assert_sealed(test, id, document_id, WRAP_MK).await;
    let (meta, raw) = open_message(test, id, STRONG, document_id).await;
    assert!(
        contains(&raw, "created body"),
        "{}",
        String::from_utf8_lossy(&raw)
    );
    assert!(meta.preview.contains("created body"), "{}", meta.preview);
}

/// Mail spec 6.2: the upload's hash is not reused; the message blob is a
/// fresh sealed blob. Task 6 seals the upload itself.
async fn email_import_reseals_the_upload(test: &TestServer, key: &Account, id: u32) {
    println!("email_import_reseals_the_upload");
    let before = message_ids(test, id).await;
    let message = message_with("imported", "import body");
    let mut email = key
        .jmap_client()
        .await
        .email_import(
            message.clone().into_bytes(),
            [Id::from(INBOX_ID).to_string()],
            None::<Vec<&str>>,
            None,
        )
        .await
        .unwrap();
    let document_id = new_message(test, id, &before).await;
    assert_eq!(
        Id::from_str(&email.take_id()).unwrap().document_id(),
        document_id
    );
    assert_sealed(test, id, document_id, WRAP_MK).await;
    assert_ne!(
        blob_hash_of(test, id, document_id).await,
        BlobHash::generate(message.as_bytes()),
        "the upload's blob was reused"
    );
    let (_, raw) = open_message(test, id, STRONG, document_id).await;
    assert_eq!(raw, message.as_bytes());
}

/// Mail spec 6.1 step 4, 6.4: no sample for a key account's session writes.
/// The control: the plain account's APPEND into its Junk writes one.
async fn no_sample_on_session_writes(test: &TestServer, key: &Account, id: u32, appended: u32) {
    println!("no_sample_on_session_writes");
    let samples = spam_samples(test).await;
    let mut plain = ImapConnection::connect(b"_c ").await;
    plain.authenticate(PLAIN, STRONG).await;
    plain
        .append("Junk Mail", &message_with("plain junk", "control"))
        .await;
    plain.send_ok("LOGOUT").await;
    assert_eq!(spam_samples(test).await, samples + 1, "control: APPEND");
    let samples = samples + 1;

    let before = message_ids(test, id).await;
    let mut imap = ImapConnection::connect(b"_j ").await;
    imap.authenticate(NAME, STRONG).await;
    imap.append("Junk Mail", &message_with("key junk", "j"))
        .await;
    imap.send_ok("LOGOUT").await;
    assert_eq!(spam_samples(test).await, samples, "a sample from APPEND");
    let document_id = new_message(test, id, &before).await;
    assert_sealed(test, id, document_id, WRAP_MK).await;

    let email_id = Id::from_parts(appended, appended).to_string();
    key.jmap_update(
        "Email",
        [(&email_id, json!({ "keywords/$junk": true }))],
        Vec::<(&str, &str)>::new(),
    )
    .await
    .updated(&email_id);
    assert_eq!(spam_samples(test).await, samples, "a sample from $junk");

    key.jmap_create(
        "Email",
        [created_email(JUNK_ID, "created junk", "junk body")],
        Vec::<(&str, &str)>::new(),
    )
    .await
    .created_id(0);
    assert_eq!(
        spam_samples(test).await,
        samples,
        "a sample from a create into Junk"
    );
}

/// Review focus 3: the appender's token carries keys for its own account
/// only; the target is plain, so ingest takes the plain path.
async fn rf_3_append_into_plain_share(test: &TestServer, plain_id: u32) {
    println!("rf_3_append_into_plain_share");
    let mut owner = ImapConnection::connect(b"_o ").await;
    owner.authenticate(PLAIN, STRONG).await;
    owner.send_ok(&format!("SETACL INBOX {NAME} lrswi")).await;
    let before = message_ids(test, plain_id).await;

    let mut imap = ImapConnection::connect(b"_s ").await;
    imap.authenticate(NAME, STRONG).await;
    imap.append(
        &format!("Shared Folders/{PLAIN}/INBOX"),
        &message_with("shared", "plain body"),
    )
    .await;
    imap.send_ok("LOGOUT").await;

    let document_id = new_message(test, plain_id, &before).await;
    let archive = stored_metadata(test, plain_id, document_id).await;
    assert!(
        !is_stub(archive.unarchive::<MessageMetadata>().unwrap()),
        "the plain account's message is sealed"
    );
    owner.send_ok("SELECT INBOX").await;
    owner.send("FETCH 1 (BODY[TEXT])").await;
    let lines = owner.assert_read(Type::Tagged, ResponseType::Ok).await;
    assert!(
        lines.iter().any(|line| line.contains("plain body")),
        "{lines:?}"
    );
    owner.send_ok(&format!("DELETEACL INBOX {NAME}")).await;
    owner.send_ok("LOGOUT").await;
}
