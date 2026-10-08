/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Spec section 10, "Traces": nothing a key account sends or receives over
//! DAV or JMAP, and no credential header from any request, reaches the HTTP
//! body traces.

use super::STRONG;
use crate::utils::{server::TestServer, webdav::DummyWebDavClient, za::SERVER_URL};
use hyper::StatusCode;
use serde_json::json;
use trc::{
    Collector, EventType, HttpEvent, JmapEvent,
    ipc::subscriber::{Interests, SubscriberBuilder},
};

const KEY_CANARY: &str = "trace-canary-key-9f3c";
const PLAIN_CANARY: &str = "trace-canary-plain-7a1d";
const KEY_JMAP_CANARY: &str = "trace-canary-jmap-key-3b7e";
const PLAIN_JMAP_CANARY: &str = "trace-canary-jmap-plain-5c21";
const KEY_BAD_CANARY: &str = "trace-canary-jmap-key-bad-8d2f";
const PLAIN_BAD_CANARY: &str = "trace-canary-jmap-plain-bad-1e6a";
const KEY_UPLOAD_CANARY: &str = "trace-canary-upload-key-4a90";
const PLAIN_UPLOAD_CANARY: &str = "trace-canary-upload-plain-c2d7";
const SUBSCRIBER_ID: &str = "za-trace-test";
const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");

fn event(uid: &str, summary: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:{summary}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// calendar-query returning `calendar-data` for every event whose SUMMARY
/// matches `summary`.
fn query(summary: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><A:calendar-query xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><D:prop><A:calendar-data/></D:prop><A:filter><A:comp-filter name=\"VCALENDAR\"><A:comp-filter name=\"VEVENT\"><A:prop-filter name=\"SUMMARY\"><A:text-match>{summary}</A:text-match></A:prop-filter></A:comp-filter></A:comp-filter></A:filter></A:calendar-query>"
    )
}

/// Every string anywhere in an event's values, flattened.
fn strings(value: &trc::Value, out: &mut Vec<String>) {
    match value {
        trc::Value::String(s) => out.push(s.to_string()),
        trc::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// PUT one event into `collection`, then read it back with a REPORT whose
/// response carries the event's plaintext.
async fn put_and_report(client: &DummyWebDavClient, collection: &str, uid: &str, canary: &str) {
    client
        .request_with_headers(
            "PUT",
            &format!("{collection}{uid}.ics"),
            [CONTENT_TYPE],
            event(uid, canary),
        )
        .await
        .with_status(StatusCode::CREATED);
    let body = client
        .request_with_headers("REPORT", collection, [("depth", "1")], query(canary))
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(body.contains(canary), "{body}");
}

/// POST a `Core/echo` call carrying `canary` to `/jmap`; the echoed
/// response carries it back.
async fn jmap_echo(user: &str, secret: &str, canary: &str) {
    let response = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .post(format!("{SERVER_URL}/jmap"))
        .basic_auth(user, Some(secret))
        .json(&json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["Core/echo", {"canary": canary}, "c0"]],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(body.contains(canary), "{body}");
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
}

/// POST a truncated `Core/echo` call carrying `canary` to `/jmap`. The
/// parser rejects it, and its error echoes the body it could not parse.
async fn jmap_malformed(user: &str, secret: &str, canary: &str) {
    let response = http_client()
        .post(format!("{SERVER_URL}/jmap"))
        .basic_auth(user, Some(secret))
        .header("content-type", "application/json")
        .body(format!(
            "{{\"using\":[\"urn:ietf:params:jmap:core\"],\"methodCalls\":[[\"Core/echo\",{{\"canary\":\"{canary}\""
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Upload a `text/plain` blob carrying `canary` to `account_id`.
async fn jmap_upload(user: &str, secret: &str, account_id: &str, canary: &str) {
    let response = http_client()
        .post(format!("{SERVER_URL}/jmap/upload/{account_id}/"))
        .basic_auth(user, Some(secret))
        .header("content-type", "text/plain")
        .body(format!("upload {canary}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access HTTP trace tests...");
    let key1 = test.account("key1@example.com").clone();
    let key_client = DummyWebDavClient::new(
        key1.id().document_id(),
        "key1@example.com",
        STRONG,
        "key1@example.com",
    );
    let plain = test.account("plain@example.com").clone();
    // `plain` has no email address, so `webdav_client()` cannot be used.
    let plain_client = DummyWebDavClient::new(
        plain.id().document_id(),
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    let key_cal = "/dav/cal/key1%40example.com/default/";
    let plain_cal = "/dav/cal/plain%40example.com/tracing/";
    plain_client
        .request(
            "MKCALENDAR",
            plain_cal,
            "<?xml version=\"1.0\" encoding=\"utf-8\" ?><A:mkcalendar xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"/>",
        )
        .await
        .with_status(StatusCode::CREATED);

    // The test server configures no tracer, so the two Trace-level events
    // are not emitted at all until something declares interest in them.
    // `jmap.not-request` carries the body the JMAP parser rejected.
    let traced = [
        EventType::Http(HttpEvent::RequestBody),
        EventType::Http(HttpEvent::ResponseBody),
        EventType::Jmap(JmapEvent::NotRequest),
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

    put_and_report(&key_client, key_cal, "trace-key", KEY_CANARY).await;
    put_and_report(&plain_client, plain_cal, "trace-plain", PLAIN_CANARY).await;
    // JMAP: a key account's request body can carry its password (an
    // `x:AccountPassword/set`), so neither side of its exchange is traced.
    jmap_echo("key1@example.com", STRONG, KEY_JMAP_CANARY).await;
    jmap_echo(plain.name(), plain.secret(), PLAIN_JMAP_CANARY).await;
    // A malformed or truncated body is echoed by the parser's error.
    jmap_malformed("key1@example.com", STRONG, KEY_BAD_CANARY).await;
    jmap_malformed(plain.name(), plain.secret(), PLAIN_BAD_CANARY).await;
    jmap_upload(
        "key1@example.com",
        STRONG,
        key1.id_string(),
        KEY_UPLOAD_CANARY,
    )
    .await;
    jmap_upload(
        plain.name(),
        plain.secret(),
        plain.id_string(),
        PLAIN_UPLOAD_CANARY,
    )
    .await;
    // A login through `/api/auth` carries the key account's password in its
    // body; it is traced before any verification, whatever the outcome.
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .post(format!("{SERVER_URL}/api/auth"))
        .json(&json!({
            "type": "authCode",
            "accountName": "key1@example.com",
            "accountSecret": STRONG,
            "clientId": "za-trace-test",
        }))
        .send()
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let mut seen = Vec::new();
    while let Ok(batch) = rx.try_recv() {
        for event in batch {
            let mut values = Vec::new();
            for (_, value) in event.keys.iter() {
                strings(value, &mut values);
            }
            seen.push(values.join("\n"));
        }
    }
    Collector::remove_subscriber(SUBSCRIBER_ID.into());
    Collector::reload();
    let all = seen.join("\n");
    // Positive control: the subscriber works and plain traffic is traced.
    assert!(
        all.contains(PLAIN_CANARY),
        "no plain-account trace captured: {all}"
    );
    assert!(
        all.contains(PLAIN_JMAP_CANARY),
        "no plain-account JMAP trace captured: {all}"
    );
    assert!(
        all.contains(PLAIN_BAD_CANARY),
        "no plain-account malformed JMAP trace captured: {all}"
    );
    assert!(
        all.contains(PLAIN_UPLOAD_CANARY),
        "no plain-account upload trace captured: {all}"
    );
    // Key-account traffic is absent on both sides.
    assert!(!all.contains(KEY_CANARY), "key-account body traced: {all}");
    assert!(
        !all.contains(KEY_JMAP_CANARY),
        "key-account JMAP body traced: {all}"
    );
    assert!(
        !all.contains(KEY_BAD_CANARY),
        "key-account malformed JMAP body traced: {all}"
    );
    assert!(
        !all.contains(KEY_UPLOAD_CANARY),
        "key-account upload traced: {all}"
    );
    // The key account's password never reaches the trace.
    assert!(!all.contains(STRONG), "login body traced: {all}");
    // Credentials are never traced, for any account.
    assert!(
        !all.to_ascii_lowercase().contains("basic ") && !all.contains("Bearer "),
        "credential header traced: {all}"
    );
    assert!(
        all.contains("[redacted]"),
        "redaction marker missing: {all}"
    );

    test.wait_for_tasks().await;
    test.blob_expire_all().await;
    key_client
        .request("DELETE", &format!("{key_cal}trace-key.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    plain_client
        .request("DELETE", plain_cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}
