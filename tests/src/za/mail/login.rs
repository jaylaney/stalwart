/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 3.3 and 4: an IMAP or POP3 session of a key account holds the
//! account's session keys for the connection, from the login until LOGOUT,
//! UNAUTHENTICATE, QUIT or the connection's end, across STARTTLS; a mail
//! login that yields no keys for a key account (OAuth Bearer, master user)
//! is refused with `NO [NOPERM]` and a fixed text, the connection kept open.

use super::{deliver, key_mail_account, live_keys, message_with};
use crate::{
    utils::{
        imap::{ImapConnection, Type},
        pop3::{Pop3Connection, ResponseType as Pop3Reply},
        server::TestServer,
        za::{bearer_token, wait_for_delivery, za_app_password, za_post},
    },
    za::{
        STRONG,
        raw_input::Conn,
        totp::{code, totp_url},
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{auth::credential::AppPassword, za::ZA_MAIL_LOGIN_REFUSED};
use imap_proto::ResponseType;
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

const NAME: &str = "keymail1@example.com";
const NEW: &str = "a different long passphrase with 4 numbers";
const IMAP_PLAIN: &str = "127.0.0.1:9991";
const IMAP_TLS_PORT: u16 = 9992;
/// The master user: the suite's admin, which may impersonate.
const MASTER: &str = "admin@example.com";
const MASTER_SECRET: &str = "these_pretzels_are_making_me_thirsty";
const BODY_CANARY: &str = "rf-4 body canary 5d1e";
/// How long a session's end may take to drop its keys.
const LOGOUT_DROP: Duration = Duration::from_millis(200);
const CLOSE_DROP: Duration = Duration::from_secs(2);

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mail login tests...");
    let account = key_mail_account(test, NAME, STRONG).await;
    let id = account.id().document_id();
    let app = za_app_password(NAME, STRONG, "Mail client").await;

    password_login_holds_keys(test, id).await;
    app_password_login_holds_keys(test, id, &app).await;
    dropped_connection_drops_keys(test, id).await;
    two_sessions_are_independent(test, id).await;
    pop3_logins(test, id, &app).await;
    keyless_mechanisms_are_refused(test, id).await;
    totp_account_needs_an_app_password(test, id, &app).await;
    starttls_keeps_keys(test, id).await;
    rf_4_password_change_mid_session(test).await;

    let credential_id = AppPassword::parse(&app).unwrap().credential_id;
    za_post(
        "app-password/revoke",
        &json!({ "username": NAME, "password": STRONG, "credential_id": credential_id }),
    )
    .await
    .expect(200);
    test.destroy_all_mailboxes(&account).await;
    // The control's IMAP login created the ordinary account's mailboxes.
    test.destroy_all_mailboxes(test.account("plain@example.com"))
        .await;
    wait_for_delivery(test).await;
}

/// Controller ruling R6: an HTTP login legitimately leaves the account's keys
/// in the key cache, so each measurement drops that cache's entries first
/// and counts relative to what is left.
fn baseline(test: &TestServer, id: u32) -> usize {
    let caches = &test.server.inner.cache;
    caches.za_keys.remove_account(id);
    caches.http_auth.clear();
    live_keys(id)
}

/// The live count reaches `expected` within `within`.
async fn assert_keys_within(id: u32, expected: usize, within: Duration, what: &str) {
    let deadline = Instant::now() + within;
    while live_keys(id) != expected && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(live_keys(id), expected, "{what}");
}

/// The SASL PLAIN message of `user` and `secret`, base64-encoded.
fn plain_sasl(user: &str, secret: &str) -> String {
    STANDARD.encode(format!("\0{user}\0{secret}"))
}

/// An IMAP AUTHENTICATE PLAIN answered NO: the tagged line.
async fn imap_refused(imap: &mut ImapConnection, user: &str, secret: &str) -> String {
    imap.send(&format!("AUTHENTICATE PLAIN {}", plain_sasl(user, secret)))
        .await;
    imap.assert_read(Type::Tagged, ResponseType::No)
        .await
        .pop()
        .unwrap()
}

/// The `BODY[]` literal of a FETCH response, cut by its announced length
/// (the reader strips each line's CRLF; the message's lines end in CRLF).
fn body_literal(lines: &[String]) -> String {
    let text = lines.join("\r\n");
    let open = text.find("BODY[] {").expect("a BODY[] literal") + "BODY[] {".len();
    let close = open + text[open..].find('}').unwrap();
    let len: usize = text[open..close].parse().unwrap();
    let start = close + "}\r\n".len();
    text[start..start + len].to_string()
}

async fn password_login_holds_keys(test: &TestServer, id: u32) {
    println!("password_login_holds_keys");
    let base = baseline(test, id);
    let mut imap = ImapConnection::connect(b"_m ").await;
    imap.authenticate(NAME, STRONG).await;
    assert_eq!(live_keys(id), base + 1, "a password login holds the keys");
    imap.send_ok("SELECT INBOX").await;
    imap.send_ok("LOGOUT").await;
    assert_keys_within(id, base, LOGOUT_DROP, "LOGOUT drops the keys").await;
}

async fn app_password_login_holds_keys(test: &TestServer, id: u32, app: &str) {
    println!("app_password_login_holds_keys");
    let base = baseline(test, id);
    let mut imap = ImapConnection::connect(b"_m ").await;
    imap.authenticate(NAME, app).await;
    assert_eq!(
        live_keys(id),
        base + 1,
        "an app-password login holds the keys"
    );
    imap.send_ok("UNAUTHENTICATE").await;
    assert_keys_within(id, base, LOGOUT_DROP, "UNAUTHENTICATE drops the keys").await;
    imap.authenticate(NAME, app).await;
    assert_eq!(
        live_keys(id),
        base + 1,
        "a second login on the connection holds them again"
    );
    imap.send_ok("LOGOUT").await;
    assert_keys_within(id, base, LOGOUT_DROP, "LOGOUT drops the keys").await;
}

async fn dropped_connection_drops_keys(test: &TestServer, id: u32) {
    println!("dropped_connection_drops_keys");
    let base = baseline(test, id);
    let mut imap = ImapConnection::connect(b"_m ").await;
    imap.authenticate(NAME, STRONG).await;
    assert_eq!(live_keys(id), base + 1);
    drop(imap);
    assert_keys_within(
        id,
        base,
        CLOSE_DROP,
        "a connection closed without LOGOUT drops the keys",
    )
    .await;
}

async fn two_sessions_are_independent(test: &TestServer, id: u32) {
    println!("two_sessions_are_independent");
    let base = baseline(test, id);
    let mut first = ImapConnection::connect(b"_a ").await;
    first.authenticate(NAME, STRONG).await;
    let mut second = ImapConnection::connect(b"_b ").await;
    second.authenticate(NAME, STRONG).await;
    assert_eq!(live_keys(id), base + 2, "each login derives its own keys");
    first.send_ok("LOGOUT").await;
    assert_keys_within(
        id,
        base + 1,
        LOGOUT_DROP,
        "one LOGOUT drops one session's keys",
    )
    .await;
    second.send_ok("SELECT INBOX").await;
    second.send_ok("FETCH 1:* FLAGS").await;
    second.send_ok("LOGOUT").await;
    assert_keys_within(id, base, LOGOUT_DROP, "LOGOUT drops the keys").await;
}

async fn pop3_logins(test: &TestServer, id: u32, app: &str) {
    println!("pop3_logins");
    let base = baseline(test, id);
    for secret in [STRONG, app] {
        let mut pop3 = Pop3Connection::connect().await;
        pop3.authenticate(NAME, secret).await;
        pop3.send("STAT").await;
        pop3.assert_read(Pop3Reply::Ok).await;
        assert_eq!(live_keys(id), base + 1, "a POP3 login holds the keys");
        pop3.send("QUIT").await;
        pop3.assert_read(Pop3Reply::Ok).await;
        assert_keys_within(id, base, LOGOUT_DROP, "QUIT drops the keys").await;
    }
}

async fn keyless_mechanisms_are_refused(test: &TestServer, id: u32) {
    println!("keyless_mechanisms_are_refused");
    let base = baseline(test, id);
    let token = bearer_token(NAME, STRONG).await;
    let oauthbearer = STANDARD.encode(format!("n,a={NAME},\u{1}auth=Bearer {token}\u{1}\u{1}"));
    let xoauth2 = STANDARD.encode(format!("user={NAME}\u{1}auth=Bearer {token}\u{1}\u{1}"));
    let master = plain_sasl(&format!("{NAME}%{MASTER}"), MASTER_SECRET);

    let mut imap = Conn::open(IMAP_TLS_PORT).await;
    imap.until(&["* OK"]).await;
    for (mechanism, response) in [
        ("OAUTHBEARER", &oauthbearer),
        ("XOAUTH2", &xoauth2),
        ("PLAIN", &master),
    ] {
        imap.send(&format!("k1 AUTHENTICATE {mechanism} {response}\r\n"))
            .await;
        let line = imap.until(&["k1 "]).await;
        assert_eq!(
            line.trim_end(),
            format!("k1 NO [NOPERM] {ZA_MAIL_LOGIN_REFUSED}"),
            "{mechanism}"
        );
        imap.exchange("k2 NOOP\r\n", "k2 OK").await;
        assert_eq!(
            live_keys(id),
            base,
            "{mechanism}: no keys after the refusal"
        );
    }
    imap.exchange("k3 LOGOUT\r\n", "k3 OK").await;

    let mut pop3 = Pop3Connection::connect().await;
    pop3.send(&format!("AUTH XOAUTH2 {xoauth2}")).await;
    let line = pop3.assert_read(Pop3Reply::Err).await.pop().unwrap();
    assert_eq!(line, format!("-ERR {ZA_MAIL_LOGIN_REFUSED}"));
    assert_eq!(live_keys(id), base, "POP3: no keys after the refusal");
    pop3.send("QUIT").await;
    pop3.assert_read(Pop3Reply::Ok).await;

    // Control: an ordinary account's Bearer login takes upstream's path.
    let plain = test.account("plain@example.com");
    let token = bearer_token(plain.name(), plain.secret()).await;
    let oauthbearer = STANDARD.encode(format!(
        "n,a={},\u{1}auth=Bearer {token}\u{1}\u{1}",
        plain.name()
    ));
    let mut imap = Conn::open(IMAP_TLS_PORT).await;
    imap.until(&["* OK"]).await;
    imap.exchange(
        &format!("p1 AUTHENTICATE OAUTHBEARER {oauthbearer}\r\n"),
        "p1 OK",
    )
    .await;
    imap.exchange("p2 LOGOUT\r\n", "p2 OK").await;
}

async fn totp_account_needs_an_app_password(test: &TestServer, id: u32, app: &str) {
    println!("totp_account_needs_an_app_password");
    let url = totp_url();
    za_post(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "otp_auth": url, "confirm": code(&url) }),
    )
    .await
    .expect(200);
    let base = baseline(test, id);
    // Upstream's client text for a password login without the code.
    let mfa_required = trc::EventType::Auth(trc::AuthEvent::MfaRequired).message();

    let mut imap = ImapConnection::connect(b"_t ").await;
    let refused = imap_refused(&mut imap, NAME, STRONG).await;
    assert_eq!(
        refused,
        format!("_t NO [AUTHENTICATIONFAILED] {mfa_required}")
    );
    assert_eq!(live_keys(id), base);
    imap.authenticate(NAME, app).await;
    assert_eq!(
        live_keys(id),
        base + 1,
        "the app password logs in, with keys"
    );
    imap.send_ok("LOGOUT").await;
    assert_keys_within(id, base, LOGOUT_DROP, "LOGOUT drops the keys").await;

    let mut pop3 = Pop3Connection::connect().await;
    pop3.send(&format!("AUTH PLAIN {}", plain_sasl(NAME, STRONG)))
        .await;
    let refused = pop3.assert_read(Pop3Reply::Err).await.pop().unwrap();
    assert_eq!(refused, format!("-ERR {mfa_required}"));
    pop3.authenticate(NAME, app).await;
    assert_eq!(
        live_keys(id),
        base + 1,
        "the app password logs in, with keys"
    );
    pop3.send("QUIT").await;
    pop3.assert_read(Pop3Reply::Ok).await;
    assert_keys_within(id, base, LOGOUT_DROP, "QUIT drops the keys").await;

    // Removed here, as `totp::test` ends: the later steps log in with the
    // password.
    za_post(
        "totp",
        &json!({ "username": NAME, "password": STRONG, "totp": code(&url), "otp_auth": null }),
    )
    .await
    .expect(200);
}

/// Reads lines of a plain-text connection until one starts with `prefix`.
async fn plain_line(reader: &mut BufReader<TcpStream>, prefix: &str) -> String {
    loop {
        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {prefix:?}"))
            .unwrap();
        assert!(read > 0, "connection closed waiting for {prefix:?}");
        if line.starts_with(prefix) {
            return line;
        }
    }
}

async fn starttls_keeps_keys(test: &TestServer, id: u32) {
    println!("starttls_keeps_keys");
    let base = baseline(test, id);
    let mut plain = BufReader::new(TcpStream::connect(IMAP_PLAIN).await.unwrap());
    plain_line(&mut plain, "* OK").await;
    plain
        .get_mut()
        .write_all(format!("s1 AUTHENTICATE PLAIN {}\r\n", plain_sasl(NAME, STRONG)).as_bytes())
        .await
        .unwrap();
    let line = plain_line(&mut plain, "s1 ").await;
    assert!(line.starts_with("s1 OK"), "{line}");
    assert_eq!(live_keys(id), base + 1);
    plain.get_mut().write_all(b"s2 STARTTLS\r\n").await.unwrap();
    let line = plain_line(&mut plain, "s2 ").await;
    assert!(line.starts_with("s2 OK"), "{line}");
    assert!(
        plain.buffer().is_empty(),
        "nothing read past the STARTTLS answer"
    );

    let mut tls = Conn::upgrade(plain.into_inner()).await;
    tls.exchange("s3 SELECT INBOX\r\n", "s3 OK").await;
    tls.exchange("s4 FETCH 1:* FLAGS\r\n", "s4 OK").await;
    assert_eq!(
        live_keys(id),
        base + 1,
        "the keys follow the session into TLS"
    );
    tls.exchange("s5 LOGOUT\r\n", "s5 OK").await;
    assert_keys_within(id, base, LOGOUT_DROP, "LOGOUT drops the keys").await;
}

/// Mail spec 4 (RF-4): a password change ends no open session; the old
/// password no longer logs in and the new one reads the same message.
async fn rf_4_password_change_mid_session(test: &TestServer) {
    println!("rf_4_password_change_mid_session");
    deliver(
        test,
        "sender@example.org",
        NAME,
        &message_with("RF-4", BODY_CANARY),
    )
    .await;
    let mut first = ImapConnection::connect(b"_a ").await;
    first.authenticate(NAME, STRONG).await;
    first.send_ok("SELECT INBOX").await;

    za_post(
        "password",
        &json!({ "username": NAME, "password": STRONG, "new_password": NEW }),
    )
    .await
    .expect(200);
    first.send("FETCH 1 (BODY[])").await;
    let before = body_literal(&first.assert_read(Type::Tagged, ResponseType::Ok).await);
    assert!(before.contains(BODY_CANARY), "{before}");

    let mut second = ImapConnection::connect(b"_b ").await;
    let refused = imap_refused(&mut second, NAME, STRONG).await;
    assert!(
        refused.starts_with("_b NO [AUTHENTICATIONFAILED]"),
        "{refused}"
    );
    second.authenticate(NAME, NEW).await;
    second.send_ok("SELECT INBOX").await;
    second.send("FETCH 1 (BODY[])").await;
    let after = body_literal(&second.assert_read(Type::Tagged, ResponseType::Ok).await);
    assert_eq!(after, before, "the new password reads the same bytes");

    za_post(
        "password",
        &json!({ "username": NAME, "password": NEW, "new_password": STRONG }),
    )
    .await
    .expect(200);
    first.send_ok("LOGOUT").await;
    second.send_ok("LOGOUT").await;
}
