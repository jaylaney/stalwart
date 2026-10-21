/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 4, 6.3 and 9: the access gates of a key account's mail. The
//! JMAP dispatcher refuses the key-needing methods to a token without the
//! account's keys and serves the keyless ones; copies and moves across
//! accounts, and a sealed source read into another account (ruling R31),
//! are refused when either side is a key account; a key account's
//! mailboxes cannot be shared, and a grant made before the account was
//! provisioned stops routing (ruling R5).

use super::{Traced, deliver, fence::set_marker, key_mail_account, message_ids, message_with};
use crate::{
    utils::{
        account::Account,
        imap::{AssertResult, ImapConnection, Type},
        jmap::JmapResponse,
        server::TestServer,
        za::{bearer_token, wait_for_delivery, za_setup, za_setup_token},
    },
    za::{
        STRONG,
        tracing::{WsStream, ws_call, ws_close, ws_connect},
        user_permissions,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::za::ZA_MAIL_KEYLESS;
use email::mailbox::{INBOX_ID, TRASH_ID};
use imap_proto::ResponseType;
use serde_json::{Value, json};
use std::time::Duration;
use trc::{EventType, SecurityEvent, StoreEvent};
use types::id::Id;

const KEY: &str = "keymail8@example.com";
const OTHER_KEY: &str = "keymail9@example.com";
const PLAIN: &str = "plainmail7@example.com";
const OTHER_PLAIN: &str = "plainmail8@example.com";
/// `rf_2_stale_grant`'s account: plain when it shares, then provisioned.
const STALE: &str = "plainmail9@example.com";
const SENDER: &str = "sender@example.org";
/// In every message of the key accounts; no answer to another session and
/// no trace carries it.
const CANARY: &str = "gates canary";
/// The attachment of the key account's delivered message.
const ATTACHMENT: &[u8] = b"gates canary attachment \x00\x01\xfe\xff";
const KEYLESS_REFUSED: &str = "zero-access: mail access without session keys";
const COPY_REFUSED: &str = "zero-access: mail copy across accounts";
const SHARING_REFUSED: &str = "zero-access: sharing refused on a key account's mailbox";
/// The capabilities of a keyless request (`jmap_method_calls`' list, less
/// the calendar, contact and file ones these calls do not use).
const USING: &[&str] = &[
    "urn:ietf:params:jmap:core",
    "urn:ietf:params:jmap:mail",
    "urn:ietf:params:jmap:submission",
    "urn:ietf:params:jmap:vacationresponse",
    "urn:ietf:params:jmap:sieve",
    "urn:ietf:params:jmap:blob",
    "urn:ietf:params:jmap:mail:share",
];

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mail access gate tests...");
    let admin = test.account("admin@example.com").clone();
    let key = key_mail_account(test, KEY, STRONG).await;
    let other_key = key_mail_account(test, OTHER_KEY, STRONG).await;
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    let other_plain = admin
        .create_user_account(OTHER_PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    deliver(test, SENDER, KEY, &message_with_attachment(KEY)).await;
    deliver(
        test,
        SENDER,
        OTHER_KEY,
        &message_with(CANARY, "gates key body"),
    )
    .await;
    for to in [PLAIN, OTHER_PLAIN] {
        deliver(
            test,
            SENDER,
            to,
            &message_with(&format!("gates plain for {to}"), "gates body"),
        )
        .await;
    }

    keyless_tokens_hit_the_dispatcher_gate(test, &key).await;
    cross_account_copies_are_refused(test, &key, &plain, &other_plain).await;
    sharing_is_refused_on_key_mailboxes(&key, &plain).await;
    let stale = rf_2_stale_grant(test, &admin, &plain).await;

    for account in [&key, &other_key, &plain, &other_plain, &stale] {
        test.destroy_all_mailboxes(account).await;
    }
    // Uploads are temporary links: expired here, as the purge would.
    test.blob_expire_all().await;
    wait_for_delivery(test).await;
}

/// A multipart/mixed message to `to`: a text part and `ATTACHMENT`,
/// base64-encoded.
fn message_with_attachment(to: &str) -> String {
    format!(
        "From: Sender <{SENDER}>\r\n\
         To: <{to}>\r\n\
         Subject: {CANARY} delivered\r\n\
         Message-ID: <{:016x}.za-gates@example.org>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"za\"\r\n\
         \r\n\
         --za\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         {CANARY} body\r\n\
         --za\r\n\
         Content-Type: application/octet-stream; name=\"gates.bin\"\r\n\
         Content-Disposition: attachment; filename=\"gates.bin\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         {}\r\n\
         --za--\r\n",
        store::rand::random::<u64>(),
        STANDARD.encode(ATTACHMENT)
    )
}

fn inbox() -> String {
    Id::from(INBOX_ID).to_string()
}

/// The first method response of `reply`.
fn first(reply: &JmapResponse) -> &Value {
    &reply.0["methodResponses"][0]
}

/// The method call answered the keyless refusal (ruling R22).
fn assert_refused(response: &Value, what: &str) {
    assert_eq!(response[0], "error", "{what}: {response}");
    assert_eq!(
        response[1],
        json!({ "type": "forbidden", "description": ZA_MAIL_KEYLESS }),
        "{what}: {response}"
    );
}

/// The method call answered upstream's or this task's `forbidden`,
/// whichever check answers first.
fn assert_forbidden(response: &Value, what: &str) {
    assert_eq!(response[0], "error", "{what}: {response}");
    assert_eq!(response[1]["type"], "forbidden", "{what}: {response}");
    assert!(!response.to_string().contains(CANARY), "{what}: {response}");
}

/// The account's messages: id, blob id, thread id and attachments, read
/// with the account's own credential (its keys).
async fn emails(account: &Account) -> Vec<Value> {
    account
        .jmap_method_call(
            "Email/get",
            json!({
                "accountId": account.id_string(),
                "ids": null,
                "properties": ["id", "blobId", "threadId", "attachments"],
            }),
        )
        .await
        .list()
        .to_vec()
}

/// The id of `pointer` in the first method response, which must exist.
fn accepted<'x>(reply: &'x JmapResponse, pointer: &str, what: &str) -> &'x str {
    reply
        .pointer(&format!("/methodResponses/0/1{pointer}"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{what}: {:?}", reply.0))
}

/// An Email/set create of a text message into `mailbox_id`, with
/// `attachments`.
fn created_email(mailbox_id: u32, subject: &str, attachments: Value) -> Value {
    json!({
        "mailboxIds": { Id::from(mailbox_id).to_string(): true },
        "from": [{ "email": KEY }],
        "to": [{ "email": "recipient@example.com" }],
        "subject": subject,
        "textBody": [{ "partId": "t", "type": "text/plain" }],
        "bodyValues": { "t": { "value": format!("{CANARY} created") } },
        "attachments": attachments
    })
}

/// A keyless channel of the key account: its OAuth Bearer token over HTTP,
/// or a WebSocket (which never holds keys, spec 3.3).
enum Keyless {
    Bearer(String),
    WebSocket(WsStream),
}

impl Keyless {
    fn name(&self) -> &'static str {
        match self {
            Keyless::Bearer(_) => "Bearer",
            Keyless::WebSocket(_) => "WebSocket",
        }
    }

    /// One method call; its response.
    async fn call(&mut self, account: &Account, method: &str, args: Value) -> Value {
        let reply: Value = match self {
            Keyless::Bearer(token) => reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap()
                .post(account.api_url())
                .bearer_auth(token)
                .body(json!({ "using": USING, "methodCalls": [[method, args, "c0"]] }).to_string())
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap(),
            Keyless::WebSocket(socket) => serde_json::from_str(
                &ws_call(
                    socket,
                    &json!({
                        "@type": "Request",
                        "id": "1",
                        "using": USING,
                        "methodCalls": [[method, args, "c0"]]
                    })
                    .to_string(),
                )
                .await,
            )
            .unwrap(),
        };
        reply["methodResponses"][0].clone()
    }
}

/// Mail spec 4: over a keyless channel (the Bearer token, the WebSocket)
/// the key-needing methods answer `forbidden` with `ZA_MAIL_KEYLESS` at the
/// dispatcher, traced once per call as `Security(Unauthorized)` naming the
/// account, with no method or argument in the trace; the keyless methods,
/// Email/set's updates and destroys among them, succeed.
async fn keyless_tokens_hit_the_dispatcher_gate(test: &TestServer, key: &Account) {
    println!("keyless_tokens_hit_the_dispatcher_gate");
    let key_id = key.id().document_id();
    let account = key.id_string();
    let list = emails(key).await;
    assert_eq!(list.len(), 1, "{list:?}");
    let email_id = list[0]["id"].as_str().unwrap().to_string();
    let blob_id = list[0]["blobId"].as_str().unwrap().to_string();
    let thread_id = list[0]["threadId"].as_str().unwrap().to_string();
    // A message per channel for its keyless destroy, created with the
    // account's keys.
    let mut destroy_ids = Vec::new();
    for channel in ["Bearer", "WebSocket"] {
        let reply = key
            .jmap_method_call(
                "Email/set",
                json!({
                    "accountId": account,
                    "create": { "d": created_email(INBOX_ID, &format!("destroyed over {channel}"), json!([])) }
                }),
            )
            .await;
        destroy_ids.push(accepted(&reply, "/created/d/id", "a message to destroy").to_string());
    }
    let email_state = key
        .jmap_method_call("Email/get", json!({ "accountId": account, "ids": [] }))
        .await
        .state()
        .to_string();
    let thread_state = key
        .jmap_method_call("Thread/get", json!({ "accountId": account, "ids": [] }))
        .await
        .state()
        .to_string();
    let filter = json!({ "inMailbox": inbox() });
    let query_state = key
        .jmap_method_call(
            "Email/query",
            json!({ "accountId": account, "filter": filter }),
        )
        .await
        .pointer("/methodResponses/0/1/queryState")
        .and_then(Value::as_str)
        .unwrap()
        .to_string();

    // The gate answers before the method reads its arguments: an Email/get
    // of more ids than `maxObjectsInGet` is refused, not too large.
    let max_objects = key.jmap_session_object().await.0["capabilities"]["urn:ietf:params:jmap:core"]
        ["maxObjectsInGet"]
        .as_u64()
        .unwrap() as usize;
    let key_needing = [
        (
            "Email/get",
            json!({ "accountId": account, "ids": [&email_id], "properties": ["id"] }),
        ),
        (
            "Email/get",
            json!({ "accountId": account, "ids": vec![&email_id; max_objects + 1], "properties": ["id"] }),
        ),
        (
            "Email/parse",
            json!({ "accountId": account, "blobIds": [&blob_id] }),
        ),
        (
            "SearchSnippet/get",
            json!({ "accountId": account, "emailIds": [&email_id] }),
        ),
        (
            "Blob/get",
            json!({ "accountId": account, "ids": [&blob_id] }),
        ),
        (
            "Blob/upload",
            json!({ "accountId": account, "create": { "b": { "data": [{ "data:asText": CANARY }] } } }),
        ),
        (
            "Email/import",
            json!({ "accountId": account, "emails": { "i": { "blobId": &blob_id, "mailboxIds": { inbox(): true } } } }),
        ),
        (
            "Email/copy",
            json!({ "fromAccountId": account, "accountId": account, "create": { "c": { "id": &email_id, "mailboxIds": { inbox(): true } } } }),
        ),
        (
            "EmailSubmission/set",
            json!({ "accountId": account, "create": { "s": { "emailId": &email_id, "identityId": Id::from(0u32).to_string() } } }),
        ),
        (
            "Email/set",
            json!({ "accountId": account, "create": { "e": created_email(INBOX_ID, "keyless create", json!([])) } }),
        ),
    ];

    let channels = [
        Keyless::Bearer(bearer_token(KEY, STRONG).await),
        Keyless::WebSocket(ws_connect(KEY, STRONG).await),
    ];
    let traced = Traced::start(&[EventType::Security(SecurityEvent::Unauthorized)]);
    for (mut channel, destroy_id) in channels.into_iter().zip(destroy_ids) {
        let name = channel.name();
        for (method, args) in &key_needing {
            let response = channel.call(key, method, args.clone()).await;
            assert_refused(&response, &format!("{method} over the {name} channel"));
        }
        let keyless = [
            ("Mailbox/get", json!({ "accountId": account, "ids": null })),
            (
                "Email/query",
                json!({ "accountId": account, "filter": filter }),
            ),
            (
                "Email/queryChanges",
                json!({ "accountId": account, "filter": filter, "sinceQueryState": query_state }),
            ),
            (
                "Email/changes",
                json!({ "accountId": account, "sinceState": email_state }),
            ),
            (
                "Thread/get",
                json!({ "accountId": account, "ids": [&thread_id] }),
            ),
            (
                "Thread/changes",
                json!({ "accountId": account, "sinceState": thread_state }),
            ),
            (
                "Email/set",
                json!({
                    "accountId": account,
                    "update": { &email_id: {
                        "keywords/$flagged": true,
                        format!("mailboxIds/{}", Id::from(TRASH_ID)): true
                    } },
                    "destroy": [&destroy_id]
                }),
            ),
            (
                "Email/set",
                json!({
                    "accountId": account,
                    "update": { &email_id: {
                        "keywords/$flagged": null,
                        format!("mailboxIds/{}", Id::from(TRASH_ID)): null
                    } }
                }),
            ),
            ("Identity/get", json!({ "accountId": account, "ids": null })),
            (
                "VacationResponse/get",
                json!({ "accountId": account, "ids": null }),
            ),
            (
                "SieveScript/get",
                json!({ "accountId": account, "ids": null }),
            ),
            ("PushSubscription/get", json!({ "ids": null })),
        ];
        for (method, args) in keyless {
            let what = format!("{method} over the {name} channel");
            let response = channel.call(key, method, args).await;
            assert_eq!(response[0], method, "{what}: {response}");
            assert!(!response.to_string().contains(CANARY), "{what}: {response}");
            if method == "Email/set" {
                assert!(
                    response[1]
                        .get("updated")
                        .is_some_and(|u| u.get(&email_id).is_some())
                        && response[1]["notUpdated"].is_null()
                        && response[1]["notDestroyed"].is_null(),
                    "{what}: {response}"
                );
            }
        }
        if let Keyless::WebSocket(socket) = channel {
            ws_close(socket).await;
        }
    }
    let events = traced.finish().await;
    let refused = events
        .iter()
        .filter(|e| e.details == KEYLESS_REFUSED)
        .collect::<Vec<_>>();
    assert_eq!(
        refused.len(),
        2 * key_needing.len(),
        "one trace per refused call: {events:?}"
    );
    for event in &refused {
        assert_eq!(event.account_id, Some(u64::from(key_id)), "{event:?}");
        for (method, _) in &key_needing {
            assert!(!event.text.contains(method), "{event:?}");
        }
        assert!(!event.text.contains(CANARY), "{event:?}");
    }
    assert!(
        events.iter().all(|e| e.details != COPY_REFUSED),
        "the gate answers before the copy's own refusal: {events:?}"
    );
    // Nothing was created or imported; both destroys happened.
    assert_eq!(
        message_ids(test, key_id).await.len(),
        1,
        "the account's messages"
    );
}

/// Mail spec 6.3, 9, ruling R31: a copy or move across accounts is refused
/// when either side is a key account, over IMAP (`NO [NOPERM]`) and JMAP
/// (`forbidden`), and so is `Blob/copy`; a key account's sealed upload or
/// section does not become another account's message through Email/import
/// or an Email/set attachment. Between plain accounts the copy works, and
/// within the key account the import and the attachment do. Nothing is
/// stored by a refused call.
async fn cross_account_copies_are_refused(
    test: &TestServer,
    key: &Account,
    plain: &Account,
    other_plain: &Account,
) {
    println!("cross_account_copies_are_refused");
    let key_id = key.id().document_id();
    let plain_id = plain.id().document_id();
    let other_plain_id = other_plain.id().document_id();
    let shared_name = format!("Shared Folders/{PLAIN}/INBOX");
    let mut owner = ImapConnection::connect(b"_o ").await;
    owner.authenticate(PLAIN, STRONG).await;
    owner.send_ok(&format!("SETACL INBOX {KEY} lrswi")).await;
    let key_before = message_ids(test, key_id).await;
    let plain_before = message_ids(test, plain_id).await;
    let traced = Traced::start(&[EventType::Security(SecurityEvent::Unauthorized)]);

    // IMAP: the key account's COPY and UID MOVE into the plain share.
    let mut imap = ImapConnection::connect(b"_k ").await;
    imap.authenticate(KEY, STRONG).await;
    imap.send(r#"LIST "" "*""#).await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains(&shared_name);
    imap.send_ok("SELECT INBOX").await;
    for command in [
        format!("COPY 1 \"{shared_name}\""),
        format!("UID MOVE 1:* \"{shared_name}\""),
    ] {
        imap.send(&command).await;
        imap.assert_read(Type::Tagged, ResponseType::No)
            .await
            .assert_response_code("NOPERM")
            .assert_contains(ZA_MAIL_KEYLESS);
    }
    imap.send_ok("LOGOUT").await;
    // The plain account cannot name the key account's mailbox.
    owner.send(r#"LIST "" "*""#).await;
    owner
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_not_contains(KEY);
    owner.send_ok("SELECT INBOX").await;
    owner
        .send(&format!("COPY 1 \"Shared Folders/{KEY}/INBOX\""))
        .await;
    owner
        .assert_read(Type::Tagged, ResponseType::No)
        .await
        .assert_response_code("TRYCREATE");

    // JMAP Email/copy, both directions.
    let key_email = emails(key).await[0]["id"].as_str().unwrap().to_string();
    let plain_list = emails(plain).await;
    let plain_email = plain_list[0]["id"].as_str().unwrap().to_string();
    let plain_blob = plain_list[0]["blobId"].as_str().unwrap().to_string();
    let key_blob = emails(key).await[0]["blobId"].as_str().unwrap().to_string();
    let copy = |from: &Account, to: &Account, id: &str| {
        json!({
            "fromAccountId": from.id_string(),
            "accountId": to.id_string(),
            "create": { "c": { "id": id, "mailboxIds": { inbox(): true } } }
        })
    };
    assert_refused(
        first(
            &key.jmap_method_call("Email/copy", copy(key, plain, &key_email))
                .await,
        ),
        "Email/copy out of the key account",
    );
    assert_forbidden(
        first(
            &plain
                .jmap_method_call("Email/copy", copy(plain, key, &plain_email))
                .await,
        ),
        "Email/copy into the key account",
    );
    let blob_copy = |from: &Account, to: &Account, id: &str| {
        json!({
            "fromAccountId": from.id_string(),
            "accountId": to.id_string(),
            "blobIds": [id]
        })
    };
    assert_forbidden(
        first(
            &key.jmap_method_call("Blob/copy", blob_copy(plain, key, &plain_blob))
                .await,
        ),
        "Blob/copy into the key account",
    );
    assert_forbidden(
        first(
            &plain
                .jmap_method_call("Blob/copy", blob_copy(key, plain, &key_blob))
                .await,
        ),
        "Blob/copy out of the key account",
    );

    // Ruling R31: the key account's own upload and section, into the plain
    // share.
    let upload = key
        .jmap_client()
        .await
        .upload(
            None,
            message_with(CANARY, "gates upload").into_bytes(),
            None,
        )
        .await
        .unwrap()
        .take_blob_id();
    let section = emails(key).await[0]["attachments"][0]["blobId"]
        .as_str()
        .unwrap()
        .to_string();
    let import = |to: &Account| {
        json!({
            "accountId": to.id_string(),
            "emails": { "i": { "blobId": &upload, "mailboxIds": { inbox(): true } } }
        })
    };
    let attach = |to: &Account| {
        json!({
            "accountId": to.id_string(),
            "create": { "e": created_email(INBOX_ID, "attached", json!([{
                "blobId": &section,
                "type": "application/octet-stream",
                "name": "gates.bin"
            }])) }
        })
    };
    assert_refused(
        first(&key.jmap_method_call("Email/import", import(plain)).await),
        "Email/import of the key account's upload into the plain share",
    );
    let reply = key.jmap_method_call("Email/set", attach(plain)).await;
    assert_eq!(
        reply.pointer("/methodResponses/0/1/notCreated/e"),
        Some(&json!({ "type": "forbidden", "description": ZA_MAIL_KEYLESS })),
        "an attachment of the key account's section into the plain share: {:?}",
        reply.0
    );
    assert_eq!(
        message_ids(test, plain_id).await,
        plain_before,
        "a refused copy, import or create stored a message in the plain account"
    );
    assert_eq!(
        message_ids(test, key_id).await,
        key_before,
        "a refused move changed the key account"
    );
    let events = traced.finish().await;
    let copies = events
        .iter()
        .filter(|e| e.details == COPY_REFUSED)
        .map(|e| e.account_id)
        .collect::<Vec<_>>();
    // IMAP COPY and MOVE and JMAP Email/copy name the source account,
    // Blob/copy the key account, the R31 refusals the source's owner: the
    // key account every time.
    assert_eq!(
        copies,
        vec![Some(u64::from(key_id)); 7],
        "the copy refusals' traces: {events:?}"
    );
    for event in &events {
        assert!(!event.text.contains(CANARY), "{event:?}");
    }

    // The controls: within the key account the import and the attachment
    // work, and between plain accounts the copy does.
    for (what, method, args, pointer) in [
        ("Email/import", "Email/import", import(key), "/created/i/id"),
        ("the attachment", "Email/set", attach(key), "/created/e/id"),
    ] {
        let reply = key.jmap_method_call(method, args).await;
        accepted(
            &reply,
            pointer,
            &format!("control: {what} into the key account"),
        );
    }
    let mut other_owner = ImapConnection::connect(b"_p ").await;
    other_owner.authenticate(OTHER_PLAIN, STRONG).await;
    other_owner
        .send_ok(&format!("SETACL INBOX {PLAIN} lrswi"))
        .await;
    let other_before = message_ids(test, other_plain_id).await;
    let reply = plain
        .jmap_method_call("Email/copy", copy(plain, other_plain, &plain_email))
        .await;
    accepted(
        &reply,
        "/created/c/id",
        "control: Email/copy between plain accounts",
    );
    assert_eq!(
        message_ids(test, other_plain_id).await.len(),
        other_before.len() + 1,
        "control: the copy"
    );
    other_owner
        .send_ok(&format!("DELETEACL INBOX {PLAIN}"))
        .await;
    other_owner.send_ok("LOGOUT").await;
    owner.send_ok(&format!("DELETEACL INBOX {KEY}")).await;
    owner.send_ok("LOGOUT").await;
}

/// Mail spec 6.3, 9: a key account's mailboxes cannot be shared, over IMAP
/// (SETACL and DELETEACL, `NO [NOPERM]`) or JMAP (`shareWith`, `forbidden`),
/// with a `zero-access:` trace; reading its own rights still works. A plain
/// account shares with a key account, which reads the share's plaintext.
async fn sharing_is_refused_on_key_mailboxes(key: &Account, plain: &Account) {
    println!("sharing_is_refused_on_key_mailboxes");
    let key_id = key.id().document_id();
    let traced = Traced::start(&[EventType::Security(SecurityEvent::Unauthorized)]);
    let mut imap = ImapConnection::connect(b"_s ").await;
    imap.authenticate(KEY, STRONG).await;
    for command in [
        format!("SETACL INBOX {PLAIN} lr"),
        format!("SETACL INBOX {OTHER_KEY} lr"),
        format!("DELETEACL INBOX {PLAIN}"),
    ] {
        imap.send(&command).await;
        imap.assert_read(Type::Tagged, ResponseType::No)
            .await
            .assert_response_code("NOPERM")
            .assert_contains(ZA_MAIL_KEYLESS);
    }
    imap.send("GETACL INBOX").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("* ACL ");
    imap.send("MYRIGHTS INBOX").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("* MYRIGHTS ");
    imap.send_ok("LOGOUT").await;

    let share_with = json!({ plain.id_string(): { "mayReadItems": true } });
    let mailboxes = key
        .jmap_method_call(
            "Mailbox/get",
            json!({ "accountId": key.id_string(), "ids": null }),
        )
        .await
        .list()
        .len();
    let reply = key
        .jmap_method_call(
            "Mailbox/set",
            json!({
                "accountId": key.id_string(),
                "update": { inbox(): { "shareWith": share_with } },
                "create": { "m": { "name": "gates shared", "shareWith": share_with } }
            }),
        )
        .await;
    let forbidden = json!({ "type": "forbidden", "description": ZA_MAIL_KEYLESS });
    assert_eq!(
        reply.pointer(&format!("/methodResponses/0/1/notUpdated/{}", inbox())),
        Some(&forbidden),
        "Mailbox/set update with shareWith: {:?}",
        reply.0
    );
    assert_eq!(
        reply.pointer("/methodResponses/0/1/notCreated/m"),
        Some(&forbidden),
        "Mailbox/set create with shareWith: {:?}",
        reply.0
    );
    assert_eq!(
        key.jmap_method_call(
            "Mailbox/get",
            json!({ "accountId": key.id_string(), "ids": null }),
        )
        .await
        .list()
        .len(),
        mailboxes,
        "the refused create stored a mailbox"
    );
    let refused = traced
        .finish()
        .await
        .into_iter()
        .filter(|e| e.details == SHARING_REFUSED)
        .map(|e| e.account_id)
        .collect::<Vec<_>>();
    assert_eq!(
        refused,
        vec![Some(u64::from(key_id)); 5],
        "one trace per refused share"
    );

    // A plain account shares with the key account, whose session lists the
    // share and reads its plaintext.
    let mut owner = ImapConnection::connect(b"_o ").await;
    owner.authenticate(PLAIN, STRONG).await;
    owner.send_ok(&format!("SETACL INBOX {KEY} lr")).await;
    let shared_name = format!("Shared Folders/{PLAIN}/INBOX");
    let mut imap = ImapConnection::connect(b"_s ").await;
    imap.authenticate(KEY, STRONG).await;
    imap.send(r#"LIST "" "*""#).await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains(&shared_name);
    imap.send_ok(&format!("SELECT \"{shared_name}\"")).await;
    imap.send("FETCH 1 (BODY.PEEK[TEXT])").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("gates body");
    imap.send_ok("LOGOUT").await;
    owner.send_ok(&format!("DELETEACL INBOX {KEY}")).await;
    owner.send_ok("LOGOUT").await;
}

/// Review focus 2, rulings R5 and R11: a plain account shares its inbox,
/// then is provisioned. Its grantee no longer lists, selects, appends to or
/// reads the mailbox (IMAP and JMAP, its session object included); a
/// grantee session that had selected the mailbox before reads a message
/// delivered after with `NO [NOPERM]` and the `zero-access:` trace, never
/// an unreadable-message error, and no plaintext. Returns the provisioned
/// account.
async fn rf_2_stale_grant(test: &mut TestServer, admin: &Account, plain: &Account) -> Account {
    println!("rf_2_stale_grant");
    let mut stale = admin
        .create_user_account(STALE, STRONG, "Stale grant", &[], user_permissions())
        .await;
    let stale_id = stale.id().document_id();
    let shared_name = format!("Shared Folders/{STALE}/INBOX");
    let mut owner = ImapConnection::connect(b"_o ").await;
    owner.authenticate(STALE, STRONG).await;
    owner.send_ok(&format!("SETACL INBOX {PLAIN} lrswi")).await;
    owner.send_ok("LOGOUT").await;

    // The grantee's sessions, opened while the account is plain: an IMAP
    // session that selects the share, and its JMAP credential.
    let mut imap = ImapConnection::connect(b"_g ").await;
    imap.authenticate(PLAIN, STRONG).await;
    imap.send(r#"LIST "" "*""#).await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains(&shared_name);
    imap.send_ok(&format!("SELECT \"{shared_name}\"")).await;
    let mailbox_get = json!({ "accountId": stale.id_string(), "ids": null });
    assert_eq!(
        first(
            &plain
                .jmap_method_call("Mailbox/get", mailbox_get.clone())
                .await
        )[0],
        "Mailbox/get",
        "control: the share before provisioning"
    );
    assert!(
        plain.jmap_session_object().await.0["accounts"]
            .get(stale.id_string())
            .is_some(),
        "control: the session object lists the share's account"
    );

    // Provisioning: setup requires an account without credentials.
    set_marker(&test.server, stale.id(), false).await;
    let token = za_setup_token(admin, STALE).await;
    stale.recovery_key = Some(za_setup(STALE, &token, STRONG).await);
    test.insert_account(stale.clone());
    deliver(
        test,
        SENDER,
        STALE,
        &message_with(&format!("{CANARY} stale"), "stale body canary"),
    )
    .await;
    let stale_doc = message_ids(test, stale_id).await[0];

    let traced = Traced::start(&[
        EventType::Security(SecurityEvent::Unauthorized),
        EventType::Store(StoreEvent::DataCorruption),
    ]);
    // Ruling R11: the session selected before provisioning reaches the stub
    // and is refused without its owner's keys.
    imap.send("FETCH 1 (BODY[])").await;
    let lines = imap
        .assert_read(Type::Tagged, ResponseType::No)
        .await
        .assert_response_code("NOPERM")
        .assert_contains(ZA_MAIL_KEYLESS);
    assert!(
        !lines.iter().any(|line| line.contains("canary")),
        "{lines:?}"
    );
    // The share is gone from the grantee's listing (the filter in
    // `build_access_token`, the token rebuilt by ruling R5).
    imap.send(r#"LIST "" "*""#).await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_not_contains(STALE);
    imap.send(&format!("SELECT \"{shared_name}\"")).await;
    imap.assert_read(Type::Tagged, ResponseType::No).await;
    let message = message_with("stale append", "stale append body");
    imap.send(&format!(
        "APPEND \"{shared_name}\" {{{}+}}\r\n{message}",
        message.len()
    ))
    .await;
    imap.assert_read(Type::Tagged, ResponseType::No).await;
    imap.send_ok("LOGOUT").await;

    assert_forbidden(
        first(&plain.jmap_method_call("Mailbox/get", mailbox_get).await),
        "Mailbox/get of the provisioned account",
    );
    assert!(
        plain.jmap_session_object().await.0["accounts"]
            .get(stale.id_string())
            .is_none(),
        "the session object lists the provisioned account"
    );
    assert_forbidden(
        first(
            &plain
                .jmap_method_call(
                    "Email/get",
                    json!({
                        "accountId": stale.id_string(),
                        "ids": [Id::from_parts(stale_doc, stale_doc).to_string()]
                    }),
                )
                .await,
        ),
        "Email/get of the provisioned account's message",
    );
    assert_eq!(
        message_ids(test, stale_id).await,
        vec![stale_doc],
        "the refused APPEND stored a message"
    );

    let events = traced.finish().await;
    assert!(
        events
            .iter()
            .any(|e| e.details == KEYLESS_REFUSED && e.account_id == Some(u64::from(stale_id))),
        "the stub read's refusal: {events:?}"
    );
    assert!(
        events
            .iter()
            .all(|e| e.typ != EventType::Store(StoreEvent::DataCorruption)),
        "an unreadable-message error: {events:?}"
    );
    for event in &events {
        assert!(!event.text.contains("canary"), "{event:?}");
    }
    stale
}
