/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 10, "Traces" (review F2): an event emitted while a key account's
//! message is processed, or inside a key account's session, carries no
//! header name or value, body text, search term or other client-supplied
//! string. A delivery to a key account, a submission by one, and its IMAP
//! and POP3 sessions leave none of the canaries in any captured event; the
//! milter events, the raw I/O and the IMAP command events record
//! `[redacted]`, while the client still receives everything. The same flows
//! of an ordinary account are the controls, traced as upstream traces them.
//! The vacation compile error names a position only, for every account
//! (issue #21), and a transaction with a key-account recipient adds no
//! autolearn sample (6.1 step 4).

use super::{
    Traced, TracedEvent, assert_sealed, deliver, key_mail_account, message_ids, new_message,
    spam_samples,
};
use crate::{
    utils::{
        account::Account,
        imap::{ImapConnection, Type},
        milter::spawn_mock_milter_server,
        pop3::{Pop3Connection, ResponseType as Pop3Reply},
        server::TestServer,
        smtp::SmtpConnection,
        za::{Conn, wait_for_delivery},
    },
    za::{
        STRONG,
        raw_input::{SUBMISSION_PORT, strings},
        user_permissions,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use groupware::calendar::seal::WRAP_PK;
use imap_proto::ResponseType;
use registry::{
    schema::{
        enums::{MilterVersion, MtaStage},
        prelude::{ObjectType, Property},
        structs::{
            Expression, MemoryLookupKey, MtaMilter, MtaStageData, SpamClassifier, SpamTag,
            SpamTagScore,
        },
    },
    types::{float::Float, map::Map},
};
use serde_json::json;
use std::time::Duration;
use trc::{
    EvalEvent, EventType, ImapEvent, Key, MessageIngestEvent, MilterEvent, Pop3Event, SmtpEvent,
    SpamEvent, StoreEvent,
};
use types::id::Id;

const KEY: &str = "keymail11@example.com";
const PLAIN: &str = "plainmail15@example.com";
/// The spam trap (`spamtrap@*` in the `spam-traps` lookup, as `imap_tests`
/// configures it), an ordinary account.
const TRAP: &str = "spamtrap@example.com";
const SENDER: &str = "sender@example.org";
const MARKER: &str = "[redacted]";
/// What a trace of a key account must not carry: a header's name and value,
/// the subject, the body and the Message-ID of `canary_message`; an IMAP
/// keyword, search term and malformed attribute; a POP3 mechanism.
const CANARIES: &[&str] = &[
    "hdr-name-canary",
    "hdr-value-canary",
    "subject-canary",
    "body-canary",
    "msgid-canary",
    "kw-canary",
    "search-canary",
    "bad-canary",
    "mech-canary",
];
const VACATION_CANARY: &str = "vacation-canary";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mail trace tests...");
    let admin = test.account("admin@example.com").clone();
    let key = key_mail_account(test, KEY, STRONG).await;
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    let trap = admin
        .create_user_account(TRAP, STRONG, "Spam trap", &[], user_permissions())
        .await;
    let settings = configure(&admin).await;
    let milter = spawn_mock_milter_server();
    tokio::time::sleep(Duration::from_millis(100)).await;

    delivery_leaves_no_content_in_events(test).await;
    smtp_submission_by_a_key_account_is_redacted(test, &key, &plain).await;
    imap_session_is_redacted(&key, &plain).await;
    pop3_session_is_redacted(&key, &plain).await;
    vacation_compile_error_reason_is_fixed(&plain).await;
    autolearn_skips_key_transactions(test, &key).await;

    unconfigure(&admin, settings).await;
    let _ = milter.send(false);
    for account in [&key, &plain, &trap] {
        test.destroy_all_mailboxes(account).await;
    }
    admin
        .registry_destroy_all(ObjectType::SpamTrainingSample)
        .await;
    wait_for_delivery(test).await;
}

/// The module's settings: the mock milter at the DATA stage, as
/// `milter_session` configures it; the spam trap of `imap_tests`, scored as
/// spam, with the classifier learning from traps; the spam filter on for
/// every transaction.
struct Settings {
    milter: Id,
    trap_tag: Id,
    trap_key: Id,
}

async fn configure(admin: &Account) -> Settings {
    let milter = admin
        .registry_create_object(MtaMilter {
            enable: Expression {
                else_: "true".into(),
                ..Default::default()
            },
            hostname: "127.0.0.1".into(),
            port: 9332,
            use_tls: false,
            stages: Map::new(vec![MtaStage::Data]),
            protocol_version: MilterVersion::V6,
            ..Default::default()
        })
        .await;
    let trap_tag = admin
        .registry_create_object(SpamTag::Score(SpamTagScore {
            score: Float::new(100.0),
            tag: "SPAM_TRAP".into(),
        }))
        .await;
    let trap_key = admin
        .registry_create_object(MemoryLookupKey {
            is_glob_pattern: true,
            key: "spamtrap@*".into(),
            namespace: "spam-traps".into(),
        })
        .await;
    admin
        .registry_update_setting(
            SpamClassifier {
                learn_spam_from_traps: true,
                ..Default::default()
            },
            &[Property::LearnSpamFromTraps],
        )
        .await;
    admin
        .registry_update_setting(
            MtaStageData {
                enable_spam_filter: Expression {
                    else_: "true".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            &[Property::EnableSpamFilter],
        )
        .await;
    admin.reload_settings().await;
    admin.reload_lookup_stores().await;
    Settings {
        milter,
        trap_tag,
        trap_key,
    }
}

async fn unconfigure(admin: &Account, settings: Settings) {
    for (object, id) in [
        (ObjectType::MtaMilter, settings.milter),
        (ObjectType::SpamTag, settings.trap_tag),
        (ObjectType::MemoryLookupKey, settings.trap_key),
    ] {
        admin
            .registry_destroy(object, [id])
            .await
            .assert_destroyed(&[id]);
    }
    admin
        .registry_update_setting(SpamClassifier::default(), &[Property::LearnSpamFromTraps])
        .await;
    admin
        .registry_update_setting(MtaStageData::default(), &[Property::EnableSpamFilter])
        .await;
    admin.reload_settings().await;
    admin.reload_lookup_stores().await;
}

/// A message holding every content canary: a header of its own (name and
/// value), the subject, the body and the Message-ID, which is unique per
/// call (an ordinary account drops a duplicate).
fn canary_message() -> String {
    format!(
        "From: Sender <sender@example.org>\r\n\
         To: Recipient <recipient@example.com>\r\n\
         Subject: subject-canary\r\n\
         Message-ID: <{:016x}.msgid-canary@example.com>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         X-Hdr-Name-Canary: hdr-value-canary\r\n\
         \r\n\
         body-canary\r\n",
        store::rand::random::<u64>()
    )
}

/// The events every step captures.
fn traced_types() -> Vec<EventType> {
    vec![
        EventType::Imap(ImapEvent::RawInput),
        EventType::Imap(ImapEvent::RawOutput),
        EventType::Imap(ImapEvent::Error),
        EventType::Imap(ImapEvent::Fetch),
        EventType::Imap(ImapEvent::Store),
        EventType::Pop3(Pop3Event::RawInput),
        EventType::Pop3(Pop3Event::RawOutput),
        EventType::Pop3(Pop3Event::Error),
        EventType::Smtp(SmtpEvent::RawInput),
        EventType::Smtp(SmtpEvent::RawOutput),
        EventType::Milter(MilterEvent::Write),
        EventType::Milter(MilterEvent::Read),
        EventType::Spam(SpamEvent::Dnsbl),
        EventType::Spam(SpamEvent::DnsblError),
        EventType::Spam(SpamEvent::Classify),
        EventType::MessageIngest(MessageIngestEvent::Ham),
        EventType::MessageIngest(MessageIngestEvent::Spam),
        EventType::MessageIngest(MessageIngestEvent::JmapAppend),
        EventType::MessageIngest(MessageIngestEvent::ImapAppend),
        EventType::Eval(EvalEvent::Result),
        EventType::Store(StoreEvent::DataCorruption),
    ]
}

/// Every string of `value`, joined.
fn text_of(value: &trc::Value) -> String {
    let mut out = Vec::new();
    strings(value, &mut out);
    out.join(" ")
}

/// The value of `key` in `event`, as text.
fn value(event: &TracedEvent, key: Key) -> Option<String> {
    event
        .keys
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, value)| text_of(value))
}

/// The `key` values of the events of type `typ`, in emission order.
fn values(events: &[TracedEvent], typ: EventType, key: Key) -> Vec<String> {
    events
        .iter()
        .filter(|event| event.typ == typ)
        .map(|event| value(event, key).unwrap_or_default())
        .collect()
}

/// Every string an event carries, lower-cased.
fn content(event: &TracedEvent) -> String {
    event
        .keys
        .iter()
        .map(|(_, value)| text_of(value))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn assert_no_canary<'x>(events: impl IntoIterator<Item = &'x TracedEvent>, what: &str) {
    for event in events {
        let content = content(event);
        for canary in CANARIES {
            assert!(
                !content.contains(canary),
                "{what}: {:?} carries {canary}: {}",
                event.typ,
                event.text
            );
        }
    }
}

fn assert_all_redacted(values: &[String], what: &str) {
    assert!(!values.is_empty(), "{what}: nothing traced");
    assert!(
        values.iter().all(|value| value == MARKER),
        "{what}: {values:?}"
    );
}

fn assert_any_contains(values: &[String], needle: &str, what: &str) {
    assert!(
        values.iter().any(|value| value.contains(needle)),
        "{what}: no {needle:?} in {values:?}"
    );
}

/// Spec 10 (review F2): a delivery over LMTP to a key account leaves no
/// content in any event. The milter is sent the plaintext (transit) but its
/// events record the marker; the DATA reads are masked while the envelope
/// stays readable; the ingest event names no Message-ID. The same delivery
/// to the ordinary account is the control.
async fn delivery_leaves_no_content_in_events(test: &TestServer) {
    println!("delivery_leaves_no_content_in_events");
    for to in [KEY, PLAIN] {
        let traced = Traced::start(&traced_types());
        deliver(test, SENDER, to, &canary_message()).await;
        let events = traced.finish().await;
        let writes = values(
            &events,
            EventType::Milter(MilterEvent::Write),
            Key::Contents,
        );
        let message_ids = values(
            &events,
            EventType::MessageIngest(MessageIngestEvent::Ham),
            Key::MessageId,
        );
        let reads = values(&events, EventType::Smtp(SmtpEvent::RawInput), Key::Contents);
        assert_eq!(message_ids.len(), 1, "{to}: one ingest event");
        assert_any_contains(&reads, "RCPT TO", to);
        if to == KEY {
            assert_no_canary(&events, "a delivery to a key account");
            assert_all_redacted(&writes, "the milter writes of a key delivery");
            assert_eq!(message_ids[0], MARKER, "the key ingest's Message-ID");
            assert_any_contains(&reads, MARKER, "the DATA reads of a key delivery");
        } else {
            assert_any_contains(
                &writes,
                "HEADER (X-Hdr-Name-Canary: \"hdr-value-canary\")",
                "control: the milter writes",
            );
            assert_any_contains(&message_ids, "msgid-canary@example.com", "control");
            assert_any_contains(&reads, "body-canary", "control: the DATA reads");
        }
    }
}

/// The SASL PLAIN message of `account`, base64-encoded.
fn sasl_plain(account: &Account) -> String {
    STANDARD.encode(format!("\0{}\0{}", account.name(), account.secret()))
}

/// A submission by `account` to the ordinary account over the implicit-TLS
/// listener: EHLO, AUTH PLAIN, MAIL FROM, RCPT TO, DATA, the message.
async fn submit(account: &Account) {
    let mut conn = Conn::open_smtp(SUBMISSION_PORT).await;
    conn.send("EHLO submit.test\r\n").await;
    conn.until(&["250 "]).await;
    conn.exchange(&format!("AUTH PLAIN {}\r\n", sasl_plain(account)), "235")
        .await;
    conn.exchange(&format!("MAIL FROM:<{}>\r\n", account.name()), "250")
        .await;
    conn.exchange(&format!("RCPT TO:<{PLAIN}>\r\n"), "250")
        .await;
    conn.exchange("DATA\r\n", "354").await;
    conn.exchange(&format!("{}\r\n.\r\n", canary_message()), "250")
        .await;
    conn.exchange("QUIT\r\n", "221").await;
}

/// Spec 10: every read of a submission session authenticated as a key
/// account is masked (the EHLO before it stays readable), and its milter
/// events record the marker; an ordinary account's submission is the
/// control. A BDAT command and its chunk in one write, for a key recipient
/// over LMTP, are one masked read, while the envelope before it is traced.
async fn smtp_submission_by_a_key_account_is_redacted(
    test: &TestServer,
    key: &Account,
    plain: &Account,
) {
    println!("smtp_submission_by_a_key_account_is_redacted");
    let plain_id = u64::from(plain.id().document_id());
    for account in [key, plain] {
        let traced = Traced::start(&traced_types());
        submit(account).await;
        let events = traced.finish().await;
        let reads = values(&events, EventType::Smtp(SmtpEvent::RawInput), Key::Contents);
        assert!(
            reads
                .first()
                .is_some_and(|read| read.contains("EHLO submit.test")),
            "{}: the EHLO read: {reads:?}",
            account.name()
        );
        if account.name() == KEY {
            // The recipient's own ingest is an ordinary account's, traced as
            // upstream traces it.
            assert_no_canary(
                events.iter().filter(|event| {
                    !matches!(event.typ, EventType::MessageIngest(_))
                        || event.account_id != Some(plain_id)
                }),
                "a submission by a key account",
            );
            assert_all_redacted(&reads[1..], "the reads after a key account's AUTH");
            assert_all_redacted(
                &values(
                    &events,
                    EventType::Milter(MilterEvent::Write),
                    Key::Contents,
                ),
                "the milter writes of a key account's submission",
            );
        } else {
            assert_any_contains(&reads, "MAIL FROM", "control");
            assert_any_contains(&reads, "body-canary", "control: the DATA reads");
        }
    }
    wait_for_delivery(test).await;

    let traced = Traced::start(&traced_types());
    let mut lmtp = SmtpConnection::connect().await;
    lmtp.mail_from(SENDER, 2).await;
    lmtp.rcpt_to(KEY, 2).await;
    lmtp.bdat_last(&canary_message(), 1, 2).await;
    lmtp.quit().await;
    wait_for_delivery(test).await;
    let events = traced.finish().await;
    assert_no_canary(&events, "a BDAT delivery to a key account");
    let reads = values(&events, EventType::Smtp(SmtpEvent::RawInput), Key::Contents);
    for command in ["LHLO", "MAIL FROM", "RCPT TO"] {
        assert_any_contains(&reads, command, "the envelope of a BDAT delivery");
    }
    assert_any_contains(&reads, MARKER, "the BDAT read");
    assert!(
        !reads.iter().any(|read| read.contains("BDAT")),
        "a BDAT read is traced: {reads:?}"
    );
}

/// One IMAP session of `account`: SELECT, a FETCH of the canary header, a
/// STORE of a keyword, an APPEND, a SEARCH on content (refused for a key
/// account), a malformed FETCH. Returns the FETCH's answer.
async fn imap_session(account: &Account) -> String {
    let is_key = account.name() == KEY;
    let mut imap = ImapConnection::connect(b"_t ").await;
    imap.authenticate(account.name(), account.secret()).await;
    imap.send_ok("SELECT INBOX").await;
    imap.send("FETCH 1 (BODY[HEADER.FIELDS (X-Hdr-Name-Canary)])")
        .await;
    let fetched = imap
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .join("\n");
    imap.send_ok("STORE 1 +FLAGS ($kw-canary)").await;
    imap.append("INBOX", &canary_message()).await;
    imap.send("SEARCH FROM search-canary").await;
    imap.assert_read(
        Type::Tagged,
        if is_key {
            ResponseType::No
        } else {
            ResponseType::Ok
        },
    )
    .await;
    // Its BAD answer quotes the attribute.
    imap.send("FETCH 1 (bad-canary)").await;
    imap.assert_read(Type::Tagged, ResponseType::Bad).await;
    imap.send("LOGOUT").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    fetched
}

/// Spec 10: in a key account's IMAP session every raw read and write is
/// masked, the FETCH and STORE events name no header or keyword, and a BAD
/// error is traced without the input it quotes; the client still receives
/// the full answers. An ordinary account's session is the control.
async fn imap_session_is_redacted(key: &Account, plain: &Account) {
    println!("imap_session_is_redacted");
    for account in [key, plain] {
        let traced = Traced::start(&traced_types());
        let fetched = imap_session(account).await;
        let events = traced.finish().await;
        assert!(
            fetched.contains("hdr-value-canary"),
            "{}: the FETCH answer: {fetched}",
            account.name()
        );
        let fetch = values(&events, EventType::Imap(ImapEvent::Fetch), Key::Details);
        let store = values(&events, EventType::Imap(ImapEvent::Store), Key::Details);
        let output = values(
            &events,
            EventType::Imap(ImapEvent::RawOutput),
            Key::Contents,
        );
        let bad = events
            .iter()
            .filter(|event| {
                event.typ == EventType::Imap(ImapEvent::Error)
                    && value(event, Key::Type).as_deref() == Some("BAD")
            })
            .collect::<Vec<_>>();
        assert!(!bad.is_empty(), "{}: no BAD error traced", account.name());
        if account.name() == KEY {
            assert_no_canary(&events, "a key account's IMAP session");
            let mut raw = values(&events, EventType::Imap(ImapEvent::RawInput), Key::Contents);
            raw.extend(output);
            assert_all_redacted(&raw, "the raw I/O of a key account's session");
            assert_all_redacted(&fetch, "the FETCH details");
            assert_all_redacted(&store, "the STORE details");
            for event in bad {
                for key in [Key::Details, Key::Id] {
                    assert_eq!(
                        value(event, key).as_deref(),
                        Some(MARKER),
                        "the BAD error's {key:?}: {}",
                        event.text
                    );
                }
            }
            assert_all_redacted(
                &values(
                    &events,
                    EventType::MessageIngest(MessageIngestEvent::ImapAppend),
                    Key::MessageId,
                ),
                "the APPEND's Message-ID",
            );
        } else {
            assert_any_contains(&fetch, "X-Hdr-Name-Canary", "control: the FETCH details");
            assert_any_contains(&store, "kw-canary", "control: the STORE details");
            assert_any_contains(&output, "hdr-value-canary", "control: the FETCH literal");
            let details = bad
                .iter()
                .map(|event| value(event, Key::Details).unwrap_or_default())
                .collect::<Vec<_>>();
            assert_any_contains(&details, "bad-canary", "control: the BAD error");
        }
    }
}

/// One POP3 session of `account`: RETR, TOP, an AUTH with an unsupported
/// mechanism after login. Returns the RETR answer.
async fn pop3_session(account: &Account) -> String {
    let mut pop3 = Pop3Connection::connect().await;
    pop3.authenticate(account.name(), account.secret()).await;
    pop3.send("RETR 1").await;
    let retrieved = pop3.assert_read(Pop3Reply::Multiline).await.join("\n");
    pop3.send("TOP 1 0").await;
    pop3.assert_read(Pop3Reply::Multiline).await;
    pop3.send("AUTH mech-canary").await;
    pop3.assert_read(Pop3Reply::Err).await;
    pop3.send("QUIT").await;
    pop3.assert_read(Pop3Reply::Ok).await;
    retrieved
}

/// Spec 10: in a key account's POP3 session every raw read and write is
/// masked and the parse error quoting the mechanism is traced without it;
/// the client still receives the message. An ordinary account's session is
/// the control.
async fn pop3_session_is_redacted(key: &Account, plain: &Account) {
    println!("pop3_session_is_redacted");
    for account in [key, plain] {
        let traced = Traced::start(&traced_types());
        let retrieved = pop3_session(account).await;
        let events = traced.finish().await;
        assert!(
            retrieved.contains("body-canary"),
            "{}: the RETR answer: {retrieved}",
            account.name()
        );
        let output = values(
            &events,
            EventType::Pop3(Pop3Event::RawOutput),
            Key::Contents,
        );
        let errors = values(&events, EventType::Pop3(Pop3Event::Error), Key::Details);
        if account.name() == KEY {
            assert_no_canary(&events, "a key account's POP3 session");
            let mut raw = values(&events, EventType::Pop3(Pop3Event::RawInput), Key::Contents);
            raw.extend(output);
            assert_all_redacted(&raw, "the raw I/O of a key account's session");
            assert_all_redacted(&errors, "the POP3 parse error");
        } else {
            assert_any_contains(&output, "body-canary", "control: the RETR answer");
            assert_any_contains(&errors, "mech-canary", "control: the parse error");
        }
    }
}

/// Whether `reason` is `compile error at line <n>, column <n>`.
fn is_position_reason(reason: &str) -> bool {
    reason
        .strip_prefix("compile error at line ")
        .and_then(|rest| rest.split_once(", column "))
        .is_some_and(|(line, column)| line.parse::<u32>().is_ok() && column.parse::<u32>().is_ok())
}

/// Spec 10, issue #21: a vacation response whose script does not compile is
/// traced with the error's position only, for every account. A raw NUL
/// byte and an unbalanced quote in the text are the attempt; when the
/// generated script compiles anyway, the unit test in
/// `crates/jmap/src/vacation/set.rs` is the pin.
async fn vacation_compile_error_reason_is_fixed(plain: &Account) {
    println!("vacation_compile_error_reason_is_fixed");
    let mut types = traced_types();
    types.push(EventType::Store(StoreEvent::UnexpectedError));
    let traced = Traced::start(&types);
    let reply = plain
        .jmap_method_call(
            "VacationResponse/set",
            json!({
                "accountId": plain.id_string(),
                "update": { "singleton": {
                    "isEnabled": true,
                    "subject": VACATION_CANARY,
                    "textBody": format!("nul \u{0} quote \" {VACATION_CANARY}"),
                } }
            }),
        )
        .await;
    let events = traced.finish().await;
    for event in &events {
        assert!(
            !content(event).contains(VACATION_CANARY),
            "{:?} carries the vacation text: {}",
            event.typ,
            event.text
        );
    }
    if reply.is_error_at(0) {
        let reasons = values(
            &events,
            EventType::Store(StoreEvent::UnexpectedError),
            Key::Reason,
        );
        assert!(
            !reasons.is_empty(),
            "no compile error traced: {:?}",
            reply.0
        );
        for reason in reasons {
            assert!(is_position_reason(&reason), "the traced reason: {reason:?}");
        }
    } else {
        println!("vacation_compile_error_reason_is_fixed: the generated script compiles");
        // The script is removed: a later delivery would answer it.
        plain
            .jmap_method_call(
                "VacationResponse/set",
                json!({
                    "accountId": plain.id_string(),
                    "update": { "singleton": { "isEnabled": false } }
                }),
            )
            .await;
        let scripts = plain
            .jmap_method_call(
                "SieveScript/get",
                json!({ "accountId": plain.id_string(), "ids": null, "properties": ["id"] }),
            )
            .await;
        let ids = scripts
            .list()
            .iter()
            .map(|script| script["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            plain
                .jmap_destroy("SieveScript", &ids, Vec::<(&str, &str)>::new())
                .await
                .destroyed()
                .count(),
            ids.len(),
            "the vacation script"
        );
    }
}

/// Mail spec 6.1 step 4: a transaction to a spam trap and a key account
/// adds no autolearn sample (the sample would store the subject and the
/// queued plaintext), and the key account's copy is sealed; the same
/// transaction with an ordinary account adds one (the control).
async fn autolearn_skips_key_transactions(test: &TestServer, key: &Account) {
    println!("autolearn_skips_key_transactions");
    let key_id = key.id().document_id();
    for to in [KEY, PLAIN] {
        let before = message_ids(test, key_id).await;
        let samples = spam_samples(test).await;
        let traced = Traced::start(&[EventType::Spam(SpamEvent::TrainSampleAdded)]);
        SmtpConnection::connect()
            .await
            .ingest(SENDER, &[TRAP, to], &canary_message())
            .await;
        wait_for_delivery(test).await;
        let added = traced.finish().await.len();
        if to == KEY {
            assert_eq!(
                spam_samples(test).await,
                samples,
                "a key transaction's sample"
            );
            assert_eq!(added, 0, "a key transaction's sample event");
            let document_id = new_message(test, key_id, &before).await;
            assert_sealed(test, key_id, document_id, WRAP_PK).await;
        } else {
            assert_eq!(spam_samples(test).await, samples + 1, "control: the sample");
            assert_eq!(added, 1, "control: the sample event");
        }
    }
}
