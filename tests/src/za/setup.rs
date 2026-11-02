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
use groupware::calendar::Timezone;
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
        account.za_generation(),
        3,
        "pending record was revision 1, the reissue made it 2, setup made it 3"
    );
    assert!(account.za_public_key().is_some());
    // Registry: marker only, no TOTP.
    let password = registry_password(test, key1.id()).await;
    assert_eq!(password.secret, "$za$");
    assert!(password.otp_url.is_none());

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
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(key1_id)]);
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
    test.server.store().write_batch(&mut batch).await.unwrap();
    test.server.invalidate_local_caches(&[
        CacheInvalidation::AccessToken(key1_id),
        CacheInvalidation::Account(key1_id),
    ]);
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
    test.server.invalidate_local_caches(&[
        CacheInvalidation::AccessToken(key1_id),
        CacheInvalidation::Account(key1_id),
    ]);
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
        .insert(account_info.changed_by(), key4_id, 0, &mut batch)
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
        .insert(account_info.changed_by(), key6_id, calendar_id, &mut batch)
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
            write::{Archive, ArchiveBytes},
        };
        use types::collection::Collection;
        let account_info = test.server.account_info(key6_id).await.unwrap();
        let archive = test
            .server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
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
                &test.server,
                account_info.changed_by(),
                key6_id,
                calendar_id,
                None,
                &mut batch,
            )
            .await
            .unwrap();
        test.server.commit_batch(batch).await.unwrap();
    }

    // A server-created default calendar with untouched preferences is not
    // user data: setup must not be blocked by it (plan 3 outcome, candidate 4).
    let default_name = test
        .server
        .core
        .groupware
        .default_calendar_name
        .clone()
        .expect("test server configures a default calendar");
    let default_display = format!(
        "{} ({})",
        test.server
            .core
            .groupware
            .default_calendar_display_name
            .as_deref()
            .unwrap_or(default_name.as_str()),
        "key6@example.com"
    );

    // 1. Renamed default calendar: user data, refused.
    insert_default_calendar(
        test,
        key6_id,
        1,
        default_name.clone(),
        "My calendar".into(),
        Timezone::Default,
    )
    .await;
    let reply = za_post("setup", &body).await.expect(409);
    assert_eq!(reply["error"], "account already holds calendar data");
    remove_calendar(test, key6_id, 1).await;

    // 2. Default calendar carrying a custom `calendar-timezone` (sealed
    //    material under spec 7.2): user data, refused.
    let vtimezone = calcard::icalendar::ICalendar::parse(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VTIMEZONE\r\nTZID:Za/Custom\r\nBEGIN:STANDARD\r\nDTSTART:19700101T000000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0100\r\nTZNAME:ZAT\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nEND:VCALENDAR\r\n",
    )
    .unwrap();
    assert!(vtimezone.is_timezone());
    insert_default_calendar(
        test,
        key6_id,
        3,
        default_name.clone(),
        default_display.clone(),
        Timezone::Custom(vtimezone),
    )
    .await;
    let reply = za_post("setup", &body).await.expect(409);
    assert_eq!(reply["error"], "account already holds calendar data");
    remove_calendar(test, key6_id, 3).await;

    // 3. Untouched default calendar plus one event: refused.
    insert_default_calendar(
        test,
        key6_id,
        2,
        default_name.clone(),
        default_display,
        Timezone::Default,
    )
    .await;
    let event_id = 0u32;
    {
        use groupware::calendar::{CalendarEvent, CalendarEventContent, CalendarEventData};
        let account_info = test.server.account_info(key6_id).await.unwrap();
        let mut batch = store::write::BatchBuilder::new();
        // 1.0 splits an event into its row and its content record.
        CalendarEvent {
            names: vec![common::DavName {
                name: "stray.ics".into(),
                parent_id: 2,
            }],
            ..Default::default()
        }
        .insert(
            CalendarEventContent {
                data: CalendarEventData::default(),
                ..Default::default()
            },
            account_info.changed_by(),
            key6_id,
            event_id,
            None,
            Default::default(),
            &mut batch,
        )
        .unwrap();
        test.server.commit_batch(batch).await.unwrap();
    }
    // The event's index task runs while the event exists: run after its
    // deletion, it would retry the missing document for many seconds, and
    // setup waits for it below (mail ruling R54). A direct batch does not
    // wake the task queue.
    test.server.notify_task_queue();
    test.wait_for_tasks().await;
    let reply = za_post("setup", &body).await.expect(409);
    assert_eq!(reply["error"], "account already holds calendar data");
    {
        use groupware::{DestroyArchive, calendar::CalendarEvent};
        use store::{
            ValueKey,
            write::{Archive, ArchiveBytes},
        };
        use types::collection::Collection;
        let account_info = test.server.account_info(key6_id).await.unwrap();
        let archive = test
            .server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                key6_id,
                Collection::CalendarEvent,
                event_id,
            ))
            .await
            .unwrap()
            .expect("event archive");
        let mut batch = store::write::BatchBuilder::new();
        DestroyArchive(archive.to_unarchived::<CalendarEvent>().unwrap())
            .delete_all(
                &test.server,
                &account_info,
                key6_id,
                event_id,
                None,
                false,
                &mut batch,
            )
            .await
            .unwrap();
        test.server.commit_batch(batch).await.unwrap();
    }

    // 4. Sole untouched default calendar (document 2): the setup below
    //    succeeds once the stray event's unindex task has run (mail ruling
    //    R54); the calendar is destroyed with the account.
    test.server.notify_task_queue();
    test.wait_for_tasks().await;
    let mut key6 = key6;
    key6.recovery_key = Some(za_setup("key6@example.com", &token, STRONG).await);
    admin.destroy_account(key6).await;
    test.wait_for_tasks().await;
}

/// Plants a calendar shaped like upstream's `create_default_calendar`, with
/// the given calendar name, preferences display name and time zone.
async fn insert_default_calendar(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
    name: String,
    display: String,
    time_zone: Timezone,
) {
    use groupware::calendar::{CALENDAR_SUBSCRIBED, Calendar, CalendarPreferences};
    let account_info = test.server.account_info(account_id).await.unwrap();
    let mut batch = store::write::BatchBuilder::new();
    Calendar {
        name,
        preferences: vec![CalendarPreferences {
            account_id,
            name: display,
            flags: CALENDAR_SUBSCRIBED,
            time_zone,
            ..Default::default()
        }],
        ..Default::default()
    }
    .insert(
        account_info.changed_by(),
        account_id,
        document_id,
        &mut batch,
    )
    .unwrap();
    test.server.commit_batch(batch).await.unwrap();
}

async fn remove_calendar(test: &TestServer, account_id: u32, document_id: u32) {
    use groupware::{DestroyArchive, calendar::Calendar};
    use store::{
        ValueKey,
        write::{Archive, ArchiveBytes},
    };
    use types::collection::Collection;
    let account_info = test.server.account_info(account_id).await.unwrap();
    let archive = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            account_id,
            Collection::Calendar,
            document_id,
        ))
        .await
        .unwrap()
        .expect("calendar archive");
    let mut batch = store::write::BatchBuilder::new();
    DestroyArchive(archive.to_unarchived::<Calendar>().unwrap())
        .delete(
            &test.server,
            account_info.changed_by(),
            account_id,
            document_id,
            None,
            &mut batch,
        )
        .await
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
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

/// Upgrade ruling 175 (plan 13's R37 closed): an OAuth grant minted while
/// the account had no credential carries credential version 0, which every
/// refresh re-encodes and no version check refuses. The marker write of
/// `setup-token` is a password change and takes upstream's
/// credentials-changed path, which deletes the account's grants: after it,
/// the grant's access token is refused and its refresh token finds no grant
/// (the refresh route's `InvalidGrant`). Before it, both work (the control).
pub async fn test_grants_die_with_the_marker(test: &mut TestServer) {
    use common::auth::oauth::{
        GrantType,
        scope::OAuthScopes,
        token::{RefreshTokenParams, TokenGrant},
    };
    println!("Running zero-access setup-token grant revocation test...");
    const NAME: &str = "keymail35@example.com";
    let admin = test.account("admin@example.com").clone();
    let mut account = admin
        .create_passwordless_user_account(NAME, STRONG, "Grant", &[], user_permissions())
        .await;
    let id = account.id().document_id();
    let (grant_id, access) =
        crate::utils::za::za_grant_token(&test.server, NAME, OAuthScopes::ALL).await;
    let credential_version = test
        .server
        .access_token(id)
        .await
        .unwrap()
        .credential_version();
    assert_eq!(credential_version, 0, "a credential-less account's version");
    let refresh = test
        .server
        .encode_refresh_token(RefreshTokenParams {
            account_id: id,
            account_name: NAME,
            grant: TokenGrant {
                id: grant_id,
                generation: 0,
            },
            rotated_at: now(),
            credential_version,
            scope: None,
        })
        .unwrap();
    // The refresh route's checks before the client's: the token validates
    // and names a stored grant of the account.
    let refreshable = async |test: &TestServer, refresh: &str| {
        let Ok(info) = test
            .server
            .validate_access_token(GrantType::RefreshToken.into(), refresh)
            .await
        else {
            return false;
        };
        let grant = info.grant.expect("a grant-bound refresh token");
        test.server
            .oauth_grant(grant.id)
            .await
            .unwrap()
            .is_some_and(|stored| stored.account_id() == id)
    };
    assert_eq!(
        super::caches::session_status(&access).await,
        200,
        "control: the access token before setup-token"
    );
    assert!(
        refreshable(test, &refresh).await,
        "control: the grant before setup-token"
    );

    let token = za_setup_token(&admin, NAME).await;
    assert!(
        test.server.oauth_grant(grant_id).await.unwrap().is_none(),
        "the grant outlived the marker write"
    );
    assert!(
        !refreshable(test, &refresh).await,
        "the grant's refresh token still finds its grant"
    );
    assert_eq!(
        super::caches::session_status(&access).await,
        401,
        "the grant's access token outlived the marker write"
    );
    account.recovery_key = Some(za_setup(NAME, &token, STRONG).await);

    // Upgrade ruling 185: a grant minted for the key account with credential
    // version 0, as one minted in the window between the marker write's
    // revocation query and its invalidation would carry. Its access and
    // refresh tokens are refused, while the same grant's tokens at the
    // account's own version (the vault revision) work: the control.
    let (grant_id, current) =
        crate::utils::za::za_grant_token(&test.server, NAME, OAuthScopes::ALL).await;
    let version = test
        .server
        .access_token(id)
        .await
        .unwrap()
        .credential_version();
    assert!(
        version >= 1,
        "a key account's version is its vault revision"
    );
    let unversioned = test
        .server
        .encode_grant_access_token(id, NAME, grant_id, 0)
        .unwrap();
    let refresh_at = |credential_version| {
        test.server
            .encode_refresh_token(RefreshTokenParams {
                account_id: id,
                account_name: NAME,
                grant: TokenGrant {
                    id: grant_id,
                    generation: 0,
                },
                rotated_at: now(),
                credential_version,
                scope: None,
            })
            .unwrap()
    };
    assert_eq!(
        super::caches::session_status(&current).await,
        200,
        "control: the grant's access token at the account's version"
    );
    assert!(
        refreshable(test, &refresh_at(version)).await,
        "control: the grant's refresh token at the account's version"
    );
    assert_eq!(
        super::caches::session_status(&unversioned).await,
        401,
        "a version-0 access token of a key account was accepted"
    );
    assert!(
        !refreshable(test, &refresh_at(0)).await,
        "a version-0 refresh token of a key account validated"
    );
    crate::utils::za::za_revoke_grant(&test.server, grant_id).await;
    test.insert_account(account);
}

/// PR review ruling 196: `setup-token` holds its caller's HTTP concurrency
/// slot until it returns, as 0.16 did and as 1.0's other API handlers do.
/// With the limit at one, a provisioning caller (`SysAccountUpdate`, without
/// `UnlimitedRequests`) whose first request is parked inside the handler
/// (`SETUP_TOKEN_PAUSE`, after the first emptiness check) is refused a
/// second concurrent request for the limit; once the first returns, the slot
/// is free again (the control).
pub async fn test_setup_token_concurrency(test: &mut TestServer) {
    use crate::utils::za::{ConcurrentRequestsGuard, HookParked};
    use common::za::test_hooks;
    use registry::schema::enums::Permission;
    println!("Running zero-access setup-token concurrency test...");
    const CALLER: &str = "setup-token-limit-caller@example.com";
    const TARGET: &str = "setup-token-limit@example.com";
    let admin = test.account("admin@example.com").clone();
    // Set before the caller's first request, so its token carries the limit.
    let limit = ConcurrentRequestsGuard::set(&admin, 1).await;
    let caller = admin
        .create_user_account(
            CALLER,
            STRONG,
            "Provisioner",
            &[],
            vec![Permission::SysAccountUpdate],
        )
        .await;
    let mut target = admin
        .create_passwordless_user_account(TARGET, STRONG, "Limit", &[], user_permissions())
        .await;
    let target_id = target.id().document_id();

    let first = caller.clone();
    let parked = HookParked::start(target_id, test_hooks::set_setup_token_pause, async move {
        za_post_as(&first, "setup-token", &json!({ "account": TARGET })).await
    })
    .await;
    let second = za_post_as(&caller, "setup-token", &json!({ "account": TARGET })).await;
    assert_eq!(
        second.status, 400,
        "a second setup-token ran while the first held the caller's only slot: {}",
        second.body
    );
    assert_eq!(
        second.body["limit"], "maxConcurrentRequests",
        "the second setup-token was refused for another reason: {}",
        second.body
    );
    parked.finish().await.expect(200);
    // Control: the first request released its slot when it returned.
    let token = za_post_as(&caller, "setup-token", &json!({ "account": TARGET }))
        .await
        .str(200, "token");
    limit.restore().await;

    target.recovery_key = Some(za_setup(TARGET, &token, STRONG).await);
    test.insert_account(target);
    admin.destroy_account(caller).await;
}
