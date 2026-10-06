/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::{account::Account, http::HttpRequest, server::TestServer};
use http::auth::authenticate::za_test::{self, Pause};
use hyper::{Method, StatusCode};
use serde::Serialize;
use serde_json::{Value, json};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::task::JoinHandle;

/// The suite's HTTPS listener.
pub const SERVER_URL: &str = "https://127.0.0.1:8899";

pub struct VaultReply {
    pub status: u16,
    pub body: Value,
}

impl VaultReply {
    pub fn expect(self, status: u16) -> Value {
        assert_eq!(
            self.status, status,
            "unexpected status, body: {}",
            self.body
        );
        self.body
    }

    pub fn str(self, status: u16, field: &str) -> String {
        self.expect(status)[field]
            .as_str()
            .unwrap_or_else(|| panic!("missing field {field}"))
            .to_string()
    }
}

/// Unauthenticated POST to `/api/vault/<path>`.
pub async fn za_post(path: &str, body: &impl Serialize) -> VaultReply {
    za_post_with(HttpRequest::new(), path, body).await
}

/// POST to `/api/vault/<path>` with Basic credentials of `account`.
pub async fn za_post_as(account: &Account, path: &str, body: &impl Serialize) -> VaultReply {
    za_post_with(
        HttpRequest::with_credentials(account.http_listener_port, account.name(), account.secret()),
        path,
        body,
    )
    .await
}

async fn za_post_with(request: HttpRequest, path: &str, body: &impl Serialize) -> VaultReply {
    let response = request
        .send_full(
            Method::POST,
            &format!("/api/vault/{path}"),
            Some(serde_json::to_vec(body).unwrap()),
            Some("application/json"),
        )
        .await;
    VaultReply {
        status: response.status.as_u16(),
        body: serde_json::from_str(&response.body).unwrap_or(Value::Null),
    }
}

/// Unauthenticated POST from `remote_ip` (via `X-Forwarded-For`; the za
/// suite enables forwarded addresses), for fail2ban tests that must not ban
/// the loopback address the rest of the suite uses.
pub async fn za_post_from(remote_ip: &str, path: &str, body: &impl Serialize) -> VaultReply {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .post(format!("{SERVER_URL}/api/vault/{path}"))
        .header("X-Forwarded-For", remote_ip)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(body).unwrap())
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    VaultReply {
        status,
        body: serde_json::from_str(&body).unwrap_or(Value::Null),
    }
}

/// Unauthenticated POSTs to `/api/vault/<path>`, one per body, issued at
/// once. Each client opens its TLS connection beforehand and sends from its
/// own task, so client setup does not stagger the requests; each comes from
/// its own forwarded address (`10.0.8.<n>`), so they do not contend on one
/// anonymous rate-limit counter (a conflicting RocksDB commit backs off for
/// up to 300 ms). Arrival order is still not guaranteed.
pub async fn za_post_concurrent(path: &str, bodies: &[Value]) -> Vec<VaultReply> {
    let barrier = Arc::new(tokio::sync::Barrier::new(bodies.len()));
    let mut tasks = Vec::with_capacity(bodies.len());
    for (i, body) in bodies.iter().enumerate() {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        // Read to the end so the connection returns to the pool for reuse.
        let warm_up = client
            .get(format!("{SERVER_URL}/healthz/live"))
            .send()
            .await
            .unwrap();
        assert_eq!(warm_up.status().as_u16(), 200);
        warm_up.bytes().await.unwrap();
        let url = format!("{SERVER_URL}/api/vault/{path}");
        let body = serde_json::to_vec(body).unwrap();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            let response = client
                .post(url)
                .header(hyper::header::CONTENT_TYPE, "application/json")
                .header("X-Forwarded-For", format!("10.0.8.{}", i + 1))
                .body(body)
                .send()
                .await
                .unwrap();
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            VaultReply {
                status,
                body: serde_json::from_str(&body).unwrap_or(Value::Null),
            }
        }));
    }
    let mut replies = Vec::with_capacity(tasks.len());
    for task in tasks {
        replies.push(task.await.unwrap());
    }
    replies
}

pub async fn za_setup_token(admin: &Account, account: &str) -> String {
    za_post_as(admin, "setup-token", &json!({ "account": account }))
        .await
        .str(200, "token")
}

/// Completes setup and returns the recovery key.
pub async fn za_setup(username: &str, token: &str, password: &str) -> String {
    za_post(
        "setup",
        &json!({ "username": username, "token": token, "password": password }),
    )
    .await
    .str(200, "recovery_key")
}

/// PROPFIND on the calendar home. Requests expected to fail are sent from
/// `fail_ip`, so a module's deliberate failures do not consume the loopback
/// address's fail2ban budget.
pub async fn caldav_from(fail_ip: &str, name: &str, secret: &str, status: StatusCode) {
    let mut request = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .request(
            Method::from_bytes(b"PROPFIND").unwrap(),
            format!("{SERVER_URL}/dav/cal/{name}/"),
        )
        .basic_auth(name, Some(secret));
    if !status.is_success() {
        request = request.header("X-Forwarded-For", fail_ip);
    }
    let response = request.send().await.unwrap();
    assert_eq!(
        response.status().as_u16(),
        status.as_u16(),
        "PROPFIND as {name}"
    );
}

pub fn assert_nothing_cached(test: &TestServer, account_id: u32) {
    assert!(
        !test
            .server
            .inner
            .cache
            .http_auth
            .inner()
            .iter()
            .any(|(_, v)| v.account_id == account_id),
        "no cached authentication for the account"
    );
    assert!(
        !test.server.inner.cache.za_keys.contains_account(account_id),
        "no resident keys for the account"
    );
}

/// A request held at a test-mode pause point. `set` names the slot:
/// `za_test::set` (after verification, before the cache-insert fence),
/// `za_test::set_endpoint` (vault endpoints, after verification, before the
/// record re-read) or `za_test::set_publish` (`app-password`, between the
/// Pending wrap and the registry credential). Dropping it clears that
/// process-global slot and releases the request, also on a failed assertion.
pub struct Parked<T> {
    pause: Arc<Pause>,
    clear: fn(Option<Arc<Pause>>),
    handle: Option<JoinHandle<T>>,
}

impl<T: Send + 'static> Parked<T> {
    pub async fn start(
        account_id: u32,
        set: fn(Option<Arc<Pause>>),
        request: impl Future<Output = T> + Send + 'static,
    ) -> Self {
        let pause = Arc::new(Pause::new(account_id));
        set(Some(pause.clone()));
        let mut parked = Parked {
            pause: pause.clone(),
            clear: set,
            handle: None,
        };
        parked.handle = Some(tokio::spawn(request));
        tokio::time::timeout(Duration::from_secs(5), pause.arrived.notified())
            .await
            .expect("the request did not reach the pause point");
        parked
    }

    pub async fn finish(mut self) -> T {
        self.pause.release.notify_one();
        self.handle.take().unwrap().await.unwrap()
    }
}

impl<T> Drop for Parked<T> {
    fn drop(&mut self) {
        (self.clear)(None);
        self.pause.release.notify_one();
    }
}

/// A CalDAV login parked after its credentials were verified. The harness
/// client times out after 5 s, long enough to stay parked.
pub async fn park_login(account_id: u32, name: &str, secret: &str) -> Parked<u16> {
    let request = HttpRequest::with_credentials(8899, name, secret);
    let path = format!("/dav/cal/{name}/");
    Parked::start(account_id, za_test::set, async move {
        request
            .send_full(Method::from_bytes(b"PROPFIND").unwrap(), &path, None, None)
            .await
            .status
            .as_u16()
    })
    .await
}

/// A vault request parked between its fresh verification and the re-read.
pub async fn park_endpoint(account_id: u32, path: &'static str, body: Value) -> Parked<VaultReply> {
    Parked::start(account_id, za_test::set_endpoint, async move {
        za_post(path, &body).await
    })
    .await
}
