/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{STRONG, user_permissions};
use crate::utils::{
    account::Account,
    registry::UnwrapRegistryId,
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{za_post, za_post_as, za_post_from, za_setup, za_setup_token},
};
use common::ipc::{CacheInvalidation, RegistryChange};
use http::api::vault::SETUP_TOKEN_TTL_SECS;
use hyper::StatusCode;
use registry::{
    schema::{
        enums::BlockReason,
        prelude::{ObjectType, Property},
        structs::{self, BlockedIp, PasswordCredential, Rate},
    },
    types::{duration::Duration, ipmask::IpAddrOrMask},
};
use serde_json::json;
use store::{registry::write::RegistryWrite, write::now};
use types::id::Id;
use vault::record::{VaultRecord, VaultState};

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access setup tests...");
    let admin = test.account("admin@example.com").clone();
    let plain = test.account("plain@example.com").clone();

    // Non-admin caller lacks SysAccountUpdate.
    za_post_as(
        &plain,
        "setup-token",
        &json!({ "account": "plain@example.com" }),
    )
    .await
    .expect(403);
    // Unknown account.
    za_post_as(
        &admin,
        "setup-token",
        &json!({ "account": "nobody@example.com" }),
    )
    .await
    .expect(404);
    // Ineligible: already has a password credential.
    za_post_as(
        &admin,
        "setup-token",
        &json!({ "account": "plain@example.com" }),
    )
    .await
    .expect(409);

    // Happy path with reissue.
    let key1 = admin
        .create_passwordless_user_account(
            "key1@example.com",
            STRONG,
            "Key One",
            &[],
            user_permissions(),
        )
        .await;
    let key1_id = key1.id().document_id();
    // Warm the account cache first: provisioning must be observed through invalidation.
    assert!(!test.server.account(key1_id).await.unwrap().is_key_account());
    let reply = za_post_as(
        &admin,
        "setup-token",
        &json!({ "account": "key1@example.com" }),
    )
    .await
    .expect(200);
    let token_a = reply["token"].as_str().unwrap().to_string();
    let expires = reply["expires"].as_i64().unwrap();
    assert!((expires - now() as i64 - SETUP_TOKEN_TTL_SECS).abs() < 60);
    assert!(
        test.server.account(key1_id).await.unwrap().is_key_account(),
        "classification observed after invalidation"
    );
    // Reissue: old token invalid, new token works.
    let token_b = za_setup_token(&admin, "key1@example.com").await;
    assert_ne!(token_a, token_b);
    za_post(
        "setup",
        &json!({ "username": "key1@example.com", "token": token_a, "password": STRONG }),
    )
    .await
    .expect(401);
    // Weak password refused before any state change.
    za_post(
        "setup",
        &json!({ "username": "key1@example.com", "token": token_b, "password": "short" }),
    )
    .await
    .expect(400);
    // Concurrent setup with the same token: exactly one succeeds (single use).
    let body = json!({ "username": "key1@example.com", "token": token_b, "password": STRONG });
    let results = futures::future::join_all((0..4).map(|_| za_post("setup", &body))).await;
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
    let recovery = results
        .into_iter()
        .find(|r| r.status == 200)
        .unwrap()
        .str(200, "recovery_key");
    assert_eq!(recovery.len(), 27 + 6, "{recovery}");
    assert!(recovery.split('-').all(|g| g.len() == 4 || g.len() == 3));
    // Active: setup and setup-token are refused.
    za_post(
        "setup",
        &json!({ "username": "key1@example.com", "token": token_b, "password": STRONG }),
    )
    .await
    .expect(409);
    za_post_as(
        &admin,
        "setup-token",
        &json!({ "account": "key1@example.com" }),
    )
    .await
    .expect(409);
    let account = test.server.account(key1_id).await.unwrap();
    assert!(account.is_key_account());
    assert_eq!(
        account.za_generation, 3,
        "pending record was revision 1, the reissue made it 2, setup made it 3"
    );
    assert!(account.za_public_key.is_some());
    // Registry: marker only, no TOTP.
    let password = registry_password(test, key1.id()).await;
    assert_eq!(password.secret, "$za$");
    assert!(password.otp_auth.is_none());

    // CalDAV login with the new password; second request is a cache hit that carries keys.
    let client = DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    client
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_eq!(test.server.inner.cache.za_keys.len(), 1);
    client
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_eq!(test.server.inner.cache.za_keys.len(), 1);
    // Invalidation drops the resident keys.
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(key1_id)])
        .await;
    assert_eq!(test.server.inner.cache.za_keys.len(), 0);
    // Wrong password, and the literal marker as a password, are refused.
    for bad in ["wrong password here", "$za$"] {
        DummyWebDavClient::new(key1_id, "key1@example.com", bad, "key1@example.com")
            .request("PROPFIND", "/dav/cal/key1@example.com/", "")
            .await
            .with_status(StatusCode::UNAUTHORIZED);
    }

    // Marker without a record is corruption: login refused, account still classified.
    let saved = test.server.za_vault_record(key1_id).await.unwrap().unwrap();
    let mut batch = store::write::BatchBuilder::new();
    batch
        .with_account_id(key1_id)
        .with_collection(types::collection::Collection::Principal)
        .with_document(0)
        .clear(types::field::PrincipalField::ZeroAccessVault);
    test.server.store().write(batch.build_all()).await.unwrap();
    test.server
        .invalidate_local_caches(&[
            CacheInvalidation::AccessToken(key1_id),
            CacheInvalidation::Account(key1_id),
        ])
        .await;
    assert!(test.server.account(key1_id).await.unwrap().is_key_account());
    client
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::UNAUTHORIZED);
    za_post_as(
        &admin,
        "setup-token",
        &json!({ "account": "key1@example.com" }),
    )
    .await
    .expect(409);
    test.server
        .za_vault_write(key1_id, &saved.record, None)
        .await
        .unwrap();
    test.server
        .invalidate_local_caches(&[
            CacheInvalidation::AccessToken(key1_id),
            CacheInvalidation::Account(key1_id),
        ])
        .await;
    client
        .request("PROPFIND", "/dav/cal/key1@example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let mut key1 = key1;
    key1.recovery_key = Some(recovery);
    test.insert_account(key1);

    // Expired token (Review Focus 2).
    let key2 = admin
        .create_passwordless_user_account(
            "key2@example.com",
            STRONG,
            "Key Two",
            &[],
            user_permissions(),
        )
        .await;
    let key2_id = key2.id().document_id();
    let token = za_setup_token(&admin, "key2@example.com").await;
    let read = test.server.za_vault_record(key2_id).await.unwrap().unwrap();
    let mut record = read.record.clone();
    record.setup_token_expires = now() as i64 - 1;
    record.revision += 1;
    test.server
        .za_vault_write(key2_id, &record, Some(&read))
        .await
        .unwrap();
    za_post(
        "setup",
        &json!({ "username": "key2@example.com", "token": token, "password": STRONG }),
    )
    .await
    .expect(401);
    let token = za_setup_token(&admin, "key2@example.com").await;
    let mut key2 = key2;
    key2.recovery_key = Some(za_setup("key2@example.com", &token, STRONG).await);
    test.insert_account(key2);

    // Crash after the PendingSetup record and before the marker: reissue recovers.
    let key3 = admin
        .create_passwordless_user_account(
            "key3@example.com",
            STRONG,
            "Key Three",
            &[],
            user_permissions(),
        )
        .await;
    let key3_id = key3.id().document_id();
    test.server
        .za_vault_write(
            key3_id,
            &VaultRecord::pending(vec![0; 32], now() as i64 + 100),
            None,
        )
        .await
        .unwrap();
    let token = za_setup_token(&admin, "key3@example.com").await;
    assert_eq!(registry_password(test, key3.id()).await.secret, "$za$");
    let read = test.server.za_vault_record(key3_id).await.unwrap().unwrap();
    assert_eq!(read.record.state, VaultState::PendingSetup);
    assert_eq!(read.record.revision, 2);
    let mut key3 = key3;
    key3.recovery_key = Some(za_setup("key3@example.com", &token, STRONG).await);
    test.insert_account(key3);

    // Ineligible: calendar data already present (written directly, no login needed).
    let key4 = admin
        .create_passwordless_user_account(
            "key4@example.com",
            STRONG,
            "Key Four",
            &[],
            user_permissions(),
        )
        .await;
    let key4_id = key4.id().document_id();
    {
        use groupware::calendar::{Calendar, CalendarPreferences};
        let account_info = test.server.account_info(key4_id).await.unwrap();
        let mut batch = store::write::BatchBuilder::new();
        Calendar {
            name: "stray".into(),
            preferences: vec![CalendarPreferences {
                account_id: key4_id,
                name: "stray".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
        .insert(account_info.account_tenant_ids(), key4_id, 0, &mut batch)
        .unwrap();
        test.server.commit_batch(batch).await.unwrap();
    }
    za_post_as(
        &admin,
        "setup-token",
        &json!({ "account": "key4@example.com" }),
    )
    .await
    .expect(409);
    admin.destroy_account(key4).await;
    test.wait_for_tasks().await;

    // Review Focus 2: failed setup attempts go through fail2ban. A low ban
    // rate for the duration of the block, a forwarded client address and a
    // dedicated account keep the loopback address and other logins unbanned.
    const BANNED_IP: &str = "10.0.0.77";
    const BAN_RATE: u64 = 5;
    let key5 = admin
        .create_passwordless_user_account(
            "key5@example.com",
            STRONG,
            "Key Five",
            &[],
            user_permissions(),
        )
        .await;
    let token = za_setup_token(&admin, "key5@example.com").await;
    set_auth_ban_rate(&admin, BAN_RATE).await;
    let wrong =
        json!({ "username": "key5@example.com", "token": "not-the-token", "password": STRONG });
    let mut statuses = Vec::new();
    for _ in 0..BAN_RATE + 1 {
        statuses.push(za_post_from(BANNED_IP, "setup", &wrong).await.status);
    }
    assert_eq!(
        statuses,
        [vec![401; BAN_RATE as usize], vec![429]].concat(),
        "every failure is counted, the one over the rate is a ban"
    );
    let blocked_id = test
        .server
        .registry()
        .primary_key(
            ObjectType::BlockedIp.into(),
            Property::Address,
            IpAddrOrMask::from_ip(BANNED_IP.parse().unwrap()).to_index_key(),
        )
        .await
        .unwrap()
        .expect("failed setup attempts must ban the client address");
    let blocked = test
        .server
        .registry()
        .object::<BlockedIp>(blocked_id.id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blocked.reason, BlockReason::AuthFailure);
    // Lift the ban and restore the default rate.
    test.server
        .registry()
        .write(RegistryWrite::delete(blocked_id))
        .await
        .unwrap()
        .unwrap_id(trc::location!());
    test.server
        .reload_registry(RegistryChange::Delete(blocked_id))
        .await
        .unwrap();
    set_auth_ban_rate(&admin, 100).await;
    // The failures did not consume the real token.
    let mut key5 = key5;
    key5.recovery_key = Some(za_setup("key5@example.com", &token, STRONG).await);
    test.insert_account(key5);
}

/// The setup token outlives the moment of issue: calendar data that appears
/// in the still-plain account before `setup` must make `setup` refuse.
pub async fn test_data_check(test: &mut TestServer) {
    println!("Running zero-access setup data check tests...");
    let admin = test.account("admin@example.com").clone();
    let key6 = admin
        .create_passwordless_user_account(
            "key6@example.com",
            STRONG,
            "Key Six",
            &[],
            user_permissions(),
        )
        .await;
    let key6_id = key6.id().document_id();
    let token = za_setup_token(&admin, "key6@example.com").await;

    // Plaintext calendar data arrives behind the DAV gate (which already
    // refuses a pending account), as an internal write would.
    let calendar_id = 0u32;
    {
        use groupware::calendar::{Calendar, CalendarPreferences};
        let account_info = test.server.account_info(key6_id).await.unwrap();
        let mut batch = store::write::BatchBuilder::new();
        Calendar {
            name: "stray".into(),
            preferences: vec![CalendarPreferences {
                account_id: key6_id,
                name: "stray".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
        .insert(
            account_info.account_tenant_ids(),
            key6_id,
            calendar_id,
            &mut batch,
        )
        .unwrap();
        test.server.commit_batch(batch).await.unwrap();
    }
    let body = json!({ "username": "key6@example.com", "token": token, "password": STRONG });
    let reply = za_post("setup", &body).await.expect(409);
    assert_eq!(reply["error"], "account already holds calendar data");

    // Remove the data: the same token still works.
    {
        use groupware::{DestroyArchive, calendar::Calendar};
        use store::{
            ValueKey,
            write::{AlignedBytes, Archive},
        };
        use types::collection::Collection;
        let account_info = test.server.account_info(key6_id).await.unwrap();
        let archive = test
            .server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                key6_id,
                Collection::Calendar,
                calendar_id,
            ))
            .await
            .unwrap()
            .expect("calendar archive");
        let mut batch = store::write::BatchBuilder::new();
        DestroyArchive(archive.to_unarchived::<Calendar>().unwrap())
            .delete(
                account_info.account_tenant_ids(),
                key6_id,
                calendar_id,
                None,
                &mut batch,
            )
            .unwrap();
        test.server.commit_batch(batch).await.unwrap();
    }
    let mut key6 = key6;
    key6.recovery_key = Some(za_setup("key6@example.com", &token, STRONG).await);
    admin.destroy_account(key6).await;
    test.wait_for_tasks().await;
}

async fn set_auth_ban_rate(admin: &Account, count: u64) {
    admin
        .registry_update_object(
            ObjectType::Security,
            Id::singleton(),
            json!({
                Property::AuthBanRate: Rate {
                    count,
                    period: Duration::from_millis(86_400_000),
                }
            }),
        )
        .await;
    admin.reload_settings().await;
}

/// Read from the registry directly: the JMAP view masks credential secrets.
async fn registry_password(test: &TestServer, id: Id) -> PasswordCredential {
    test.server
        .registry()
        .object::<structs::Account>(id)
        .await
        .unwrap()
        .unwrap()
        .into_user()
        .unwrap()
        .into_password_credential()
        .unwrap()
}
