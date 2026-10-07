/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{STRONG, user_permissions};
use crate::utils::{
    server::TestServer,
    za::{
        VaultReply, assert_nothing_cached, caldav_from, park_endpoint, park_login, za_post,
        za_post_concurrent, za_post_from, za_setup_token,
    },
};
use common::{auth::credential::AppPassword, ipc::CacheInvalidation};
use hyper::StatusCode;
use serde_json::{Value, json};
use vault::recovery::RecoveryKey;

const STRONG2: &str = "another long passphrase with 2 numbers";
const STRONG3: &str = "yet another passphrase, number 3 of them";

/// Client address of every deliberate failure in this module, so the
/// loopback address keeps its fail2ban budget for the other modules.
const FAIL_IP: &str = "10.0.9.1";

/// Body of `za_commit`'s generation-fence refusal (the CAS refusal differs).
const FENCE_ERROR: &str = "vault changed since verification";

/// PROPFIND on the calendar home. Refusals are sent from `FAIL_IP`.
async fn caldav(name: &str, secret: &str, status: StatusCode) {
    caldav_from(FAIL_IP, name, secret, status).await;
}

/// Deliberate failure: sent from `FAIL_IP`.
async fn za_fail(path: &str, body: &Value) -> VaultReply {
    za_post_from(FAIL_IP, path, body).await
}

fn assert_shape_refused(reply: VaultReply) {
    let body = reply.expect(400);
    let detail = body["detail"].as_str().unwrap_or_default();
    assert!(
        detail.contains("must not look like an app password"),
        "{detail}"
    );
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access password tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let mut recovery = test.account(name).recovery_key.clone().unwrap();

    // Wrong password 401; app password and master separator 400 before any
    // verification (Review Focus 4 and 5).
    za_fail(
        "password",
        &json!({ "username": name, "password": "nope nope nope", "new_password": STRONG2 }),
    )
    .await
    .expect(401);
    za_fail(
        "recovery-key",
        &json!({ "username": name, "password": "nope nope nope" }),
    )
    .await
    .expect(401);
    let app_password = AppPassword::new(1).build();
    assert!(AppPassword::parse(&app_password).is_some());
    for endpoint in ["password", "recovery-key"] {
        za_fail(
            endpoint,
            &json!({ "username": name, "password": app_password, "new_password": STRONG2 }),
        )
        .await
        .expect(400);
        za_fail(
            endpoint,
            &json!({ "username": "key1@example.com%admin@example.com", "password": STRONG, "new_password": STRONG2 }),
        )
        .await
        .expect(400);
        // Not a key account.
        za_fail(
            endpoint,
            &json!({ "username": "plain@example.com", "password": "plain secret with entropy 9", "new_password": STRONG2 }),
        )
        .await
        .expect(409);
    }
    // Weak new password.
    za_fail(
        "password",
        &json!({ "username": name, "password": STRONG, "new_password": "weak" }),
    )
    .await
    .expect(400);
    // A new password shaped like an app password could never log in as a
    // primary password: refused by password and recover (setup below),
    // nothing written.
    let shaped = AppPassword::new(7).build();
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    for (endpoint, body) in [
        (
            "password",
            json!({ "username": name, "password": STRONG, "new_password": shaped }),
        ),
        (
            "recover",
            json!({ "username": name, "recovery_key": recovery, "new_password": shaped }),
        ),
    ] {
        assert_shape_refused(za_fail(endpoint, &body).await);
    }
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert_eq!(after.cas, before.cas, "nothing written");
    // recover: an account without a vault record is 401, a pending one 409.
    za_fail(
        "recover",
        &json!({ "username": "plain@example.com", "recovery_key": RecoveryKey::generate().encode(), "new_password": STRONG2 }),
    )
    .await
    .expect(401);
    let admin = test.account("admin@example.com").clone();
    let pending = admin
        .create_passwordless_user_account(
            "key6@example.com",
            STRONG,
            "Key Six",
            &[],
            user_permissions(),
        )
        .await;
    let token = za_setup_token(&admin, "key6@example.com").await;
    assert_shape_refused(
        za_fail(
            "setup",
            &json!({ "username": "key6@example.com", "token": token, "password": shaped }),
        )
        .await,
    );
    za_fail(
        "recover",
        &json!({ "username": "key6@example.com", "recovery_key": RecoveryKey::generate().encode(), "new_password": STRONG2 }),
    )
    .await
    .expect(409);
    admin.destroy_account(pending).await;
    test.wait_for_tasks().await;

    // Change then read: cached authentication for the old password is gone.
    caldav(name, STRONG, StatusCode::MULTI_STATUS).await;
    assert!(test.server.inner.cache.za_keys.contains_account(id));
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
    assert_nothing_cached(test, id);
    caldav(name, STRONG, StatusCode::UNAUTHORIZED).await;
    caldav(name, STRONG2, StatusCode::MULTI_STATUS).await;

    // Same password again still rotates the salt and drops the cached
    // authentication (Review Focus 3).
    assert!(test.server.inner.cache.za_keys.contains_account(id));
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    za_post(
        "password",
        &json!({ "username": name, "password": STRONG2, "new_password": STRONG2 }),
    )
    .await
    .expect(200);
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert_ne!(after.record.salt, before.record.salt);
    assert_nothing_cached(test, id);
    caldav(name, STRONG2, StatusCode::MULTI_STATUS).await;

    // Paused verification across a password change: the login verified the
    // old password before the change and succeeds, but nothing is cached.
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(id)])
        .await;
    let parked = park_login(id, name, STRONG2).await;
    za_post(
        "password",
        &json!({ "username": name, "password": STRONG2, "new_password": STRONG }),
    )
    .await
    .expect(200);
    assert_eq!(parked.finish().await, 207, "verified before the change");
    assert_nothing_cached(test, id);
    caldav(name, STRONG2, StatusCode::UNAUTHORIZED).await;
    caldav(name, STRONG, StatusCode::MULTI_STATUS).await;

    // Generation fence, deterministic: `password` A verifies STRONG and is
    // parked before its re-read; a recovery B commits STRONG2; A resumes on
    // top of a newer generation and gets the fence's 409.
    let parked = park_endpoint(
        id,
        "password",
        json!({ "username": name, "password": STRONG, "new_password": STRONG3 }),
    )
    .await;
    recovery = za_post(
        "recover",
        &json!({ "username": name, "recovery_key": recovery, "new_password": STRONG2 }),
    )
    .await
    .str(200, "recovery_key");
    let reply = parked.finish().await;
    assert_eq!(reply.expect(409)["error"], FENCE_ERROR);
    caldav(name, STRONG3, StatusCode::UNAUTHORIZED).await;
    caldav(name, STRONG, StatusCode::UNAUTHORIZED).await;
    caldav(name, STRONG2, StatusCode::MULTI_STATUS).await;

    // Concurrent password changes, a smoke check only: arrival order is not
    // controllable here (the deterministic fence test is above). Exactly one
    // 200 holds in any order, since the old password stops verifying once
    // the first change commits; the loser sees 409 or 401 depending on
    // whether it verified before or after that commit.
    let results = za_post_concurrent(
        "password",
        &[
            json!({ "username": name, "password": STRONG2, "new_password": STRONG }),
            json!({ "username": name, "password": STRONG2, "new_password": STRONG3 }),
        ],
    )
    .await;
    let statuses: Vec<u16> = results.iter().map(|r| r.status).collect();
    assert!(
        statuses.iter().all(|s| matches!(*s, 200 | 401 | 409)),
        "{statuses:?}"
    );
    assert_eq!(
        statuses.iter().filter(|s| **s == 200).count(),
        1,
        "{statuses:?}"
    );
    let (winner, loser) = if statuses[0] == 200 {
        (STRONG, STRONG3)
    } else {
        (STRONG3, STRONG)
    };
    caldav(name, STRONG2, StatusCode::UNAUTHORIZED).await;
    caldav(name, loser, StatusCode::UNAUTHORIZED).await;
    caldav(name, winner, StatusCode::MULTI_STATUS).await;
    if winner != STRONG {
        za_post(
            "password",
            &json!({ "username": name, "password": winner, "new_password": STRONG }),
        )
        .await
        .expect(200);
    }
    caldav(name, STRONG, StatusCode::MULTI_STATUS).await;

    // Recovery: wrong key 401, bad format 401.
    za_fail(
        "recover",
        &json!({ "username": name, "recovery_key": RecoveryKey::generate().encode(), "new_password": STRONG2 }),
    )
    .await
    .expect(401);
    za_fail(
        "recover",
        &json!({ "username": name, "recovery_key": "not a key", "new_password": STRONG2 }),
    )
    .await
    .expect(401);
    // Concurrent recovery with the same key, a smoke check: exactly one 200
    // holds in any order (the winner rotates the recovery wrap, so a later
    // arrival is 401; a simultaneous one loses the revision check, 409).
    let body = json!({ "username": name, "recovery_key": recovery, "new_password": STRONG2 });
    let results = za_post_concurrent("recover", &vec![body; 4]).await;
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
    let old_recovery = recovery;
    recovery = results
        .into_iter()
        .find(|r| r.status == 200)
        .unwrap()
        .str(200, "recovery_key");
    assert!(RecoveryKey::parse(&recovery).is_some());
    assert_ne!(recovery, old_recovery);
    assert_nothing_cached(test, id);
    caldav(name, STRONG, StatusCode::UNAUTHORIZED).await;
    caldav(name, STRONG2, StatusCode::MULTI_STATUS).await;
    // The old recovery key stops working.
    za_fail(
        "recover",
        &json!({ "username": name, "recovery_key": old_recovery, "new_password": STRONG }),
    )
    .await
    .expect(401);

    // Paused verification across a recovery: the login verified the old
    // password before the recovery and succeeds, but nothing is cached.
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(id)])
        .await;
    let parked = park_login(id, name, STRONG2).await;
    recovery = za_post(
        "recover",
        &json!({ "username": name, "recovery_key": recovery, "new_password": STRONG }),
    )
    .await
    .str(200, "recovery_key");
    assert_eq!(parked.finish().await, 207, "verified before the recovery");
    assert_nothing_cached(test, id);
    caldav(name, STRONG2, StatusCode::UNAUTHORIZED).await;
    caldav(name, STRONG, StatusCode::MULTI_STATUS).await;

    // recovery-key: fresh wrap, password wrap untouched, previous key dead.
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
    za_fail(
        "recover",
        &json!({ "username": name, "recovery_key": recovery, "new_password": STRONG2 }),
    )
    .await
    .expect(401);
    recovery = newer;

    // Generation fence, deterministic: `recovery-key` A is parked after
    // verification; `recovery-key` B commits; A gets the fence's 409 and
    // only B's key works.
    let parked = park_endpoint(
        id,
        "recovery-key",
        json!({ "username": name, "password": STRONG }),
    )
    .await;
    let from_b = za_post(
        "recovery-key",
        &json!({ "username": name, "password": STRONG }),
    )
    .await
    .str(200, "recovery_key");
    let reply = parked.finish().await;
    assert_eq!(reply.expect(409)["error"], FENCE_ERROR);
    za_fail(
        "recover",
        &json!({ "username": name, "recovery_key": recovery, "new_password": STRONG }),
    )
    .await
    .expect(401);
    recovery = za_post(
        "recover",
        &json!({ "username": name, "recovery_key": from_b, "new_password": STRONG }),
    )
    .await
    .str(200, "recovery_key");

    // Concurrent recovery-key calls, a smoke check only: calls that verify
    // the same generation give one 200 and 409s, but serialized arrivals
    // each verify a fresh generation and all succeed, so the count of 200s
    // depends on timing (the deterministic fence test is above).
    let body = json!({ "username": name, "password": STRONG });
    let results = za_post_concurrent("recovery-key", &vec![body; 4]).await;
    let statuses: Vec<u16> = results.iter().map(|r| r.status).collect();
    assert!(
        statuses.iter().all(|s| matches!(*s, 200 | 409)),
        "{statuses:?}"
    );
    assert!(statuses.contains(&200), "{statuses:?}");
    // Only the last committed key opens the recovery wrap; the earlier ones
    // and the key from before the burst are dead.
    za_fail(
        "recover",
        &json!({ "username": name, "recovery_key": recovery, "new_password": STRONG }),
    )
    .await
    .expect(401);
    let mut recovered = 0;
    for reply in results.into_iter().filter(|r| r.status == 200) {
        let key = reply.str(200, "recovery_key");
        let reply = za_fail(
            "recover",
            &json!({ "username": name, "recovery_key": key, "new_password": STRONG }),
        )
        .await;
        assert!(matches!(reply.status, 200 | 401), "{}", reply.status);
        recovered += usize::from(reply.status == 200);
    }
    assert_eq!(recovered, 1, "exactly one issued key was current");
    caldav(name, STRONG, StatusCode::MULTI_STATUS).await;

    // Leave key1 on STRONG with a fresh recovery key for later modules.
    let final_key = za_post(
        "recovery-key",
        &json!({ "username": name, "password": STRONG }),
    )
    .await
    .str(200, "recovery_key");
    test.accounts.get_mut(name).unwrap().recovery_key = Some(final_key);
}
