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
use base64::{Engine, engine::general_purpose::STANDARD};
use hyper::StatusCode;
use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_rustls::{
    TlsConnector,
    rustls::{
        self, DigitallySignedStruct, SignatureScheme,
        client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        pki_types::{CertificateDer, ServerName, UnixTime},
    },
};
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
const KEY_WS_CANARY: &str = "trace-canary-ws-key-6f1b";
const PLAIN_WS_CANARY: &str = "trace-canary-ws-plain-2d8c";
const KEY_ARGS_CANARY: &str = "trace-canary-args-key-7e4d";
const PLAIN_ARGS_CANARY: &str = "trace-canary-args-plain-9b02";
const KEY_WS_ARGS_CANARY: &str = "trace-canary-ws-args-key-5a6c";
const PLAIN_WS_ARGS_CANARY: &str = "trace-canary-ws-args-plain-0d13";
const KEY_METHOD_CANARY: &str = "trace-canary-method-key-2c8e";
const PLAIN_METHOD_CANARY: &str = "trace-canary-method-plain-6f41";
const KEY_REF_CANARY: &str = "trace-canary-ref-key-4d07";
const PLAIN_REF_CANARY: &str = "trace-canary-ref-plain-8a35";
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

/// Accepts the test server's self-signed certificate.
#[derive(Debug)]
struct AcceptAnyCert(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// Write one masked client frame (RFC 6455 section 5.2).
async fn ws_send(stream: &mut (impl AsyncWriteExt + Unpin), opcode: u8, payload: &[u8]) {
    let mask = [0x5a, 0x17, 0xc3, 0x8e];
    let mut frame = vec![0x80 | opcode];
    match payload.len() {
        len @ 0..=125 => frame.push(0x80 | len as u8),
        len @ 126..=0xffff => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        }
        len => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    stream.write_all(&frame).await.unwrap();
    stream.flush().await.unwrap();
}

/// Read one unmasked server frame: its opcode and payload, or `None` once
/// the server has hung up.
async fn ws_recv(stream: &mut (impl AsyncReadExt + Unpin)) -> Option<(u8, Vec<u8>)> {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).await.ok()?;
    let len = match head[1] & 0x7f {
        126 => stream.read_u16().await.ok()? as usize,
        127 => stream.read_u64().await.ok()? as usize,
        len => len as usize,
    };
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).await.ok()?;
    Some((head[0] & 0x0f, payload))
}

/// Open a JMAP WebSocket, send `message` as one text frame, return the
/// server's text reply and close the socket. A minimal client, since
/// `jmap_client` cannot send a malformed message.
async fn ws_exchange(user: &str, secret: &str, message: &str) -> String {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCert(provider)))
        .with_no_client_auth();
    let tcp = tokio::net::TcpStream::connect("127.0.0.1:8899")
        .await
        .unwrap();
    let tls = TlsConnector::from(Arc::new(config))
        .connect(ServerName::try_from("127.0.0.1").unwrap(), tcp)
        .await
        .unwrap();
    let mut stream = BufReader::new(tls);
    let auth = STANDARD.encode(format!("{user}:{secret}"));
    stream
        .write_all(
            format!(
                "GET /jmap/ws HTTP/1.1\r\nHost: 127.0.0.1:8899\r\nAuthorization: Basic {auth}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: jmap\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    stream.flush().await.unwrap();
    let mut status = String::new();
    stream.read_line(&mut status).await.unwrap();
    assert!(status.starts_with("HTTP/1.1 101"), "{status}");
    loop {
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        if line == "\r\n" {
            break;
        }
    }

    ws_send(&mut stream, 0x1, message.as_bytes()).await;
    let reply = loop {
        match ws_recv(&mut stream)
            .await
            .expect("server hung up before replying")
        {
            (0x1, payload) => break String::from_utf8(payload).unwrap(),
            (0x9, payload) => ws_send(&mut stream, 0xa, &payload).await,
            (opcode, _) => panic!("unexpected WebSocket frame {opcode:#x}"),
        }
    };

    // Close handshake: wait for the server's close frame, then hang up.
    ws_send(&mut stream, 0x8, &[]).await;
    while let Some((opcode, _)) = ws_recv(&mut stream).await {
        if opcode == 0x8 {
            break;
        }
    }
    let _ = stream.shutdown().await;
    reply
}

/// Send a WebSocket JMAP request whose `methodCalls` is a string carrying
/// `canary`. The parser rejects it, and its error echoes the string.
async fn ws_malformed(user: &str, secret: &str, canary: &str) -> String {
    let reply = ws_exchange(
        user,
        secret,
        &format!(
            "{{\"@type\":\"Request\",\"id\":\"1\",\"using\":[\"urn:ietf:params:jmap:core\"],\"methodCalls\":\"{canary}\"}}"
        ),
    )
    .await;
    assert!(
        reply.contains("RequestError") && reply.contains("notRequest"),
        "{reply}"
    );
    reply
}

/// A well-formed request with one `Email/get` call whose `ids` is a string
/// carrying `canary`. The call's `invalidArguments` error echoes it.
fn bad_arguments(canary: &str) -> serde_json::Value {
    json!({
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [["Email/get", {"ids": canary}, "c0"]],
    })
}

/// POST `bad_arguments` to `/jmap`; the client keeps the detailed error.
async fn jmap_bad_arguments(user: &str, secret: &str, canary: &str) {
    let response = http_client()
        .post(format!("{SERVER_URL}/jmap"))
        .basic_auth(user, Some(secret))
        .json(&bad_arguments(canary))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(
        body.contains(canary) && body.contains("invalidArguments"),
        "{body}"
    );
}

/// POST a well-formed request with the single method call `call`, which
/// fails with `error_type` and a description echoing `canary`; the client
/// keeps the detailed error.
async fn jmap_call_error(
    user: &str,
    secret: &str,
    call: serde_json::Value,
    error_type: &str,
    canary: &str,
) {
    let response = http_client()
        .post(format!("{SERVER_URL}/jmap"))
        .basic_auth(user, Some(secret))
        .json(&json!({
            "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
            "methodCalls": [call],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.unwrap();
    assert!(body.contains(canary) && body.contains(error_type), "{body}");
}

/// An unknown method whose name carries `canary`.
async fn jmap_unknown_method(user: &str, secret: &str, canary: &str) {
    let call = json!([format!("Email/{canary}"), {}, "c0"]);
    jmap_call_error(user, secret, call, "unknownMethod", canary).await;
}

/// A result reference to a call id, `canary`, that is not in the request.
async fn jmap_bad_reference(user: &str, secret: &str, canary: &str) {
    let call = json!([
        "Email/get",
        {"#ids": {"resultOf": canary, "name": "Email/query", "path": "/ids"}},
        "c0"
    ]);
    jmap_call_error(user, secret, call, "invalidResultReference", canary).await;
}

/// Send `bad_arguments` over a WebSocket; the client keeps the detailed
/// error in the `Response` frame.
async fn ws_bad_arguments(user: &str, secret: &str, canary: &str) {
    let mut message = bad_arguments(canary);
    message["@type"] = json!("Request");
    message["id"] = json!("1");
    let reply = ws_exchange(user, secret, &message.to_string()).await;
    assert!(
        reply.contains("\"Response\"")
            && reply.contains(canary)
            && reply.contains("invalidArguments"),
        "{reply}"
    );
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
    // `jmap.not-request` carries the body the JMAP parser rejected, and
    // `jmap.invalid-arguments` a method call's rejected arguments.
    let traced = [
        EventType::Http(HttpEvent::RequestBody),
        EventType::Http(HttpEvent::ResponseBody),
        EventType::Jmap(JmapEvent::NotRequest),
        EventType::Jmap(JmapEvent::InvalidArguments),
        EventType::Jmap(JmapEvent::UnknownMethod),
        EventType::Jmap(JmapEvent::InvalidResultReference),
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
    // The same over a WebSocket: the error frame and the trace both carry
    // the parser's error.
    let key_ws_reply = ws_malformed("key1@example.com", STRONG, KEY_WS_CANARY).await;
    let plain_ws_reply = ws_malformed(plain.name(), plain.secret(), PLAIN_WS_CANARY).await;
    // A well-formed request whose method arguments do not parse: the
    // per-method error echoes them, over HTTP and over a WebSocket.
    jmap_bad_arguments("key1@example.com", STRONG, KEY_ARGS_CANARY).await;
    jmap_bad_arguments(plain.name(), plain.secret(), PLAIN_ARGS_CANARY).await;
    ws_bad_arguments("key1@example.com", STRONG, KEY_WS_ARGS_CANARY).await;
    ws_bad_arguments(plain.name(), plain.secret(), PLAIN_WS_ARGS_CANARY).await;
    // An unknown method name and a dangling result reference are echoed
    // the same way.
    jmap_unknown_method("key1@example.com", STRONG, KEY_METHOD_CANARY).await;
    jmap_unknown_method(plain.name(), plain.secret(), PLAIN_METHOD_CANARY).await;
    jmap_bad_reference("key1@example.com", STRONG, KEY_REF_CANARY).await;
    jmap_bad_reference(plain.name(), plain.secret(), PLAIN_REF_CANARY).await;
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
    let mut typed = Vec::new();
    while let Ok(batch) = rx.try_recv() {
        for event in batch {
            let mut values = Vec::new();
            for (_, value) in event.keys.iter() {
                strings(value, &mut values);
            }
            let values = values.join("\n");
            typed.push((event.inner.typ, values.clone()));
            seen.push(values);
        }
    }
    Collector::remove_subscriber(SUBSCRIBER_ID.into());
    Collector::reload();
    let all = seen.join("\n");
    // Every string of the captured events of one type.
    let traced_as = |event: JmapEvent| {
        typed
            .iter()
            .filter(|(typ, _)| *typ == EventType::Jmap(event))
            .map(|(_, values)| values.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let invalid_arguments = traced_as(JmapEvent::InvalidArguments);
    let unknown_method = traced_as(JmapEvent::UnknownMethod);
    let result_reference = traced_as(JmapEvent::InvalidResultReference);
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
    assert!(
        all.contains(PLAIN_WS_CANARY),
        "no plain-account malformed WebSocket trace captured: {all}"
    );
    assert!(
        plain_ws_reply.contains(PLAIN_WS_CANARY),
        "plain-account WebSocket error frame lost the parser's error: {plain_ws_reply}"
    );
    // The plain account's request body is traced too, so look for its
    // method-error canaries in the method-error events themselves.
    assert!(
        invalid_arguments.contains(PLAIN_ARGS_CANARY),
        "no plain-account invalid-arguments trace captured: {invalid_arguments}"
    );
    assert!(
        invalid_arguments.contains(PLAIN_WS_ARGS_CANARY),
        "no plain-account WebSocket invalid-arguments trace captured: {invalid_arguments}"
    );
    assert!(
        unknown_method.contains(PLAIN_METHOD_CANARY),
        "no plain-account unknown-method trace captured: {unknown_method}"
    );
    assert!(
        result_reference.contains(PLAIN_REF_CANARY),
        "no plain-account invalid-result-reference trace captured: {result_reference}"
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
    assert!(
        !all.contains(KEY_WS_CANARY),
        "key-account malformed WebSocket message traced: {all}"
    );
    assert!(
        !key_ws_reply.contains(KEY_WS_CANARY),
        "key-account WebSocket error frame echoes the message: {key_ws_reply}"
    );
    assert!(
        !all.contains(KEY_ARGS_CANARY),
        "key-account method arguments traced: {all}"
    );
    assert!(
        !all.contains(KEY_WS_ARGS_CANARY),
        "key-account WebSocket method arguments traced: {all}"
    );
    assert!(
        !all.contains(KEY_METHOD_CANARY),
        "key-account unknown method name traced: {all}"
    );
    assert!(
        !all.contains(KEY_REF_CANARY),
        "key-account result reference traced: {all}"
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
