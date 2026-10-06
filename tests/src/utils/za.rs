/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::{account::Account, http::HttpRequest};
use hyper::Method;
use serde::Serialize;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

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
