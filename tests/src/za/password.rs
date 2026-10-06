/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{
    http::HttpRequest,
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{VaultReply, za_post},
};
use common::{auth::credential::AppPassword, ipc::CacheInvalidation};
use http::auth::authenticate::za_test::{self, Pause};
use hyper::{Method, StatusCode};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::task::JoinHandle;
use vault::recovery::RecoveryKey;

const STRONG2: &str = "another long passphrase with 2 numbers";
const STRONG3: &str = "yet another passphrase, number 3 of them";

async fn caldav(id: u32, name: &'static str, secret: &'static str, status: StatusCode) {
    DummyWebDavClient::new(id, name, secret, name)
        .request("PROPFIND", &format!("/dav/cal/{name}/"), "")
        .await
        .with_status(status);
}

fn cache_sizes(test: &TestServer) -> (usize, usize) {
    (
        test.server.inner.cache.http_auth.inner().len(),
        test.server.inner.cache.za_keys.len(),
    )
}

/// Unauthenticated POSTs to `/api/vault/<path>`, one per body, issued at
/// once so that all of them verify the same generation. Each client opens
/// its TLS connection beforehand and sends from its own task, so client
/// setup does not stagger the requests; each comes from its own forwarded
/// address, so they do not contend on one anonymous rate-limit counter
/// (a conflicting RocksDB commit backs off for up to 300 ms).
async fn za_post_concurrent(path: &str, bodies: &[Value]) -> Vec<VaultReply> {
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
            .get("https://127.0.0.1:8899/healthz/live")
            .send()
            .await
            .unwrap();
        assert_eq!(warm_up.status().as_u16(), 200);
        warm_up.bytes().await.unwrap();
        let url = format!("https://127.0.0.1:8899/api/vault/{path}");
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

/// A CalDAV login held at the test-mode pause point, after its credentials
/// were verified and before the generation fence. Dropping it clears the
/// process-global pause and releases the request, also on a failed assertion.
struct PausedLogin {
    pause: Arc<Pause>,
    handle: Option<JoinHandle<u16>>,
}

impl PausedLogin {
    async fn start(account_id: u32, name: &str, secret: &str) -> Self {
        let pause = Arc::new(Pause::new(account_id));
        za_test::set(Some(pause.clone()));
        let mut paused = PausedLogin {
            pause: pause.clone(),
            handle: None,
        };
        // The harness client times out after 5 s, long enough to stay paused.
        let request = HttpRequest::with_credentials(8899, name, secret);
        let path = format!("/dav/cal/{name}/");
        paused.handle = Some(tokio::spawn(async move {
            request
                .send_full(Method::from_bytes(b"PROPFIND").unwrap(), &path, None, None)
                .await
                .status
                .as_u16()
        }));
        tokio::time::timeout(Duration::from_secs(5), pause.arrived.notified())
            .await
            .expect("the CalDAV login did not reach the pause point");
        paused
    }

    async fn finish(mut self) -> u16 {
        self.pause.release.notify_one();
        self.handle.take().unwrap().await.unwrap()
    }
}

impl Drop for PausedLogin {
    fn drop(&mut self) {
        za_test::set(None);
        self.pause.release.notify_one();
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access password tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let recovery = test.account(name).recovery_key.clone().unwrap();

    // Wrong password 401; app password and master separator 400 before any
    // verification (Review Focus 4 and 5).
    za_post(
        "password",
        &json!({ "username": name, "password": "nope nope nope", "new_password": STRONG2 }),
    )
    .await
    .expect(401);
    let app_password = AppPassword::new(1).build();
    assert!(AppPassword::parse(&app_password).is_some());
    for endpoint in ["password", "recovery-key"] {
        za_post(
            endpoint,
            &json!({ "username": name, "password": app_password, "new_password": STRONG2 }),
        )
        .await
        .expect(400);
        za_post(
            endpoint,
            &json!({ "username": "key1@example.com%admin@example.com", "password": STRONG, "new_password": STRONG2 }),
        )
        .await
        .expect(400);
    }
    // Not a key account.
    za_post(
        "password",
        &json!({ "username": "plain@example.com", "password": "plain secret with entropy 9", "new_password": STRONG2 }),
    )
    .await
    .expect(409);
    // Weak new password.
    za_post(
        "password",
        &json!({ "username": name, "password": STRONG, "new_password": "weak" }),
    )
    .await
    .expect(400);

    // Change then read: cached authentication for the old password is gone.
    caldav(id, name, STRONG, StatusCode::MULTI_STATUS).await;
    assert_eq!(test.server.inner.cache.za_keys.len(), 1);
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    za_post(
        "password",
        &json!({ "username": name, "password": STRONG, "new_password": STRONG2 }),
    )
    .await
    .expect(200);
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert_eq!(after.record.revision, before.record.revision + 1);
    assert_ne!(after.record.salt, before.record.salt);
    assert_ne!(after.record.password_wrap, before.record.password_wrap);
    assert_ne!(after.record.verifier_hash, before.record.verifier_hash);
    assert_eq!(
        after.record.recovery_wrap, before.record.recovery_wrap,
        "recovery wrap untouched"
    );
    assert_eq!(
        after.record.private_key_wrap, before.record.private_key_wrap,
        "master key unchanged"
    );
    assert_eq!(test.server.inner.cache.za_keys.len(), 0);
    caldav(id, name, STRONG, StatusCode::UNAUTHORIZED).await;
    caldav(id, name, STRONG2, StatusCode::MULTI_STATUS).await;

    // Same password again still rotates the salt and drops the cached
    // authentication (Review Focus 3).
    assert_eq!(test.server.inner.cache.za_keys.len(), 1);
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    za_post(
        "password",
        &json!({ "username": name, "password": STRONG2, "new_password": STRONG2 }),
    )
    .await
    .expect(200);
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert_ne!(after.record.salt, before.record.salt);
    assert_eq!(test.server.inner.cache.za_keys.len(), 0);
    assert!(
        !test
            .server
            .inner
            .cache
            .http_auth
            .inner()
            .iter()
            .any(|(_, v)| v.account_id == id),
        "cached authentication dropped"
    );
    caldav(id, name, STRONG2, StatusCode::MULTI_STATUS).await;

    // Paused verification across a password change: the login verified the
    // old password before the change and succeeds, but nothing is cached.
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(id)])
        .await;
    let sizes = cache_sizes(test);
    let paused = PausedLogin::start(id, name, STRONG2).await;
    za_post(
        "password",
        &json!({ "username": name, "password": STRONG2, "new_password": STRONG }),
    )
    .await
    .expect(200);
    assert_eq!(paused.finish().await, 207, "verified before the change");
    assert_eq!(
        cache_sizes(test),
        sizes,
        "superseded verification not cached"
    );
    caldav(id, name, STRONG2, StatusCode::UNAUTHORIZED).await;
    caldav(id, name, STRONG, StatusCode::MULTI_STATUS).await;

    // Concurrent password changes with the same old password: both verify
    // the same generation, the second commit fails the revision check.
    let mut results = za_post_concurrent(
        "password",
        &[
            json!({ "username": name, "password": STRONG, "new_password": STRONG2 }),
            json!({ "username": name, "password": STRONG, "new_password": STRONG3 }),
        ],
    )
    .await;
    let r3 = results.pop().unwrap();
    let r2 = results.pop().unwrap();
    let mut statuses = [r2.status, r3.status];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 409], "{:?} {:?}", r2.body, r3.body);
    let (winner, loser) = if r2.status == 200 {
        (STRONG2, STRONG3)
    } else {
        (STRONG3, STRONG2)
    };
    caldav(id, name, STRONG, StatusCode::UNAUTHORIZED).await;
    caldav(id, name, loser, StatusCode::UNAUTHORIZED).await;
    caldav(id, name, winner, StatusCode::MULTI_STATUS).await;
    za_post(
        "password",
        &json!({ "username": name, "password": winner, "new_password": STRONG }),
    )
    .await
    .expect(200);
    caldav(id, name, STRONG, StatusCode::MULTI_STATUS).await;

    // Recovery: wrong key 401, bad format 401, then a real recovery.
    za_post(
        "recover",
        &json!({ "username": name, "recovery_key": RecoveryKey::generate().encode(), "new_password": STRONG2 }),
    )
    .await
    .expect(401);
    za_post(
        "recover",
        &json!({ "username": name, "recovery_key": "not a key", "new_password": STRONG2 }),
    )
    .await
    .expect(401);
    // Concurrent recovery with the same key: one wins, the rest fail (409 on the
    // revision check, or 401 once the winner rotated the recovery wrap).
    let body = json!({ "username": name, "recovery_key": recovery, "new_password": STRONG2 });
    let results = futures::future::join_all((0..4).map(|_| za_post("recover", &body))).await;
    let statuses: Vec<u16> = results.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses.iter().filter(|s| **s == 200).count(),
        1,
        "{statuses:?}"
    );
    assert!(
        statuses.iter().all(|s| matches!(*s, 200 | 401 | 409)),
        "{statuses:?}"
    );
    let new_recovery = results
        .into_iter()
        .find(|r| r.status == 200)
        .unwrap()
        .str(200, "recovery_key");
    assert!(RecoveryKey::parse(&new_recovery).is_some());
    assert_ne!(new_recovery, recovery);
    assert_eq!(test.server.inner.cache.za_keys.len(), 0);
    caldav(id, name, STRONG, StatusCode::UNAUTHORIZED).await;
    caldav(id, name, STRONG2, StatusCode::MULTI_STATUS).await;
    // The old recovery key stops working.
    za_post(
        "recover",
        &json!({ "username": name, "recovery_key": recovery, "new_password": STRONG }),
    )
    .await
    .expect(401);

    // Paused verification across a recovery: the login verified the old
    // password before the recovery and succeeds, but nothing is cached.
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(id)])
        .await;
    let sizes = cache_sizes(test);
    let paused = PausedLogin::start(id, name, STRONG2).await;
    let recovered = za_post(
        "recover",
        &json!({ "username": name, "recovery_key": new_recovery, "new_password": STRONG }),
    )
    .await
    .str(200, "recovery_key");
    assert_eq!(paused.finish().await, 207, "verified before the recovery");
    assert_eq!(
        cache_sizes(test),
        sizes,
        "superseded verification not cached"
    );
    caldav(id, name, STRONG2, StatusCode::UNAUTHORIZED).await;
    caldav(id, name, STRONG, StatusCode::MULTI_STATUS).await;

    // recovery-key: fresh wrap; old key dead; concurrent calls: one wins, the rest 409.
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    let newer = za_post(
        "recovery-key",
        &json!({ "username": name, "password": STRONG }),
    )
    .await
    .str(200, "recovery_key");
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert_eq!(after.record.revision, before.record.revision + 1);
    assert_ne!(after.record.recovery_wrap, before.record.recovery_wrap);
    assert_eq!(
        after.record.password_wrap, before.record.password_wrap,
        "password wrap untouched"
    );
    assert_eq!(after.record.salt, before.record.salt);
    za_post(
        "recover",
        &json!({ "username": name, "recovery_key": recovered, "new_password": STRONG2 }),
    )
    .await
    .expect(401);
    let body = json!({ "username": name, "password": STRONG });
    let results = za_post_concurrent("recovery-key", &vec![body; 4]).await;
    let statuses: Vec<u16> = results.iter().map(|r| r.status).collect();
    assert_eq!(
        statuses.iter().filter(|s| **s == 200).count(),
        1,
        "{statuses:?}"
    );
    assert!(
        statuses.iter().all(|s| *s == 200 || *s == 409),
        "{statuses:?}"
    );
    let winner = results
        .into_iter()
        .find(|r| r.status == 200)
        .unwrap()
        .str(200, "recovery_key");
    za_post(
        "recover",
        &json!({ "username": name, "recovery_key": newer, "new_password": STRONG2 }),
    )
    .await
    .expect(401);
    za_post(
        "recover",
        &json!({ "username": name, "recovery_key": winner, "new_password": STRONG }),
    )
    .await
    .expect(200);

    // Leave key1 on STRONG with a fresh recovery key for later modules.
    let final_key = za_post(
        "recovery-key",
        &json!({ "username": name, "password": STRONG }),
    )
    .await
    .str(200, "recovery_key");
    caldav(id, name, STRONG, StatusCode::MULTI_STATUS).await;
    test.accounts.get_mut(name).unwrap().recovery_key = Some(final_key);
}
