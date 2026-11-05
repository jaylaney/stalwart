/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The gateway harvest accepts only committed ordinary delivery. Upstream
//! at-rest encryption leaves key-account sealing intact, and outbound
//! encryption events preserve the protected content boundary.

use super::{
    Traced, TracedEvent, assert_sealed, deliver, delivery::assert_received_as, key_mail_account,
    message_ids, message_with, new_message, open_message, stored_blob, stored_hash,
};
use crate::{
    utils::{
        account::Account,
        imap::{ImapConnection, Type},
        jmap::JmapUtils,
        server::TestServer,
        smtp::SmtpConnection,
        smtp_sink::sink,
        webdav::DummyWebDavClient,
        za::{Conn, HookParked, wait_for_delivery, wait_for_temporary_failure, za_setup_token},
    },
    za::{
        STRONG, imip,
        raw_input::{SUBMISSION_PORT, strings},
        user_permissions,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::za::test_hooks;
use email::mailbox::INBOX_ID;
use groupware::calendar::seal::{WRAP_MK, WRAP_PK};
use hyper::StatusCode;
use imap_proto::ResponseType;
use mail_parser::{Message, MessageParser};
use registry::{
    schema::{
        prelude::{ObjectType, Property},
        structs::{
            Encryption, EncryptionAtRest, EncryptionSettings, ExternalPublicKey, KeySource,
            MtaDeliverySchedule, MtaDeliveryScheduleInterval, MtaDeliveryScheduleIntervals,
            MtaDeliveryScheduleIntervalsOrDefault,
        },
    },
    types::list::List,
};
use serde_json::json;
use std::time::{Duration, Instant};
use store::write::now;
use trc::{EncryptionEvent, EventType, Key, StoreEvent, Value};
use types::id::Id;
use vault::record::VaultRecord;

const KEY: &str = "keymail36@example.com";
const MIXED_KEY: &str = "keymail37@example.com";
const RACE: &str = "keymail38@example.com";
const PENDING: &str = "keymail39@example.com";
const PLAIN: &str = "plainmail41@example.com";
const MIXED_PLAIN: &str = "plainmail42@example.com";
const DISCARD: &str = "plainmail43@example.com";
const AT_REST_PLAIN: &str = "plainmail44@example.com";
const KEY_SENDER: &str = "dmarc-ac-key@remote.test";
const PLAIN_SENDER: &str = "dmarc-ac-plain@remote.test";
const MIXED_SENDER: &str = "dmarc-ac-mixed@remote.test";
const RACE_SENDER: &str = "dmarc-ac-race@remote.test";
const PENDING_SENDER: &str = "dmarc-ac-pending@remote.test";
const DISCARD_SENDER: &str = "dmarc-ac-sieve@remote.test";
const ATTENDEE: &str = "imip-att@remote.test";
const PUBLIC_KEY: &str = include_str!("../../../resources/za/gateway-public.asc");
const FINGERPRINT: &str = "d2821f624f90042729f7d3a8e026dfc8adf65836";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access gateway tests...");
    let admin = test.account("admin@example.com").clone();
    let original = admin.registry_get::<Encryption>(Id::singleton()).await;
    let properties = [Property::GatewayEnable, Property::AtRestEnable];
    admin
        .registry_update_setting(
            Encryption {
                gateway_enable: true,
                at_rest_enable: true,
                ..original.clone()
            },
            &properties,
        )
        .await;
    reload(test, &admin).await;
    assert!(test.server.core.email.gateway.is_some());
    assert!(test.server.core.email.encrypt);

    let key = key_mail_account(test, KEY, STRONG).await;
    let mixed_key = key_mail_account(test, MIXED_KEY, STRONG).await;
    let plain = plain_account(&admin, PLAIN).await;
    let mixed_plain = plain_account(&admin, MIXED_PLAIN).await;
    let discard = plain_account(&admin, DISCARD).await;
    let at_rest_plain = plain_account(&admin, AT_REST_PLAIN).await;

    harvest_skips_key_accounts(test, &admin, &key).await;
    harvest_control_plain_account(test, &admin).await;
    harvest_mixed_recipients(test, &admin, &mixed_key).await;
    harvest_loses_to_provisioning(test, &admin).await;
    harvest_skips_pending_account(test, &admin).await;
    harvest_skips_discarding_sieve(test, &admin, &discard).await;
    at_rest_encryption_is_inert_for_key_accounts(test, &mixed_key, &at_rest_plain).await;
    gateway_events_carry_no_content(test, &admin, &key).await;

    admin.registry_update_setting(original, &properties).await;
    reload(test, &admin).await;
    for account in [&key, &mixed_key] {
        test.destroy_all_mailboxes(account).await;
    }
    for account in [plain, mixed_plain, discard, at_rest_plain] {
        admin.destroy_account(account).await;
    }
    wait_for_delivery(test).await;
}

async fn plain_account(admin: &Account, address: &'static str) -> Account {
    admin
        .create_user_account(address, STRONG, "Gateway test", &[], user_permissions())
        .await
}

async fn reload(test: &mut TestServer, admin: &Account) {
    admin.reload_settings().await;
    test.reload_core();
}

fn autocrypt(sender: &str) -> String {
    // The armor contains one public certificate with a self-certified user ID
    // for each sender. Autocrypt carries its binary packets as folded Base64.
    let keydata = PUBLIC_KEY
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with(['-', '=']))
        .collect::<Vec<_>>()
        .join("\r\n ");
    message_with("Gateway harvest", "Gateway test body").replace(
        "From: Sender <sender@example.org>\r\n",
        &format!("From: <{sender}>\r\nAutocrypt: addr={sender}; keydata=\r\n {keydata}\r\n"),
    )
}

fn harvest_permits(test: &TestServer) -> usize {
    test.server.inner.data.harvest_tasks.available_permits()
}

/// Call after the delivery attempt has finished. The worker acquires its
/// permit before spawning and keeps it through the registry write and trace.
async fn wait_for_harvest(test: &TestServer, baseline: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while harvest_permits(test) != baseline {
        assert!(Instant::now() < deadline, "key harvest did not finish");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn harvested_rows(admin: &Account, sender: &str) -> Vec<(Id, ExternalPublicKey)> {
    admin
        .registry_get_all::<ExternalPublicKey>()
        .await
        .into_iter()
        .filter(|(_, key)| key.email_address == sender)
        .collect()
}

async fn assert_harvested_and_remove(admin: &Account, sender: &str) {
    let rows = harvested_rows(admin, sender).await;
    assert!(
        rows.iter()
            .any(|(_, key)| key.source == KeySource::Autocrypt),
        "no Autocrypt row for {sender}"
    );
    let ids = rows.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
    admin
        .registry_destroy(ObjectType::ExternalPublicKey, ids.iter().copied())
        .await
        .assert_destroyed(&ids);
}

fn trace_content(event: &TracedEvent) -> String {
    let mut values = Vec::new();
    for (_, value) in &event.keys {
        strings(value, &mut values);
    }
    values.join(" ").to_ascii_lowercase()
}

fn assert_no_harvest_trace(events: &[TracedEvent]) {
    fn contains_bytes(value: &Value, fingerprint: &[u8]) -> bool {
        match value {
            Value::Bytes(bytes) => bytes
                .windows(fingerprint.len())
                .any(|part| part == fingerprint),
            Value::Array(items) => items.iter().any(|value| contains_bytes(value, fingerprint)),
            Value::Event(event) => event
                .keys()
                .iter()
                .any(|(_, value)| contains_bytes(value, fingerprint)),
            _ => false,
        }
    }

    let fingerprint = FINGERPRINT
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    for event in events {
        assert!(
            !matches!(event.typ, EventType::Encryption(_)),
            "unexpected encryption trace: {event:?}"
        );
        assert!(
            !trace_content(event).contains(FINGERPRINT)
                && !event
                    .keys
                    .iter()
                    .any(|(_, value)| contains_bytes(value, &fingerprint)),
            "fingerprint reached a trace: {event:?}"
        );
    }
}

async fn harvest_skips_key_accounts(test: &TestServer, admin: &Account, key: &Account) {
    println!("harvest_skips_key_accounts");
    let baseline = harvest_permits(test);
    let account_id = key.id().document_id();
    let before = message_ids(test, account_id).await;
    let traced = Traced::start(EventType::variants());
    deliver(test, KEY_SENDER, KEY, &autocrypt(KEY_SENDER)).await;
    wait_for_harvest(test, baseline).await;
    assert_no_harvest_trace(&traced.finish().await);
    assert!(harvested_rows(admin, KEY_SENDER).await.is_empty());
    let document_id = new_message(test, account_id, &before).await;
    assert_sealed(test, account_id, document_id, WRAP_PK).await;
}

async fn harvest_control_plain_account(test: &TestServer, admin: &Account) {
    println!("harvest_control_plain_account");
    let baseline = harvest_permits(test);
    let traced = Traced::start(&[EventType::Encryption(EncryptionEvent::KeyHarvested)]);
    deliver(test, PLAIN_SENDER, PLAIN, &autocrypt(PLAIN_SENDER)).await;
    wait_for_harvest(test, baseline).await;
    assert!(
        traced.finish().await.iter().any(|event| {
            event.typ == EventType::Encryption(EncryptionEvent::KeyHarvested)
                && event
                    .keys
                    .iter()
                    .any(|(key, value)| *key == Key::From && value.as_str() == Some(PLAIN_SENDER))
        }),
        "missing the ordinary control's KeyHarvested event"
    );
    assert_harvested_and_remove(admin, PLAIN_SENDER).await;
}

async fn harvest_mixed_recipients(test: &TestServer, admin: &Account, key: &Account) {
    println!("harvest_mixed_recipients");
    let baseline = harvest_permits(test);
    let account_id = key.id().document_id();
    let before = message_ids(test, account_id).await;
    SmtpConnection::connect()
        .await
        .ingest(
            MIXED_SENDER,
            &[MIXED_KEY, MIXED_PLAIN],
            &autocrypt(MIXED_SENDER),
        )
        .await;
    wait_for_delivery(test).await;
    wait_for_harvest(test, baseline).await;
    assert_harvested_and_remove(admin, MIXED_SENDER).await;
    let document_id = new_message(test, account_id, &before).await;
    assert_sealed(test, account_id, document_id, WRAP_PK).await;
}

async fn harvest_loses_to_provisioning(test: &mut TestServer, admin: &Account) {
    println!("harvest_loses_to_provisioning");
    let baseline = harvest_permits(test);
    let account = admin
        .create_passwordless_user_account(RACE, STRONG, "Gateway race", &[], user_permissions())
        .await;
    let account_id = account.id().document_id();
    let (schedule_id, schedule) = admin
        .registry_get_all::<MtaDeliverySchedule>()
        .await
        .into_iter()
        .find(|(_, schedule)| schedule.name == "local")
        .expect("the local delivery schedule");
    set_local_retry(
        test,
        admin,
        schedule_id,
        MtaDeliveryScheduleIntervalsOrDefault::Custom(MtaDeliveryScheduleIntervals {
            intervals: List::from_iter([MtaDeliveryScheduleInterval {
                duration: 1_000u64.into(),
            }]),
        }),
    )
    .await;

    let traced = Traced::start(EventType::variants());
    let parked = HookParked::start(account_id, test_hooks::set_ingest_pause, async {
        SmtpConnection::connect()
            .await
            .ingest(RACE_SENDER, &[RACE], &autocrypt(RACE_SENDER))
            .await;
    })
    .await;
    let mut record = VaultRecord::pending(vec![3; 32], now() as i64 + 3600);
    test.server
        .za_vault_commit(account_id, &mut record, None, None)
        .await
        .unwrap();
    parked.finish().await;
    wait_for_temporary_failure(test, RACE).await;
    wait_for_harvest(test, baseline).await;
    let events = traced.finish().await;
    assert_no_harvest_trace(&events);
    assert!(
        events.iter().any(|event| {
            event.typ == EventType::Store(StoreEvent::AssertValueFailed) && event.to == RACE
        }),
        "the stale delivery did not lose the provisioning fence"
    );
    assert!(harvested_rows(admin, RACE_SENDER).await.is_empty());
    assert!(message_ids(test, account_id).await.is_empty());

    let read = test
        .server
        .za_vault_record(account_id)
        .await
        .unwrap()
        .unwrap();
    test.server
        .za_vault_delete(account_id, &read)
        .await
        .unwrap();
    wait_for_delivery(test).await;
    wait_for_harvest(test, baseline).await;
    assert_eq!(message_ids(test, account_id).await.len(), 1);
    assert_harvested_and_remove(admin, RACE_SENDER).await;

    // The pending-account case removes its message before the next attempt.
    set_local_retry(test, admin, schedule_id, schedule.retry).await;
    admin.destroy_account(account).await;
}

async fn set_local_retry(
    test: &mut TestServer,
    admin: &Account,
    schedule_id: Id,
    retry: MtaDeliveryScheduleIntervalsOrDefault,
) {
    admin
        .registry_update_object(
            ObjectType::MtaDeliverySchedule,
            schedule_id,
            json!({ "retry": retry }),
        )
        .await;
    reload(test, admin).await;
}

async fn harvest_skips_pending_account(test: &TestServer, admin: &Account) {
    println!("harvest_skips_pending_account");
    let baseline = harvest_permits(test);
    let account = admin
        .create_passwordless_user_account(
            PENDING,
            STRONG,
            "Gateway pending",
            &[],
            user_permissions(),
        )
        .await;
    za_setup_token(admin, PENDING).await;

    let traced = Traced::start(EventType::variants());
    SmtpConnection::connect()
        .await
        .ingest(PENDING_SENDER, &[PENDING], &autocrypt(PENDING_SENDER))
        .await;
    wait_for_temporary_failure(test, PENDING).await;
    wait_for_harvest(test, baseline).await;
    let events = traced.finish().await;
    assert_no_harvest_trace(&events);
    assert!(
        events.iter().any(|event| {
            event.to == PENDING
                && event.reason == "zero-access: delivery without a served public key"
        }),
        "the pending recipient was not refused for lack of a served key"
    );
    assert!(harvested_rows(admin, PENDING_SENDER).await.is_empty());
    assert!(
        message_ids(test, account.id().document_id())
            .await
            .is_empty()
    );

    let queued = test
        .read_queued_messages()
        .await
        .into_iter()
        .find(|message| {
            message
                .message
                .recipients
                .iter()
                .any(|rcpt| &*rcpt.address == PENDING)
        })
        .expect("the pending account's deferred message");
    let recipient = queued
        .message
        .recipients
        .iter()
        .find(|rcpt| &*rcpt.address == PENDING)
        .unwrap();
    assert!(
        recipient.retry.due > now() + 10,
        "the fast retry interval was left active"
    );
    let queue_id = queued.queue_id;
    assert!(queued.remove(&test.server, None).await);
    assert!(
        test.read_queued_messages()
            .await
            .iter()
            .all(|message| message.queue_id != queue_id),
        "the deferred message is still queued"
    );
    assert_eq!(sink().count_to(PENDING_SENDER), 0, "a DSN reached the sink");
    sink().clear();
    admin.destroy_account(account).await;
}

async fn harvest_skips_discarding_sieve(test: &TestServer, admin: &Account, plain: &Account) {
    println!("harvest_skips_discarding_sieve");
    let baseline = harvest_permits(test);
    let client = plain.jmap_client().await;
    let script = client
        .sieve_script_create("gateway-discard", "discard;", true)
        .await
        .unwrap()
        .take_id();
    deliver(test, DISCARD_SENDER, DISCARD, &autocrypt(DISCARD_SENDER)).await;
    wait_for_harvest(test, baseline).await;
    assert!(harvested_rows(admin, DISCARD_SENDER).await.is_empty());
    assert!(message_ids(test, plain.id().document_id()).await.is_empty());
    client.sieve_script_destroy(&script).await.unwrap();
}

async fn at_rest_encryption_is_inert_for_key_accounts(
    test: &TestServer,
    key: &Account,
    plain: &Account,
) {
    println!("at_rest_encryption_is_inert_for_key_accounts");
    let baseline = harvest_permits(test);
    let mut public_keys = Vec::new();
    for account in [key, plain] {
        let id = account
            .registry_create_many(
                ObjectType::PublicKey,
                [json!({ "description": "Gateway fixture", "key": PUBLIC_KEY })],
            )
            .await
            .created(0)
            .object_id();
        set_at_rest(
            account,
            EncryptionAtRest::Enabled(EncryptionSettings {
                public_key: id,
                encrypt_on_append: true,
                ..Default::default()
            }),
        )
        .await;
        assert!(
            test.server
                .account(account.id().document_id())
                .await
                .unwrap()
                .encryption()
                .unwrap()
                .encrypt_on_append
        );
        public_keys.push(id);
    }

    let key_id = key.id().document_id();
    let plain_id = plain.id().document_id();
    let traced = Traced::start(&[
        EventType::Encryption(EncryptionEvent::AtRestEncrypted),
        EventType::Encryption(EncryptionEvent::EncryptionSkipped),
    ]);

    let raw = message_with("At-rest delivery", "Original SMTP body");
    let before = message_ids(test, key_id).await;
    deliver(test, "sender@example.com", key.name(), &raw).await;
    let document_id = new_message(test, key_id, &before).await;
    assert_sealed(test, key_id, document_id, WRAP_PK).await;
    let (_, opened) = open_message(test, key_id, STRONG, document_id).await;
    assert_received_as(&opened, &raw);
    assert_clear_message(&opened);

    let raw = message_with("At-rest append", "Original IMAP body");
    let before = message_ids(test, key_id).await;
    let mut imap = ImapConnection::connect(b"_gw ").await;
    imap.authenticate(key.name(), STRONG).await;
    imap.send(&format!("APPEND INBOX {{{}+}}\r\n{raw}", raw.len()))
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send_ok("LOGOUT").await;
    assert_opened_as(test, key_id, &before, raw.as_bytes()).await;

    let raw = message_with("At-rest import", "Original JMAP body");
    let before = message_ids(test, key_id).await;
    let upload = key
        .jmap_client()
        .await
        .upload(None, raw.clone().into_bytes(), None)
        .await
        .unwrap()
        .take_blob_id();
    let mailbox = Id::from(INBOX_ID).to_string();
    let reply = key
        .jmap_method_call(
            "Email/import",
            json!({
                "accountId": key.id_string(),
                "emails": { "i": { "blobId": upload, "mailboxIds": { (mailbox): true } } }
            }),
        )
        .await;
    assert!(
        reply.pointer("/methodResponses/0/1/created/i/id").is_some(),
        "{:?}",
        reply.0
    );
    assert_opened_as(test, key_id, &before, raw.as_bytes()).await;

    let before = message_ids(test, plain_id).await;
    deliver(test, "sender@example.com", plain.name(), &raw).await;
    let document_id = new_message(test, plain_id, &before).await;
    let stored = stored_blob(test, &stored_hash(test, plain_id, document_id).await).await;
    assert_encrypted_message(&MessageParser::new().parse(&stored).unwrap());
    wait_for_harvest(test, baseline).await;
    let events = traced.finish().await;
    assert!(
        events
            .iter()
            .all(|event| event.account_id != Some(u64::from(key_id)))
    );
    assert!(
        events.iter().any(|event| {
            event.typ == EventType::Encryption(EncryptionEvent::AtRestEncrypted)
                && event.account_id == Some(u64::from(plain_id))
        }),
        "no AtRestEncrypted event for the ordinary control"
    );

    for (account, id) in [key, plain].into_iter().zip(public_keys) {
        set_at_rest(account, EncryptionAtRest::Disabled).await;
        account
            .registry_destroy(ObjectType::PublicKey, [id])
            .await
            .assert_destroyed(&[id]);
    }
}

async fn set_at_rest(account: &Account, setting: EncryptionAtRest) {
    account
        .registry_update_object(
            ObjectType::AccountSettings,
            Id::singleton(),
            json!({ Property::EncryptionAtRest: setting }),
        )
        .await;
}

async fn assert_opened_as(test: &TestServer, account_id: u32, before: &[u32], original: &[u8]) {
    let document_id = new_message(test, account_id, before).await;
    assert_sealed(test, account_id, document_id, WRAP_MK).await;
    let (_, raw) = open_message(test, account_id, STRONG, document_id).await;
    assert_eq!(raw, original);
    assert_clear_message(&raw);
}

fn assert_clear_message(raw: &[u8]) {
    let raw = String::from_utf8_lossy(raw);
    assert!(!raw.to_ascii_lowercase().contains("multipart/encrypted"));
    assert!(!raw.contains("-----BEGIN PGP MESSAGE-----"));
}

fn assert_encrypted_message(message: &Message<'_>) {
    assert!(
        message
            .root_part()
            .is_content_type("multipart", "encrypted"),
        "message was not OpenPGP encrypted"
    );
}

async fn gateway_events_carry_no_content(test: &TestServer, admin: &Account, key: &Account) {
    println!("gateway_events_carry_no_content");
    let baseline = harvest_permits(test);
    let id = admin
        .registry_create_many(
            ObjectType::ExternalPublicKey,
            [json!({ "emailAddress": ATTENDEE, "key": PUBLIC_KEY })],
        )
        .await
        .created(0)
        .object_id();
    let client = DummyWebDavClient::new(key.id().document_id(), KEY, STRONG, KEY);
    let path = "/dav/cal/keymail36%40example.com/default/gateway-invitation.ics";
    let calendar = imip::event(
        "gateway-invitation",
        KEY,
        &[ATTENDEE],
        0,
        "DESCRIPTION:gateway-calendar-body-canary",
    )
    .replace(
        "SUMMARY:imip gateway-invitation",
        "SUMMARY:gateway-calendar-subject-canary",
    );

    sink().clear();
    let traced = Traced::start(EventType::variants());
    client
        .request_with_headers("PUT", path, [("content-type", "text/calendar")], calendar)
        .await
        .with_status(StatusCode::CREATED);
    wait_for_delivery(test).await;
    wait_for_harvest(test, baseline).await;
    assert_gateway_ciphertext();

    let raw = message_with("gateway-mail-subject-canary", "gateway-mail-body-canary").replace(
        "Sender <sender@example.org>",
        &format!("gateway-mail-from-canary <{KEY}>"),
    );
    let mut conn = Conn::open_smtp(SUBMISSION_PORT).await;
    conn.send("EHLO gateway.test\r\n").await;
    conn.until(&["250 "]).await;
    let auth = STANDARD.encode(format!("\0{}\0{}", key.name(), key.secret()));
    conn.exchange(&format!("AUTH PLAIN {auth}\r\n"), "235")
        .await;
    conn.exchange(&format!("MAIL FROM:<{KEY}>\r\n"), "250")
        .await;
    conn.exchange(&format!("RCPT TO:<{ATTENDEE}>\r\n"), "250")
        .await;
    conn.exchange("DATA\r\n", "354").await;
    conn.exchange(&format!("{raw}\r\n.\r\n"), "250").await;
    conn.exchange("QUIT\r\n", "221").await;
    wait_for_delivery(test).await;
    wait_for_harvest(test, baseline).await;
    assert_gateway_ciphertext();

    let events = traced.finish().await;
    assert!(
        events
            .iter()
            .filter(|event| {
                event.typ == EventType::Encryption(EncryptionEvent::GatewayEncrypted)
            })
            .count()
            >= 2,
        "expected encryption traces for the invitation and the submission"
    );
    for event in events {
        if matches!(event.typ, EventType::Encryption(_)) {
            assert!(
                event.keys.iter().all(|(key, _)| {
                    matches!(key, Key::SpanId | Key::Reason | Key::Fingerprint)
                }),
                "unexpected field in gateway event: {event:?}"
            );
        }
        let text = trace_content(&event);
        for canary in [
            "gateway-calendar-subject-canary",
            "gateway-calendar-body-canary",
            "gateway-mail-subject-canary",
            "gateway-mail-body-canary",
            "gateway-mail-from-canary",
        ] {
            assert!(
                !text.contains(canary),
                "protected content traced: {event:?}"
            );
        }
    }

    // Remove the invitation (and its CANCEL) while the sink's key still exists.
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    sink().clear();
    admin
        .registry_destroy(ObjectType::ExternalPublicKey, [id])
        .await
        .assert_destroyed(&[id]);
}

fn assert_gateway_ciphertext() {
    let messages = sink().take_to(ATTENDEE);
    assert_eq!(
        messages.len(),
        1,
        "expected one encrypted message at the sink"
    );
    assert_encrypted_message(&messages[0].parsed());
}
