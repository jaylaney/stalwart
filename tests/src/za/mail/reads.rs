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
    assert_sealed, corrupt_blob, corrupt_stub, deliver, key_mail_account, message_ids,
    message_with, new_message, open_message, stored_metadata,
};
use crate::{
    utils::{
        account::Account,
        imap::{ImapConnection, Type},
        pop3::{Pop3Connection, ResponseType as Pop3Reply},
        push::{PUSH_ORIGIN, spawn_mock_push_server},
        server::TestServer,
        smtp::SmtpConnection,
        za::{bearer_token, plant_plain_message, wait_for_delivery},
    },
    za::{STRONG, user_permissions},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use common::{
    auth::AccessToken,
    za::{ZA_MAIL_KEYLESS, ZA_MAIL_SEARCH_REFUSED, ZA_MAIL_UNREADABLE, test_hooks},
};
use email::{
    mailbox::{DRAFTS_ID, SENT_ID},
    message::{
        metadata::MessageMetadata,
        za::{is_stub, view::ZaMailView},
    },
    push::{EmailPush, Urgency},
};
use groupware::calendar::seal::{WRAP_MK, WRAP_PK};
use imap_proto::{ResponseType, protocol::SerializeResponse as ImapResponse};
use jmap_proto::object::push_subscription::EmailPushProperty;
use pop3::protocol::response::SerializeResponse as Pop3Response;
use serde_json::{Value, json};
use services::state_manager::email_push::build_email_push_object;
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};
use store::{Serialize, write::Archiver};
use tokio::sync::mpsc;
use trc::{
    Collector, EventType, Key, StoreEvent,
    ipc::subscriber::{Interests, SubscriberBuilder},
};
use types::{blob_hash::BlobHash, id::Id};

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

    for account in [&key, &plain, &bcc] {
        test.destroy_all_mailboxes(account).await;
    }
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
    for (attribute, (key, plain)) in FETCH_ATTRIBUTES
        .iter()
        .zip(key_fetched.iter().zip(plain_fetched))
    {
        assert_eq!(key, plain, "FETCH {attribute}");
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
    let token = bearer_token(KEY, STRONG).await;
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
    // ordinary account's read) the view and the opener pass the stored
    // archive and bytes through, even a stub's.
    assert!(is_stub(stored.unarchive::<MessageMetadata>().unwrap()));
    let view = test
        .server
        .za_metadata_view(&stored, None, key_id, corpus_doc)
        .await
        .unwrap();
    assert!(
        matches!(&view, Cow::Borrowed(borrowed) if std::ptr::eq(*borrowed, &stored)),
        "the stored archive, borrowed"
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
    let archive = stored_metadata(test, plain_id, document_id).await;
    assert!(
        is_stub(archive.unarchive::<MessageMetadata>().unwrap()),
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
        test.server.inner.cache.http_auth.clear();
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
    let blob_hash = BlobHash::from(
        &stored_metadata(test, key_id, blob_doc)
            .await
            .unarchive::<MessageMetadata>()
            .unwrap()
            .blob_hash,
    );
    corrupt_blob(test, &blob_hash).await;

    let traced = [EventType::Store(StoreEvent::DataCorruption)];
    let mut interests = Interests::default();
    for event_type in traced {
        interests.set(event_type);
    }
    let (_tx, mut rx) = SubscriberBuilder::new(SUBSCRIBER_ID.into())
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

    let archive = stored_metadata(test, key_id, legacy_doc).await;
    let stored = archive.unarchive::<MessageMetadata>().unwrap();
    assert!(!is_stub(stored), "the planted message reads as a stub");
    assert!(!stored.contents.is_empty() && !stored.contents[0].parts.is_empty());
    assert_sealed(test, key_id, corpus_doc, WRAP_PK).await;
    let archive = stored_metadata(test, key_id, corpus_doc).await;
    let stub = archive.unarchive::<MessageMetadata>().unwrap();
    assert!(!stub.contents.is_empty() && !stub.contents[0].parts.is_empty());

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
    for account in [key, plain] {
        let is_key = account.name() == KEY;
        let created = account
            .jmap_request(
                &[
                    "urn:ietf:params:jmap:core",
                    "urn:ietf:params:jmap:emailpush",
                ],
                json!([[
                    "PushSubscription/set",
                    {
                        "create": {
                            "i0": {
                                "deviceClientId": "za-mail-reads",
                                "url": format!("{PUSH_ORIGIN}/push?skip_checks=true"),
                                "keys": {
                                    "p256dh": URL_SAFE_NO_PAD.encode(&push.public_key),
                                    "auth": URL_SAFE_NO_PAD.encode(&push.auth_secret),
                                },
                                "types": [],
                                "emailPush": {
                                    (account.id_string()): { "properties": ["subject", "from"] }
                                }
                            }
                        }
                    },
                    "0"
                ]]),
            )
            .await;
        let subscription_id = created.created_id(0).to_string();
        let verification = pushes_within(&mut rx, Duration::from_secs(10))
            .await
            .into_iter()
            .find(|push| push["@type"] == "PushVerification")
            .expect("a push verification");
        assert_eq!(verification["pushSubscriptionId"], subscription_id);
        account
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

        deliver(
            test,
            SENDER,
            account.name(),
            &message_with("push subject canary", "push body canary"),
        )
        .await;
        let pushes = pushes_within(&mut rx, Duration::from_secs(5)).await;
        if is_key {
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
                    changed.get("Email").is_some() && changed.get("Mailbox").is_some()
                }),
                "no Email and Mailbox state change: {pushes:?}"
            );
        } else {
            assert!(
                pushes.iter().any(|push| push["@type"] == "EmailPush"
                    && push.to_string().contains("push subject canary")),
                "the control received no EmailPush object: {pushes:?}"
            );
        }
        account
            .jmap_request(
                &["urn:ietf:params:jmap:core"],
                json!([[
                    "PushSubscription/set",
                    { "destroy": [subscription_id] },
                    "0"
                ]]),
            )
            .await;
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

    let archive = stored_metadata(test, key_id, document_id).await;
    let bundle = archive
        .unarchive::<MessageMetadata>()
        .unwrap()
        .raw_headers
        .len();
    assert!(bundle > 8192, "the bundle is {bundle} bytes");
    let (full, _) = open_message(test, key_id, STRONG, document_id).await;
    let header_block = full.raw_headers.len();
    let view_archive = Archiver::new(full).serialize().unwrap().len();
    assert!(
        view_archive < header_block,
        "the view's archive ({view_archive} bytes) is not compressed below the header block ({header_block} bytes)"
    );
    clear_messages(key).await;
}
