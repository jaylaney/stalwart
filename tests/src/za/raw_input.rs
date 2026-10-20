/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Spec section 10, "Traces", issue #20: the mail protocols' raw-input traces
//! never record the authentication exchange. A key account logs in with its
//! password and with an app password over IMAP, POP3, ManageSieve and SMTP
//! submission while every raw-input and raw-output event is subscribed; no
//! captured event may carry either secret or a base64 form of it, while raw
//! input outside the exchange is still traced.

use super::{tracing::AcceptAnyCert, user_permissions};
use crate::utils::{
    server::TestServer,
    za::{za_app_password, za_post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_rustls::{
    TlsConnector,
    client::TlsStream,
    rustls::{self, pki_types::ServerName},
};
use trc::{
    AuthEvent, Collector, EventType, ImapEvent, Key, ManageSieveEvent, Pop3Event, SmtpEvent,
    ipc::subscriber::{Interests, SubscriberBuilder},
};

/// A fresh key account whose password has no spaces: POP3's PASS takes a
/// single argument.
const NAME: &str = "key12@example.com";
const PASSWORD: &str = "RawInput-Secret-7c41e9d2b8f6a3";
const SUBSCRIBER_ID: &str = "za-raw-input-test";

const IMAP_PORT: u16 = 9992;
const POP3_PORT: u16 = 4110;
const SIEVE_PORT: u16 = 4190;
/// The submission listener the suite adds (`za_tests`).
pub const SUBMISSION_PORT: u16 = 4465;

/// Distinctive raw input sent outside the authentication exchange: the
/// positive controls, which a masking that hid everything would fail.
const IMAP_CONTROL: &str = "rawimapctl";
const SIEVE_CONTROL: &str = "rawsievectl";
const SMTP_CONTROL: &str = "rawsmtpctl.test";
/// Sent after the line answering the MFA 334: traced, as the 334's masking
/// ends with that line.
const MFA_CONTROL: &str = "rawmfactl";
/// A static diagnostic written before authentication (STARTTLS on an
/// implicit-TLS port): it quotes no input and stays readable.
const STATIC_DIAGNOSTIC: &str = "Already in TLS mode.";
const MARKER: &str = "[redacted]";

/// The SASL PLAIN message of `NAME` and `secret`, base64-encoded.
fn plain(secret: &str) -> String {
    STANDARD.encode(format!("\0{NAME}\0{secret}"))
}

struct Conn(BufReader<TlsStream<tokio::net::TcpStream>>);

impl Conn {
    async fn open(port: u16) -> Self {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCert(provider)))
            .with_no_client_auth();
        let tcp = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        let tls = TlsConnector::from(Arc::new(config))
            .connect(ServerName::try_from("127.0.0.1").unwrap(), tcp)
            .await
            .unwrap();
        Conn(BufReader::new(tls))
    }

    /// One write, flushed: a read of its own on the server unless the
    /// server reads slower than the client writes.
    async fn send(&mut self, data: &str) {
        self.0.get_mut().write_all(data.as_bytes()).await.unwrap();
        self.0.get_mut().flush().await.unwrap();
    }

    /// A submission session past its greeting. The default inbound
    /// throttle admits 5 connections a second from one address, LMTP
    /// deliveries included, and drops the rest before the greeting: a
    /// dropped connection is retried after the throttle's window.
    async fn open_smtp() -> Self {
        for _ in 0..5 {
            let mut conn = Conn::open(SUBMISSION_PORT).await;
            let mut line = String::new();
            if let Ok(Ok(read)) =
                tokio::time::timeout(Duration::from_secs(10), conn.0.read_line(&mut line)).await
                && read > 0
            {
                assert!(line.starts_with("220"), "SMTP greeting {line:?}");
                return conn;
            }
            tokio::time::sleep(Duration::from_millis(1100)).await;
        }
        panic!("the submission listener dropped every connection");
    }

    /// Reads lines until one starts with any of `prefixes`, and returns it.
    async fn until(&mut self, prefixes: &[&str]) -> String {
        loop {
            let mut line = String::new();
            let read = tokio::time::timeout(Duration::from_secs(10), self.0.read_line(&mut line))
                .await
                .unwrap_or_else(|_| panic!("timed out waiting for {prefixes:?}"))
                .unwrap_or_else(|err| panic!("read failed waiting for {prefixes:?}: {err}"));
            assert!(read > 0, "connection closed waiting for {prefixes:?}");
            if prefixes.iter().any(|prefix| line.starts_with(prefix)) {
                return line;
            }
        }
    }

    /// Sends `data` and expects a line starting with `expected`.
    async fn exchange(&mut self, data: &str, expected: &str) {
        self.send(data).await;
        let line = self
            .until(&[expected, "-ERR", "NO", "BAD", "a1 ", "5", "4"])
            .await;
        assert!(line.starts_with(expected), "{data:?} answered {line:?}");
    }
}

/// The IMAP logins: LOGIN; LOGIN with the password as a synchronizing
/// literal (sent on a read of its own) and as a LITERAL+ literal (in the
/// same write); AUTHENTICATE PLAIN with the initial response, with a
/// continuation, and with the response pipelined after the command (parsed
/// as a tag and echoed by the parse error, issue #20 fix round 1); and a
/// LOGIN after UNAUTHENTICATE. The first session then sends a command whose
/// raw input must still be traced.
async fn imap(secret: &str) {
    for flow in 0..7 {
        let mut conn = Conn::open(IMAP_PORT).await;
        conn.until(&["* OK"]).await;
        match flow {
            0 => {
                conn.exchange("a9 STARTTLS\r\n", "a9 ").await;
                conn.exchange(&format!("a1 LOGIN \"{NAME}\" \"{secret}\"\r\n"), "a1 OK")
                    .await
            }
            1 => {
                conn.exchange(
                    &format!("a1 LOGIN \"{NAME}\" {{{}}}\r\n", secret.len()),
                    "+",
                )
                .await;
                conn.exchange(&format!("{secret}\r\n"), "a1 OK").await;
            }
            2 => {
                conn.exchange(
                    &format!("a1 LOGIN \"{NAME}\" {{{}+}}\r\n{secret}\r\n", secret.len()),
                    "a1 OK",
                )
                .await
            }
            3 => {
                conn.exchange(
                    &format!("a1 AUTHENTICATE PLAIN {}\r\n", plain(secret)),
                    "a1 OK",
                )
                .await
            }
            4 => {
                conn.exchange("a1 AUTHENTICATE PLAIN\r\n", "+").await;
                conn.exchange(&format!("{}\r\n", plain(secret)), "a1 OK")
                    .await;
            }
            5 => {
                // The pipelined response is read before the continuation is
                // sent, as a request of its own: a parse error that quotes
                // it. The exchange then completes as a continuation.
                conn.exchange(
                    &format!("a1 AUTHENTICATE PLAIN\r\n{}\r\n", plain(secret)),
                    "+",
                )
                .await;
                conn.exchange(&format!("{}\r\n", plain(secret)), "a1 OK")
                    .await;
            }
            _ => {
                conn.exchange(&format!("a1 LOGIN \"{NAME}\" \"{secret}\"\r\n"), "a1 OK")
                    .await;
                conn.exchange("a3 UNAUTHENTICATE\r\n", "a3 OK").await;
                conn.exchange(&format!("a4 LOGIN \"{NAME}\" \"{secret}\"\r\n"), "a4 OK")
                    .await;
            }
        }
        if flow == 0 {
            conn.exchange(&format!("{IMAP_CONTROL} NOOP\r\n"), IMAP_CONTROL)
                .await;
        }
        conn.exchange("a2 LOGOUT\r\n", "a2 OK").await;
    }
}

/// USER and PASS, then AUTH PLAIN with the initial response and with a
/// continuation; then USER, PASS and an AUTH whose mechanism is base64 of
/// the secret, in one write (its parse error, quoting the mechanism, is
/// written after the login); a post-authentication NOOP is the control.
/// The first session sends STLS first, for the static diagnostic.
async fn pop3(secret: &str) {
    for flow in 0..4 {
        let mut conn = Conn::open(POP3_PORT).await;
        conn.until(&["+OK"]).await;
        match flow {
            0 => {
                conn.exchange("STLS\r\n", "-ERR").await;
                conn.exchange(&format!("USER {NAME}\r\n"), "+OK").await;
                conn.exchange(&format!("PASS {secret}\r\n"), "+OK").await;
            }
            3 => {
                conn.send(&format!(
                    "USER {NAME}\r\nPASS {secret}\r\nAUTH {}\r\n",
                    STANDARD.encode(secret)
                ))
                .await;
                conn.until(&["+OK"]).await;
                conn.until(&["+OK"]).await;
                conn.until(&["-ERR"]).await;
            }
            1 => {
                conn.exchange(&format!("AUTH PLAIN {}\r\n", plain(secret)), "+OK")
                    .await
            }
            _ => {
                conn.exchange("AUTH PLAIN\r\n", "+").await;
                conn.exchange(&format!("{}\r\n", plain(secret)), "+OK")
                    .await;
            }
        }
        conn.exchange("NOOP\r\n", "+OK").await;
        conn.exchange("QUIT\r\n", "+OK").await;
    }
}

/// AUTHENTICATE PLAIN with the initial response, after STARTTLS (the static
/// diagnostic) and an AUTHENTICATE whose mechanism is the base64 secret
/// (refused without quoting it); a post-authentication HAVESPACE is the
/// control. With `pipelined`, a second session sends the response pipelined
/// ahead of the continuation (parsed as a command, refused), then answers
/// the continuation.
async fn sieve(secret: &str, pipelined: bool) {
    if pipelined {
        let mut conn = Conn::open(SIEVE_PORT).await;
        conn.until(&["OK"]).await;
        conn.send(&format!(
            "AUTHENTICATE \"PLAIN\"\r\n\"{}\"\r\n",
            plain(secret)
        ))
        .await;
        conn.until(&["{0}"]).await;
        conn.exchange(&format!("\"{}\"\r\n", plain(secret)), "OK")
            .await;
        conn.exchange("LOGOUT\r\n", "OK").await;
    }
    let mut conn = Conn::open(SIEVE_PORT).await;
    conn.until(&["OK"]).await;
    conn.exchange("STARTTLS\r\n", "NO").await;
    conn.exchange(
        &format!("AUTHENTICATE \"{}\"\r\n", STANDARD.encode(secret)),
        "NO",
    )
    .await;
    conn.exchange(
        &format!("AUTHENTICATE \"PLAIN\" \"{}\"\r\n", plain(secret)),
        "OK",
    )
    .await;
    // Answered OK or NO; only its raw input matters.
    conn.send(&format!("HAVESPACE \"{SIEVE_CONTROL}\" 100\r\n"))
        .await;
    conn.until(&["OK", "NO"]).await;
    conn.exchange("LOGOUT\r\n", "OK").await;
}

/// Sends `writes` as separate writes, 300 ms apart so that the server reads
/// each before the next exists, and expects a line starting with `expected`.
async fn split_writes(conn: &mut Conn, writes: &[String], expected: &str) {
    let (last, first) = writes.split_last().unwrap();
    for write in first {
        conn.send(write).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    conn.exchange(last, expected).await;
}

/// The SMTP logins: AUTH PLAIN; AUTH LOGIN; an AUTH PLAIN line split across
/// two writes and across three (`AU` / `TH PLAIN <head>` / `<tail>`); EHLO
/// and AUTH PLAIN pipelined in one write; a lower-case `auth` after a
/// space; `AUTH`, a form feed and `PLAIN`; and an AUTH command that starts
/// mid-line, after a BDAT chunk.
/// Returns the writes that must each be a masked read of their own.
/// The EHLO before each login is the control.
async fn smtp(secret: &str) -> Vec<String> {
    let response = plain(secret);
    let (head, tail) = response.split_at(response.len() / 2);
    let mut own_reads = Vec::new();
    for flow in 0..8 {
        let mut conn = Conn::open_smtp().await;
        if flow != 4 {
            conn.send(&format!("EHLO {SMTP_CONTROL}\r\n")).await;
            conn.until(&["250 "]).await;
        }
        match flow {
            0 => {
                conn.exchange(&format!("AUTH PLAIN {response}\r\n"), "235")
                    .await
            }
            1 => {
                conn.exchange("AUTH LOGIN\r\n", "334").await;
                conn.exchange(&format!("{}\r\n", STANDARD.encode(NAME)), "334")
                    .await;
                conn.exchange(&format!("{}\r\n", STANDARD.encode(secret)), "235")
                    .await;
            }
            2 => {
                let writes = [format!("AUTH PLAIN {head}"), format!("{tail}\r\n")];
                split_writes(&mut conn, &writes, "235").await;
                own_reads.extend(writes);
            }
            3 => {
                let writes = [
                    "AU".to_string(),
                    format!("TH PLAIN {head}"),
                    format!("{tail}\r\n"),
                ];
                split_writes(&mut conn, &writes, "235").await;
                // `AU` alone carries nothing and is traced.
                own_reads.extend(writes.into_iter().skip(1));
            }
            4 => {
                conn.send(&format!("EHLO {SMTP_CONTROL}\r\nAUTH PLAIN {response}\r\n"))
                    .await;
                let line = conn.until(&["235", "4", "5"]).await;
                assert!(line.starts_with("235"), "pipelined AUTH answered {line:?}");
            }
            5 => {
                conn.exchange(&format!(" auth plain {response}\r\n"), "235")
                    .await
            }
            6 => {
                // A form feed is a stop character of the command parser.
                conn.exchange(&format!("AUTH\x0CPLAIN {response}\r\n"), "235")
                    .await
            }
            _ => {
                // Mid-line: the refused BDAT's 5-byte chunk ends where the
                // AUTH command starts, inside the line.
                conn.send(&format!("BDAT 5\r\nhelloAUTH PLAIN {response}\r\n"))
                    .await;
                let line = conn.until(&["235", "535", "454"]).await;
                assert!(line.starts_with("235"), "mid-line AUTH answered {line:?}");
            }
        }
        conn.exchange("QUIT\r\n", "221").await;
    }
    own_reads
}

/// The MFA reply (issue #20 fix round 1): once TOTP is enrolled, AUTH PLAIN
/// with the password alone is answered with a 334 that leaves the exchange.
/// The client answers it with a PLAIN response carrying a code, split
/// across two writes; both are masked reads of their own. Returns the
/// answer's writes.
async fn smtp_mfa(answer: &str) -> Vec<String> {
    let mut conn = Conn::open_smtp().await;
    conn.send(&format!("EHLO {SMTP_CONTROL}\r\n")).await;
    conn.until(&["250 "]).await;
    conn.exchange(&format!("AUTH PLAIN {}\r\n", plain(PASSWORD)), "334")
        .await;
    let (head, tail) = answer.split_at(answer.len() / 2);
    let writes = vec![head.to_string(), format!("{tail}\r\n")];
    // Parsed as a command once the exchange is left: refused.
    split_writes(&mut conn, &writes, "5").await;
    conn.exchange(&format!("NOOP {MFA_CONTROL}\r\n"), "250")
        .await;
    conn.exchange("QUIT\r\n", "221").await;
    writes
}

fn totp_url() -> String {
    totp_rs::Builder::new()
        .with_secret(store::rand::random::<[u8; 20]>())
        .with_account_name("key12")
        .with_issuer(Some("Stalwart"))
        .build()
        .unwrap()
        .to_url()
        .unwrap()
}

/// Every string in `value`, bytes read as UTF-8 (lossy).
fn strings(value: &trc::Value, out: &mut Vec<String>) {
    match value {
        trc::Value::String(s) => out.push(s.to_string()),
        trc::Value::Bytes(b) => out.push(String::from_utf8_lossy(b).into_owned()),
        trc::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        trc::Value::Event(err) => err.keys().iter().for_each(|(_, v)| strings(v, out)),
        _ => {}
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access raw-input trace tests...");
    let admin = test.account("admin@example.com").clone();
    let account = admin
        .create_key_user_account(NAME, PASSWORD, "Key Twelve", &[], user_permissions())
        .await;
    test.insert_account(account);
    let app_password = za_app_password(NAME, PASSWORD, "raw-input traces").await;

    // The test server configures no tracer: the raw-input events are not
    // emitted until something declares interest in them.
    let traced = [
        EventType::Imap(ImapEvent::RawInput),
        EventType::Imap(ImapEvent::RawOutput),
        EventType::Imap(ImapEvent::Error),
        EventType::Pop3(Pop3Event::RawInput),
        EventType::Pop3(Pop3Event::RawOutput),
        EventType::Pop3(Pop3Event::Error),
        EventType::ManageSieve(ManageSieveEvent::RawInput),
        EventType::ManageSieve(ManageSieveEvent::RawOutput),
        EventType::ManageSieve(ManageSieveEvent::Error),
        EventType::Auth(AuthEvent::Error),
        EventType::Auth(AuthEvent::Failed),
        EventType::Auth(AuthEvent::MfaRequired),
        EventType::Smtp(SmtpEvent::RawInput),
        EventType::Smtp(SmtpEvent::RawOutput),
    ];
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

    let mut secrets = Vec::new();
    let mut own_reads = Vec::new();
    for secret in [PASSWORD, app_password.as_str()] {
        imap(secret).await;
        pop3(secret).await;
        // Once: each pipelined response counts toward the scanner ban.
        sieve(secret, secret == PASSWORD).await;
        own_reads.extend(smtp(secret).await);
        let response = plain(secret);
        let (head, tail) = response.split_at(response.len() / 2);
        secrets.extend([
            secret.to_string(),
            STANDARD.encode(secret),
            response.clone(),
            // Each half of a split response.
            head.to_string(),
            tail.to_string(),
        ]);
    }
    // Last: enrolling TOTP ends the password logins.
    let url = totp_url();
    let code = totp_rs::Totp::from_url(&url)
        .unwrap()
        .generate_current()
        .to_string();
    za_post(
        "totp",
        &json!({ "username": NAME, "password": PASSWORD, "otp_auth": url, "confirm": code }),
    )
    .await
    .expect(200);
    let answer = STANDARD.encode(format!("\0{NAME}\0{PASSWORD}${code}"));
    let (head, tail) = answer.split_at(answer.len() / 2);
    secrets.extend([answer.clone(), head.to_string(), tail.to_string()]);
    own_reads.extend(smtp_mfa(&answer).await);

    tokio::time::sleep(Duration::from_millis(500)).await;
    // (type, size, every string of the event)
    let mut captured: Vec<(EventType, Option<u64>, String)> = Vec::new();
    while let Ok(batch) = rx.try_recv() {
        for event in batch {
            let mut values = Vec::new();
            let mut size = None;
            for (key, value) in event.keys.iter() {
                if *key == Key::Size {
                    size = value.to_uint();
                }
                strings(value, &mut values);
            }
            captured.push((event.inner.typ, size, values.join("\n")));
        }
    }
    Collector::remove_subscriber(SUBSCRIBER_ID.into());
    Collector::reload();

    let inputs = |event_type: EventType| {
        captured
            .iter()
            .filter(move |(typ, ..)| *typ == event_type)
            .map(|(_, size, values)| (*size, values.as_str()))
    };
    // The secrets and their base64 forms reach no captured event.
    for (typ, _, values) in &captured {
        for secret in &secrets {
            assert!(
                !values.contains(secret.as_str()),
                "{typ:?} traced a secret ({secret:?}): {values}"
            );
        }
    }
    // Every protocol's pre-authentication raw input was emitted, as the
    // marker: the assertion above is not vacuous.
    for event_type in [
        EventType::Imap(ImapEvent::RawInput),
        EventType::Pop3(Pop3Event::RawInput),
        EventType::ManageSieve(ManageSieveEvent::RawInput),
        EventType::Smtp(SmtpEvent::RawInput),
    ] {
        assert!(
            inputs(event_type).any(|(_, values)| values == MARKER),
            "no masked {event_type:?} captured"
        );
    }
    // Each write of a split AUTH line, and of the answer to the MFA 334,
    // reached the server as a read of its own, masked.
    for write in &own_reads {
        assert!(
            inputs(EventType::Smtp(SmtpEvent::RawInput))
                .any(|(size, values)| size == Some(write.len() as u64) && values == MARKER),
            "the write {write:?} was not a masked read of its own"
        );
    }
    // The pipelined IMAP response was echoed by a parse error, redacted.
    assert!(
        inputs(EventType::Imap(ImapEvent::Error)).any(|(_, values)| values.contains(MARKER)),
        "no redacted IMAP parse error captured"
    );
    // The queued POP3 parse error quoting the mechanism was traced redacted,
    // though written after the login.
    assert!(
        inputs(EventType::Pop3(Pop3Event::Error)).any(|(_, values)| values.contains(MARKER)),
        "no redacted POP3 parse error captured"
    );
    // ManageSieve refused the base64 mechanism without quoting it.
    assert!(
        inputs(EventType::Auth(AuthEvent::Error))
            .any(|(_, values)| values.contains("Authentication mechanism not supported.")),
        "no ManageSieve mechanism refusal captured"
    );
    // Static pre-authentication diagnostics stay readable.
    for event_type in [
        EventType::Imap(ImapEvent::Error),
        EventType::Pop3(Pop3Event::Error),
        EventType::ManageSieve(ManageSieveEvent::Error),
    ] {
        assert!(
            inputs(event_type).any(|(_, values)| values.contains(STATIC_DIAGNOSTIC)),
            "the static diagnostic of {event_type:?} was redacted"
        );
    }
    // The 334's masking ended with the line answering it.
    assert!(
        inputs(EventType::Smtp(SmtpEvent::RawInput))
            .any(|(_, values)| values.contains(MFA_CONTROL)),
        "the command after the MFA answer was not traced"
    );
    // Positive controls: raw input outside the exchange is still traced.
    assert!(
        inputs(EventType::Imap(ImapEvent::RawInput))
            .any(|(_, values)| values.contains(IMAP_CONTROL)),
        "the post-authentication IMAP command was not traced"
    );
    assert!(
        inputs(EventType::Pop3(Pop3Event::RawInput)).any(|(_, values)| values == "NOOP\r\n"),
        "the post-authentication POP3 command was not traced"
    );
    assert!(
        inputs(EventType::ManageSieve(ManageSieveEvent::RawInput))
            .any(|(_, values)| values.contains(SIEVE_CONTROL)),
        "the post-authentication ManageSieve command was not traced"
    );
    assert!(
        inputs(EventType::Smtp(SmtpEvent::RawInput))
            .any(|(_, values)| values.contains(SMTP_CONTROL)),
        "the SMTP EHLO was not traced"
    );
}
