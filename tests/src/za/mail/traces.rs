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
//! autolearn sample (6.1 step 4). The sweeps subscribe to every event type.

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
            Expression, ExpressionMatch, MemoryLookupKey, MtaMilter, MtaStageData,
            SieveSystemScript, SpamClassifier, SpamRule, SpamRuleHeader, SpamTag, SpamTagScore,
        },
    },
    types::{float::Float, list::List, map::Map},
};
use serde_json::json;
use std::time::Duration;
use trc::{
    EvalEvent, EventType, HttpEvent, ImapEvent, Key, MessageIngestEvent, MilterEvent, Pop3Event,
    SieveEvent, SmtpEvent, SpamEvent, StoreEvent,
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
/// the subject, the body, the domains of the body's address and URL and the
/// Message-ID of `canary_message`; an IMAP keyword, search term and
/// malformed attribute; a POP3 mechanism.
const CANARIES: &[&str] = &[
    "hdr-name-canary",
    "hdr-value-canary",
    "subject-canary",
    "body-canary",
    "domain-canary",
    "url-canary",
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
    pipelined_rcpt_and_bdat_are_redacted(test, &key).await;
    key_rcpt_after_a_chunk_is_refused(test, &key, &plain).await;
    imap_session_is_redacted(&key, &plain).await;
    pop3_session_is_redacted(&key, &plain).await;
    vacation_compile_error_reason_is_fixed(&plain).await;
    autolearn_skips_key_transactions(test, &key).await;
    late_recipients_are_classified(test, &key).await;

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
/// every transaction; a header rule whose result is the canary header's
/// value (its `Eval(Result)` trace is the redaction's case); a trusted
/// DATA-stage script whose `error` quotes the subject (its
/// `Sieve(RuntimeError)` trace is ruling R57's case; the error ends the
/// script with an implicit keep, so delivery proceeds).
struct Settings {
    milter: Id,
    trap_tag: Id,
    trap_key: Id,
    header_rule: Id,
    script: Id,
}

/// The trusted DATA-stage script. `ihave` is the one capability the trusted
/// compiler still checks, and `error` is part of it.
const ERROR_SCRIPT: &str = "require [\"ihave\", \"variables\"];\nerror \"${header.subject}\";\n";

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
    let header_rule = admin
        .registry_create_object(SpamRule::Header(SpamRuleHeader {
            condition: Expression {
                else_: "false".into(),
                match_: List::from_iter([ExpressionMatch {
                    if_: "name_lower == 'x-hdr-name-canary'".into(),
                    then: "value".into(),
                }]),
            },
            name: "ZA_TRACES_CANARY_HEADER".into(),
            description: None,
            enable: true,
            priority: 500,
        }))
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
    let script = admin
        .registry_create_object(SieveSystemScript {
            name: "za-traces-error".into(),
            description: None,
            is_active: true,
            contents: ERROR_SCRIPT.into(),
        })
        .await;
    admin
        .registry_update_setting(
            MtaStageData {
                enable_spam_filter: Expression {
                    else_: "true".into(),
                    ..Default::default()
                },
                script: Expression {
                    else_: "'za-traces-error'".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            &[Property::EnableSpamFilter, Property::Script],
        )
        .await;
    admin.reload_settings().await;
    admin.reload_lookup_stores().await;
    Settings {
        milter,
        trap_tag,
        trap_key,
        header_rule,
        script,
    }
}

async fn unconfigure(admin: &Account, settings: Settings) {
    for (object, id) in [
        (ObjectType::MtaMilter, settings.milter),
        (ObjectType::SpamTag, settings.trap_tag),
        (ObjectType::MemoryLookupKey, settings.trap_key),
        (ObjectType::SpamRule, settings.header_rule),
        (ObjectType::SieveSystemScript, settings.script),
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
        .registry_update_setting(
            MtaStageData::default(),
            &[Property::EnableSpamFilter, Property::Script],
        )
        .await;
    admin.reload_settings().await;
    admin.reload_lookup_stores().await;
}

/// A message holding every content canary: a header of its own (name and
/// value), the subject, the body with an address and a URL whose domains are
/// canaries, and the Message-ID. All are unique per call: an ordinary
/// account drops a duplicate Message-ID, and the spam filter's domain
/// lookup of each new name is a cache miss. The domains are under a public
/// suffix (`.net`): the spam filter skips an address whose domain has no
/// registrable part, as under `.example`.
fn canary_message() -> String {
    let unique = store::rand::random::<u64>();
    format!(
        "From: Sender <sender@example.org>\r\n\
         To: Recipient <recipient@example.com>\r\n\
         Subject: subject-canary\r\n\
         Message-ID: <{unique:016x}.msgid-canary@example.com>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         X-Hdr-Name-Canary: hdr-value-canary\r\n\
         \r\n\
         body-canary\r\n\
         Write to addr@{unique:016x}.domain-canary.net or see \
         https://{unique:016x}.url-canary.net/x for more.\r\n"
    )
}

/// The events every step captures: all of them.
fn traced_types() -> &'static [EventType] {
    EventType::variants()
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
/// stays readable; the ingest event names no Message-ID; the spam filter's
/// lookups of the domains the body names emit no cache event carrying them
/// (ruling R56); the trusted DATA-stage script's runtime error, which quotes
/// the subject, records the marker (ruling R57). The same delivery to the
/// ordinary account is the control.
async fn delivery_leaves_no_content_in_events(test: &TestServer) {
    println!("delivery_leaves_no_content_in_events");
    for to in [KEY, PLAIN] {
        let traced = Traced::start(traced_types());
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
        let script_errors = values(
            &events,
            EventType::Sieve(SieveEvent::RuntimeError),
            Key::Reason,
        );
        assert_eq!(message_ids.len(), 1, "{to}: one ingest event");
        assert_any_contains(&reads, "RCPT TO", to);
        if to == KEY {
            assert_no_canary(&events, "a delivery to a key account");
            assert_all_redacted(
                &script_errors,
                "the trusted script's runtime error of a key delivery",
            );
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
            assert_any_contains(
                &values(&events, EventType::Eval(EvalEvent::Result), Key::Result),
                "hdr-value-canary",
                "control: the header rule's result",
            );
            assert_any_contains(
                &script_errors,
                "subject-canary",
                "control: the trusted script's runtime error",
            );
            // Ruling R56's control: the spam filter looked up the domains
            // the body names, and the ordinary lookup traces them (the URL's
            // registrable domain may already be in the negative cache).
            assert_any_contains(
                &values(&events, EventType::Store(StoreEvent::CacheMiss), Key::Key),
                "domain-canary",
                "control: the domain lookup of the body's address",
            );
            let lookups = [StoreEvent::CacheMiss, StoreEvent::CacheHit]
                .into_iter()
                .flat_map(|event| values(&events, EventType::Store(event), Key::Key))
                .collect::<Vec<_>>();
            assert_any_contains(
                &lookups,
                "url-canary",
                "control: the domain lookup of the body's URL",
            );
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
        let traced = Traced::start(traced_types());
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

    let traced = Traced::start(traced_types());
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

/// Ruling R41: an RCPT, a BDAT command and its chunk in one write are one read
/// traced before the recipient is known, so it is masked (for every account);
/// the MAIL FROM before it stays readable.
async fn pipelined_rcpt_and_bdat_are_redacted(test: &TestServer, key: &Account) {
    println!("pipelined_rcpt_and_bdat_are_redacted");
    let key_id = key.id().document_id();
    let before = message_ids(test, key_id).await;
    let message = canary_message();
    let traced = Traced::start(traced_types());
    let mut lmtp = SmtpConnection::connect().await;
    lmtp.mail_from(SENDER, 2).await;
    lmtp.send_raw(&format!(
        "RCPT TO:<{KEY}>\r\nBDAT {} LAST\r\n{message}",
        message.len()
    ))
    .await;
    // The RCPT's answer and the BDAT's, one per recipient.
    lmtp.read(2, 2).await;
    lmtp.quit().await;
    wait_for_delivery(test).await;
    let events = traced.finish().await;
    assert_no_canary(&events, "a pipelined BDAT delivery to a key account");
    let reads = values(&events, EventType::Smtp(SmtpEvent::RawInput), Key::Contents);
    assert_any_contains(
        &reads,
        "MAIL FROM",
        "the MAIL FROM before the pipelined read",
    );
    assert_any_contains(&reads, MARKER, "the pipelined read");
    assert!(
        !reads.iter().any(|read| read.contains("RCPT TO")),
        "the pipelined read is traced: {reads:?}"
    );
    let document_id = new_message(test, key_id, &before).await;
    assert_sealed(test, key_id, document_id, WRAP_PK).await;
}

/// Mail spec 10, RFC 3030 (RCPT precedes BDAT): a key recipient named after a
/// BDAT chunk, which was traced in the clear, is refused with 503; the
/// message reaches the earlier recipient only.
async fn key_rcpt_after_a_chunk_is_refused(test: &TestServer, key: &Account, plain: &Account) {
    println!("key_rcpt_after_a_chunk_is_refused");
    let key_id = key.id().document_id();
    let plain_id = plain.id().document_id();
    let key_before = message_ids(test, key_id).await;
    let plain_before = message_ids(test, plain_id).await;
    let message = canary_message();
    let (head, tail) = message.split_at(message.len() / 2);
    let mut lmtp = SmtpConnection::connect().await;
    lmtp.mail_from(SENDER, 2).await;
    lmtp.rcpt_to(PLAIN, 2).await;
    lmtp.bdat(head, 2).await;
    let refused = lmtp.rcpt_to(KEY, 5).await;
    assert!(
        refused[0].starts_with("503 5.5.1"),
        "a key recipient after a chunk: {refused:?}"
    );
    lmtp.bdat_last(tail, 1, 2).await;
    lmtp.quit().await;
    wait_for_delivery(test).await;
    assert_eq!(
        message_ids(test, key_id).await,
        key_before,
        "the refused key recipient got the message"
    );
    new_message(test, plain_id, &plain_before).await;
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
        let traced = Traced::start(traced_types());
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
        let traced = Traced::start(traced_types());
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
/// traced with the error's position only, for every account. A text and an
/// HTML body, each under the handler's 2048-byte limit, whose MIME-encoded
/// body exceeds the untrusted compiler's string limit (4096 bytes by default)
/// fail the handler's own compile; the unit test in
/// `crates/jmap/src/vacation/set.rs` pins a compiler message that quotes the
/// script.
async fn vacation_compile_error_reason_is_fixed(plain: &Account) {
    println!("vacation_compile_error_reason_is_fixed");
    let traced = Traced::start(traced_types());
    let reply = plain
        .jmap_method_call(
            "VacationResponse/set",
            json!({
                "accountId": plain.id_string(),
                "update": { "singleton": {
                    "isEnabled": true,
                    "subject": VACATION_CANARY,
                    "textBody": format!("{VACATION_CANARY} {}", "é".repeat(1000)),
                    "htmlBody": format!("<p>{}</p>", "é".repeat(1000)),
                } }
            }),
        )
        .await;
    let events = traced.finish().await;
    assert!(
        reply.is_error_at(0),
        "the vacation script compiled: {:?}",
        reply.response_at(0)
    );
    // The HTTP body traces echo an ordinary account's own request, as upstream
    // traces them (release 1 withholds them for key accounts).
    for event in events.iter().filter(|event| {
        !matches!(
            event.typ,
            EventType::Http(HttpEvent::RequestBody | HttpEvent::ResponseBody)
        )
    }) {
        assert!(
            !content(event).contains(VACATION_CANARY),
            "{:?} carries the vacation text: {}",
            event.typ,
            event.text
        );
    }
    let reasons = values(
        &events,
        EventType::Store(StoreEvent::UnexpectedError),
        Key::Reason,
    );
    assert!(!reasons.is_empty(), "no compile error traced");
    for reason in reasons {
        assert!(is_position_reason(&reason), "the traced reason: {reason:?}");
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

/// Mail spec 10 and 6.1 step 4: a recipient the DATA-stage milter adds
/// (`AddRcpt`, from the mock's `add_rcpt.<name>@<domain>` sender) is
/// classified too: a trap transaction that gains the key account adds no
/// autolearn sample, and the key account's copy is sealed; one that gains the
/// ordinary account adds the sample (the control).
async fn late_recipients_are_classified(test: &TestServer, key: &Account) {
    println!("late_recipients_are_classified");
    let key_id = key.id().document_id();
    for added in [KEY, PLAIN] {
        let before = message_ids(test, key_id).await;
        let samples = spam_samples(test).await;
        SmtpConnection::connect()
            .await
            .ingest(&format!("add_rcpt.{added}"), &[TRAP], &canary_message())
            .await;
        wait_for_delivery(test).await;
        if added == KEY {
            assert_eq!(
                spam_samples(test).await,
                samples,
                "a sample with a late key recipient"
            );
            let document_id = new_message(test, key_id, &before).await;
            assert_sealed(test, key_id, document_id, WRAP_PK).await;
        } else {
            assert_eq!(
                spam_samples(test).await,
                samples + 1,
                "control: the sample with a late ordinary recipient"
            );
        }
    }
}
