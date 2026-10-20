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
use crate::utils::{server::TestServer, za::za_app_password};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_rustls::{
    TlsConnector,
    client::TlsStream,
    rustls::{self, pki_types::ServerName},
};
use trc::{
    Collector, EventType, ImapEvent, Key, ManageSieveEvent, Pop3Event, SmtpEvent,
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

/// The four IMAP logins: LOGIN, LOGIN with the password as a literal (sent
/// on a read of its own), AUTHENTICATE PLAIN with the initial response and
/// with a continuation. The first session then sends a command whose raw
/// input must still be traced.
async fn imap(secret: &str) {
    for flow in 0..4 {
        let mut conn = Conn::open(IMAP_PORT).await;
        conn.until(&["* OK"]).await;
        match flow {
            0 => {
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
                    &format!("a1 AUTHENTICATE PLAIN {}\r\n", plain(secret)),
                    "a1 OK",
                )
                .await
            }
            _ => {
                conn.exchange("a1 AUTHENTICATE PLAIN\r\n", "+").await;
                conn.exchange(&format!("{}\r\n", plain(secret)), "a1 OK")
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
/// continuation; a post-authentication NOOP is the control.
async fn pop3(secret: &str) {
    for flow in 0..3 {
        let mut conn = Conn::open(POP3_PORT).await;
        conn.until(&["+OK"]).await;
        match flow {
            0 => {
                conn.exchange(&format!("USER {NAME}\r\n"), "+OK").await;
                conn.exchange(&format!("PASS {secret}\r\n"), "+OK").await;
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

/// AUTHENTICATE PLAIN with the initial response; a post-authentication
/// HAVESPACE is the control.
async fn sieve(secret: &str) {
    let mut conn = Conn::open(SIEVE_PORT).await;
    conn.until(&["OK"]).await;
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

/// AUTH PLAIN, AUTH LOGIN, and an AUTH PLAIN line split across two writes
/// (returned: the split's two writes). The EHLO before each is the control.
async fn smtp(secret: &str) -> [String; 2] {
    let response = plain(secret);
    let (head, tail) = response.split_at(response.len() / 2);
    let split = [format!("AUTH PLAIN {head}"), format!("{tail}\r\n")];
    for flow in 0..3 {
        let mut conn = Conn::open_smtp().await;
        conn.send(&format!("EHLO {SMTP_CONTROL}\r\n")).await;
        conn.until(&["250 "]).await;
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
            _ => {
                conn.send(&split[0]).await;
                // The server reads the first write before the second exists.
                tokio::time::sleep(Duration::from_millis(300)).await;
                conn.exchange(&split[1], "235").await;
            }
        }
        conn.exchange("QUIT\r\n", "221").await;
    }
    split
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
        EventType::Pop3(Pop3Event::RawInput),
        EventType::Pop3(Pop3Event::RawOutput),
        EventType::ManageSieve(ManageSieveEvent::RawInput),
        EventType::ManageSieve(ManageSieveEvent::RawOutput),
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
    let mut splits = Vec::new();
    for secret in [PASSWORD, app_password.as_str()] {
        imap(secret).await;
        pop3(secret).await;
        sieve(secret).await;
        let split = smtp(secret).await;
        secrets.extend([
            secret.to_string(),
            plain(secret),
            STANDARD.encode(secret),
            // Each write of the split line, without the line break.
            split[0].clone(),
            split[1].trim_end().to_string(),
        ]);
        splits.push(split);
    }

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
    // The split AUTH line reached the server as two reads, both masked.
    for split in &splits {
        for write in split {
            assert!(
                inputs(EventType::Smtp(SmtpEvent::RawInput))
                    .any(|(size, values)| size == Some(write.len() as u64) && values == MARKER),
                "the split write {write:?} was not a masked read of its own"
            );
        }
    }
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
