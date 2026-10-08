/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{STRONG, user_permissions};
use crate::utils::{
    server::TestServer,
    za::{
        Parked, VaultReply, assert_nothing_cached, caldav_from, park_login, za_post,
        za_post_concurrent, za_post_from,
    },
};
use common::{auth::credential::AppPassword, ipc::CacheInvalidation};
use http::{api::vault::MAX_APP_PASSWORD_DESCRIPTION, auth::authenticate::za_test};
use hyper::StatusCode;
use registry::{
    schema::{
        prelude::{Object, ObjectInner, ObjectType},
        structs::{self, Credential},
    },
    types::id::ObjectId,
};
use serde_json::{Value, json};
use store::{
    registry::write::{RegistryWrite, RegistryWriteResult},
    write::now,
};
use vault::record::{AppWrap, PENDING_WRAP_MAX_AGE_SECS, WrapState};

const STRONG2: &str = "another long passphrase with 2 numbers";

/// Client address of every deliberate failure in this module.
const FAIL_IP: &str = "10.0.9.2";

/// A fresh key account: its fail2ban login-name budget is not shared with
/// the other modules' deliberate failures.
const NAME: &str = "key7@example.com";

async fn caldav(secret: &str, status: StatusCode) {
    caldav_from(FAIL_IP, NAME, secret, status).await;
}

/// Deliberate failure: sent from `FAIL_IP`.
async fn za_fail(path: &str, body: &Value) -> VaultReply {
    za_post_from(FAIL_IP, path, body).await
}

async fn create(description: &str) -> (String, u32) {
    let reply = za_post(
        "app-password",
        &json!({ "username": NAME, "password": STRONG, "description": description }),
    )
    .await
    .expect(200);
    let app_password = reply["app_password"].as_str().unwrap().to_string();
    let credential_id = reply["credential_id"].as_u64().unwrap() as u32;
    assert_eq!(
        AppPassword::parse(&app_password).unwrap().credential_id,
        credential_id
    );
    (app_password, credential_id)
}

async fn revoke(credential_id: u32) -> VaultReply {
    za_post(
        "app-password/revoke",
        &json!({ "username": NAME, "password": STRONG, "credential_id": credential_id }),
    )
    .await
}

/// App-password credential ids in the registry, read directly (the JMAP
/// view masks secrets).
async fn registry_app_ids(test: &TestServer) -> Vec<u32> {
    test.server
        .registry()
        .object::<structs::Account>(test.account(NAME).id())
        .await
        .unwrap()
        .unwrap()
        .into_user()
        .unwrap()
        .credentials
        .values()
        .filter_map(|c| match c {
            Credential::AppPassword(c) => Some(c.credential_id.document_id()),
            _ => None,
        })
        .collect()
}

/// Writes the record directly, conditional on the read it was taken from.
async fn edit_record(test: &TestServer, id: u32, edit: impl FnOnce(&mut Vec<AppWrap>)) {
    let read = test.server.za_vault_record(id).await.unwrap().unwrap();
    let mut record = read.record.clone();
    edit(&mut record.app_wraps);
    record.revision += 1;
    test.server
        .za_vault_write(id, &record, Some(&read))
        .await
        .unwrap();
    test.server.za_invalidate_account(id).await.unwrap();
}

async fn wrap_state(test: &TestServer, id: u32, credential_id: u32) -> Option<WrapState> {
    test.server
        .za_vault_record(id)
        .await
        .unwrap()
        .unwrap()
        .record
        .app_wrap(credential_id)
        .map(|w| w.state)
}

/// A creation parked at `set`'s pause point (`za_test::set_publish`:
/// between its Pending wrap and the registry credential;
/// `za_test::set_registered`: after the registry credential, before
/// publication), with the id of its Pending wrap.
async fn park_creation(
    test: &TestServer,
    id: u32,
    set: fn(Option<std::sync::Arc<za_test::Pause>>),
    description: &'static str,
) -> (Parked<VaultReply>, u32) {
    let parked = Parked::start(id, set, async move {
        za_post(
            "app-password",
            &json!({ "username": NAME, "password": STRONG, "description": description }),
        )
        .await
    })
    .await;
    let pending: Vec<u32> = test
        .server
        .za_vault_record(id)
        .await
        .unwrap()
        .unwrap()
        .record
        .app_wraps
        .iter()
        .filter(|w| w.state == WrapState::Pending)
        .map(|w| w.credential_id)
        .collect();
    assert_eq!(pending.len(), 1, "the parked creation holds a Pending wrap");
    (parked, pending[0])
}

/// A harmless registry edit of the account: bumps the object's revision,
/// so a registry write based on an earlier read loses its revision check.
async fn bump_registry_revision(test: &TestServer) {
    let id = test.account(NAME).id();
    let object = test
        .server
        .registry()
        .get(ObjectId::new(ObjectType::Account, id))
        .await
        .unwrap()
        .unwrap();
    let old_object = Object::with_revision(object.inner.clone(), object.revision);
    let mut inner = object.inner;
    let ObjectInner::Account(structs::Account::User(user)) = &mut inner else {
        panic!("not a user account");
    };
    user.description = Some(format!("Key Seven, revision {}", object.revision));
    let result = test
        .server
        .registry()
        .write(RegistryWrite::Update {
            object: &Object::new(inner),
            id,
            old_object: &old_object,
        })
        .await
        .unwrap();
    assert!(matches!(result, RegistryWriteResult::Success(_)));
}

fn garbage_wrap(credential_id: u32, state: WrapState, created: i64) -> AppWrap {
    AppWrap {
        credential_id,
        wrap: vec![1; 72],
        state,
        created,
        publication_id: 7,
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access app-password tests...");
    let admin = test.account("admin@example.com").clone();
    let account = admin
        .create_key_user_account(NAME, STRONG, "Key Seven", &[], user_permissions())
        .await;
    let id = account.id().document_id();
    test.insert_account(account);

    // Bad input 400, wrong password 401, app passwords and master logins
    // 400, non-key accounts 409.
    for description in [
        String::new(),
        "   ".into(),
        "x".repeat(MAX_APP_PASSWORD_DESCRIPTION + 1),
    ] {
        za_fail(
            "app-password",
            &json!({ "username": NAME, "password": STRONG, "description": description }),
        )
        .await
        .expect(400);
    }
    za_fail(
        "app-password",
        &json!({ "username": NAME, "password": "nope nope nope", "description": "Phone" }),
    )
    .await
    .expect(401);
    za_fail(
        "app-password/revoke",
        &json!({ "username": NAME, "password": "nope nope nope", "credential_id": 1 }),
    )
    .await
    .expect(401);
    let syntactic = AppPassword::new(1).build();
    za_fail(
        "app-password",
        &json!({ "username": NAME, "password": syntactic, "description": "Phone" }),
    )
    .await
    .expect(400);
    za_fail(
        "app-password/revoke",
        &json!({ "username": NAME, "password": syntactic, "credential_id": 1 }),
    )
    .await
    .expect(400);
    za_fail(
        "app-password",
        &json!({ "username": "key7@example.com%admin@example.com", "password": STRONG, "description": "Phone" }),
    )
    .await
    .expect(400);
    za_fail(
        "app-password",
        &json!({ "username": "plain@example.com", "password": "plain secret with entropy 9", "description": "Phone" }),
    )
    .await
    .expect(409);
    assert!(registry_app_ids(test).await.is_empty());

    // Creation: Published wrap, registry credential, CalDAV login with keys.
    let (phone, phone_id) = create("Phone").await;
    assert_eq!(
        wrap_state(test, id, phone_id).await,
        Some(WrapState::Published)
    );
    assert_eq!(registry_app_ids(test).await, vec![phone_id]);
    caldav(&phone, StatusCode::MULTI_STATUS).await;
    assert!(test.server.inner.cache.za_keys.contains_account(id));
    // The vault endpoints refuse it.
    za_fail(
        "password",
        &json!({ "username": NAME, "password": phone, "new_password": STRONG }),
    )
    .await
    .expect(400);

    // Interleaving (spec 11): a password change lands between the Pending
    // wrap and the registry credential. The wrap is under the app secret,
    // not the password, and pruning never touches a fresh Pending wrap, so
    // the returned app password still decrypts afterwards.
    let (parked, pending_id) = park_creation(test, id, za_test::set_publish, "Laptop").await;
    za_post(
        "password",
        &json!({ "username": NAME, "password": STRONG, "new_password": STRONG2 }),
    )
    .await
    .expect(200);
    assert_eq!(
        wrap_state(test, id, pending_id).await,
        Some(WrapState::Pending),
        "the password change kept the fresh Pending wrap"
    );
    let reply = parked.finish().await.expect(200);
    let laptop = reply["app_password"].as_str().unwrap().to_string();
    let laptop_id = reply["credential_id"].as_u64().unwrap() as u32;
    assert_eq!(laptop_id, pending_id);
    assert_eq!(
        wrap_state(test, id, laptop_id).await,
        Some(WrapState::Published)
    );
    caldav(&laptop, StatusCode::MULTI_STATUS).await;
    caldav(STRONG, StatusCode::UNAUTHORIZED).await;
    za_post(
        "password",
        &json!({ "username": NAME, "password": STRONG2, "new_password": STRONG }),
    )
    .await
    .expect(200);
    caldav(&laptop, StatusCode::MULTI_STATUS).await;
    caldav(&phone, StatusCode::MULTI_STATUS).await;

    // Reused id: revoking the highest credential frees its id; a stale
    // Published wrap left under that id (registry credential gone) is pruned
    // before the collision check, so the next creation reuses the id.
    revoke(laptop_id).await.expect(200);
    edit_record(test, id, |wraps| {
        wraps.push(garbage_wrap(laptop_id, WrapState::Published, 0))
    })
    .await;
    let (tablet, tablet_id) = create("Tablet").await;
    assert_eq!(tablet_id, laptop_id, "the id is reused");
    caldav(&tablet, StatusCode::MULTI_STATUS).await;

    // Simulated crash between the pending wrap and the registry write: a
    // fresh Pending wrap under the next id with no registry credential.
    // The secret cannot log in (no registry credential); a creation that
    // computes the same id is refused rather than touching the wrap; a vault
    // write keeps the fresh wrap; once older than PENDING_WRAP_MAX_AGE_SECS
    // the next vault write prunes it, along with a Published wrap whose
    // registry credential does not exist, and the id becomes usable.
    let crashed_id = tablet_id + 1;
    let orphan_id = tablet_id + 99;
    edit_record(test, id, |wraps| {
        wraps.push(garbage_wrap(crashed_id, WrapState::Pending, now() as i64));
        wraps.push(garbage_wrap(orphan_id, WrapState::Published, 0));
    })
    .await;
    caldav(
        &AppPassword::new(crashed_id).build(),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let reply = za_post(
        "app-password",
        &json!({ "username": NAME, "password": STRONG, "description": "Watch" }),
    )
    .await
    .expect(409);
    assert_eq!(reply["error"], "app password publication in progress");
    za_post(
        "recovery-key",
        &json!({ "username": NAME, "password": STRONG }),
    )
    .await
    .expect(200);
    assert_eq!(
        wrap_state(test, id, crashed_id).await,
        Some(WrapState::Pending)
    );
    assert_eq!(wrap_state(test, id, orphan_id).await, None);
    edit_record(test, id, |wraps| {
        for wrap in wraps.iter_mut().filter(|w| w.credential_id == crashed_id) {
            wrap.created -= PENDING_WRAP_MAX_AGE_SECS + 1;
        }
    })
    .await;
    za_post(
        "recovery-key",
        &json!({ "username": NAME, "password": STRONG }),
    )
    .await
    .expect(200);
    assert_eq!(wrap_state(test, id, crashed_id).await, None);
    for credential_id in [phone_id, tablet_id] {
        assert_eq!(
            wrap_state(test, id, credential_id).await,
            Some(WrapState::Published),
            "published wraps with a registry credential survive"
        );
    }
    let (watch, watch_id) = create("Watch").await;
    assert_eq!(watch_id, crashed_id);
    caldav(&watch, StatusCode::MULTI_STATUS).await;
    revoke(watch_id).await.expect(200);
    revoke(tablet_id).await.expect(200);

    // Quota: the account allows five app passwords.
    let mut extra = Vec::new();
    for i in 0..4 {
        extra.push(create(&format!("Extra {i}")).await.1);
    }
    let reply = za_post(
        "app-password",
        &json!({ "username": NAME, "password": STRONG, "description": "Sixth" }),
    )
    .await
    .expect(409);
    assert_eq!(reply["error"], "app password quota exceeded");
    for credential_id in extra {
        revoke(credential_id).await.expect(200);
    }

    // Concurrent creation, a smoke check: every call either succeeds or is
    // refused with 409 (fence, revision check, id collision or lost registry
    // race); at least one succeeds, and every success is usable.
    let body = json!({ "username": NAME, "password": STRONG, "description": "Concurrent" });
    let results = za_post_concurrent("app-password", &vec![body; 4]).await;
    let statuses: Vec<u16> = results.iter().map(|r| r.status).collect();
    assert!(
        statuses.contains(&200) && statuses.iter().all(|s| matches!(*s, 200 | 409)),
        "{statuses:?}"
    );
    for reply in results.into_iter().filter(|r| r.status == 200) {
        let body = reply.expect(200);
        caldav(
            body["app_password"].as_str().unwrap(),
            StatusCode::MULTI_STATUS,
        )
        .await;
        revoke(body["credential_id"].as_u64().unwrap() as u32)
            .await
            .expect(200);
    }
    assert_eq!(registry_app_ids(test).await, vec![phone_id]);

    // Rollback, entry gone (spec 4.1): the parked creation's Pending wrap is
    // revoked before its registry credential lands; step (3) finds the
    // entry gone, deletes the registry credential and fails.
    let (parked, gone_id) = park_creation(test, id, za_test::set_publish, "Gone").await;
    revoke(gone_id).await.expect(200);
    let reply = parked.finish().await;
    assert_eq!(
        reply.expect(409)["error"],
        "app password publication failed"
    );
    assert_eq!(wrap_state(test, id, gone_id).await, None);
    assert!(!registry_app_ids(test).await.contains(&gone_id));

    // Rollback must not touch a replacement (PR #5 P2): creation A parks
    // after step (2), is revoked, and creation B reuses its id and lands.
    // A's cleanup must leave B's registry credential alone.
    let (parked, reused_id) = park_creation(test, id, za_test::set_registered, "Parked").await;
    revoke(reused_id).await.expect(200);
    let (replacement, replacement_id) = create("Replacement").await;
    assert_eq!(replacement_id, reused_id, "the id is reused");
    caldav(&replacement, StatusCode::MULTI_STATUS).await;
    let reply = parked.finish().await;
    assert_eq!(
        reply.expect(409)["error"],
        "app password publication failed"
    );
    assert_eq!(
        wrap_state(test, id, replacement_id).await,
        Some(WrapState::Published)
    );
    assert!(registry_app_ids(test).await.contains(&replacement_id));
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(id)])
        .await;
    caldav(&replacement, StatusCode::MULTI_STATUS).await;
    revoke(replacement_id).await.expect(200);

    // Rollback, registry write failed (spec 4.1): the account's registry
    // object changes while the creation is parked, so step (2) loses its
    // revision check and the Pending wrap is removed.
    let (parked, raced_id) = park_creation(test, id, za_test::set_publish, "Raced").await;
    bump_registry_revision(test).await;
    let reply = parked.finish().await;
    assert_eq!(reply.expect(409)["error"], "registry write failed");
    assert_eq!(wrap_state(test, id, raced_id).await, None);
    assert!(!registry_app_ids(test).await.contains(&raced_id));
    assert_eq!(registry_app_ids(test).await, vec![phone_id]);

    // Paused verification across a revocation: the login verified the app
    // password before the revocation and succeeds, but nothing is cached.
    let (desk, desk_id) = create("Desk").await;
    caldav(&desk, StatusCode::MULTI_STATUS).await;
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(id)])
        .await;
    let parked = park_login(id, NAME, &desk).await;
    revoke(desk_id).await.expect(200);
    assert_eq!(parked.finish().await, 207, "verified before the revocation");
    assert_nothing_cached(test, id);
    caldav(&desk, StatusCode::UNAUTHORIZED).await;
    caldav(&phone, StatusCode::MULTI_STATUS).await;

    // Revocation: unknown id 409; the wrap goes in one write (generation
    // +1), resident keys are dropped, the registry credential is deleted and
    // the app password no longer logs in.
    revoke(phone_id + 12345).await.expect(409);
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    revoke(phone_id).await.expect(200);
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert_eq!(after.record.revision, before.record.revision + 1);
    assert!(after.record.app_wrap(phone_id).is_none());
    assert!(
        !test.server.inner.cache.za_keys.contains_account(id),
        "revocation drops resident keys"
    );
    assert!(
        registry_app_ids(test).await.is_empty(),
        "registry credential deleted"
    );
    let phone_secret = AppPassword::parse(&phone).unwrap().secret;
    assert!(
        test.server
            .za_open_app_wrap(id, phone_id, &phone_secret)
            .await
            .unwrap()
            .is_none()
    );
    caldav(&phone, StatusCode::UNAUTHORIZED).await;

    // A registry credential whose wrap is gone cannot log in (spec 4.3);
    // revoke deletes it, freeing its quota slot.
    let (dangling, dangling_id) = create("Dangling").await;
    edit_record(test, id, |wraps| {
        wraps.retain(|w| w.credential_id != dangling_id)
    })
    .await;
    caldav(&dangling, StatusCode::UNAUTHORIZED).await;
    revoke(dangling_id).await.expect(200);
    assert!(
        registry_app_ids(test).await.is_empty(),
        "dangling credential deleted"
    );
    // Neither a wrap nor a registry credential: unknown.
    revoke(dangling_id).await.expect(409);
    caldav(STRONG, StatusCode::MULTI_STATUS).await;
}
