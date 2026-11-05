/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 5.2, 5.5, 6.6, 7 and 10: every reader of a key account's
//! message opens the stub and the blob, and answers exactly what it answers
//! for an ordinary account's copy of the same message; an unreadable message
//! fails alone; a plaintext message in a key account is served as plaintext;
//! the search index holds the visible fields only; push carries state
//! changes only.

use super::{
    assert_no_plaintext, assert_sealed, corrupt_blob, corrupt_stub, deliver, group_ingest,
    key_mail_account, keyed_token, keyless_token, message_ids, message_with, new_message,
    open_message, row_is_stub, stored_hash, stored_metadata,
};
use crate::{
    utils::{
        account::Account,
        imap::{ImapConnection, Type},
        pop3::{Pop3Connection, ResponseType as Pop3Reply},
        push::{PUSH_ORIGIN, spawn_mock_push_server},
        server::TestServer,
        smtp::SmtpConnection,
        za::{plant_plain_message, wait_for_delivery, za_bearer_token},
    },
    za::{STRONG, user_permissions},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use common::{
    auth::AccessToken,
    ipc::PushNotification,
    za::{ZA_MAIL_KEYLESS, ZA_MAIL_SEARCH_REFUSED, ZA_MAIL_UNREADABLE, test_hooks},
};
use email::{
    mailbox::{DRAFTS_ID, SENT_ID},
    message::{
        ingest::group::IngestGroup,
        za::{open_stub, view::ZaMailView},
    },
    push::{EmailPush, Urgency},
};
use groupware::calendar::seal::{WRAP_MK, WRAP_PK};
use imap_proto::{ResponseType, protocol::SerializeResponse as ImapResponse};
use jmap_proto::object::push_subscription::EmailPushProperty;
use pop3::protocol::response::SerializeResponse as Pop3Response;
use registry::schema::prelude::ObjectType;
use serde_json::{Value, json};
use services::state_manager::email_push::build_email_push_object;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use trc::{
    Collector, EventType, Key, StoreEvent,
    ipc::subscriber::{Interests, SubscriberBuilder},
};
use types::{blob::BlobClass, collection::Collection, id::Id, type_state::StateChange};

const KEY: &str = "keymail5@example.com";
const PLAIN: &str = "plainmail3@example.com";
const BCC: &str = "plainmail4@example.com";
const SENDER: &str = "sender@example.org";
/// What each account's own address is replaced with before two answers are
/// compared: the `Delivered-To` header names the recipient.
const RCPT: &str = "<rcpt>";
const ATTACHMENT: &[u8] = b"attachment canary \x00\x01\x02\xfe\xff bytes";
const CORPUS_BODY: &str = "corpus body canary";
const INNER_BODY: &str = "inner body canary";
const SUBSCRIBER_ID: &str = "za-mail-reads-test";

/// The attributes `offsets_round_trip` compares, one FETCH each.
const FETCH_ATTRIBUTES: &[&str] = &[
    "BODY[]",
    "BODY[HEADER]",
    "BODY[HEADER.FIELDS (Subject)]",
    "BODY[1]",
    "BODY[2]",
    "BODY[3.1]",
    "BODYSTRUCTURE",
    "ENVELOPE",
    "RFC822.SIZE",
    "PREVIEW",
];

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mail read tests...");
    let admin = test.account("admin@example.com").clone();
    let key = key_mail_account(test, KEY, STRONG).await;
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    let bcc = admin
        .create_user_account(BCC, STRONG, "Plain mail", &[], user_permissions())
        .await;
    let key_id = key.id().document_id();

    // The same message to both accounts, in one transaction: only the
    // `Delivered-To` header tells the two stored messages apart.
    SmtpConnection::connect()
        .await
        .ingest(SENDER, &[KEY, PLAIN], &corpus())
        .await;
    wait_for_delivery(test).await;
    let corpus_doc = *message_ids(test, key_id).await.first().unwrap();
    assert_sealed(test, key_id, corpus_doc, WRAP_PK).await;

    stale_prefix_not_found(&key, corpus_doc).await;
    offsets_round_trip(&key, &plain).await;
    pop3_retr_and_top(&key, &plain).await;
    keyless_reads_are_refused(test, &key, corpus_doc).await;
    content_free_reads_open_nothing(test, &key).await;
    corrupted_stub_and_blob(test, &key).await;
    legacy_plaintext_is_served_as_plaintext(test, &key, corpus_doc).await;
    plain_header_block_with_the_format_byte(test, &plain).await;
    clear_messages(&key).await;
    threads_are_singletons(test, &key).await;
    submission_strips_bcc(test, &key, &plain, &bcc).await;
    push_is_state_only(test, &key, &plain).await;
    rf_1_idle_delivery(test, &key).await;
    rf_5_large_header_block(test, &key).await;
    assigned_ids_preserve_sealed_read_offsets(test, &key, &plain).await;

    for account in [&key, &plain, &bcc] {
        test.destroy_all_mailboxes(account).await;
    }
    // The grouped ingest trains as IMAP APPEND does: the ordinary
    // account's samples.
    admin
        .registry_destroy_all(ObjectType::SpamTrainingSample)
        .await;
    wait_for_delivery(test).await;
}

/// A multipart/mixed message: a text part, a base64 attachment and a nested
/// message/rfc822 with a text part and an attachment of its own.
fn corpus() -> String {
    format!(
        "From: Sender <sender@example.org>\r\n\
         To: Recipient <recipient@example.com>\r\n\
         Subject: reads corpus\r\n\
         Message-ID: <{:016x}.za-reads@example.org>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"outer\"\r\n\
         \r\n\
         --outer\r\n\
         Content-Type: text/plain; charset=us-ascii\r\n\
         \r\n\
         {CORPUS_BODY}\r\n\
         --outer\r\n\
         Content-Type: application/octet-stream; name=\"attachment.bin\"\r\n\
         Content-Disposition: attachment; filename=\"attachment.bin\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         {}\r\n\
         --outer\r\n\
         Content-Type: message/rfc822\r\n\
         \r\n\
         From: Inner <inner@example.org>\r\n\
         Subject: inner message\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"inner\"\r\n\
         \r\n\
         --inner\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         {INNER_BODY}\r\n\
         --inner\r\n\
         Content-Type: application/octet-stream; name=\"inner.bin\"\r\n\
         Content-Disposition: attachment; filename=\"inner.bin\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         aW5uZXIgYXR0YWNobWVudA==\r\n\
         --inner--\r\n\
         \r\n\
         --outer--\r\n",
        store::rand::random::<u64>(),
        STANDARD.encode(ATTACHMENT),
    )
}

/// A size counted with the account's own address replaced by `RCPT`.
fn rcpt_size(size: u64, address: &str) -> u64 {
    size - address.len() as u64 + RCPT.len() as u64
}

/// An IMAP answer with the account's own address replaced by `RCPT`, each
/// literal's announced length and `RFC822.SIZE` counted for the replaced
/// text.
fn normalize(answer: &str, address: &str) -> String {
    let mut out = String::with_capacity(answer.len());
    let mut rest = answer;
    while let Some(open) = rest.find('{') {
        let literal = rest[open..].find("}\r\n").and_then(|close| {
            rest[open + 1..open + close]
                .parse::<usize>()
                .ok()
                .map(|len| (open + close + 3, len))
        });
        match literal {
            Some((start, len)) => {
                out.push_str(&rest[..open].replace(address, RCPT));
                let contents = rest[start..start + len].replace(address, RCPT);
                out.push_str(&format!("{{{}}}\r\n{contents}", contents.len()));
                rest = &rest[start + len..];
            }
            None => {
                out.push_str(&rest[..=open].replace(address, RCPT));
                rest = &rest[open + 1..];
            }
        }
    }
    out.push_str(&rest.replace(address, RCPT));
    if let Some(at) = out.find("RFC822.SIZE ") {
        let start = at + "RFC822.SIZE ".len();
        let end = start
            + out[start..]
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(out.len() - start);
        let size = rcpt_size(out[start..end].parse().unwrap(), address);
        out.replace_range(start..end, &size.to_string());
    }
    out
}

/// The keyword 1.0 sets on an ordinary account's message with an
/// attachment (ruling 55).
const ATTACHMENT_FLAG: &str = "$hasattachment";

/// `answer` with `ATTACHMENT_FLAG` taken out of its last FLAGS item, and
/// whether it was there.
fn without_attachment_flag(answer: &str) -> (String, bool) {
    let Some(open) = answer.rfind("FLAGS (") else {
        return (answer.to_string(), false);
    };
    let start = open + "FLAGS (".len();
    let Some(len) = answer[start..].find(')') else {
        return (answer.to_string(), false);
    };
    let flags = answer[start..start + len].split(' ').collect::<Vec<_>>();
    let kept = flags
        .iter()
        .filter(|flag| **flag != ATTACHMENT_FLAG)
        .copied()
        .collect::<Vec<_>>();
    (
        format!(
            "{}{}{}",
            &answer[..start],
            kept.join(" "),
            &answer[start + len..]
        ),
        kept.len() + 1 == flags.len(),
    )
}

/// A JMAP object with its `id` and `blobId` values removed at every level,
/// the account's own address replaced by `RCPT` and the top-level `size`
/// counted for the replaced text.
fn normalize_json(mut value: Value, address: &str) -> Value {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("id");
                map.remove("blobId");
                map.values_mut().for_each(strip);
            }
            Value::Array(list) => list.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut value);
    if let Some(size) = value.get_mut("size") {
        *size = json!(rcpt_size(size.as_u64().unwrap(), address));
    }
    serde_json::from_str(&value.to_string().replace(address, RCPT)).unwrap()
}

/// An IMAP session of `account` with INBOX selected.
async fn imap_session(account: &Account, tag: &'static [u8]) -> ImapConnection {
    let mut imap = ImapConnection::connect(tag).await;
    imap.authenticate(account.name(), account.secret()).await;
    imap.send_ok("SELECT INBOX").await;
    imap
}

/// The answer to `command`, its lines joined as they were sent.
async fn imap_answer(imap: &mut ImapConnection, command: &str) -> String {
    imap.send(command).await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .join("\r\n")
}

/// `Email/get` of `ids` (all of the account's messages when `None`).
async fn email_get(
    account: &Account,
    ids: Option<Vec<String>>,
    properties: &[&str],
) -> (Vec<Value>, Vec<String>) {
    let response = account
        .jmap_method_call(
            "Email/get",
            json!({
                "accountId": account.id_string(),
                "ids": ids,
                "properties": properties,
                "fetchAllBodyValues": true,
            }),
        )
        .await;
    let list = response.list().to_vec();
    let not_found = response
        .pointer("/methodResponses/0/1/notFound")
        .and_then(|v| v.as_array())
        .map(|ids| {
            ids.iter()
                .map(|id| id.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default();
    (list, not_found)
}

/// The JMAP id of a key account's message: its thread is itself (spec 5.4).
fn key_email_id(document_id: u32) -> String {
    Id::from_parts(document_id, document_id).to_string()
}

/// Email ids name both the thread and the message, including for singletons.
async fn stale_prefix_not_found(key: &Account, document_id: u32) {
    let stale = Id::from_parts(document_id ^ 1, document_id).to_string();
    let valid = key_email_id(document_id);
    let (list, not_found) = email_get(key, Some(vec![stale.clone(), valid.clone()]), &["id"]).await;
    assert_eq!(not_found, [stale]);
    assert_eq!(list, [json!({ "id": valid })]);
}

/// Destroys every message of the account.
async fn clear_messages(account: &Account) {
    let ids = email_get(account, None, &["id"])
        .await
        .0
        .iter()
        .map(|email| email["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    if !ids.is_empty() {
        let destroyed = account
            .jmap_destroy("Email", &ids, Vec::<(&str, &str)>::new())
            .await
            .destroyed()
            .count();
        assert_eq!(destroyed, ids.len(), "{}", account.name());
    }
}

/// `/jmap/download` of `blob_id`: the status and the bytes.
async fn download(account: &Account, blob_id: &str) -> (u16, Vec<u8>) {
    let response = account
        .http_get_raw(
            &format!(
                "{}/jmap/download/{}/{blob_id}/blob",
                account.base_url(),
                account.id_string()
            ),
            None,
        )
        .await;
    (response.status, response.body)
}

/// Review F5: FETCH sections, Email/get and the downloads of a key account's
/// message equal an ordinary account's answers for the same message.
async fn offsets_round_trip(key: &Account, plain: &Account) {
    println!("offsets_round_trip");
    let mut answers = Vec::new();
    for account in [key, plain] {
        let (list, _) = email_get(
            account,
            None,
            &[
                "id",
                "blobId",
                "bodyValues",
                "textBody",
                "attachments",
                "headers",
                "preview",
                "size",
                "hasAttachment",
                "sentAt",
            ],
        )
        .await;
        assert_eq!(list.len(), 1, "{list:?}");
        let email = list[0].clone();
        let attachment = email["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|part| part["name"] == "attachment.bin")
            .unwrap_or_else(|| panic!("no attachment.bin in {email}"));
        let (status, section) = download(account, attachment["blobId"].as_str().unwrap()).await;
        assert_eq!(status, 200, "{}: the attachment's download", account.name());
        assert_eq!(
            section,
            ATTACHMENT,
            "{}: the decoded section",
            account.name()
        );
        let (status, whole) = download(account, email["blobId"].as_str().unwrap()).await;
        assert_eq!(status, 200, "{}: the message's download", account.name());
        let whole = String::from_utf8(whole)
            .unwrap()
            .replace(account.name(), RCPT);
        // Blob/get and Email/parse read through `blob_download` too.
        let (message_blob, section_blob) = (&email["blobId"], &attachment["blobId"]);
        let blobs = account
            .jmap_method_call(
                "Blob/get",
                json!({
                    "accountId": account.id_string(),
                    "ids": [message_blob, section_blob],
                    "properties": ["data:asBase64", "size"],
                }),
            )
            .await;
        let blob_data = |id: &Value| {
            let blob = blobs
                .list()
                .iter()
                .find(|blob| &blob["id"] == id)
                .unwrap_or_else(|| panic!("Blob/get lacks {id}: {:?}", blobs.0));
            STANDARD
                .decode(blob["data:asBase64"].as_str().unwrap())
                .unwrap()
        };
        assert_eq!(
            blob_data(section_blob),
            ATTACHMENT,
            "{}: Blob/get of the section",
            account.name()
        );
        let blob_whole = String::from_utf8(blob_data(message_blob))
            .unwrap()
            .replace(account.name(), RCPT);
        assert_eq!(
            blob_whole,
            whole,
            "{}: Blob/get of the message",
            account.name()
        );
        let parsed = account
            .jmap_method_call(
                "Email/parse",
                json!({
                    "accountId": account.id_string(),
                    "blobIds": [message_blob],
                    "properties": [
                        "subject", "from", "headers", "bodyValues", "textBody",
                        "attachments", "preview", "size", "hasAttachment"
                    ],
                    "fetchAllBodyValues": true,
                }),
            )
            .await;
        let parsed = parsed
            .pointer(&format!(
                "/methodResponses/0/1/parsed/{}",
                message_blob.as_str().unwrap()
            ))
            .unwrap_or_else(|| panic!("Email/parse: {:?}", parsed.0))
            .clone();
        // Mail spec 8: a term snippet is refused for the key account
        // (`search.rs` covers the refusals in full); the control still
        // marks the term in its preview.
        let snippets = account
            .jmap_method_call(
                "SearchSnippet/get",
                json!({
                    "accountId": account.id_string(),
                    "filter": { "text": "corpus" },
                    "emailIds": [email["id"]],
                }),
            )
            .await;
        if account.name() == key.name() {
            let error = &snippets.0["methodResponses"][0];
            assert_eq!(error[0], "error", "{}: {error}", account.name());
            assert_eq!(error[1]["type"], "unsupportedFilter", "{error}");
            assert!(
                error[1]["description"]
                    .as_str()
                    .is_some_and(|text| text.contains(ZA_MAIL_SEARCH_REFUSED)),
                "{error}"
            );
        } else {
            let snippet = &snippets.list()[0];
            assert!(
                snippet["preview"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("canary"),
                "{}: {snippet}",
                account.name()
            );
        }

        let mut imap = imap_session(account, b"_r ").await;
        let mut fetched = Vec::new();
        for attribute in FETCH_ATTRIBUTES {
            fetched.push(normalize(
                &imap_answer(&mut imap, &format!("FETCH 1 ({attribute})")).await,
                account.name(),
            ));
        }
        // Mail spec 5.5: the search index holds the arrival time and the
        // plaintext size (a stub's sealed pair is larger).
        let size = email["size"].as_u64().unwrap();
        for (search, expected) in [
            (format!("SEARCH LARGER {}", size - 1), "* SEARCH 1"),
            (format!("SEARCH LARGER {size}"), "* SEARCH"),
            (format!("SEARCH SMALLER {}", size + 1), "* SEARCH 1"),
            ("SEARCH SINCE 1-Jan-2020".to_string(), "* SEARCH 1"),
            ("SEARCH BEFORE 1-Jan-2020".to_string(), "* SEARCH"),
        ] {
            let answer = imap_answer(&mut imap, &search).await;
            let found = answer
                .lines()
                .find(|line| line.starts_with("* SEARCH"))
                .unwrap_or_else(|| panic!("{search}: {answer}"));
            assert_eq!(found.trim_end(), expected, "{}: {search}", account.name());
        }
        imap.send_ok("LOGOUT").await;
        answers.push((
            fetched,
            normalize_json(email, account.name()),
            whole,
            normalize_json(parsed, account.name()),
        ));
    }

    let (key_fetched, key_email, key_whole, key_parsed) = &answers[0];
    let (plain_fetched, plain_email, plain_whole, plain_parsed) = &answers[1];
    assert_eq!(key_parsed, plain_parsed, "Email/parse");
    assert_eq!(key_parsed["subject"], "reads corpus", "{key_parsed}");
    // Ruling 55: at 1.0 the server sets `$hasattachment` on an ordinary
    // account's message and none on a key account's (spec 5.2), so the one
    // difference is that flag in the FLAGS item a non-peek FETCH adds.
    for (attribute, (key, plain)) in FETCH_ATTRIBUTES
        .iter()
        .zip(key_fetched.iter().zip(plain_fetched))
    {
        assert!(
            !key.contains(ATTACHMENT_FLAG),
            "FETCH {attribute}: a key account's FLAGS carry {ATTACHMENT_FLAG}: {key}"
        );
        let has_flags = key.contains("FLAGS (");
        let (plain, removed) = without_attachment_flag(plain);
        assert_eq!(
            removed, has_flags,
            "FETCH {attribute}: the ordinary account's FLAGS and {ATTACHMENT_FLAG}: {plain}"
        );
        assert_eq!(key, &plain, "FETCH {attribute}");
    }
    let body = &key_fetched[0];
    for expected in [
        format!("Delivered-To: {RCPT}\r\n"),
        "Subject: reads corpus".to_string(),
        CORPUS_BODY.to_string(),
        INNER_BODY.to_string(),
    ] {
        assert!(
            body.contains(&expected),
            "BODY[] lacks {expected:?}: {body}"
        );
    }
    assert!(key_fetched[5].contains(INNER_BODY), "{}", key_fetched[5]);
    assert_eq!(key_email, plain_email, "Email/get");
    assert_eq!(key_email["hasAttachment"], true, "{key_email}");
    assert_eq!(key_whole, plain_whole, "the whole-message download");
    assert!(
        key_whole.starts_with(&format!("Delivered-To: {RCPT}\r\n")),
        "{key_whole}"
    );
}

/// POP3 RETR and TOP of the key account's message equal the ordinary
/// account's; STAT and LIST count the same size.
async fn pop3_retr_and_top(key: &Account, plain: &Account) {
    println!("pop3_retr_and_top");
    let mut answers = Vec::new();
    for account in [key, plain] {
        let address = account.name();
        let mut pop3 = Pop3Connection::connect().await;
        pop3.authenticate(address, account.secret()).await;
        pop3.send("STAT").await;
        let stat = pop3.assert_read(Pop3Reply::Ok).await.pop().unwrap();
        let (count, size) = stat
            .strip_prefix("+OK ")
            .and_then(|stat| stat.split_once(' '))
            .unwrap_or_else(|| panic!("{stat}"));
        pop3.send("LIST").await;
        let list = pop3.assert_read(Pop3Reply::Multiline).await;
        let listed = list[1].split_once(' ').unwrap().1.parse::<u64>().unwrap();
        let mut messages = Vec::new();
        for command in ["RETR 1", "TOP 1 2"] {
            pop3.send(command).await;
            let lines = pop3.assert_read(Pop3Reply::Multiline).await;
            let octets = lines[0]
                .strip_prefix("+OK ")
                .and_then(|line| line.strip_suffix(" octets"))
                .unwrap_or_else(|| panic!("{command}: {}", lines[0]))
                .parse::<u64>()
                .unwrap();
            messages.push((
                rcpt_size(octets, address),
                lines[1..].join("\r\n").replace(address, RCPT),
            ));
        }
        pop3.send("QUIT").await;
        pop3.assert_read(Pop3Reply::Ok).await;
        answers.push((
            count.to_string(),
            rcpt_size(size.parse().unwrap(), address),
            rcpt_size(listed, address),
            messages,
        ));
    }
    assert_eq!(answers[0], answers[1], "STAT, LIST, RETR and TOP");
    let (_, size, listed, messages) = &answers[0];
    assert_eq!(size, listed);
    assert!(messages[0].1.contains(CORPUS_BODY), "{}", messages[0].1);
    assert!(
        !messages[1].1.contains(CORPUS_BODY) && messages[1].1.contains("Subject: reads corpus"),
        "TOP 2 is the header block and two body lines: {}",
        messages[1].1
    );
}

/// Mail spec 10, ruling R22: a read of a key account's mail by a token
/// without its keys (an OAuth Bearer token) is refused with the error every
/// protocol writes, `ZA_MAIL_KEYLESS` as JMAP `forbidden`, IMAP
/// `NO [NOPERM]` and POP3 `-ERR`.
async fn keyless_reads_are_refused(test: &TestServer, key: &Account, corpus_doc: u32) {
    println!("keyless_reads_are_refused");
    let key_id = key.id().document_id();
    let token = za_bearer_token(&test.server, KEY).await;
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let reply: Value = client
        .post(key.api_url())
        .bearer_auth(&token)
        .body(
            json!({
                "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
                "methodCalls": [["Email/get", {
                    "accountId": key.id_string(),
                    "ids": null,
                    "properties": ["subject"]
                }, "0"]]
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        reply["methodResponses"][0],
        json!(["error", { "type": "forbidden", "description": ZA_MAIL_KEYLESS }, "0"]),
        "{reply}"
    );
    // The message's blob id, as the key account's own session reads it.
    let (list, _) = email_get(key, Some(vec![key_email_id(corpus_doc)]), &["blobId"]).await;
    let blob_id = list[0]["blobId"].as_str().unwrap();
    let download = client
        .get(format!(
            "{}/jmap/download/{}/{blob_id}/blob",
            key.base_url(),
            key.id_string()
        ))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    // Ruling R30 (mail spec 10): the keyless refusal answers 403 at
    // `/jmap/download`, where upstream answers a JMAP error outside a method
    // call as 400 (`jmap/src/api/mod.rs`).
    let status = download.status().as_u16();
    let body = download.text().await.unwrap();
    assert!(
        status == 403
            && body.contains(ZA_MAIL_KEYLESS)
            && !body.contains("corpus")
            && !body.contains("canary"),
        "{status}: {body}"
    );

    let stored = stored_metadata(test, key_id, corpus_doc).await;
    let keyless = AccessToken::from_id_maybe_invalid(key_id);
    let err = test
        .server
        .za_mail_keys(&keyless, key_id)
        .await
        .unwrap_err();
    assert_eq!(
        String::from_utf8(ImapResponse::serialize(&err.clone().id("t1"))).unwrap(),
        format!("t1 NO [NOPERM] {ZA_MAIL_KEYLESS}\r\n"),
        "the IMAP answer"
    );
    assert_eq!(
        String::from_utf8(Pop3Response::serialize(&err)).unwrap(),
        format!("-ERR {ZA_MAIL_KEYLESS}\r\n"),
        "the POP3 answer"
    );
    assert!(
        err.matches(EventType::Jmap(trc::JmapEvent::Forbidden)),
        "{err:?}"
    );
    // Ruling R29: the refusal is `za_mail_keys`'s; without keys (an
    // ordinary account's read) the view and the opener pass the stored row
    // and bytes through, even a stub's. At 1.0 the view takes the row it
    // replaces (`ZaMetadataRow`), and the batch readers' opener answers
    // `None` for "use the stored row".
    assert!(row_is_stub(&stored));
    let stored_headers = stored.raw_headers().unwrap().into_owned();
    assert!(
        test.server
            .za_open_metadata_row(&stored, None, key_id, corpus_doc)
            .await
            .unwrap()
            .is_none(),
        "the batch opener opened a stub without keys"
    );
    let view = test
        .server
        .za_metadata_view(stored, None, key_id, corpus_doc)
        .await
        .unwrap();
    assert!(!view.opened, "the view opened a stub without keys");
    assert_eq!(
        view.row.raw_headers().unwrap().as_ref(),
        stored_headers.as_slice(),
        "the stored row, as it is"
    );
    let blob = test
        .server
        .za_open_message_blob(true, None, key_id, corpus_doc, vec![1, 2, 3])
        .await
        .unwrap();
    assert_eq!(blob.as_slice(), [1, 2, 3], "the bytes as read");
}

/// Ruling R29 (spec 5.2): the stub check runs only on key accounts'
/// archives. An ordinary account's message whose header block starts with
/// the format byte reads as a stub by the discriminator, yet is served as
/// plaintext, not refused.
async fn plain_header_block_with_the_format_byte(test: &TestServer, plain: &Account) {
    println!("plain_header_block_with_the_format_byte");
    let plain_id = plain.id().document_id();
    let crafted = format!(
        "\u{1}X-Crafted: format byte\r\n{}",
        message_with("crafted plain", "crafted body canary")
    );
    let document_id = plant_plain_message(test, plain_id, &crafted).await;
    test.wait_for_tasks().await;
    assert!(
        row_is_stub(&stored_metadata(test, plain_id, document_id).await),
        "the planted header block does not start with the format byte"
    );

    let mut imap = imap_session(plain, b"_p ").await;
    let fetched = imap_answer(&mut imap, "FETCH * (BODY[])").await;
    assert!(
        fetched.contains("crafted body canary") && fetched.contains(&crafted),
        "{fetched}"
    );
    // Ruling R47 of plan 13: the index task built upstream's full document,
    // so the message is found by its subject and body terms.
    for search in ["SEARCH SUBJECT crafted", "SEARCH BODY crafted"] {
        let answer = imap_answer(&mut imap, search).await;
        let found = answer
            .lines()
            .find(|line| line.starts_with("* SEARCH"))
            .unwrap_or_else(|| panic!("{search}: {answer}"));
        assert_eq!(
            found.trim_end().split(' ').count(),
            3,
            "{search}: the crafted message is not indexed by its content: {answer}"
        );
    }
    imap.send_ok("LOGOUT").await;
    let id = Id::from_parts(document_id, document_id).to_string();
    let (list, not_found) = email_get(
        plain,
        Some(vec![id.clone()]),
        &["id", "subject", "bodyValues"],
    )
    .await;
    assert!(not_found.is_empty(), "{not_found:?}");
    assert_eq!(list[0]["subject"], "crafted plain", "{list:?}");
    assert!(
        list[0]["bodyValues"]
            .to_string()
            .contains("crafted body canary"),
        "{}",
        list[0]
    );
    let destroyed = plain
        .jmap_destroy("Email", [&id], Vec::<(&str, &str)>::new())
        .await
        .destroyed()
        .count();
    assert_eq!(destroyed, 1);
}

/// Review m1: the attributes and properties served from the cache and the
/// stored stub open nothing: while the account's next private-key unwrap is
/// armed to fail, a FLAGS-only FETCH and an Email/get of mailboxes,
/// keywords, arrival time and size still answer; a content read then takes
/// the armed failure (the positive control).
async fn content_free_reads_open_nothing(test: &TestServer, key: &Account) {
    println!("content_free_reads_open_nothing");
    let key_id = key.id().document_id();
    // HTTP logins share cached keys; a fresh set has not unwrapped the
    // private key yet.
    let fresh_keys = || {
        test.server.inner.cache.za_keys.remove_account(key_id);
        test.server
            .inner
            .cache
            .grant_revocations
            .clear(&test.server.inner.cache.http_auth);
    };

    let mut imap = imap_session(key, b"_f ").await;
    test_hooks::set_private_key_fail(Some(key_id));
    for command in [
        "UID FETCH 1:* (FLAGS)",
        "FETCH 1:* (FLAGS UID INTERNALDATE RFC822.SIZE)",
    ] {
        let answer = imap_answer(&mut imap, command).await;
        assert!(
            answer.contains("* 1 FETCH (") && answer.contains("FLAGS ("),
            "{answer}"
        );
    }
    imap.send("FETCH 1 (BODY.PEEK[TEXT])").await;
    let lines = imap.assert_read(Type::Tagged, ResponseType::No).await;
    assert_eq!(
        lines.last().unwrap(),
        &format!("_f NO [SERVERBUG] {ZA_MAIL_UNREADABLE}"),
        "the armed failure was not taken by the content read"
    );
    imap.send_ok("LOGOUT").await;

    fresh_keys();
    test_hooks::set_private_key_fail(Some(key_id));
    let (list, not_found) = email_get(
        key,
        None,
        &[
            "id",
            "threadId",
            "blobId",
            "mailboxIds",
            "keywords",
            "receivedAt",
            "size",
        ],
    )
    .await;
    assert!(not_found.is_empty() && list.len() == 1, "{list:?}");
    let id = list[0]["id"].as_str().unwrap().to_string();
    let (list, not_found) = email_get(key, Some(vec![id.clone()]), &["id", "preview"]).await;
    assert!(
        list.is_empty() && not_found == [id],
        "the armed failure was not taken by the content read: {list:?}"
    );
    test_hooks::set_private_key_fail(None);
    fresh_keys();
}

/// Mail spec 10: a stub or a blob that does not open fails its own read
/// only, traced with the account, the message and the fixed reason.
async fn corrupted_stub_and_blob(test: &TestServer, key: &Account) {
    println!("corrupted_stub_and_blob");
    let key_id = key.id().document_id();
    let before = message_ids(test, key_id).await;
    deliver(
        test,
        SENDER,
        KEY,
        &message_with("corrupt stub canary", "corrupt stub body canary"),
    )
    .await;
    let stub_doc = new_message(test, key_id, &before).await;
    let before = message_ids(test, key_id).await;
    deliver(
        test,
        SENDER,
        KEY,
        &message_with("corrupt blob canary", "corrupt blob body canary"),
    )
    .await;
    let blob_doc = new_message(test, key_id, &before).await;
    corrupt_stub(test, key_id, stub_doc).await;
    let blob_hash = stored_hash(test, key_id, blob_doc).await;
    corrupt_blob(test, &blob_hash).await;

    let traced = [EventType::Store(StoreEvent::DataCorruption)];
    let mut interests = Interests::default();
    for event_type in traced {
        interests.set(event_type);
    }
    let (_tx, mut rx, _) = SubscriberBuilder::new(SUBSCRIBER_ID.into())
        .set_interests(traced)
        .with_lossy(false)
        .register();
    Collector::union_interests(interests);
    Collector::reload();

    let mut imap = imap_session(key, b"_c ").await;
    imap.send("FETCH 1:* (BODY[])").await;
    let lines = imap.assert_read(Type::Tagged, ResponseType::No).await;
    assert_eq!(
        lines.last().unwrap(),
        &format!("_c NO [SERVERBUG] {ZA_MAIL_UNREADABLE}")
    );
    let fetched = lines.join("\r\n");
    assert!(
        fetched.starts_with("* 1 FETCH ") && fetched.contains(CORPUS_BODY),
        "{fetched}"
    );
    assert!(
        !fetched.contains("* 2 FETCH") && !fetched.contains("* 3 FETCH"),
        "{fetched}"
    );
    imap.send_ok("LOGOUT").await;

    let ids = [corpus_id(test, key_id).await, stub_doc, blob_doc].map(key_email_id);
    let (list, mut not_found) =
        email_get(key, Some(ids.to_vec()), &["id", "subject", "bodyValues"]).await;
    not_found.sort();
    let mut unreadable = vec![ids[1].clone(), ids[2].clone()];
    unreadable.sort();
    assert_eq!(not_found, unreadable);
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["subject"], "reads corpus");

    let mut pop3 = Pop3Connection::connect().await;
    pop3.authenticate(KEY, STRONG).await;
    for message in [2, 3] {
        pop3.send(&format!("RETR {message}")).await;
        let line = pop3.assert_read(Pop3Reply::Err).await.pop().unwrap();
        assert_eq!(line, format!("-ERR {ZA_MAIL_UNREADABLE}"));
    }
    pop3.send("QUIT").await;
    pop3.assert_read(Pop3Reply::Ok).await;

    // The listing reads no content: all three, by arrival.
    let query = key
        .jmap_method_call(
            "Email/query",
            json!({
                "accountId": key.id_string(),
                "sort": [{ "property": "receivedAt", "isAscending": true }],
            }),
        )
        .await;
    assert_eq!(
        query.ids().map(String::from).collect::<Vec<_>>(),
        ids.to_vec()
    );

    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut events = Vec::new();
    while let Ok(batch) = rx.try_recv() {
        events.extend(batch);
    }
    Collector::remove_subscriber(SUBSCRIBER_ID.into());
    Collector::reload();
    let mut documents = Vec::new();
    for event in events.iter().filter(|event| {
        event
            .keys
            .iter()
            .any(|(key, value)| *key == Key::AccountId && value.to_uint() == Some(key_id as u64))
    }) {
        let mut reason = None;
        for (key, value) in event.keys.iter() {
            let text = format!("{value:?}");
            assert!(
                !text.contains("canary") && !text.contains("corpus"),
                "{key:?} carries content: {text}"
            );
            match key {
                Key::DocumentId => documents.push(value.to_uint().unwrap() as u32),
                Key::Reason => reason = value.as_str().map(String::from),
                Key::AccountId | Key::Details | Key::CausedBy | Key::SpanId => {}
                other => panic!("an unreadable message traced {other:?}: {text}"),
            }
        }
        assert_eq!(
            reason.as_deref(),
            Some("zero-access: unreadable message"),
            "{:?}",
            event.keys
        );
    }
    for document_id in [stub_doc, blob_doc] {
        assert!(
            documents.contains(&document_id),
            "message {document_id} was not traced: {documents:?}"
        );
    }
    assert!(
        documents
            .iter()
            .all(|document_id| [stub_doc, blob_doc].contains(document_id)),
        "{documents:?}"
    );

    let destroyed = key
        .jmap_destroy("Email", [&ids[1], &ids[2]], Vec::<(&str, &str)>::new())
        .await
        .destroyed()
        .count();
    assert_eq!(destroyed, 2, "the unreadable messages are destroyed");
}

/// The corpus message: the account's oldest.
async fn corpus_id(test: &TestServer, key_id: u32) -> u32 {
    *message_ids(test, key_id).await.first().unwrap()
}

/// Mail spec 6.6: a plaintext message in a key account (one that predates
/// the account's vault) is served as plaintext; the stub discriminator
/// tells it from its sealed neighbour, both with a non-empty `contents`.
async fn legacy_plaintext_is_served_as_plaintext(
    test: &TestServer,
    key: &Account,
    corpus_doc: u32,
) {
    println!("legacy_plaintext_is_served_as_plaintext");
    let key_id = key.id().document_id();
    let legacy = message_with("legacy plaintext", "legacy body canary");
    let legacy_doc = plant_plain_message(test, key_id, &legacy).await;
    test.wait_for_tasks().await;

    let row = stored_metadata(test, key_id, legacy_doc).await;
    assert!(!row_is_stub(&row), "the planted message reads as a stub");
    let stored = row.unarchive().unwrap();
    assert!(!stored.messages.is_empty() && !stored.parts.is_empty());
    assert_sealed(test, key_id, corpus_doc, WRAP_PK).await;
    // At 1.0 a stub's structure holds one binary root part
    // (`stub_structure`) and no message entry; the discriminator is the
    // format byte, not an empty structure.
    let row = stored_metadata(test, key_id, corpus_doc).await;
    let stub = row.unarchive().unwrap();
    assert_eq!(stub.parts.len(), 1, "the stub's structure");

    let mut imap = imap_session(key, b"_l ").await;
    let fetched = imap_answer(&mut imap, "FETCH 2 (BODY[])").await;
    assert!(
        fetched.contains("legacy body canary") && fetched.contains(&legacy),
        "{fetched}"
    );
    imap.send_ok("LOGOUT").await;
    let (list, not_found) = email_get(
        key,
        Some(vec![key_email_id(legacy_doc)]),
        &["id", "subject", "bodyValues"],
    )
    .await;
    assert!(not_found.is_empty(), "{not_found:?}");
    assert_eq!(list[0]["subject"], "legacy plaintext");
    assert!(
        list[0]["bodyValues"]
            .to_string()
            .contains("legacy body canary"),
        "{}",
        list[0]
    );
}

/// Mail spec 5.4: a key account's messages are singleton threads, however
/// their headers chain.
async fn threads_are_singletons(test: &TestServer, key: &Account) {
    println!("threads_are_singletons");
    let key_id = key.id().document_id();
    let ids = ["t1", "t2", "t3"]
        .map(|id| format!("<{id}.{:016x}@example.org>", store::rand::random::<u64>()));
    for (n, references) in [
        String::new(),
        format!("In-Reply-To: {}\r\nReferences: {}\r\n", ids[0], ids[0]),
        format!(
            "In-Reply-To: {}\r\nReferences: {} {}\r\n",
            ids[1], ids[0], ids[1]
        ),
    ]
    .iter()
    .enumerate()
    {
        deliver(
            test,
            SENDER,
            KEY,
            &format!(
                "From: Sender <sender@example.org>\r\n\
                 To: {KEY}\r\n\
                 Subject: chained thread\r\n\
                 Message-ID: {}\r\n\
                 {references}\
                 \r\n\
                 thread body {n}\r\n",
                ids[n]
            ),
        )
        .await;
    }
    let documents = message_ids(test, key_id).await;
    assert_eq!(documents.len(), 3);

    let (list, _) = email_get(key, None, &["id", "threadId"]).await;
    let mut thread_ids = list
        .iter()
        .map(|email| email["threadId"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    thread_ids.sort();
    thread_ids.dedup();
    assert_eq!(thread_ids.len(), 3, "{list:?}");
    let threads = key
        .jmap_method_call(
            "Thread/get",
            json!({ "accountId": key.id_string(), "ids": thread_ids }),
        )
        .await;
    assert_eq!(threads.list().len(), 3);
    for thread in threads.list() {
        assert_eq!(thread["emailIds"].as_array().unwrap().len(), 1, "{thread}");
    }

    let mut imap = imap_session(key, b"_t ").await;
    let answer = imap_answer(&mut imap, "THREAD REFERENCES UTF-8 ALL").await;
    let line = answer
        .lines()
        .find(|line| line.starts_with("* THREAD "))
        .unwrap_or_else(|| panic!("{answer}"));
    // One group per message, whatever the order.
    let mut groups = line["* THREAD ".len()..]
        .strip_prefix('(')
        .and_then(|groups| groups.strip_suffix(')'))
        .unwrap_or_else(|| panic!("{line}"))
        .split(")(")
        .map(|group| {
            group
                .parse::<u32>()
                .unwrap_or_else(|_| panic!("not a singleton: {line}"))
        })
        .collect::<Vec<_>>();
    groups.sort_unstable();
    assert_eq!(groups, [1, 2, 3], "{line}");
    imap.send_ok("LOGOUT").await;

    let collapsed = key
        .jmap_method_call(
            "Email/query",
            json!({
                "accountId": key.id_string(),
                "collapseThreads": true,
                "sort": [{ "property": "receivedAt" }],
            }),
        )
        .await;
    assert_eq!(collapsed.ids().count(), 3);
    clear_messages(key).await;
}

/// Mail spec 6.5: EmailSubmission/set reads the draft through the unsealed
/// view and blob, sends it without its Bcc header to the To and the Bcc
/// recipients, and onSuccessUpdateEmail moves it to Sent, still sealed.
async fn submission_strips_bcc(test: &TestServer, key: &Account, plain: &Account, bcc: &Account) {
    println!("submission_strips_bcc");
    let key_id = key.id().document_id();
    let before = message_ids(test, key_id).await;
    let identity_id = key
        .jmap_create(
            "Identity",
            [json!({ "name": "Key mail", "email": KEY })],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created_id(0)
        .to_string();
    let drafts = Id::from(DRAFTS_ID).to_string();
    let sent = Id::from(SENT_ID).to_string();
    let draft_id = key
        .jmap_create(
            "Email",
            [json!({
                "mailboxIds": { (drafts.clone()): true },
                "keywords": { "$draft": true },
                "from": [{ "email": KEY }],
                "to": [{ "email": PLAIN }],
                "bcc": [{ "email": BCC }],
                "subject": "submission canary",
                "textBody": [{ "partId": "t", "type": "text/plain" }],
                "bodyValues": { "t": { "value": "submission body canary" } }
            })],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created_id(0)
        .to_string();
    let draft_doc = new_message(test, key_id, &before).await;
    assert_sealed(test, key_id, draft_doc, WRAP_MK).await;

    let response = key
        .jmap_method_calls(json!([[
            "EmailSubmission/set",
            {
                "accountId": key.id_string(),
                "create": { "s1": { "identityId": identity_id, "emailId": draft_id } },
                "onSuccessUpdateEmail": {
                    "#s1": {
                        (format!("mailboxIds/{drafts}")): null,
                        (format!("mailboxIds/{sent}")): true,
                        "keywords/$draft": null
                    }
                }
            },
            "0"
        ]]))
        .await;
    let submission_id = response
        .pointer("/methodResponses/0/1/created/s1/id")
        .and_then(|id| id.as_str())
        .unwrap_or_else(|| panic!("{:?}", response.0))
        .to_string();
    wait_for_delivery(test).await;

    for account in [plain, bcc] {
        let (list, _) = email_get(account, None, &["id", "subject", "headers", "bodyValues"]).await;
        let delivered = list
            .iter()
            .find(|email| email["subject"] == "submission canary")
            .unwrap_or_else(|| panic!("{} received nothing: {list:?}", account.name()));
        let headers = delivered["headers"].as_array().unwrap();
        assert!(
            !headers
                .iter()
                .any(|header| header["name"].as_str().unwrap().eq_ignore_ascii_case("bcc")),
            "{}: the delivered copy keeps its Bcc header: {headers:?}",
            account.name()
        );
        assert!(
            delivered["bodyValues"]
                .to_string()
                .contains("submission body canary"),
            "{delivered}"
        );
        clear_messages(account).await;
    }

    let (list, _) = email_get(
        key,
        Some(vec![draft_id.clone()]),
        &["id", "mailboxIds", "keywords"],
    )
    .await;
    assert_eq!(list[0]["mailboxIds"], json!({ (sent): true }), "{list:?}");
    assert_eq!(list[0]["keywords"], json!({}), "{list:?}");
    assert_sealed(test, key_id, draft_doc, WRAP_MK).await;

    key.jmap_destroy(
        "EmailSubmission",
        [&submission_id],
        Vec::<(&str, &str)>::new(),
    )
    .await;
    key.jmap_destroy("Identity", [&identity_id], Vec::<(&str, &str)>::new())
        .await;
    clear_messages(key).await;
}

/// The push payloads received within `wait`.
async fn pushes_within(rx: &mut mpsc::Receiver<Value>, wait: Duration) -> Vec<Value> {
    let deadline = Instant::now() + wait;
    let mut pushes = Vec::new();
    while let Ok(Some(push)) = tokio::time::timeout(
        deadline.saturating_duration_since(Instant::now()),
        rx.recv(),
    )
    .await
    {
        pushes.push(push);
    }
    pushes
}

/// Mail spec 7: a key account's push subscription that asks for email
/// properties receives state changes only; an ordinary account's receives
/// the EmailPush object (the control).
async fn push_is_state_only(test: &TestServer, key: &Account, plain: &Account) {
    println!("push_is_state_only");
    let (mut rx, push) = spawn_mock_push_server(test).await;
    let admin = test.account("admin@example.com");
    // The push manager removes the last subscription from its delivery table,
    // but the router can keep the account's registration flag. Give each
    // filter case its own account so readiness measures its own activation.
    let no_email_push = admin
        .create_key_user_account(
            "keymail-push-none@example.com",
            STRONG,
            "Push filter test",
            &[],
            user_permissions(),
        )
        .await;
    let mailbox_only = admin
        .create_key_user_account(
            "keymail-push-mailbox@example.com",
            STRONG,
            "Push filter test",
            &[],
            user_permissions(),
        )
        .await;
    // Initialize the inboxes before subscribing so the Mailbox case
    // observes delivery to an existing inbox.
    for account in [&no_email_push, &mailbox_only] {
        let mailboxes = account.jmap_get("Mailbox", ["role"], [] as [&str; 0]).await;
        assert!(
            mailboxes
                .list()
                .iter()
                .any(|mailbox| mailbox["role"] == "inbox")
        );
    }
    for (account, email_push, types) in [
        (key, true, json!([])),
        (&no_email_push, false, json!([])),
        (&mailbox_only, true, json!(["Mailbox"])),
        (plain, true, json!([])),
    ] {
        let subscribers = &test.server.inner.ipc.push_subscribers;
        // An empty change matches no local subscription types and dispatch
        // alone does not queue a web push.
        let probe = PushNotification::StateChange(StateChange::new(account.id().document_id()));
        assert!(
            !subscribers.dispatch(&probe),
            "{}: already registered",
            account.name()
        );
        let mut create = json!({
            "deviceClientId": "za-mail-reads",
            "url": format!("{PUSH_ORIGIN}/push?skip_checks=true"),
            "keys": {
                "p256dh": URL_SAFE_NO_PAD.encode(&push.public_key),
                "auth": URL_SAFE_NO_PAD.encode(&push.auth_secret),
            },
            "types": types,
        });
        if email_push {
            create["emailPush"] = json!({
                (account.id_string()): { "properties": ["id", "subject", "from"] }
            });
        }
        let created = account
            .jmap_request(
                &[
                    "urn:ietf:params:jmap:core",
                    "urn:ietf:params:jmap:emailpush",
                ],
                json!([["PushSubscription/set", { "create": { "i0": create } }, "0"]]),
            )
            .await;
        let subscription_id = created.created_id(0).to_string();
        let verification = tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("a push verification")
            .unwrap();
        assert_eq!(verification["@type"], "PushVerification");
        assert_eq!(verification["pushSubscriptionId"], subscription_id);
        let verified = account
            .jmap_request(
                &["urn:ietf:params:jmap:core"],
                json!([[
                    "PushSubscription/set",
                    { "update": { (subscription_id.clone()): {
                        "verificationCode": verification["verificationCode"]
                    } } },
                    "0"
                ]]),
            )
            .await;

        verified.updated(&subscription_id);
        tokio::time::timeout(Duration::from_secs(10), async {
            while !subscribers.dispatch(&probe) {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{}: push registration did not activate", account.name()));

        deliver(
            test,
            SENDER,
            account.name(),
            &message_with("push subject canary", "push body canary"),
        )
        .await;
        let pushes = pushes_within(&mut rx, Duration::from_secs(5)).await;
        if !email_push {
            assert!(pushes.is_empty(), "unsubscribed types: {pushes:?}");
        } else if types == json!(["Mailbox"]) {
            assert_state_only(&pushes, account, &["Mailbox"]);
            assert!(
                pushes
                    .iter()
                    .all(|push| { push["changed"][account.id_string()].get("Email").is_none() }),
                "the Mailbox filter was bypassed: {pushes:?}"
            );
        } else {
            let is_key = account.name() == KEY;
            if is_key {
                assert_state_only(&pushes, account, &["Email", "Mailbox"]);
            } else {
                assert!(
                    pushes.iter().any(|push| push["@type"] == "EmailPush"
                        && push.to_string().contains("push subject canary")),
                    "the control received no EmailPush object: {pushes:?}"
                );
            }

            let mut imap = imap_session(account, b"_push ").await;
            let before_append = account.jmap_get("Email", ["id"], [] as [&str; 0]).await;
            let subject = "append push canary";
            imap.append("INBOX", &message_with(subject, "append push body canary"))
                .await;
            imap.send_ok("LOGOUT").await;
            let after_append = account
                .jmap_get("Email", ["id", "subject"], [] as [&str; 0])
                .await;
            assert_ne!(after_append.state(), before_append.state(), "APPEND state");
            let appended = after_append
                .list()
                .iter()
                .find(|email| email["subject"] == subject)
                .expect("the appended message");
            assert_message_push(
                &mut rx,
                account,
                is_key,
                &appended["id"],
                subject,
                after_append.state(),
            )
            .await;

            let subject = "import push canary";
            let blob_id = account
                .jmap_client()
                .await
                .upload(
                    None,
                    message_with(subject, "import push body canary").into_bytes(),
                    None,
                )
                .await
                .unwrap()
                .take_blob_id();
            let imported = account
                .jmap_method_call(
                    "Email/import",
                    json!({
                        "accountId": account.id_string(),
                        "emails": { "i": {
                            "blobId": blob_id,
                            "mailboxIds": { (Id::from(email::mailbox::INBOX_ID).to_string()): true },
                        } },
                    }),
                )
                .await;
            let imported_id = imported
                .pointer("/methodResponses/0/1/created/i/id")
                .expect("the imported message");
            let import_state = imported.new_state();
            assert_ne!(import_state, after_append.state(), "import state");
            assert_message_push(&mut rx, account, is_key, imported_id, subject, import_state).await;
        }

        let destroyed = account
            .jmap_request(
                &["urn:ietf:params:jmap:core"],
                json!([[
                    "PushSubscription/set",
                    { "destroy": [&subscription_id] },
                    "0"
                ]]),
            )
            .await;
        assert_eq!(
            destroyed.destroyed().collect::<Vec<_>>(),
            [subscription_id.as_str()]
        );
    }

    let key_id = key.id().document_id();
    let document_id = *message_ids(test, key_id).await.last().unwrap();
    let config = EmailPush {
        account_id: key_id,
        properties: vec![EmailPushProperty::Subject, EmailPushProperty::From],
        filter: vec![],
        urgency: Urgency::Normal,
    };
    assert!(
        build_email_push_object(&test.server, key_id, document_id, &config, 4096)
            .await
            .unwrap()
            .is_none(),
        "a key account's EmailPush object was built"
    );
    drop(push);
    clear_messages(key).await;
    clear_messages(plain).await;
    // The ordinary fixture's verification is processed after both filter
    // subscriptions' removal updates, so the filter accounts can be deleted.
    for account in [no_email_push, mailbox_only] {
        test.destroy_all_mailboxes(&account).await;
        admin.destroy_account(account).await;
    }
}

fn assert_state_only(pushes: &[Value], account: &Account, types: &[&str]) {
    assert!(
        pushes.iter().all(|push| push["@type"] == "StateChange"),
        "a key account's push carried more than state: {pushes:?}"
    );
    assert!(
        !pushes
            .iter()
            .any(|push| push.to_string().contains("canary")),
        "{pushes:?}"
    );
    assert!(
        pushes.iter().any(|push| {
            let changed = &push["changed"][account.id_string()];
            types.iter().all(|typ| changed.get(*typ).is_some())
        }),
        "no {types:?} state change: {pushes:?}"
    );
}

async fn assert_message_push(
    rx: &mut mpsc::Receiver<Value>,
    account: &Account,
    is_key: bool,
    email_id: &Value,
    subject: &str,
    expected_state: &str,
) {
    let pushes = pushes_within(rx, Duration::from_secs(5)).await;
    if is_key {
        assert_state_only(&pushes, account, &["Email", "Mailbox"]);
        assert!(
            pushes.iter().any(|push| {
                let changed = &push["changed"][account.id_string()];
                changed["Email"] == expected_state && changed.get("Mailbox").is_some()
            }),
            "no Email state {expected_state} for {email_id}: {pushes:?}"
        );
    } else {
        assert!(
            pushes.iter().any(|push| {
                push["@type"] == "EmailPush"
                    && push["accountId"] == account.id_string()
                    && push["emails"].as_array().is_some_and(|emails| {
                        emails
                            .iter()
                            .any(|email| email["id"] == *email_id && email["subject"] == subject)
                    })
            }),
            "the control received no EmailPush for {email_id}: {pushes:?}"
        );
    }
}

/// Review focus 1: a message delivered while a session idles is announced
/// and reads in the same session.
async fn rf_1_idle_delivery(test: &TestServer, key: &Account) {
    println!("rf_1_idle_delivery");
    let mut imap = imap_session(key, b"_i ").await;
    imap.send("IDLE").await;
    imap.assert_read(Type::Continuation, ResponseType::Ok).await;
    deliver(test, SENDER, KEY, &message_with("idle", "idle body canary")).await;
    let exists = loop {
        let line = imap.read(Type::Untagged).await.pop().unwrap();
        if let Some(n) = line
            .strip_prefix("* ")
            .and_then(|line| line.strip_suffix(" EXISTS"))
        {
            break n.parse::<u32>().unwrap();
        }
    };
    imap.send_untagged("DONE").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    let fetched = imap_answer(&mut imap, &format!("FETCH {exists} (BODY[TEXT])")).await;
    assert!(fetched.contains("idle body canary"), "{fetched}");
    imap.send_ok("LOGOUT").await;
    clear_messages(key).await;
}

/// Review focus 5: a header block of twenty 1000-byte headers puts the
/// bundle past the archive compression watermark (8192 bytes), so the
/// view's archive of the opened metadata is LZ4-compressed; the header
/// block still reads through FETCH and Email/get.
async fn rf_5_large_header_block(test: &TestServer, key: &Account) {
    println!("rf_5_large_header_block");
    let key_id = key.id().document_id();
    // Each value is 1000 bytes, folded into lines of 100.
    let pads = (0..20)
        .map(|n| {
            let value = format!("{n:02}{}", "p".repeat(998));
            let folded = value
                .as_bytes()
                .chunks(100)
                .map(|chunk| std::str::from_utf8(chunk).unwrap())
                .collect::<Vec<_>>()
                .join("\r\n ");
            (format!("X-Pad-{n}"), format!("X-Pad-{n}: {folded}\r\n"))
        })
        .collect::<Vec<_>>();
    let message = format!(
        "From: Sender <sender@example.org>\r\n\
         To: {KEY}\r\n\
         Subject: large header block\r\n\
         {}\
         \r\n\
         large header body\r\n",
        pads.iter()
            .map(|(_, header)| header.as_str())
            .collect::<String>()
    );
    let before = message_ids(test, key_id).await;
    deliver(test, SENDER, KEY, &message).await;
    let document_id = new_message(test, key_id, &before).await;

    let mut imap = imap_session(key, b"_h ").await;
    let fetched = imap_answer(&mut imap, "FETCH 1 (BODY[HEADER])").await;
    for (_, header) in &pads {
        assert!(
            fetched.contains(header.as_str()),
            "BODY[HEADER] lacks {header}"
        );
    }
    imap.send_ok("LOGOUT").await;

    let (list, _) = email_get(key, None, &["id", "headers"]).await;
    let headers = list[0]["headers"].as_array().unwrap();
    for (name, header) in &pads {
        let value = headers
            .iter()
            .find(|h| h["name"] == name.as_str())
            .unwrap_or_else(|| panic!("Email/get lacks {name}"))["value"]
            .as_str()
            .unwrap();
        assert_eq!(
            format!("{name}:{value}"),
            header.trim_end_matches("\r\n"),
            "{name}"
        );
    }

    // At 1.0 the row compresses its header section past the dictionary's
    // watermark (`MetadataRow::encode`): the opened row's is compressed.
    assert!(
        row_is_stub(&stored_metadata(test, key_id, document_id).await),
        "the large message is stored in the clear"
    );
    let (full, _) = open_message(test, key_id, STRONG, document_id).await;
    let header_block = full.raw_headers().unwrap().len();
    assert!(
        full.headers_compressed(),
        "the opened row's header section ({header_block} bytes) is not compressed"
    );
    clear_messages(key).await;
}

/// The attributes `assigned_ids_preserve_sealed_read_offsets` compares,
/// one `UID FETCH` each: the whole message, the outer text, the nested
/// message (a base64 `message/rfc822`) and its parts, the outer
/// attachment's encoded body, the decoded sizes, the structure and the
/// envelope.
const ASSIGNED_FETCH: &[&str] = &[
    "BODY.PEEK[]",
    "BODY.PEEK[HEADER]",
    "BODY.PEEK[1]",
    "BODY.PEEK[2]",
    "BODY.PEEK[2.1]",
    "BODY.PEEK[2.2]",
    "BODY.PEEK[3]",
    "BINARY.SIZE[2.2]",
    "BINARY.SIZE[3]",
    "BODYSTRUCTURE",
    "ENVELOPE",
    "RFC822.SIZE",
];

/// The decoded bytes of the nested message's attachment in fixture `n`.
fn assigned_inner_bytes(n: u8) -> Vec<u8> {
    [
        format!("assigned inner attachment {n} ").as_bytes(),
        &[0x00, 0x01, 0xfe, 0xff, n],
    ]
    .concat()
}

/// The decoded bytes of the outer attachment in fixture `n`.
fn assigned_outer_bytes(n: u8) -> Vec<u8> {
    [
        format!("assigned outer attachment {n} ").as_bytes(),
        &[0xff, 0x00, n, 0x80],
    ]
    .concat()
}

/// Fixture `n`'s nested message: a multipart with a text part and a
/// base64 attachment of its own.
fn assigned_inner(n: u8) -> String {
    format!(
        "From: Inner <inner@example.org>\r\n\
         Subject: assigned inner {n}\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"inner{n}\"\r\n\
         \r\n\
         --inner{n}\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         assigned inner body canary {n}\r\n\
         --inner{n}\r\n\
         Content-Type: application/octet-stream; name=\"inner{n}.bin\"\r\n\
         Content-Disposition: attachment; filename=\"inner{n}.bin\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         {}\r\n\
         --inner{n}--\r\n",
        STANDARD.encode(assigned_inner_bytes(n)),
    )
}

/// The extended read fixture `n`: a multipart/mixed message holding a text
/// part, a base64-encoded `message/rfc822` (`assigned_inner`) and a base64
/// attachment, with a unique `Message-ID`.
fn assigned_fixture(n: u8) -> String {
    format!(
        "From: Sender <sender@example.org>\r\n\
         To: Recipient <recipient@example.com>\r\n\
         Subject: assigned ids {n}\r\n\
         Message-ID: <{:016x}.za-assigned@example.org>\r\n\
         Date: Wed, 21 Oct 2026 1{n}:00:00 +0200\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"outer{n}\"\r\n\
         \r\n\
         --outer{n}\r\n\
         Content-Type: text/plain; charset=us-ascii\r\n\
         \r\n\
         assigned body canary {n}\r\n\
         --outer{n}\r\n\
         Content-Type: message/rfc822\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         {}\r\n\
         --outer{n}\r\n\
         Content-Type: application/octet-stream; name=\"outer{n}.bin\"\r\n\
         Content-Disposition: attachment; filename=\"outer{n}.bin\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         {}\r\n\
         --outer{n}--\r\n",
        store::rand::random::<u64>(),
        STANDARD.encode(assigned_inner(n)),
        STANDARD.encode(assigned_outer_bytes(n)),
    )
}

/// An IMAP answer with its sequence number and UID replaced, so two
/// accounts' answers for the same message compare equal.
fn normalize_uid(answer: &str, uid: u32) -> String {
    let answer = answer.replace(&format!("UID {uid}"), "UID <uid>");
    match answer
        .strip_prefix("* ")
        .and_then(|rest| rest.split_once(" FETCH "))
    {
        Some((_, rest)) => format!("* <seq> FETCH {rest}"),
        None => answer,
    }
}

/// A JMAP object with its `id`, `blobId` and `threadId` values removed at
/// every level (they name documents, which differ between accounts).
fn strip_ids(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in ["id", "blobId", "threadId"] {
                map.remove(key);
            }
            map.values_mut().for_each(strip_ids);
        }
        Value::Array(list) => list.iter_mut().for_each(strip_ids),
        _ => {}
    }
}

/// The part named `name` among `parts` (a JMAP `attachments` list).
fn part_named<'x>(parts: &'x Value, name: &str) -> &'x Value {
    parts
        .as_array()
        .and_then(|parts| parts.iter().find(|part| part["name"] == name))
        .unwrap_or_else(|| panic!("no part {name} in {parts}"))
}

/// Task 5 of the 1.0 upgrade: a grouped ingest (`IngestGroup`) seals each
/// message under its pending slot and learns its document id, thread and
/// UID only from the flush (`AssignedIds`, `email_ingest_finish`). Three
/// extended fixtures, each with a base64 `message/rfc822` holding an
/// attachment of its own, go to the key account in one group (with the
/// session's keys) and to the ordinary account in another. By the ids the
/// flush assigned, every key-account message is the one sealed for it (its
/// stub opens into its own row and blob, bound to its account), and every
/// read answers the ordinary account's: FETCH of the nested parts by UID,
/// `Email/get`, the downloads of each attachment and of the nested
/// message, and the nested message's own attachment through `Email/parse`
/// of its section (the decoded bytes, byte for byte). Without keys and
/// with a corrupted stub or blob, the same reads are refused or fail
/// alone.
async fn assigned_ids_preserve_sealed_read_offsets(
    test: &TestServer,
    key: &Account,
    plain: &Account,
) {
    println!("assigned_ids_preserve_sealed_read_offsets");
    let key_id = key.id().document_id();
    let plain_id = plain.id().document_id();
    let fixtures = [1u8, 2, 3].map(assigned_fixture);
    // A fourth message, without an attachment: `hasAttachment` false from
    // the opened row (ruling 55), as the ordinary account's keyword says.
    let no_attachment = message_with("assigned no attachment", "assigned plain canary");
    let messages = fixtures
        .iter()
        .chain([&no_attachment])
        .cloned()
        .collect::<Vec<_>>();

    let mut grouped = Vec::new();
    for (account, token) in [
        (key, keyed_token(test, key_id, STRONG).await),
        (plain, keyless_token(plain_id)),
    ] {
        let account_id = account.id().document_id();
        let mut group = IngestGroup::default();
        for raw in &messages {
            group_ingest(&test.server, &mut group, &token, raw)
                .await
                .unwrap();
        }
        let ingested = group.flush(&test.server).await.unwrap();
        assert_eq!(ingested.len(), messages.len(), "{}", account.name());
        for (email, raw) in ingested.iter().zip(&messages) {
            assert_eq!(
                email.blob_id.class,
                BlobClass::Linked {
                    account_id,
                    collection: Collection::Email.into(),
                    document_id: email.document_id,
                },
                "{}: the result's owner",
                account.name()
            );
            assert_eq!(email.size, raw.len(), "{}", account.name());
            assert_eq!(email.imap_uids.len(), 1, "{}", account.name());
        }
        grouped.push(ingested);
    }
    let (key_emails, plain_emails) = (&grouped[0], &grouped[1]);

    // Each key-account message, by its assigned id, is the one sealed for
    // it: its stub opens into a row naming its own sealed blob, the blob
    // into its own bytes, and the bundle is bound to the account.
    let keys = crate::utils::za::za_unlock(test, key_id, STRONG).await;
    for (email, raw) in key_emails.iter().zip(&messages) {
        assert_sealed(test, key_id, email.document_id, WRAP_MK).await;
        assert_eq!(email.thread_id, email.document_id, "a thread of its own");
        assert_eq!(
            email.blob_id.hash,
            stored_hash(test, key_id, email.document_id).await
        );
        let (opened, bytes) = open_message(test, key_id, STRONG, email.document_id).await;
        assert_eq!(bytes, raw.as_bytes(), "message {}", email.document_id);
        assert_eq!(
            opened.unarchive().unwrap().blob_hash(),
            email.blob_id.hash,
            "the opened row names the sealed blob"
        );
        let stub = stored_metadata(test, key_id, email.document_id).await;
        assert!(
            open_stub(&stub, &keys, None, plain_id).is_err(),
            "a stub opened under another account"
        );
    }
    assert_no_plaintext(test, key_id).await;

    // The reads, by the assigned UIDs and ids, equal the ordinary account's.
    let mut key_imap = imap_session(key, b"_a ").await;
    let mut plain_imap = imap_session(plain, b"_a ").await;
    for (n, ((key_email, plain_email), raw)) in key_emails
        .iter()
        .zip(plain_emails)
        .zip(&fixtures)
        .enumerate()
    {
        let n = n as u8 + 1;
        let (key_uid, plain_uid) = (key_email.imap_uids[0], plain_email.imap_uids[0]);
        for attribute in ASSIGNED_FETCH {
            let key_answer = normalize_uid(
                &imap_answer(&mut key_imap, &format!("UID FETCH {key_uid} ({attribute})")).await,
                key_uid,
            );
            let plain_answer = normalize_uid(
                &imap_answer(
                    &mut plain_imap,
                    &format!("UID FETCH {plain_uid} ({attribute})"),
                )
                .await,
                plain_uid,
            );
            assert_eq!(
                key_answer, plain_answer,
                "fixture {n}: UID FETCH {attribute}"
            );
            assert!(
                key_answer.contains("UID <uid>"),
                "fixture {n}: {attribute} did not answer its UID: {key_answer}"
            );
        }
        let whole = imap_answer(&mut key_imap, &format!("UID FETCH {key_uid} (BODY.PEEK[])")).await;
        assert!(
            whole.contains(raw.as_str()),
            "fixture {n}: BODY[] is not the message"
        );

        let mut answers = Vec::new();
        for (account, email) in [(key, key_email), (plain, plain_email)] {
            let id = Id::from_parts(email.thread_id, email.document_id).to_string();
            let (list, not_found) = email_get(
                account,
                Some(vec![id]),
                &[
                    "id",
                    "blobId",
                    "subject",
                    "sentAt",
                    "size",
                    "preview",
                    "hasAttachment",
                    "headers",
                    "bodyStructure",
                    "bodyValues",
                    "textBody",
                    "attachments",
                ],
            )
            .await;
            assert!(not_found.is_empty(), "{}: {not_found:?}", account.name());
            let email = list[0].clone();
            assert_eq!(email["subject"], format!("assigned ids {n}"), "{email}");

            // The outer attachment and the nested message, downloaded.
            let outer = part_named(&email["attachments"], &format!("outer{n}.bin"));
            let (status, bytes) = download(account, outer["blobId"].as_str().unwrap()).await;
            assert_eq!(status, 200, "{}: the outer attachment", account.name());
            assert_eq!(bytes, assigned_outer_bytes(n), "{}", account.name());
            let nested = email["attachments"]
                .as_array()
                .unwrap()
                .iter()
                .find(|part| part["type"] == "message/rfc822")
                .unwrap_or_else(|| panic!("no nested message in {email}"));
            let nested_blob = nested["blobId"].as_str().unwrap().to_string();
            let (status, bytes) = download(account, &nested_blob).await;
            assert_eq!(status, 200, "{}: the nested message", account.name());
            assert_eq!(
                bytes,
                assigned_inner(n).as_bytes(),
                "{}: the nested message's decoded bytes",
                account.name()
            );

            // The nested message's own attachment, through its section.
            let parsed = account
                .jmap_method_call(
                    "Email/parse",
                    json!({
                        "accountId": account.id_string(),
                        "blobIds": [nested_blob],
                        "properties": ["subject", "attachments", "bodyValues", "textBody"],
                        "fetchAllBodyValues": true,
                    }),
                )
                .await;
            let parsed = parsed
                .pointer(&format!("/methodResponses/0/1/parsed/{nested_blob}"))
                .unwrap_or_else(|| panic!("{}: Email/parse: {:?}", account.name(), parsed.0))
                .clone();
            assert_eq!(parsed["subject"], format!("assigned inner {n}"), "{parsed}");
            let inner = part_named(&parsed["attachments"], &format!("inner{n}.bin"));
            let inner_blob = inner["blobId"].as_str().unwrap().to_string();
            let (status, bytes) = download(account, &inner_blob).await;
            assert_eq!(status, 200, "{}: the nested attachment", account.name());
            assert_eq!(
                bytes,
                assigned_inner_bytes(n),
                "{}: the nested attachment's decoded bytes",
                account.name()
            );

            let mut email = email;
            strip_ids(&mut email);
            let mut parsed = parsed;
            strip_ids(&mut parsed);
            answers.push((email, parsed, nested_blob, inner_blob));
        }
        let (key_email_json, key_parsed, key_nested, key_inner) = &answers[0];
        let (plain_email_json, plain_parsed, _, _) = &answers[1];
        assert_eq!(key_email_json, plain_email_json, "fixture {n}: Email/get");
        assert_eq!(key_parsed, plain_parsed, "fixture {n}: Email/parse");
        assert_eq!(key_email_json["hasAttachment"], true, "{key_email_json}");

        // Owner binding: the ordinary account cannot read the key account's
        // sections (not found, as without a grant: ruling 60), and a token
        // without the key account's keys is refused.
        for blob_id in [key_nested, key_inner] {
            let (status, bytes) = download(plain, blob_id).await;
            assert_eq!(
                status, 404,
                "fixture {n}: another account's download of a sealed section"
            );
            assert!(
                !String::from_utf8_lossy(&bytes).contains("canary")
                    && !bytes
                        .windows(assigned_inner_bytes(n).len())
                        .any(|w| w == assigned_inner_bytes(n)),
                "fixture {n}: the refused download carries content"
            );
        }
        let token = za_bearer_token(&test.server, KEY).await;
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        for blob_id in [key_nested, key_inner] {
            let response = client
                .get(format!(
                    "{}/jmap/download/{}/{blob_id}/blob",
                    key.base_url(),
                    key.id_string()
                ))
                .bearer_auth(&token)
                .send()
                .await
                .unwrap();
            let status = response.status().as_u16();
            let body = response.text().await.unwrap();
            assert!(
                status == 403 && body.contains(ZA_MAIL_KEYLESS) && !body.contains("canary"),
                "fixture {n}: a keyless section download: {status}: {body}"
            );
        }
    }
    key_imap.send_ok("LOGOUT").await;
    plain_imap.send_ok("LOGOUT").await;

    // The message without an attachment: the content-derived properties,
    // read from the opened row, equal the ordinary account's.
    let mut answers = Vec::new();
    for (account, email) in [(key, &key_emails[3]), (plain, &plain_emails[3])] {
        let id = Id::from_parts(email.thread_id, email.document_id).to_string();
        let (list, not_found) = email_get(
            account,
            Some(vec![id]),
            &[
                "id",
                "hasAttachment",
                "sentAt",
                "size",
                "preview",
                "subject",
            ],
        )
        .await;
        assert!(not_found.is_empty(), "{}: {not_found:?}", account.name());
        let mut email = list[0].clone();
        strip_ids(&mut email);
        answers.push(email);
    }
    assert_eq!(answers[0], answers[1], "the message without an attachment");
    assert_eq!(answers[0]["hasAttachment"], false, "{}", answers[0]);
    assert!(
        answers[0]["sentAt"]
            .as_str()
            .is_some_and(|sent| sent.starts_with("2026-10-20T")),
        "the Date header's sentAt: {}",
        answers[0]
    );

    // Corrupt reads fail their own message only: the first fixture's stub
    // and the last one's blob; the middle one still reads.
    let (stub_email, middle, blob_email) = (&key_emails[0], &key_emails[1], &key_emails[2]);
    corrupt_stub(test, key_id, stub_email.document_id).await;
    corrupt_blob(
        test,
        &stored_hash(test, key_id, blob_email.document_id).await,
    )
    .await;
    let mut imap = imap_session(key, b"_a ").await;
    for email in [stub_email, blob_email] {
        imap.send(&format!(
            "UID FETCH {} (BODY.PEEK[2.2])",
            email.imap_uids[0]
        ))
        .await;
        let lines = imap.assert_read(Type::Tagged, ResponseType::No).await;
        assert_eq!(
            lines.last().unwrap(),
            &format!("_a NO [SERVERBUG] {ZA_MAIL_UNREADABLE}"),
            "a corrupted message's section read"
        );
        assert!(
            !lines.join("\r\n").contains("canary"),
            "a corrupted message's read carries content"
        );
    }
    let answer = imap_answer(
        &mut imap,
        &format!("UID FETCH {} (BODY.PEEK[1])", middle.imap_uids[0]),
    )
    .await;
    assert!(answer.contains("assigned body canary 2"), "{answer}");
    imap.send_ok("LOGOUT").await;
    let ids = key_emails
        .iter()
        .map(|email| key_email_id(email.document_id))
        .collect::<Vec<_>>();
    // `bodyValues` reads the blob (a subject comes from the opened stub), as
    // `corrupted_stub_and_blob` reads it.
    let (list, mut not_found) = email_get(
        key,
        Some(ids[..3].to_vec()),
        &["id", "subject", "bodyValues"],
    )
    .await;
    not_found.sort();
    let mut unreadable = vec![ids[0].clone(), ids[2].clone()];
    unreadable.sort();
    assert_eq!(not_found, unreadable, "the corrupted messages");
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0]["subject"], "assigned ids 2");

    let destroyed = key
        .jmap_destroy("Email", &ids, Vec::<(&str, &str)>::new())
        .await
        .destroyed()
        .count();
    assert_eq!(destroyed, ids.len(), "the key account's grouped messages");
    let plain_ids = plain_emails
        .iter()
        .map(|email| Id::from_parts(email.thread_id, email.document_id).to_string())
        .collect::<Vec<_>>();
    let destroyed = plain
        .jmap_destroy("Email", &plain_ids, Vec::<(&str, &str)>::new())
        .await
        .destroyed()
        .count();
    assert_eq!(
        destroyed,
        plain_ids.len(),
        "the ordinary account's grouped messages"
    );
    test.wait_for_tasks().await;
}
