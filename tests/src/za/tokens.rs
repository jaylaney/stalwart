/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    STRONG,
    drain::DrainOn,
    mail::{self, Traced, TracedEvent, live_keys},
    pinning::{forget, lookup, pin},
    setup::set_auth_ban_rate,
    uid_lease::put,
    user_permissions,
};
use crate::utils::{
    account::Account,
    pop3::{Pop3Connection, ResponseType as Pop3Reply},
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        CALENDAR_JMAP_USING, Conn, HookParked, Parked, SERVER_URL, VaultReply, ZA_SIGNIN_SCOPE,
        ZaClientKind, ZaOAuthClient, ZaTokens, assert_nothing_cached, code, handoff_exists,
        oauth_expiry, park_login, raw_token_record, run_purge_maintenance, set_raw_token_record,
        substitute_public_key, token_record, totp_url, trigger_drain, vault_revision,
        wait_for_index_queue, wrap_mark, writer_key, za_authorize, za_bearer_dav, za_bearer_jmap,
        za_bearer_session, za_confirm_consent, za_drop_oauth_client, za_exchange, za_grant_token,
        za_keyed_grant, za_oauth_client, za_post, za_refresh, za_revoke_grant, za_revoke_token,
        za_sign_in, za_split_token, za_tamper_secret, za_unlock,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{
    KV_OAUTH,
    auth::{
        credential::AppPassword,
        oauth::{grant::GrantRotation, scope::OAuthScopes},
        vault::{RawValue, ZaMark, ZaVerification},
    },
    za::{
        ZA_MAIL_KEYLESS, ZA_MAIL_LOGIN_REFUSED, test_hooks,
        tokens::{ZaIssued, ZaTokenIssue, ZaTokenRefresh, za_split_bearer},
    },
};
use http::auth::{
    authenticate::za_test,
    oauth::{DeviceAuthResponse, ErrorType, OAuthResponse, TokenResponse, auth::LoginResponse},
};
use registry::{
    schema::{
        prelude::{Object, ObjectInner, ObjectType, Property},
        structs,
    },
    types::{id::ObjectId, ipmask::IpAddrOrMask},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use store::{
    ValueKey,
    dispatch::lookup::KeyValue,
    registry::{
        RegistryQuery,
        write::{RegistryWrite, RegistryWriteResult},
    },
    write::{BatchBuilder, now},
};
use trc::{AuthEvent, EventType, HttpEvent, Key};
use types::{collection::Collection, field::PrincipalField, id::Id};
use vault::{
    keys::{Secret, derive_token_next_key, derive_token_tag, token_next_aad, unwrap_key},
    record::za_session_binding,
    session::SessionKeys,
    tokens::{
        CODE_LEN, TOKEN_RECORD_VERSION, ZaRefreshClass, ZaSuffix, append_code_secret, append_secret,
    },
};

const OWNER: &str = "token-owner@example.com";
const MFA: &str = "token-mfa@example.com";
const CONSENT: &str = "token-consent@example.com";
const RACE: &str = "token-race@example.com";
const CAP: &str = "token-cap@example.com";
const PLAIN: &str = "token-plain@example.com";
const REMOVED: &str = "token-removed@example.com";
const GONE: &str = "token-gone@example.com";
const FACTOR: &str = "token-factor@example.com";
const CAL: &str = "/dav/cal/token-owner%40example.com/";
const AUTH_FAILED: EventType = EventType::Auth(AuthEvent::Failed);
const STRONG2: &str = "another long passphrase with 2 numbers";
const PLANTED: &[u8] = &[1, 0xde, 0xad];
const HOUR: u64 = 3_600;
const MONTH: u64 = 2_592_000;

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access token tests...");
    let admin = test.account("admin@example.com").clone();
    for (name, description) in [
        (MFA, "Token MFA"),
        (OWNER, "Token Owner"),
        (CAP, "Token Cap"),
        (CONSENT, "Token Consent"),
        (RACE, "Token Race"),
        (FACTOR, "Token Factor"),
    ] {
        let account = admin
            .create_key_user_account(name, STRONG, description, &[], user_permissions())
            .await;
        test.insert_account(account);
    }
    let owner = test.account(OWNER).id().document_id();
    let mfa = test.account(MFA).id().document_id();
    let cap = test.account(CAP).id().document_id();
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Token Plain", &[], user_permissions())
        .await;
    let plain_id = plain.id().document_id();

    pin_rewrap_keeps_session(test, owner).await;
    credential_changes_end_sessions(test, owner, mfa).await;
    other_vault_writes_keep_sessions(test, owner).await;
    issue_writes_first_entries(test, owner).await;
    issue_refuses_changed_binding(test, owner).await;
    open_access_needs_entry_and_binding(test, owner).await;
    forget_grant_asserts_the_record_alone(test, owner).await;
    revocation_removes_entries(test, owner).await;
    cap_evicts_least_recently_used(test, cap).await;
    forget_failure_does_not_fail_deletion(test, owner).await;
    ordinary_grant_deletion_reads_no_record(test, plain_id).await;
    server_token_answers(test, owner, plain_id).await;

    let owner_account = test.account(OWNER).clone();
    keyed_bearer_reads_sealed_data(test, &owner_account).await;
    tampered_or_foreign_secret_is_401(test, &owner_account).await;
    suffix_on_ordinary_token_is_401(test, plain_id).await;
    keyed_token_without_secret_is_keyless(test, &owner_account).await;
    bearer_crossed_by_vault_write_is_refused(test, owner).await;
    keyed_entry_bounded_by_token_expiry(test, owner).await;
    keyed_entry_ends_before_signed_expiry(test, owner).await;
    expired_second_decode_publishes_nothing(test, owner).await;
    keyed_request_crossed_by_revocation(test, owner).await;
    mail_bearer_login_stays_refused(test, owner).await;
    revocation_and_introspection_accept_the_suffix(test, owner).await;

    let app =
        Arc::new(za_oauth_client(test, &admin, "za-token-app", ZaClientKind::Application).await);
    let registered =
        Arc::new(za_oauth_client(test, &admin, "za-token-consent", ZaClientKind::Registered).await);
    sign_in_gives_keyed_tokens(test, &app).await;
    deleted_grant_evicted_before_cleanup(test, &app).await;
    sign_in_with_second_factor(test, &app).await;
    second_factor_change_ends_sessions(test, &app).await;
    consent_path(test, &registered).await;
    exchange_without_handoff_is_invalid_grant(test, &app).await;
    ordinary_code_with_suffix_is_invalid_grant(test, &app, plain_id).await;
    handoff_gone_after_redemption_and_expiry(test, &app).await;
    device_flow_stays_keyless(test, &registered).await;
    refresh_without_secret_of_keyed_grant_refused(test, &app, &registered).await;
    refresh_rotates_and_reads(test, &app).await;
    retried_refresh_returns_the_same_token(test, &app).await;
    replayed_refresh_revokes(test, &app).await;
    superseded_token_opens_nothing(test, &app).await;
    refresh_with_missing_entry_deletes_grant(test, &app).await;
    malformed_code_lookups_miss_the_handoff(test, &app).await;
    classification_failure_removes_the_handoff(test, &admin, &app).await;

    handoff_crossed_by_password_change(test).await;
    handoff_crossed_by_other_vault_write(test).await;
    sign_in_crossed_by_vault_write_issues_no_code(test, &app).await;

    let race = test.account(RACE).clone();
    put_sign_in_canaries(test, &race).await;
    exchange_crossed_by_password_change(test, &app).await;
    exchange_crossed_by_recovery(test, &app).await;
    exchange_crossed_by_other_vault_write(test, &app).await;
    remove_sign_in_canaries(test, &race).await;

    refresh_crossed_by_credential_change(test, &app).await;
    refresh_crossed_by_other_vault_write(test, &app).await;
    retry_of_g_while_g1_completes(test, &app).await;
    retry_reading_a_stale_record_keeps_the_grant(test, &app).await;
    failed_write_then_client_retry(test, &app).await;
    interrupted_refresh_cleared_by_another_grants_write(test, &app).await;

    thirty_third_sign_in_ends_the_oldest(test, &app).await;
    granted_answer_is_untraced(&app, test).await;
    traced_urls_mask_the_code(test).await;
    sweep_removes_expired_entries(test, &admin, &app, owner).await;
    registry_destroyed_grant(test, &admin, &owner_account, &app).await;
    interrupted_refresh_cleared_by_sweep(test, &admin, &app, owner).await;
    sweep_passes_a_broken_record(test, &admin, &app, owner, mfa, race.id().document_id()).await;
    account_destruction_removes_the_record(test, &admin, &app).await;

    for id in [
        owner,
        mfa,
        cap,
        test.account(CONSENT).id().document_id(),
        race.id().document_id(),
        test.account(FACTOR).id().document_id(),
        plain_id,
    ] {
        for grant in grants_of(test, id).await {
            za_revoke_grant(&test.server, grant.id()).await;
        }
    }
    for client in [app, registered] {
        za_drop_oauth_client(
            test,
            &admin,
            Arc::into_inner(client).expect("OAuth requests finished"),
        )
        .await;
    }

    wait_for_index_queue(test, plain_id, Duration::from_secs(30)).await;
    admin.destroy_account(plain).await;
    test.wait_for_tasks().await;
}

async fn minted(
    test: &TestServer,
    name: &str,
    id: u32,
    password: &str,
) -> (u64, String, Arc<SessionKeys>, u64) {
    let (grant, access) = za_grant_token(&test.server, name, OAuthScopes::ALL).await;
    let keys = za_unlock(test, id, password).await;
    let vault = test.server.za_vault_record(id).await.unwrap().unwrap();
    (grant, access, keys, za_session_binding(&vault.record))
}

async fn issued_for(
    test: &TestServer,
    name: &str,
    id: u32,
) -> (u64, ZaIssued, Arc<SessionKeys>, u64) {
    let (grant, _, keys, binding) = minted(test, name, id, STRONG).await;
    let ZaTokenIssue::Issued(issued) = issue(test, id, grant, keys.mk(), binding).await else {
        panic!("the grant was not issued");
    };
    (grant, issued, keys, binding)
}

async fn issue(test: &TestServer, id: u32, grant: u64, mk: &Secret, binding: u64) -> ZaTokenIssue {
    test.server
        .za_token_issue(id, grant, mk, binding, now() + HOUR, now() + MONTH)
        .await
        .unwrap()
}

async fn keyed_grant(test: &TestServer, id: u32) -> (u64, String, ZaIssued) {
    za_keyed_grant(test, OWNER, id, STRONG, OAuthScopes::ALL).await
}

async fn keyed_bearer_reads_sealed_data(test: &TestServer, owner: &Account) {
    println!("  keyed_bearer_reads_sealed_data");
    let id = owner.id().document_id();
    let event_path = format!("{CAL}default/token-canary.ics");
    let event = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//ZA//EN\r\n\
                 BEGIN:VEVENT\r\nUID:token-canary\r\nDTSTAMP:20261020T100000Z\r\n\
                 DTSTART:20261020T100000Z\r\nDTEND:20261020T110000Z\r\n\
                 SUMMARY:token canary event\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    assert_eq!(
        put(OWNER, STRONG, event_path.clone(), event.to_owned()).await,
        201
    );
    mail::deliver(
        test,
        "sender@example.org",
        OWNER,
        &mail::message_with("token canary subject", "body"),
    )
    .await;

    let (grant, keyed, _) = keyed_grant(test, id).await;
    let response = za_bearer_dav(owner, &keyed)
        .request("GET", &event_path, "")
        .await;
    assert_eq!(response.status.as_u16(), 200, "keyed CalDAV GET");
    assert!(
        response
            .body
            .is_ok_and(|body| body.contains("SUMMARY:token canary event")),
        "the keyed CalDAV GET opens the event summary"
    );

    let (status, response) = za_bearer_jmap(
        &keyed,
        CALENDAR_JMAP_USING,
        json!([
            ["Email/query", { "accountId": owner.id_string() }, "q"],
            ["Email/get", {
                "accountId": owner.id_string(),
                "#ids": { "resultOf": "q", "name": "Email/query", "path": "/ids" },
                "properties": ["subject"],
            }, "g"],
        ]),
    )
    .await;
    assert_eq!(status, 200);
    assert!(response.0["methodResponses"][0][0].as_str() == Some("Email/query"));
    assert!(response.0["methodResponses"][1][0].as_str() == Some("Email/get"));
    assert!(
        response.0["methodResponses"][1][1]["list"][0]["subject"].as_str()
            == Some("token canary subject"),
        "the keyed Email/get opens the message subject"
    );
    assert_eq!(za_bearer_session(&keyed).await.0, 200);
    let cache = &test.server.inner.cache;
    assert!(cache.za_keys.contains_account(id));
    assert!(cache.http_auth.get(&keyed).is_some());

    let basic = DummyWebDavClient::new(id, OWNER, STRONG, OWNER);
    assert_eq!(
        basic
            .request("DELETE", &event_path, "")
            .await
            .status
            .as_u16(),
        204
    );
    test.destroy_all_mailboxes(owner).await;
    za_revoke_grant(&test.server, grant).await;
}

async fn tampered_or_foreign_secret_is_401(test: &TestServer, owner: &Account) {
    println!("  tampered_or_foreign_secret_is_401");
    let id = owner.id().document_id();
    let (grant, keyed, issued) = keyed_grant(test, id).await;
    assert_eq!(za_bearer_session(&keyed).await.0, 200);
    assert!(test.server.inner.cache.http_auth.get(&keyed).is_some());

    let upstream = za_split_token(&keyed).0;
    let (other_grant, other_issued, _, _) = issued_for(test, OWNER, id).await;
    let mut refresh = upstream.to_owned();
    append_secret(&mut refresh, &issued.refresh);
    let mut foreign = upstream.to_owned();
    append_secret(&mut foreign, &other_issued.access);
    let cases = [
        za_tamper_secret(&keyed),
        refresh,
        foreign,
        format!("{upstream}~short"),
        format!("{keyed}~"),
    ];

    let traced = Traced::start(&[AUTH_FAILED]);
    for (case, token) in cases.iter().enumerate() {
        assert_eq!(za_bearer_session(token).await.0, 401, "session case {case}");
        let response = za_bearer_dav(owner, token)
            .request("PROPFIND", CAL, "")
            .await;
        assert_eq!(response.status.as_u16(), 401, "CalDAV case {case}");
        assert_eq!(
            za_bearer_jmap(token, CALENDAR_JMAP_USING, mail_get(owner))
                .await
                .0,
            401,
            "JMAP case {case}"
        );
    }
    let events = traced.finish().await;
    for (reason, count) in [
        ("zero-access: token keys unavailable", 9),
        ("zero-access: malformed token secret", 6),
    ] {
        assert_eq!(
            events
                .iter()
                .filter(|event| bearer_failure(event, id, grant, reason))
                .count(),
            count,
            "{reason}"
        );
    }
    for event in &events {
        assert!(
            !event.text.contains(upstream),
            "the token appeared in a trace"
        );
        for token in [&keyed, &cases[0], &cases[1], &cases[2]] {
            let secret = za_split_token(token).1.expect("expected a token secret");
            assert!(!event.text.contains(secret), "a secret appeared in a trace");
        }
    }

    // Spec 4.6 and 9.2 (D27): a fork refusal is not counted against its
    // address. With the ban rate lowered to five, six refusals from one
    // forwarded address would ban it if they were counted (final review FA-2,
    // as `setup.rs` shows for setup), and a later request from it would be
    // answered 429.
    const D27_IP: &str = "10.0.60.20";
    const BAN_RATE: u64 = 5;
    let admin = test.account("admin@example.com").clone();
    set_auth_ban_rate(&admin, BAN_RATE).await;
    for attempt in 0..=BAN_RATE {
        assert_eq!(
            bearer_session_from(D27_IP, &cases[0]).await,
            401,
            "fork refusal {attempt} from one address"
        );
    }
    assert!(
        test.server
            .registry()
            .primary_key(
                ObjectType::BlockedIp.into(),
                Property::Address,
                IpAddrOrMask::from_ip(D27_IP.parse().unwrap()).to_index_key(),
            )
            .await
            .unwrap()
            .is_none(),
        "fork refusals banned their address"
    );
    assert_eq!(bearer_session_from(D27_IP, &keyed).await, 200);
    set_auth_ban_rate(&admin, 100).await;
    assert_eq!(za_bearer_session(&keyed).await.0, 200);
    za_revoke_grant(&test.server, grant).await;
    za_revoke_grant(&test.server, other_grant).await;
}

/// `GET /jmap/session` with `token`, sent from `ip` through `X-Forwarded-For`.
async fn bearer_session_from(ip: &str, token: &str) -> u16 {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .get(format!("{SERVER_URL}/jmap/session"))
        .bearer_auth(token)
        .header("X-Forwarded-For", ip)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

fn bearer_failure(event: &TracedEvent, id: u32, grant: u64, reason: &str) -> bool {
    event.account_id == Some(u64::from(id))
        && event.details == reason
        && event.keys.iter().any(|(key, value)| {
            *key == Key::Id && matches!(value, trc::Value::UInt(value) if *value == grant)
        })
}

fn mail_get(owner: &Account) -> Value {
    json!([["Email/get", { "accountId": owner.id_string(), "ids": [] }, "g"]])
}

async fn suffix_on_ordinary_token_is_401(test: &TestServer, id: u32) {
    println!("  suffix_on_ordinary_token_is_401");
    let (grant, token) = za_grant_token(&test.server, PLAIN, OAuthScopes::ALL).await;
    assert_eq!(za_bearer_session(&token).await.0, 200);
    let mut suffixed = token.clone();
    append_secret(&mut suffixed, &Secret::random());
    let traced = Traced::start(&[AUTH_FAILED]);
    assert_eq!(za_bearer_session(&suffixed).await.0, 401);
    assert!(traced.finish().await.iter().any(|event| bearer_failure(
        event,
        id,
        grant,
        "zero-access: token secret on an ordinary account's token"
    )));
    assert_eq!(za_bearer_session(&token).await.0, 200);
    za_revoke_grant(&test.server, grant).await;
}

async fn keyed_token_without_secret_is_keyless(test: &TestServer, owner: &Account) {
    println!("  keyed_token_without_secret_is_keyless");
    let (grant, keyed, _) = keyed_grant(test, owner.id().document_id()).await;
    let upstream = za_split_token(&keyed).0;
    assert_eq!(za_bearer_session(upstream).await.0, 200);
    let (status, response) = za_bearer_jmap(upstream, CALENDAR_JMAP_USING, mail_get(owner)).await;
    assert_eq!(status, 200);
    assert!(response.is_error_at(0));
    let error = &response.0["methodResponses"][0][1];
    assert!(error["type"].as_str() == Some("forbidden"));
    assert!(error["description"].as_str() == Some(ZA_MAIL_KEYLESS));
    let response = za_bearer_dav(owner, upstream)
        .request("PROPFIND", CAL, "")
        .await;
    assert_eq!(response.status.as_u16(), 403);
    za_revoke_grant(&test.server, grant).await;
}

async fn bearer_crossed_by_vault_write_is_refused(test: &mut TestServer, id: u32) {
    println!("  bearer_crossed_by_vault_write_is_refused");
    let superseded = "zero-access: token keys of a superseded generation";
    let (grant, keyed, _) = keyed_grant(test, id).await;
    let traced = Traced::start(&[AUTH_FAILED]);
    let parked = Parked::start(
        id,
        za_test::set,
        async move { za_bearer_session(&keyed).await },
    )
    .await;
    change_password(STRONG, STRONG2).await;
    assert_eq!(parked.finish().await.0, 401);
    assert_nothing_cached(test, id);
    assert!(
        traced
            .finish()
            .await
            .iter()
            .any(|event| bearer_failure(event, id, grant, superseded))
    );
    za_revoke_grant(&test.server, grant).await;

    let parked = park_login(id, OWNER, STRONG2).await;
    change_password(STRONG2, STRONG).await;
    assert_eq!(parked.finish().await, 207);

    let (grant, keyed, _) = keyed_grant(test, id).await;
    let token = keyed.clone();
    let traced = Traced::start(&[AUTH_FAILED]);
    let parked = Parked::start(
        id,
        za_test::set,
        async move { za_bearer_session(&token).await },
    )
    .await;
    new_recovery_key(test).await;
    assert_eq!(parked.finish().await.0, 401);
    assert_nothing_cached(test, id);
    assert!(
        traced
            .finish()
            .await
            .iter()
            .any(|event| bearer_failure(event, id, grant, superseded))
    );
    assert_eq!(za_bearer_session(&keyed).await.0, 200);
    za_revoke_grant(&test.server, grant).await;
}

/// `GET /jmap/session` with `token`, through a client whose timeout outlasts
/// a parked request.
async fn slow_session_status(token: String) -> u16 {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .get(format!("{SERVER_URL}/jmap/session"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap_or_else(|_| panic!("Bearer request failed"))
        .status()
        .as_u16()
}

/// PR #90 reviews C4617611615 and C4633314515: a keyed entry ends before its
/// token's signed expiry; at publication its deadline is lowered to the
/// token's fresh remaining lifetime less one second, so neither the fork's key
/// lookup nor the lookups after the decode extend it. A five-second token's
/// request parked after its key lookup for seven seconds, so past the token's
/// expiry, is served, uncached: no cache entry and no resident keys under the
/// token, and its next request is refused.
async fn keyed_entry_bounded_by_token_expiry(test: &TestServer, id: u32) {
    println!("  keyed_entry_bounded_by_token_expiry");
    let (grant, keyed, _) = {
        let _expiry = oauth_expiry(test, Some(5), None, None);
        keyed_grant(test, id).await
    };
    let parked = Parked::start(id, za_test::set, slow_session_status(keyed.clone())).await;
    tokio::time::sleep(Duration::from_secs(7)).await;
    assert_eq!(
        parked.finish().await,
        200,
        "a request whose key lookup outlasted its token is served uncached"
    );
    let cache = &test.server.inner.cache;
    assert!(
        cache.http_auth.get(&keyed).is_none(),
        "an expired token's entry was published"
    );
    assert!(
        cache
            .za_keys
            .get(&cache.za_fingerprint(&keyed), std::time::Instant::now())
            .is_none(),
        "an expired token's keys were made resident"
    );
    assert_eq!(
        za_bearer_session(&keyed).await.0,
        401,
        "an expired token was served from the cache"
    );
    za_revoke_grant(&test.server, grant).await;
}

/// PR #90 review C4633314515: an authentication slowed between the token's
/// decode and its grant read does not carry the entry past the signed expiry.
/// A fifteen-second token's first request is parked in the version check,
/// after the decode, until ten seconds and served, and its entry is cached; a
/// second request more than a second past the signed expiry is refused, and
/// nothing remains under the token. Without the lowered deadline the entry
/// would end the park's length late and serve it. Margins, from the instant
/// `keyed_grant` returns (the token is sealed just before it, so its signed
/// expiry lies after 14 s and by 15 s on the token clock): the release at
/// 10 s comes more than 3 s before the signed expiry, so publication takes
/// the lowering path; the second request at 16.2 s comes at least 1.2 s past
/// the signed expiry; and an unlowered entry ends after 19 s (the release,
/// plus a first lifetime of at least 10 s even when the first decode comes as
/// late as the five seconds the pause allows, less a second of the cache
/// clock's rounding), at least 2.8 s after the second request.
async fn keyed_entry_ends_before_signed_expiry(test: &TestServer, id: u32) {
    println!("  keyed_entry_ends_before_signed_expiry");
    let (grant, keyed, _) = {
        let _expiry = oauth_expiry(test, Some(15), None, None);
        keyed_grant(test, id).await
    };
    let sealed = tokio::time::Instant::now();
    let parked = HookParked::start(
        id,
        test_hooks::set_token_version_check_pause,
        slow_session_status(keyed.clone()),
    )
    .await;
    tokio::time::sleep_until(sealed + Duration::from_secs(10)).await;
    assert_eq!(
        parked.finish().await,
        200,
        "a request slowed after its decode is served"
    );
    let cache = &test.server.inner.cache;
    assert!(
        cache.http_auth.get(&keyed).is_some(),
        "no entry was published before the signed expiry, so the lowering was not tested"
    );
    tokio::time::sleep_until(sealed + Duration::from_millis(16_200)).await;
    assert_eq!(
        za_bearer_session(&keyed).await.0,
        401,
        "a cache entry outlived its token's signed expiry"
    );
    assert!(
        cache.http_auth.get(&keyed).is_none(),
        "an expired token's entry stayed cached"
    );
    assert!(
        cache
            .za_keys
            .get(&cache.za_fingerprint(&keyed), std::time::Instant::now())
            .is_none(),
        "an expired token's keys stayed resident"
    );
    za_revoke_grant(&test.server, grant).await;
}

/// PR #90 review C4633314515 (RC5-1): publication past the signed expiry
/// publishes nothing even while the first deadline is still ahead; only the
/// expired second decode stops a live entry. A ten-second token's request is
/// parked in the version check, after the decode, until ten seconds, then
/// after its key lookup until 11.2 s, and served. Margins, from the instant
/// `keyed_grant` returns (the signed expiry lies after 9 s and by 10 s on the
/// token clock): the publication at 11.2 s comes at least 1.2 s past the
/// signed expiry; and the first deadline, built at the first release, ends
/// after 14 s (the release, plus a first lifetime of at least 5 s even when
/// the first decode comes as late as the five seconds the pause allows, less
/// a second of the cache clock's rounding), at least 2.8 s after the
/// publication.
async fn expired_second_decode_publishes_nothing(test: &TestServer, id: u32) {
    println!("  expired_second_decode_publishes_nothing");
    let (grant, keyed, _) = {
        let _expiry = oauth_expiry(test, Some(10), None, None);
        keyed_grant(test, id).await
    };
    let sealed = tokio::time::Instant::now();
    let version_check = Arc::new(test_hooks::Pause::new(id));
    let publication = Arc::new(za_test::Pause::new(id));
    test_hooks::set_token_version_check_pause(Some(version_check.clone()));
    za_test::set(Some(publication.clone()));
    let request = tokio::spawn(slow_session_status(keyed.clone()));
    tokio::time::timeout(Duration::from_secs(5), version_check.arrived.notified())
        .await
        .expect("the request did not reach the version check");
    tokio::time::sleep_until(sealed + Duration::from_secs(10)).await;
    version_check.release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), publication.arrived.notified())
        .await
        .expect("the request did not reach the publication pause");
    tokio::time::sleep_until(sealed + Duration::from_millis(11_200)).await;
    publication.release.notify_one();
    assert_eq!(
        request.await.unwrap(),
        200,
        "a request whose token expired before publication is served"
    );
    test_hooks::set_token_version_check_pause(None);
    za_test::set(None);
    let cache = &test.server.inner.cache;
    assert!(
        cache.http_auth.get(&keyed).is_none(),
        "an entry was published after the signed expiry"
    );
    assert!(
        cache
            .za_keys
            .get(&cache.za_fingerprint(&keyed), std::time::Instant::now())
            .is_none(),
        "keys were made resident after the signed expiry"
    );
    assert_eq!(
        za_bearer_session(&keyed).await.0,
        401,
        "an expired token was served from the cache"
    );
    za_revoke_grant(&test.server, grant).await;
}

/// W-1 (fix-wave review): keys are not published across a revocation. A keyed
/// request parked after its key lookup while its grant is revoked leaves
/// nothing of the account cached.
async fn keyed_request_crossed_by_revocation(test: &TestServer, id: u32) {
    println!("  keyed_request_crossed_by_revocation");
    let (grant, keyed, _) = keyed_grant(test, id).await;
    let parked = Parked::start(id, za_test::set, slow_session_status(keyed.clone())).await;
    za_revoke_grant(&test.server, grant).await;
    assert_eq!(parked.finish().await, 200);
    assert_nothing_cached(test, id);
    assert_eq!(za_bearer_session(&keyed).await.0, 401);
}

async fn mail_bearer_login_stays_refused(test: &TestServer, id: u32) {
    println!("  mail_bearer_login_stays_refused");
    let (grant, keyed, _) = keyed_grant(test, id).await;
    let base = live_keys(id);
    let oauthbearer = STANDARD.encode(format!("n,a={OWNER},\u{1}auth=Bearer {keyed}\u{1}\u{1}"));
    let xoauth2 = STANDARD.encode(format!("user={OWNER}\u{1}auth=Bearer {keyed}\u{1}\u{1}"));
    let mut imap = Conn::open(9992).await;
    imap.until(&["* OK"]).await;
    for (mechanism, response) in [("OAUTHBEARER", &oauthbearer), ("XOAUTH2", &xoauth2)] {
        imap.send(&format!("k1 AUTHENTICATE {mechanism} {response}\r\n"))
            .await;
        let line = imap.until(&["k1 "]).await;
        assert!(
            line.trim_end() == format!("k1 NO [NOPERM] {ZA_MAIL_LOGIN_REFUSED}"),
            "{mechanism}"
        );
        assert_eq!(live_keys(id), base, "{mechanism}: no keys after refusal");
        imap.exchange("k2 NOOP\r\n", "k2 OK").await;
    }
    imap.exchange("k3 LOGOUT\r\n", "k3 OK").await;

    let mut pop3 = Pop3Connection::connect().await;
    pop3.send(&format!("AUTH XOAUTH2 {xoauth2}")).await;
    let reply = pop3.read(false).await;
    assert!(
        reply
            .last()
            .is_some_and(|line| *line == format!("-ERR {ZA_MAIL_LOGIN_REFUSED}"))
    );
    assert_eq!(live_keys(id), base, "POP3: no keys after refusal");
    pop3.send("QUIT").await;
    pop3.assert_read(Pop3Reply::Ok).await;
    za_revoke_grant(&test.server, grant).await;
}

async fn revocation_and_introspection_accept_the_suffix(test: &TestServer, id: u32) {
    println!("  revocation_and_introspection_accept_the_suffix");
    let (grant, keyed, _) = keyed_grant(test, id).await;
    let decoded = test
        .server
        .decode_access_token(None, &keyed)
        .unwrap_or_else(|_| panic!("decode the keyed token for revocation"));
    assert_eq!(decoded.account_id, id);
    assert_eq!(decoded.grant.map(|token_grant| token_grant.id), Some(grant));
    for token in [&keyed, &za_tamper_secret(&keyed)] {
        assert!(
            test.server
                .introspect_access_token(token)
                .await
                .is_ok_and(|response| response.active),
            "introspection reports the upstream grant's state"
        );
    }
    za_revoke_grant(&test.server, grant).await;
}

async fn issue_writes_first_entries(test: &TestServer, id: u32) {
    println!("  issue_writes_first_entries");
    let (grant, issued, keys, binding) = issued_for(test, OWNER, id).await;
    let record = token_record(test, id).await.unwrap();
    assert_eq!(record.version, TOKEN_RECORD_VERSION);
    assert_eq!(record.binding, binding);
    assert_eq!(record.grants.len(), 1);
    let entries = record.grant(grant).unwrap();
    assert_eq!(entries.refresh.len(), 1);
    assert_eq!(entries.refresh[0].generation, 0);
    assert!(entries.refresh[0].next.is_none());
    assert_eq!(entries.access.len(), 1);
    assert!(entries.access[0].tag == derive_token_tag(&issued.access));
    let opened = record
        .open_access(id, grant, &issued.access, now())
        .unwrap();
    assert!(opened.as_bytes() == keys.mk().as_bytes());
    assert!(matches!(
        record.classify_refresh(id, grant, 0, &issued.refresh, now()),
        ZaRefreshClass::Opened { next: None, .. }
    ));
    za_revoke_grant(&test.server, grant).await;
}

async fn issue_refuses_changed_binding(test: &mut TestServer, id: u32) {
    println!("  issue_refuses_changed_binding");
    let (grant, _, keys, stale) = minted(test, OWNER, id, STRONG).await;
    new_recovery_key(test).await;
    assert!(matches!(
        issue(test, id, grant, keys.mk(), stale).await,
        ZaTokenIssue::Issued(_)
    ));
    assert_eq!(token_record(test, id).await.unwrap().binding, stale);
    za_revoke_grant(&test.server, grant).await;

    let (grant, _, keys, stale) = minted(test, OWNER, id, STRONG).await;
    change_password(STRONG, STRONG2).await;
    assert!(matches!(
        issue(test, id, grant, keys.mk(), stale).await,
        ZaTokenIssue::VaultChanged
    ));
    assert!(raw_token_record(test, id).await.is_none());
    za_revoke_grant(&test.server, grant).await;
    change_password(STRONG2, STRONG).await;
}

async fn open_access_needs_entry_and_binding(test: &mut TestServer, id: u32) {
    println!("  open_access_needs_entry_and_binding");
    let (grant, issued, keys, _) = issued_for(test, OWNER, id).await;
    let (other, _, _, _) = issued_for(test, OWNER, id).await;
    let raw = raw_token_record(test, id).await.unwrap();
    let revision = vault_revision(test, id).await;
    let opened = test
        .server
        .za_token_open_access(id, grant, &issued.access)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(opened.generation, revision);
    assert_eq!(opened.account_id, id);
    assert!(opened.mk().as_bytes() == keys.mk().as_bytes());
    let random = Secret::random();
    for (grant, secret) in [
        (grant, &random),
        (other, &issued.access),
        (grant, &issued.refresh),
    ] {
        assert!(
            test.server
                .za_token_open_access(id, grant, secret)
                .await
                .unwrap()
                .is_none()
        );
    }

    new_recovery_key(test).await;
    let reopened = test
        .server
        .za_token_open_access(id, grant, &issued.access)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.generation, revision + 1);
    assert!(reopened.mk().as_bytes() == keys.mk().as_bytes());

    change_password(STRONG, STRONG2).await;
    assert!(
        test.server
            .za_token_open_access(id, grant, &issued.access)
            .await
            .unwrap()
            .is_none()
    );
    assert!(raw_token_record(test, id).await.is_none());

    set_raw_token_record(test, id, Some(&raw)).await;
    assert!(
        test.server
            .za_token_open_access(id, grant, &issued.access)
            .await
            .unwrap()
            .is_none()
    );
    set_raw_token_record(test, id, None).await;
    change_password(STRONG2, STRONG).await;
    za_revoke_grant(&test.server, grant).await;
    za_revoke_grant(&test.server, other).await;
}

async fn new_recovery_key(test: &mut TestServer) {
    let reply = za_post(
        "recovery-key",
        &json!({ "username": OWNER, "password": STRONG }),
    )
    .await;
    assert_eq!(reply.status, 200);
    test.accounts.get_mut(OWNER).unwrap().recovery_key =
        Some(reply.body["recovery_key"].as_str().unwrap().to_owned());
}

async fn change_password(old: &str, new: &str) {
    let reply = za_post(
        "password",
        &json!({ "username": OWNER, "password": old, "new_password": new }),
    )
    .await;
    assert_eq!(reply.status, 200);
}

async fn forget_grant_asserts_the_record_alone(test: &TestServer, id: u32) {
    println!("  forget_grant_asserts_the_record_alone");
    let (g1, _, _, _) = issued_for(test, OWNER, id).await;
    let (g2, _, _, _) = issued_for(test, OWNER, id).await;
    let raw_vault = test
        .server
        .store()
        .get_value::<RawValue>(ValueKey::property(
            id,
            Collection::Principal,
            0,
            PrincipalField::ZeroAccessVault,
        ))
        .await
        .unwrap()
        .unwrap()
        .0;
    let mut broken_vault = raw_vault.clone();
    *broken_vault.last_mut().unwrap() ^= u8::MAX;
    set_raw_vault(test, id, broken_vault).await;
    assert!(test.server.za_vault_record(id).await.is_err());

    test.server.za_token_forget_grant(id, g1).await.unwrap();
    let record = token_record(test, id).await.unwrap();
    assert!(record.grant(g1).is_none());
    assert!(record.grant(g2).is_some());
    let first = raw_token_record(test, id).await.unwrap();
    test.server.za_token_forget_grant(id, g1).await.unwrap();
    assert!(raw_token_record(test, id).await.as_ref() == Some(&first));

    set_raw_vault(test, id, raw_vault).await;
    test.server.za_invalidate_account(id).await.unwrap();
    za_revoke_grant(&test.server, g1).await;
    za_revoke_grant(&test.server, g2).await;
}

async fn set_raw_vault(test: &TestServer, id: u32, raw: Vec<u8>) {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .set(PrincipalField::ZeroAccessVault, raw);
    test.server.store().write_batch(&mut batch).await.unwrap();
}

async fn revocation_removes_entries(test: &TestServer, id: u32) {
    println!("  revocation_removes_entries");
    let (g1, _, _, _) = issued_for(test, OWNER, id).await;
    let (g2, _, _, _) = issued_for(test, OWNER, id).await;
    za_revoke_grant(&test.server, g1).await;
    assert!(token_record(test, id).await.unwrap().grant(g1).is_none());
    za_revoke_grant(&test.server, g2).await;
    assert!(raw_token_record(test, id).await.is_none());
}

async fn cap_evicts_least_recently_used(test: &TestServer, id: u32) {
    println!("  cap_evicts_least_recently_used");
    let mut grants = Vec::with_capacity(32);
    for _ in 0..32 {
        let (grant, _, _, _) = issued_for(test, CAP, id).await;
        grants.push(grant);
    }
    assert_eq!(token_record(test, id).await.unwrap().grants.len(), 32);

    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let stored = test.server.oauth_grant(grants[0]).await.unwrap().unwrap();
    assert!(matches!(
        test.server.rotate_oauth_grant(stored, 0).await.unwrap(),
        GrantRotation::Issue { .. }
    ));
    let (last, _, _, _) = issued_for(test, CAP, id).await;
    let record = token_record(test, id).await.unwrap();
    assert_eq!(record.grants.len(), 32);
    assert!(record.grant(grants[1]).is_none());
    assert!(record.grant(grants[0]).is_some());
    assert!(record.grant(last).is_some());
    assert!(test.server.oauth_grant(grants[1]).await.unwrap().is_none());
    assert!(test.server.oauth_grant(grants[0]).await.unwrap().is_some());

    grants.remove(1);
    for grant in grants.into_iter().chain([last]) {
        za_revoke_grant(&test.server, grant).await;
    }
    assert!(raw_token_record(test, id).await.is_none());
}

async fn forget_failure_does_not_fail_deletion(test: &TestServer, id: u32) {
    println!("  forget_failure_does_not_fail_deletion");
    let (grant, _, _, _) = issued_for(test, OWNER, id).await;
    test_hooks::set_token_forget_fail_once(Some(id));
    za_revoke_grant(&test.server, grant).await;
    assert!(test.server.oauth_grant(grant).await.unwrap().is_none());
    assert!(token_record(test, id).await.unwrap().grant(grant).is_some());
    test.server.za_token_forget_grant(id, grant).await.unwrap();
    assert!(raw_token_record(test, id).await.is_none());
}

async fn ordinary_grant_deletion_reads_no_record(test: &TestServer, id: u32) {
    println!("  ordinary_grant_deletion_reads_no_record");
    set_raw_token_record(test, id, Some(PLANTED)).await;
    test_hooks::set_token_forget_fail_once(Some(id));
    let (grant, _) = za_grant_token(&test.server, PLAIN, OAuthScopes::ALL).await;
    za_revoke_grant(&test.server, grant).await;
    assert!(raw_token_record(test, id).await.as_deref() == Some(PLANTED));
    assert!(test_hooks::take_token_forget_fail(id));
    set_raw_token_record(test, id, None).await;
}

async fn pin_rewrap_keeps_session(test: &TestServer, owner: u32) {
    const FIRST: &str = "key1@example.com";
    assert_eq!(wrap_mark(test, owner, STRONG).await, ZaMark::Legacy);

    // D19: the first pin increments the vault revision but keeps the binding.
    vault_write(test, owner, "first pin", Some(PLANTED), async {
        let fingerprint = lookup(OWNER, FIRST).await.str(200, "fingerprint");
        assert_eq!(pin(OWNER, FIRST, &fingerprint).await.status, 200);
        assert_eq!(wrap_mark(test, owner, STRONG).await, ZaMark::Pinned);
    })
    .await;

    // The next pin writes only the pin record.
    let revision = vault_revision(test, owner).await;
    let fingerprint = lookup(OWNER, MFA).await.str(200, "fingerprint");
    assert_eq!(pin(OWNER, MFA, &fingerprint).await.status, 200);
    assert_eq!(vault_revision(test, owner).await, revision);
    assert!(raw_token_record(test, owner).await.as_deref() == Some(PLANTED));

    set_raw_token_record(test, owner, None).await;
    assert_eq!(forget(OWNER, Some(FIRST)).await.status, 200);
    assert_eq!(forget(OWNER, Some(MFA)).await.status, 200);
}

async fn credential_changes_end_sessions(test: &mut TestServer, owner: u32, mfa: u32) {
    vault_write(test, owner, "password change", None, async {
        let reply = za_post(
            "password",
            &json!({ "username": OWNER, "password": STRONG, "new_password": STRONG2 }),
        )
        .await;
        assert_eq!(reply.status, 200);
    })
    .await;

    let recovery_key = vault_write(test, owner, "recovery", None, async {
        let reply = za_post(
            "recover",
            &json!({
                "username": OWNER,
                "recovery_key": test.account(OWNER).recovery_key.as_deref().unwrap(),
                "new_password": STRONG,
            }),
        )
        .await;
        assert_eq!(reply.status, 200);
        reply.body["recovery_key"].as_str().unwrap().to_owned()
    })
    .await;
    test.accounts.get_mut(OWNER).unwrap().recovery_key = Some(recovery_key);

    let url = totp_url();
    vault_write(test, mfa, "second factor enrolled", None, async {
        let reply = za_post(
            "totp",
            &json!({
                "username": MFA, "password": STRONG, "otp_auth": url, "confirm": code(&url),
            }),
        )
        .await;
        assert_eq!(reply.status, 200);
    })
    .await;
    vault_write(test, mfa, "second factor removed", None, async {
        let reply = za_post(
            "totp",
            &json!({ "username": MFA, "password": STRONG, "otp_auth": null, "totp": code(&url) }),
        )
        .await;
        assert_eq!(reply.status, 200);
    })
    .await;
}

async fn other_vault_writes_keep_sessions(test: &mut TestServer, owner: u32) {
    let recovery_key = vault_write(test, owner, "new recovery key", Some(PLANTED), async {
        let reply = za_post(
            "recovery-key",
            &json!({ "username": OWNER, "password": STRONG }),
        )
        .await;
        assert_eq!(reply.status, 200);
        reply.body["recovery_key"].as_str().unwrap().to_owned()
    })
    .await;
    test.accounts.get_mut(OWNER).unwrap().recovery_key = Some(recovery_key);

    let parked = vault_write(
        test,
        owner,
        "app password pending",
        Some(PLANTED),
        park_app_password(owner, "Publish"),
    )
    .await;
    let credential_id = vault_write(
        test,
        owner,
        "app password published",
        Some(PLANTED),
        async {
            let reply = parked.finish().await;
            assert_eq!(reply.status, 200);
            reply.body["credential_id"].as_u64().unwrap() as u32
        },
    )
    .await;
    vault_write(test, owner, "app password revoked", Some(PLANTED), async {
        let reply = za_post(
            "app-password/revoke",
            &json!({ "username": OWNER, "password": STRONG, "credential_id": credential_id }),
        )
        .await;
        assert_eq!(reply.status, 200);
    })
    .await;

    let parked = park_app_password(owner, "Withdraw").await;
    bump_registry_revision(test).await;
    vault_write(
        test,
        owner,
        "app password withdrawn",
        Some(PLANTED),
        async {
            let reply = parked.finish().await;
            assert_eq!(reply.status, 409);
            assert_eq!(reply.body["error"], "registry write failed");
        },
    )
    .await;

    {
        let _drain = DrainOn::new();
        let original = substitute_public_key(test, owner, &writer_key(), true).await;
        vault_write(test, owner, "self-check repair", Some(PLANTED), async {
            assert_eq!(trigger_drain(OWNER, STRONG).await, 207);
            let repaired = test.server.za_vault_record(owner).await.unwrap().unwrap();
            assert_eq!(repaired.record.public_key, original);
        })
        .await;
    }
    set_raw_token_record(test, owner, None).await;
}

/// All the vault-write cases check the revision and the stored bytes
/// independently. Unusable planted bytes still participate in the clear.
async fn vault_write<T>(
    test: &TestServer,
    account_id: u32,
    row: &str,
    expected: Option<&[u8]>,
    write: impl Future<Output = T>,
) -> T {
    set_raw_token_record(test, account_id, Some(PLANTED)).await;
    let before = vault_revision(test, account_id).await;
    let result = write.await;
    assert_eq!(vault_revision(test, account_id).await, before + 1, "{row}");
    assert!(
        raw_token_record(test, account_id).await.as_deref() == expected,
        "unexpected token record after {row}"
    );
    result
}

async fn park_app_password(owner: u32, description: &'static str) -> Parked<VaultReply> {
    Parked::start(owner, za_test::set_publish, async move {
        za_post(
            "app-password",
            &json!({ "username": OWNER, "password": STRONG, "description": description }),
        )
        .await
    })
    .await
}

/// Bump the registry object's revision so a write based on the old read fails.
async fn bump_registry_revision(test: &TestServer) {
    let id = test.account(OWNER).id();
    let registry = test.server.registry();
    let object = registry
        .get(ObjectId::new(ObjectType::Account, id))
        .await
        .unwrap()
        .unwrap();
    let old_object = Object::with_revision(object.inner.clone(), object.revision);
    let mut inner = object.inner;
    let ObjectInner::Account(account) = &mut inner else {
        panic!("not an account");
    };
    let structs::Account::User(user) = account.as_mut() else {
        panic!("not a user account");
    };
    user.description = Some(format!("Token Owner, revision {}", object.revision));
    let result = registry
        .write(RegistryWrite::Update {
            object: &Object::new(inner),
            id,
            old_object: &old_object,
        })
        .await
        .unwrap();
    assert!(matches!(result, RegistryWriteResult::Success(_)));
}

fn code_of(location: &str) -> String {
    reqwest::Url::parse(location)
        .expect("authorization returns a redirect URL")
        .query_pairs()
        .find(|(key, _)| key == "code")
        .expect("redirect contains a code")
        .1
        .into_owned()
}

async fn grants_of(test: &TestServer, account_id: u32) -> Vec<Id> {
    test.server
        .registry()
        .query(RegistryQuery::new(ObjectType::OAuthGrant).with_account(account_id))
        .await
        .unwrap()
}

fn authorized_code(response: LoginResponse) -> String {
    let LoginResponse::Authenticated { location } = response else {
        panic!("sign-in did not authorize a code");
    };
    code_of(&location)
}

fn consent_token(response: LoginResponse) -> String {
    let LoginResponse::ConsentRequired { token } = response else {
        panic!("sign-in did not request consent");
    };
    token
}

fn granted(response: TokenResponse) -> OAuthResponse {
    let TokenResponse::Granted(tokens) = response else {
        panic!("the OAuth grant was refused");
    };
    tokens
}

fn assert_invalid_grant(response: TokenResponse) {
    assert!(matches!(
        response,
        TokenResponse::Error {
            error: ErrorType::InvalidGrant
        }
    ));
}

fn assert_keyed_pair(access: &str, refresh: &str) {
    let (upstream, secret) = za_split_token(access);
    assert_eq!(secret.expect("the access token has no secret").len(), 43);
    assert_eq!(access.len(), upstream.len() + 44);
    let refresh_secret = za_split_token(refresh)
        .1
        .expect("the refresh token has no secret");
    assert_eq!(refresh_secret.len(), 43);
    assert!(
        Some(refresh_secret) != secret,
        "the token secrets must differ"
    );
}

fn assert_first_entries(record: &vault::tokens::ZaTokenRecord, grant_id: u64) {
    let grant = record.grant(grant_id).expect("the record names the grant");
    assert_eq!(grant.refresh.len(), 1);
    assert_eq!(grant.refresh[0].generation, 0);
    assert_eq!(grant.access.len(), 1);
}

fn sign_in_event_path(account: &Account) -> String {
    format!(
        "/dav/cal/{}/default/token-signin.ics",
        account.name().replace('@', "%40"),
    )
}

async fn put_sign_in_canaries(test: &TestServer, account: &Account) {
    let event = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//ZA//EN\r\n\
                 BEGIN:VEVENT\r\nUID:token-signin\r\nDTSTAMP:20261020T100000Z\r\n\
                 DTSTART:20261020T100000Z\r\nDTEND:20261020T110000Z\r\n\
                 SUMMARY:sign-in canary event\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    assert_eq!(
        put(
            account.name(),
            STRONG,
            sign_in_event_path(account),
            event.to_owned()
        )
        .await,
        201
    );
    mail::deliver(
        test,
        "sender@example.org",
        account.name(),
        &mail::message_with("sign-in canary subject", "body"),
    )
    .await;
}

async fn assert_sign_in_canaries(account: &Account, access: &str) {
    let response = za_bearer_dav(account, access)
        .request("GET", &sign_in_event_path(account), "")
        .await;
    assert_eq!(response.status.as_u16(), 200, "keyed CalDAV GET");
    assert!(
        response
            .body
            .is_ok_and(|body| body.contains("SUMMARY:sign-in canary event"))
    );

    let (status, response) = za_bearer_jmap(
        access,
        CALENDAR_JMAP_USING,
        json!([
            ["Email/query", { "accountId": account.id_string() }, "q"],
            ["Email/get", {
                "accountId": account.id_string(),
                "#ids": { "resultOf": "q", "name": "Email/query", "path": "/ids" },
                "properties": ["subject"],
            }, "g"],
        ]),
    )
    .await;
    assert_eq!(status, 200);
    let replies = &response.0["methodResponses"];
    assert!(replies[0][0].as_str() == Some("Email/query"));
    assert!(replies[1][0].as_str() == Some("Email/get"));
    assert!(replies[1][1]["list"][0]["subject"].as_str() == Some("sign-in canary subject"));
}

async fn remove_sign_in_canaries(test: &TestServer, account: &Account) {
    let basic = DummyWebDavClient::new(
        account.id().document_id(),
        account.name(),
        STRONG,
        account.name(),
    );
    assert_eq!(
        basic
            .request("DELETE", &sign_in_event_path(account), "")
            .await
            .status
            .as_u16(),
        204,
    );
    test.destroy_all_mailboxes(account).await;
}

async fn sign_in_gives_keyed_tokens(test: &TestServer, app: &ZaOAuthClient) {
    println!("  sign_in_gives_keyed_tokens");
    let tokens = za_sign_in(test, app, OWNER, STRONG, None).await;
    assert_keyed_pair(&tokens.access, &tokens.refresh);
    assert!(tokens.expires_in > 0);
    let owner = test.account(OWNER);
    let id = owner.id().document_id();
    put_sign_in_canaries(test, owner).await;
    assert_sign_in_canaries(owner, &tokens.access).await;
    assert_first_entries(&token_record(test, id).await.unwrap(), tokens.grant_id);
    let cache = &test.server.inner.cache;
    assert!(cache.za_keys.contains_account(id));
    assert_eq!(za_revoke_token(app, &tokens.refresh).await, 200);
    assert!(
        test.server
            .oauth_grant(tokens.grant_id)
            .await
            .unwrap()
            .is_none()
    );
    // Spec 4.6: a grant's deletion evicts the account's resident keys at once
    // (final review FA-4).
    assert!(
        !cache.za_keys.contains_account(id),
        "resident keys outlived the grant's deletion"
    );
    assert!(raw_token_record(test, id).await.is_none());
    assert_eq!(za_bearer_session(&tokens.access).await.0, 401);
    remove_sign_in_canaries(test, owner).await;
}

/// PR #90 review C4617611602: a grant's deletion evicts its cached session and
/// resident keys before the token record's cleanup is awaited. With the
/// cleanup parked, the registry holds no grant and the record still names it,
/// yet the revoked token's next request is 401 and nothing of it is cached;
/// after release the record is gone.
async fn deleted_grant_evicted_before_cleanup(test: &TestServer, app: &Arc<ZaOAuthClient>) {
    println!("  deleted_grant_evicted_before_cleanup");
    let tokens = za_sign_in(test, app, OWNER, STRONG, None).await;
    let id = test.account(OWNER).id().document_id();
    let cache = &test.server.inner.cache;
    assert_eq!(za_bearer_session(&tokens.access).await.0, 200);
    assert!(cache.http_auth.get(&tokens.access).is_some());
    assert!(cache.za_keys.contains_account(id));

    let parked = HookParked::start(id, test_hooks::set_token_forget_pause, {
        let (app, refresh) = (app.clone(), tokens.refresh.clone());
        async move { za_revoke_token(&app, &refresh).await }
    })
    .await;
    assert!(
        test.server
            .oauth_grant(tokens.grant_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        token_record(test, id)
            .await
            .unwrap()
            .grant(tokens.grant_id)
            .is_some(),
        "the token record's cleanup is not parked"
    );
    assert_eq!(
        za_bearer_session(&tokens.access).await.0,
        401,
        "a deleted grant's token was served while its cleanup was parked"
    );
    assert!(
        cache.http_auth.get(&tokens.access).is_none(),
        "a deleted grant's session stayed cached while its cleanup was parked"
    );
    assert!(
        !cache.za_keys.contains_account(id),
        "resident keys outlived the grant's deletion while its cleanup was parked"
    );
    assert_eq!(parked.finish().await, 200);
    assert!(raw_token_record(test, id).await.is_none());
}

/// The binding (D19) of the account's stored vault record.
async fn stored_binding(test: &TestServer, id: u32) -> u64 {
    za_session_binding(
        &test
            .server
            .za_vault_record(id)
            .await
            .unwrap()
            .unwrap()
            .record,
    )
}

/// The account's password, and its second factor when one is enrolled, still
/// verify and open its keys.
async fn assert_password_opens(test: &TestServer, id: u32, totp: Option<&str>, step: &str) {
    assert!(
        matches!(
            test.server
                .za_verify_password(id, STRONG, totp)
                .await
                .unwrap(),
            ZaVerification::Valid(_)
        ),
        "the password no longer opens the keys after {step}"
    );
}

/// Every form of the pair is refused: each token with its secret and with the
/// suffix cut off. The forms without a secret go first, so a token a reverted
/// binding would accept again is caught before a refusal could end its grant.
async fn assert_session_ended(app: &ZaOAuthClient, tokens: &ZaTokens, step: &str) {
    for access in [za_split_token(&tokens.access).0, tokens.access.as_str()] {
        assert_eq!(
            za_bearer_session(access).await.0,
            401,
            "an access token was served after {step}"
        );
    }
    for refresh in [za_split_token(&tokens.refresh).0, tokens.refresh.as_str()] {
        assert!(
            matches!(
                za_refresh(app, refresh).await,
                TokenResponse::Error {
                    error: ErrorType::InvalidGrant
                }
            ),
            "a refresh token was granted after {step}"
        );
    }
}

/// Spec 4.7 and D19 (final review FA-1, FA-7): a second-factor change ends
/// the account's sessions for good. Removing the factor again, or enrolling
/// the same URL again, draws a fresh salt, so the binding a token was minted
/// under never returns. A recovery ends them too. The password and an app
/// password made before the first change still open the keys.
async fn second_factor_change_ends_sessions(test: &mut TestServer, app: &ZaOAuthClient) {
    println!("  second_factor_change_ends_sessions");
    let id = test.account(FACTOR).id().document_id();
    let reply = za_post(
        "app-password",
        &json!({ "username": FACTOR, "password": STRONG, "description": "token-factor" }),
    )
    .await;
    assert_eq!(reply.status, 200);
    let app_password = AppPassword::parse(reply.body["app_password"].as_str().unwrap()).unwrap();
    let credential_id = app_password.credential_id;

    let tokens = za_sign_in(test, app, FACTOR, STRONG, None).await;
    assert_keyed_pair(&tokens.access, &tokens.refresh);
    assert_eq!(za_bearer_session(&tokens.access).await.0, 200);
    let before = stored_binding(test, id).await;

    let url = totp_url();
    let reply = za_post(
        "totp",
        &json!({ "username": FACTOR, "password": STRONG, "otp_auth": &url, "confirm": code(&url) }),
    )
    .await;
    assert_eq!(reply.status, 200);
    let enrolled = stored_binding(test, id).await;
    assert_ne!(enrolled, before);
    assert_session_ended(app, &tokens, "the enrolment").await;
    assert_password_opens(test, id, Some(&code(&url)), "the enrolment").await;

    let reply = za_post(
        "totp",
        &json!({ "username": FACTOR, "password": STRONG, "otp_auth": null, "totp": code(&url) }),
    )
    .await;
    assert_eq!(reply.status, 200);
    // The tokens first: with a recurring binding, the access token without
    // its secret would be served again here (final review FA-1).
    assert_session_ended(app, &tokens, "the removal").await;
    let removed = stored_binding(test, id).await;
    assert_ne!(
        removed, before,
        "removing the factor restored the earlier binding"
    );
    assert_password_opens(test, id, None, "the removal").await;

    let reply = za_post(
        "totp",
        &json!({ "username": FACTOR, "password": STRONG, "otp_auth": &url, "confirm": code(&url) }),
    )
    .await;
    assert_eq!(reply.status, 200);
    let again = stored_binding(test, id).await;
    assert_ne!(again, enrolled, "the same enrolment restored its binding");
    assert_ne!(again, before);
    assert_session_ended(app, &tokens, "the second enrolment").await;
    assert_password_opens(test, id, Some(&code(&url)), "the second enrolment").await;

    let minted = za_sign_in(test, app, FACTOR, STRONG, Some(&code(&url))).await;
    assert_eq!(za_bearer_session(&minted.access).await.0, 200);
    let reply = za_post(
        "recover",
        &json!({
            "username": FACTOR,
            "recovery_key": test.account(FACTOR).recovery_key.as_deref().unwrap(),
            "new_password": STRONG,
        }),
    )
    .await;
    assert_eq!(reply.status, 200);
    test.accounts.get_mut(FACTOR).unwrap().recovery_key =
        Some(reply.body["recovery_key"].as_str().unwrap().to_owned());
    assert_ne!(stored_binding(test, id).await, again);
    assert_session_ended(app, &minted, "the recovery").await;
    assert_session_ended(app, &tokens, "the recovery").await;
    assert_password_opens(test, id, None, "the recovery").await;
    assert!(
        test.server
            .za_open_app_wrap(id, credential_id, &app_password.secret)
            .await
            .unwrap()
            .is_some(),
        "the app password no longer opens the keys"
    );

    for grant in grants_of(test, id).await {
        za_revoke_grant(&test.server, grant.id()).await;
    }
    let reply = za_post(
        "app-password/revoke",
        &json!({ "username": FACTOR, "password": STRONG, "credential_id": credential_id }),
    )
    .await;
    assert_eq!(reply.status, 200);
    assert!(raw_token_record(test, id).await.is_none());
}

async fn sign_in_with_second_factor(test: &TestServer, app: &ZaOAuthClient) {
    println!("  sign_in_with_second_factor");
    let url = totp_url();
    let reply = za_post(
        "totp",
        &json!({
            "username": MFA, "password": STRONG, "otp_auth": &url, "confirm": code(&url),
        }),
    )
    .await;
    assert_eq!(reply.status, 200);
    assert!(matches!(
        za_authorize(app, MFA, STRONG, None, Some(true), Some("10.0.60.8")).await,
        LoginResponse::MfaRequired
    ));

    let tokens = za_sign_in(test, app, MFA, STRONG, Some(&code(&url))).await;
    assert!(za_split_token(&tokens.access).1.is_some());
    assert_eq!(za_bearer_session(&tokens.access).await.0, 200);
    assert_eq!(za_revoke_token(app, &tokens.refresh).await, 200);

    let reply = za_post(
        "totp",
        &json!({
            "username": MFA, "password": STRONG, "otp_auth": null, "totp": code(&url),
        }),
    )
    .await;
    assert_eq!(reply.status, 200);
}

async fn consent_path(test: &TestServer, registered: &ZaOAuthClient) {
    println!("  consent_path");
    let id = test.account(CONSENT).id().document_id();
    let request = || za_authorize(registered, CONSENT, STRONG, None, None, None);
    let opaque = consent_token(request().await);
    assert_eq!(opaque.len(), 83);
    assert!(handoff_exists(test, &opaque).await);
    let code = authorized_code(za_confirm_consent(registered, &opaque, true).await);
    assert!(!handoff_exists(test, &opaque).await);
    assert_eq!(code.len(), 83);
    assert!(handoff_exists(test, &code).await);
    let first = granted(za_exchange(registered, &code).await);
    assert!(za_split_token(&first.access_token).1.is_some());
    assert!(!handoff_exists(test, &code).await);

    let code = authorized_code(request().await);
    let second = granted(za_exchange(registered, &code).await);
    assert_keyed_pair(
        &second.access_token,
        second.refresh_token.as_deref().unwrap(),
    );
    for tokens in [first, second] {
        assert_eq!(
            za_revoke_token(registered, tokens.refresh_token.as_deref().unwrap()).await,
            200
        );
    }

    let opaque = consent_token(request().await);
    assert!(handoff_exists(test, &opaque).await);
    assert!(matches!(
        za_confirm_consent(registered, &opaque, false).await,
        LoginResponse::Redirect { .. }
    ));
    assert!(!handoff_exists(test, &opaque).await);

    let opaque = consent_token(request().await);
    assert!(handoff_exists(test, &opaque).await);
    let code = authorized_code(za_confirm_consent(registered, &opaque[..40], true).await);
    assert!(!handoff_exists(test, &opaque).await);
    assert_eq!(code.len(), 40);
    assert_invalid_grant(za_exchange(registered, &code).await);
    assert!(grants_of(test, id).await.is_empty());
    assert!(raw_token_record(test, id).await.is_none());
}

async fn exchange_without_handoff_is_invalid_grant(test: &TestServer, app: &ZaOAuthClient) {
    println!("  exchange_without_handoff_is_invalid_grant");
    let id = test.account(OWNER).id().document_id();
    let authorize = || za_authorize(app, OWNER, STRONG, None, Some(true), None);
    let code = authorized_code(authorize().await);
    assert!(handoff_exists(test, &code).await);
    assert_invalid_grant(za_exchange(app, &code[..40]).await);
    assert!(!handoff_exists(test, &code).await);
    assert_invalid_grant(za_exchange(app, &code).await);

    let code = authorized_code(authorize().await);
    assert!(handoff_exists(test, &code).await);
    let mut wrong = code[..40].to_owned();
    append_code_secret(&mut wrong, &Secret::random());
    assert_invalid_grant(za_exchange(app, &wrong).await);
    assert!(!handoff_exists(test, &code).await);
    assert!(grants_of(test, id).await.is_empty());
    assert!(raw_token_record(test, id).await.is_none());
}

async fn ordinary_code_with_suffix_is_invalid_grant(
    test: &TestServer,
    app: &ZaOAuthClient,
    plain_id: u32,
) {
    println!("  ordinary_code_with_suffix_is_invalid_grant");
    let mut code = authorized_code(za_authorize(app, PLAIN, STRONG, None, Some(true), None).await);
    assert_eq!(code.len(), 40);
    assert!(!handoff_exists(test, &code).await);
    append_code_secret(&mut code, &Secret::random());
    assert_invalid_grant(za_exchange(app, &code).await);
    assert!(grants_of(test, plain_id).await.is_empty());
    assert_invalid_grant(za_exchange(app, &code[..40]).await);
}

async fn handoff_gone_after_redemption_and_expiry(test: &TestServer, app: &ZaOAuthClient) {
    println!("  handoff_gone_after_redemption_and_expiry");
    let authorize = || za_authorize(app, OWNER, STRONG, None, Some(true), None);
    let code = authorized_code(authorize().await);
    assert!(handoff_exists(test, &code).await);
    let tokens = granted(za_exchange(app, &code).await);
    assert!(!handoff_exists(test, &code).await);
    assert_eq!(
        za_revoke_token(app, tokens.refresh_token.as_deref().unwrap()).await,
        200
    );

    let _expiry = oauth_expiry(test, None, None, Some(2));
    let code = authorized_code(authorize().await);
    assert!(handoff_exists(test, &code).await);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(!handoff_exists(test, &code).await);
    assert_invalid_grant(za_exchange(app, &code).await);
}

async fn device_grant(client: &ZaOAuthClient) -> OAuthResponse {
    let http = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let codes: DeviceAuthResponse = http
        .post(format!("{SERVER_URL}/auth/device"))
        .form(&[
            ("client_id", client.client_id.as_str()),
            ("scope", ZA_SIGNIN_SCOPE),
        ])
        .send()
        .await
        .unwrap_or_else(|_| panic!("device authorization request failed"))
        .json()
        .await
        .unwrap_or_else(|_| panic!("device authorization response is invalid"));
    let approval: Value = http
        .post(format!("{SERVER_URL}/api/auth"))
        .json(&json!({
            "type": "authDevice",
            "accountName": OWNER,
            "accountSecret": STRONG,
            "code": codes.user_code,
            "consent": true,
        }))
        .send()
        .await
        .unwrap_or_else(|_| panic!("device approval request failed"))
        .json()
        .await
        .unwrap_or_else(|_| panic!("device approval response is invalid"));
    assert!(approval == json!({ "type": "verified" }));
    let response = http
        .post(format!("{SERVER_URL}/auth/token"))
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", codes.device_code.as_str()),
            ("client_id", client.client_id.as_str()),
        ])
        .send()
        .await
        .unwrap_or_else(|_| panic!("device code exchange failed"))
        .json()
        .await
        .unwrap_or_else(|_| panic!("device code exchange response is invalid"));
    granted(response)
}

fn assert_keyless_pair(tokens: &OAuthResponse) {
    assert!(!tokens.access_token.contains('~'));
    assert!(!tokens.refresh_token.as_deref().unwrap().contains('~'));
}

async fn device_flow_stays_keyless(test: &TestServer, registered: &ZaOAuthClient) {
    println!("  device_flow_stays_keyless");
    let tokens = device_grant(registered).await;
    assert_keyless_pair(&tokens);
    let owner = test.account(OWNER);
    assert!(
        raw_token_record(test, owner.id().document_id())
            .await
            .is_none()
    );
    let (status, response) =
        za_bearer_jmap(&tokens.access_token, CALENDAR_JMAP_USING, mail_get(owner)).await;
    assert_eq!(status, 200);
    assert!(response.is_error_at(0));
    let error = &response.0["methodResponses"][0][1];
    assert!(error["type"].as_str() == Some("forbidden"));
    assert!(error["description"].as_str() == Some(ZA_MAIL_KEYLESS));
    assert_eq!(
        za_revoke_token(registered, tokens.refresh_token.as_deref().unwrap()).await,
        200
    );
}

async fn refresh_without_secret_of_keyed_grant_refused(
    test: &TestServer,
    app: &ZaOAuthClient,
    registered: &ZaOAuthClient,
) {
    println!("  refresh_without_secret_of_keyed_grant_refused");
    let id = test.account(OWNER).id().document_id();
    let t0 = za_sign_in(test, app, OWNER, STRONG, None).await;
    let before = test.server.oauth_grant(t0.grant_id).await.unwrap().unwrap();
    let raw = raw_token_record(test, id)
        .await
        .expect("the keyed grant has a record");
    let upstream = za_split_token(&t0.refresh).0;
    let malformed = format!("{upstream}~short");
    for refresh in [upstream, &malformed] {
        assert_invalid_grant(za_refresh(app, refresh).await);
        let after = test.server.oauth_grant(t0.grant_id).await.unwrap().unwrap();
        assert_eq!(after.grant.generation, before.grant.generation);
        assert_eq!(after.grant.pending, before.grant.pending);
        assert!(raw_token_record(test, id).await.as_deref() == Some(raw.as_slice()));
    }

    // Spec 4.5: with its secret, the keyed grant's refresh token refreshes.
    granted(za_refresh(app, &t0.refresh).await);
    assert_eq!(za_revoke_token(app, &t0.refresh).await, 200);

    let device = device_grant(registered).await;
    let refreshed = granted(za_refresh(registered, device.refresh_token.as_deref().unwrap()).await);
    assert_keyless_pair(&refreshed);
    assert_eq!(
        za_revoke_token(registered, refreshed.refresh_token.as_deref().unwrap()).await,
        200
    );
}

/// The public RFC 7636 example verifier of `za_authorize`'s challenge.
const PKCE_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
/// A synthetic code of the server's shape (40 alphanumeric characters) that
/// the server never issued.
const ALIAS_CODE: &str = "r17aliasSyntheticCode0000000000000000000";
/// Inert bytes planted at `ALIAS_CODE`'s hand-off alias: no key, wrap,
/// credential, token or code. Their last byte lacks the archive marker bit,
/// so upstream's archive reader refuses them every time.
const ALIAS_CANARY: &[u8] = b"za-r17-1 inert alias canary, holds nothing";
const _: () = assert!(ALIAS_CANARY[ALIAS_CANARY.len() - 1] & 0x80 == 0);
/// Raw token requests that may end in an authentication error.
const RAW_TOKEN_IP: &str = "10.0.60.10";

/// A raw token request through a five-second client: the status and the
/// JSON body, or `Null` when the body is not JSON.
async fn token_form(form: &[(&str, &str)]) -> (u16, Value) {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .post(format!("{SERVER_URL}/auth/token"))
        .header("X-Forwarded-For", RAW_TOKEN_IP)
        .form(form)
        .send()
        .await
        .unwrap_or_else(|_| panic!("the raw token request failed"));
    let status = response.status().as_u16();
    let body = response
        .bytes()
        .await
        .unwrap_or_else(|_| panic!("the raw token response was not read"));
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// Whether upstream's code entry of `code` (its first 40 characters) exists.
async fn code_exists(test: &TestServer, code: &str) -> bool {
    // `get` keeps a short code out of a slicing panic's message.
    let code = code.get(..CODE_LEN).expect("OAuth code prefix");
    test.server
        .in_memory_store()
        .key_exists(KeyValue::<()>::build_key(KV_OAUTH, code))
        .await
        .unwrap()
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Whether a traced value holds `needle`, through nested events and arrays,
/// compared without formatting either.
fn value_holds(value: &trc::Value, needle: &[u8]) -> bool {
    match value {
        trc::Value::Bytes(bytes) => contains_bytes(bytes, needle),
        trc::Value::String(text) => contains_bytes(text.as_bytes(), needle),
        trc::Value::Event(error) => error
            .keys()
            .iter()
            .any(|(_, value)| value_holds(value, needle)),
        trc::Value::Array(values) => values.iter().any(|value| value_holds(value, needle)),
        _ => false,
    }
}

fn holds_alias_canary(event: &TracedEvent) -> bool {
    contains_bytes(event.text.as_bytes(), ALIAS_CANARY)
        || event
            .keys
            .iter()
            .any(|(_, value)| value_holds(value, ALIAS_CANARY))
}

/// R17-1: upstream's code and device-code lookups build their key from the
/// caller's input, so `~` and a code would select that code's hand-off.
/// Only the server's own code shape reaches a lookup.
async fn malformed_code_lookups_miss_the_handoff(test: &TestServer, app: &ZaOAuthClient) {
    println!("  malformed_code_lookups_miss_the_handoff");
    assert!(
        ALIAS_CODE.len() == CODE_LEN && ALIAS_CODE.bytes().all(|ch| ch.is_ascii_alphanumeric())
    );
    let id = test.account(OWNER).id().document_id();
    // Control: a real sign-in outstanding beside the planted entry.
    let code = authorized_code(za_authorize(app, OWNER, STRONG, None, Some(true), None).await);
    assert!(code_exists(test, &code).await);
    assert!(handoff_exists(test, &code).await);

    let store = test.server.in_memory_store();
    let alias = format!("~{ALIAS_CODE}");
    store
        .key_set(KeyValue::with_prefix(KV_OAUTH, &alias, ALIAS_CANARY.to_vec()).expires(60))
        .await
        .unwrap();
    assert!(handoff_exists(test, ALIAS_CODE).await);

    let authorization_code = [
        ("grant_type", "authorization_code"),
        ("client_id", app.client_id.as_str()),
        ("redirect_uri", app.redirect_uri.as_str()),
        ("code", alias.as_str()),
        ("code_verifier", PKCE_VERIFIER),
    ];
    let device_code = [
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("client_id", app.client_id.as_str()),
        ("device_code", alias.as_str()),
    ];
    for (form, expected) in [
        (&authorization_code[..], "invalid_grant"),
        (&device_code[..], "expired_token"),
    ] {
        let traced = Traced::start(EventType::variants());
        let (status, body) = token_form(form).await;
        let events = traced.finish().await;
        assert!(
            !events.iter().any(holds_alias_canary),
            "a trace event holds the planted canary"
        );
        assert_eq!(status, 400);
        assert!(
            body["error"].as_str() == Some(expected),
            "the malformed code lookup did not answer {expected}"
        );
        assert!(
            handoff_exists(test, ALIAS_CODE).await,
            "the planted entry is gone"
        );
        assert!(code_exists(test, &code).await);
        assert!(handoff_exists(test, &code).await);
    }

    assert!(
        store
            .key_delete(KeyValue::<()>::build_key(KV_OAUTH, &alias))
            .await
            .unwrap()
    );
    assert!(!handoff_exists(test, ALIAS_CODE).await);

    let tokens = granted(za_exchange(app, &code).await);
    let refresh = tokens.refresh_token.as_deref().unwrap();
    assert_keyed_pair(&tokens.access_token, refresh);
    assert!(!code_exists(test, &code).await);
    assert!(!handoff_exists(test, &code).await);
    assert_eq!(za_revoke_token(app, refresh).await, 200);
    assert!(grants_of(test, id).await.is_empty());
    assert!(raw_token_record(test, id).await.is_none());
}

/// R17-2: upstream has consumed the code before the account is classified,
/// so a classification failure must not leave the code's hand-off behind.
/// The account's removal makes the classification fail while the in-memory
/// store stays available.
async fn classification_failure_removes_the_handoff(
    test: &TestServer,
    admin: &Account,
    app: &ZaOAuthClient,
) {
    println!("  classification_failure_removes_the_handoff");
    let account = admin
        .create_key_user_account(REMOVED, STRONG, "Token Removed", &[], user_permissions())
        .await;
    let id = account.id().document_id();
    let code = authorized_code(za_authorize(app, REMOVED, STRONG, None, Some(true), None).await);
    // The case owns the account: it is never registered with the test server.
    admin.destroy_account(account).await;
    test.wait_for_tasks().await;
    assert!(
        test.server.account(id).await.is_err(),
        "the removed account still classifies"
    );
    assert!(code_exists(test, &code).await);
    assert!(handoff_exists(test, &code).await);

    let (status, body) = token_form(&[
        ("grant_type", "authorization_code"),
        ("client_id", app.client_id.as_str()),
        ("redirect_uri", app.redirect_uri.as_str()),
        ("code", code.as_str()),
        ("code_verifier", PKCE_VERIFIER),
    ])
    .await;
    assert_eq!(status, 401);
    assert!(
        body.get("access_token").is_none() && body.get("refresh_token").is_none(),
        "the failed exchange returned a token"
    );
    assert!(!code_exists(test, &code).await, "the code was not consumed");
    assert!(
        !handoff_exists(test, &code).await,
        "the hand-off outlived its code"
    );
    assert!(grants_of(test, id).await.is_empty());
    assert!(raw_token_record(test, id).await.is_none());
}

const SIGN_IN_VAULT_CHANGED: &str = "zero-access: vault changed during sign-in";

async fn park_handoff(
    test: &TestServer,
    id: u32,
    key_id: &'static str,
) -> HookParked<trc::Result<Secret>> {
    assert!(!handoff_exists(test, key_id).await);
    let keys = za_unlock(test, id, STRONG).await;
    let server = test.server.clone();
    HookParked::start(id, test_hooks::set_token_handoff_write_pause, async move {
        server
            .za_handoff_put(key_id, id, &keys, server.core.oauth.oauth_expiry_auth_code)
            .await
    })
    .await
}

async fn assert_handoff_refused(
    test: &TestServer,
    id: u32,
    key_id: &str,
    result: trc::Result<Secret>,
) {
    let Err(err) = result else {
        panic!("the crossed hand-off returned a secret");
    };
    assert!(err.matches(AUTH_FAILED));
    assert!(err.value_as_str(Key::Details) == Some(SIGN_IN_VAULT_CHANGED));
    assert!(!handoff_exists(test, key_id).await);
    assert!(grants_of(test, id).await.is_empty());
    assert!(raw_token_record(test, id).await.is_none());
}

async fn handoff_crossed_by_password_change(test: &TestServer) {
    println!("  handoff_crossed_by_password_change");
    const KEY_ID: &str = concat!("handoffpassword", "0123456789012345678901234");
    let id = test.account(RACE).id().document_id();
    let parked = park_handoff(test, id, KEY_ID).await;
    let reply = za_post(
        "password",
        &json!({ "username": RACE, "password": STRONG, "new_password": STRONG2 }),
    )
    .await;
    assert_eq!(reply.status, 200);
    let result = parked.finish().await;
    let reply = za_post(
        "password",
        &json!({ "username": RACE, "password": STRONG2, "new_password": STRONG }),
    )
    .await;
    assert_eq!(reply.status, 200);
    assert_handoff_refused(test, id, KEY_ID, result).await;
}

async fn rotate_race_recovery_key(test: &mut TestServer) {
    let reply = za_post(
        "recovery-key",
        &json!({ "username": RACE, "password": STRONG }),
    )
    .await;
    assert_eq!(reply.status, 200);
    test.accounts.get_mut(RACE).unwrap().recovery_key =
        Some(reply.body["recovery_key"].as_str().unwrap().to_owned());
}

async fn handoff_crossed_by_other_vault_write(test: &mut TestServer) {
    println!("  handoff_crossed_by_other_vault_write");
    const KEY_ID: &str = concat!("handoffrecovery", "0123456789012345678901234");
    let id = test.account(RACE).id().document_id();
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    let parked = park_handoff(test, id, KEY_ID).await;
    rotate_race_recovery_key(test).await;
    let after = test.server.za_vault_record(id).await.unwrap().unwrap();
    assert!(after.record.revision > before.record.revision);
    assert_eq!(
        za_session_binding(&after.record),
        za_session_binding(&before.record)
    );
    assert_handoff_refused(test, id, KEY_ID, parked.finish().await).await;
}

async fn sign_in_crossed_by_vault_write_issues_no_code(
    test: &mut TestServer,
    app: &Arc<ZaOAuthClient>,
) {
    println!("  sign_in_crossed_by_vault_write_issues_no_code");
    let id = test.account(RACE).id().document_id();
    let traced = Traced::start(&[AUTH_FAILED]);
    let parked = HookParked::start(id, test_hooks::set_token_handoff_write_pause, {
        let app = app.clone();
        async move {
            let mut query = reqwest::Url::parse("https://example.org/").unwrap();
            query.query_pairs_mut().extend_pairs([
                ("response_type", "code"),
                ("client_id", app.client_id.as_str()),
                ("redirect_uri", app.redirect_uri.as_str()),
                ("scope", ZA_SIGNIN_SCOPE),
                (
                    "code_challenge",
                    "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
                ),
                ("code_challenge_method", "S256"),
                ("state", "za-token-sign-in-race"),
            ]);
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .danger_accept_invalid_certs(true)
                .build()
                .unwrap()
                .post(format!("{SERVER_URL}/api/auth"))
                .header("X-Forwarded-For", "10.0.60.9")
                .json(&json!({
                    "type": "authCode",
                    "accountName": RACE,
                    "accountSecret": STRONG,
                    "query": query.query().unwrap(),
                    "consent": true,
                }))
                .send()
                .await
                .unwrap_or_else(|_| panic!("the parked authorization request failed"))
        }
    })
    .await;
    rotate_race_recovery_key(test).await;
    let response = parked.finish().await;
    assert_eq!(response.status().as_u16(), 401);
    assert!(!response.headers().contains_key("location"));
    let body: Value = response
        .json()
        .await
        .unwrap_or_else(|_| panic!("authorization failure did not return a problem body"));
    assert!(body.get("location").is_none());
    assert!(body.get("token").is_none());
    assert!(grants_of(test, id).await.is_empty());
    assert!(raw_token_record(test, id).await.is_none());
    assert!(traced.finish().await.iter().any(|event| {
        event.typ == AUTH_FAILED
            && event.account_id == Some(u64::from(id))
            && event.details == SIGN_IN_VAULT_CHANGED
    }));
}

async fn park_exchange(
    test: &TestServer,
    app: &Arc<ZaOAuthClient>,
    password: &str,
) -> (String, HookParked<TokenResponse>) {
    let id = test.account(RACE).id().document_id();
    let code = authorized_code(za_authorize(app, RACE, password, None, Some(true), None).await);
    assert!(handoff_exists(test, &code).await);
    let parked = HookParked::start(id, test_hooks::set_token_exchange_pause, {
        let app = app.clone();
        let code = code.clone();
        async move { za_exchange(&app, &code).await }
    })
    .await;
    assert!(raw_token_record(test, id).await.is_none());
    assert_eq!(grants_of(test, id).await.len(), 1);
    (code, parked)
}

async fn assert_crossed_exchange_refused(
    test: &TestServer,
    code: &str,
    parked: HookParked<TokenResponse>,
) {
    assert_invalid_grant(parked.finish().await);
    let id = test.account(RACE).id().document_id();
    assert!(raw_token_record(test, id).await.is_none());
    assert!(grants_of(test, id).await.is_empty());
    assert!(!handoff_exists(test, code).await);
}

async fn exchange_crossed_by_password_change(test: &TestServer, app: &Arc<ZaOAuthClient>) {
    println!("  exchange_crossed_by_password_change");
    let (code, parked) = park_exchange(test, app, STRONG).await;
    let reply = za_post(
        "password",
        &json!({
            "username": RACE, "password": STRONG, "new_password": STRONG2,
        }),
    )
    .await;
    assert_eq!(reply.status, 200);
    assert_crossed_exchange_refused(test, &code, parked).await;
    let tokens = za_sign_in(test, app, RACE, STRONG2, None).await;
    assert_sign_in_canaries(test.account(RACE), &tokens.access).await;
    assert_eq!(za_revoke_token(app, &tokens.refresh).await, 200);
}

async fn exchange_crossed_by_recovery(test: &mut TestServer, app: &Arc<ZaOAuthClient>) {
    println!("  exchange_crossed_by_recovery");
    let (code, parked) = park_exchange(test, app, STRONG2).await;
    let reply = za_post(
        "recover",
        &json!({
            "username": RACE,
            "recovery_key": test.account(RACE).recovery_key.as_deref().unwrap(),
            "new_password": STRONG,
        }),
    )
    .await;
    assert_eq!(reply.status, 200);
    test.accounts.get_mut(RACE).unwrap().recovery_key =
        Some(reply.body["recovery_key"].as_str().unwrap().to_owned());
    assert_crossed_exchange_refused(test, &code, parked).await;
    let tokens = za_sign_in(test, app, RACE, STRONG, None).await;
    assert_sign_in_canaries(test.account(RACE), &tokens.access).await;
    assert_eq!(za_revoke_token(app, &tokens.refresh).await, 200);
}

async fn exchange_crossed_by_other_vault_write(test: &mut TestServer, app: &Arc<ZaOAuthClient>) {
    println!("  exchange_crossed_by_other_vault_write");
    let id = test.account(RACE).id().document_id();
    let before = test.server.za_vault_record(id).await.unwrap().unwrap();
    let binding = za_session_binding(&before.record);
    let (code, parked) = park_exchange(test, app, STRONG).await;
    rotate_race_recovery_key(test).await;
    assert!(vault_revision(test, id).await > before.record.revision);
    let tokens = granted(parked.finish().await);
    assert!(!handoff_exists(test, &code).await);
    let refresh = tokens.refresh_token.as_deref().unwrap();
    assert_keyed_pair(&tokens.access_token, refresh);
    let grant = test
        .server
        .decode_access_token(None, &tokens.access_token)
        .unwrap_or_else(|_| panic!("could not decode the keyed access token"))
        .grant
        .unwrap();
    let record = token_record(test, id).await.unwrap();
    assert_eq!(record.binding, binding);
    assert_first_entries(&record, grant.id);
    assert_sign_in_canaries(test.account(RACE), &tokens.access_token).await;
    assert_eq!(grants_of(test, id).await, vec![Id::new(grant.id)]);
    assert_eq!(za_revoke_token(app, refresh).await, 200);
}

async fn thirty_third_sign_in_ends_the_oldest(test: &TestServer, app: &ZaOAuthClient) {
    println!("  thirty_third_sign_in_ends_the_oldest");
    let id = test.account(CAP).id().document_id();
    let mut tokens = Vec::with_capacity(32);
    for _ in 0..32 {
        tokens.push(za_sign_in(test, app, CAP, STRONG, None).await);
    }
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let stored = test
        .server
        .oauth_grant(tokens[0].grant_id)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        test.server.rotate_oauth_grant(stored, 0).await.unwrap(),
        GrantRotation::Issue { .. }
    ));
    let last = za_sign_in(test, app, CAP, STRONG, None).await;
    let record = token_record(test, id).await.unwrap();
    let evicted = tokens.remove(1);
    assert_eq!(record.grants.len(), 32);
    assert!(record.grant(evicted.grant_id).is_none());
    assert!(record.grant(tokens[0].grant_id).is_some());
    assert!(
        test.server
            .oauth_grant(evicted.grant_id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(za_bearer_session(&evicted.access).await.0, 401);
    assert_invalid_grant(za_refresh(app, &evicted.refresh).await);
    assert_eq!(za_bearer_session(&last.access).await.0, 200);
    assert_eq!(za_bearer_session(&tokens[0].access).await.0, 200);
    for tokens in tokens.into_iter().chain([last]) {
        assert_eq!(za_revoke_token(app, &tokens.refresh).await, 200);
    }
    assert!(grants_of(test, id).await.is_empty());
    assert!(raw_token_record(test, id).await.is_none());
}

async fn granted_answer_is_untraced(app: &ZaOAuthClient, test: &TestServer) {
    println!("  granted_answer_is_untraced");
    let traced = Traced::start(EventType::variants());
    let keyed = za_sign_in(test, app, OWNER, STRONG, None).await;
    let plain = za_sign_in(test, app, PLAIN, STRONG, None).await;
    assert!(za_split_token(&plain.access).1.is_none());
    assert!(za_split_token(&plain.refresh).1.is_none());
    assert_invalid_grant(za_exchange(app, &"x".repeat(40)).await);

    let events = traced.finish().await;
    for token in [&keyed.access, &keyed.refresh, &plain.access, &plain.refresh] {
        let (upstream, secret) = za_split_token(token);
        assert!(
            events.iter().all(|event| {
                !event.text.contains(upstream) && secret.is_none_or(|s| !event.text.contains(s))
            }),
            "the trace contains an OAuth token or a secret"
        );
    }
    let has_body = |text| {
        events.iter().any(|event| {
            event.typ == EventType::Http(HttpEvent::ResponseBody) && event.text.contains(text)
        })
    };
    assert!(has_body("[redacted]"));
    assert!(has_body("invalid_grant"));
    for tokens in [keyed, plain] {
        assert_eq!(za_revoke_token(app, &tokens.refresh).await, 200);
    }
}

/// Final review FA-3: the value of a `code` query parameter is masked in the
/// traced request URL and in a traced `Referer`, for every account. The
/// canaries are inert strings; the request is an ordinary account's, whose
/// request body (and so its headers) is traced.
async fn traced_urls_mask_the_code(test: &TestServer) {
    println!("  traced_urls_mask_the_code");
    const URL_CANARY: &str = "tokenurlcanaryalpha";
    const REFERER_CANARY: &str = "tokenrefcanarybravo";
    assert!(
        test.server
            .account_id_from_email(PLAIN, false)
            .await
            .unwrap()
            .is_some()
    );
    let traced = Traced::start(EventType::variants());
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap()
        .post(format!("{SERVER_URL}/jmap?code={URL_CANARY}&state=x"))
        .basic_auth(PLAIN, Some(STRONG))
        .header(
            hyper::header::REFERER,
            format!("https://example.org/za-token-app/?state=y&code={REFERER_CANARY}"),
        )
        .json(&json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [["Core/echo", { "hello": "token" }, "e"]],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    let events = traced.finish().await;
    assert!(!events.is_empty(), "nothing was captured");
    for canary in [URL_CANARY, REFERER_CANARY] {
        assert!(
            events.iter().all(|event| !event.text.contains(canary)),
            "a code value appeared in a trace"
        );
    }
    assert!(
        events.iter().any(|event| {
            event.typ == EventType::Http(HttpEvent::RequestUrl)
                && event.text.contains("/jmap?code=[redacted]&state=x")
        }),
        "the request's URL was not traced"
    );
    assert!(
        events.iter().any(|event| {
            event.typ == EventType::Http(HttpEvent::RequestBody)
                && event.text.contains("referer")
                && event.text.contains("code=[redacted]")
        }),
        "the request's headers were not traced"
    );
}

/// The refresh generations the token record holds for `grant`, in order.
async fn generations(test: &TestServer, id: u32, grant: u64) -> Vec<u64> {
    token_record(test, id)
        .await
        .and_then(|record| {
            record
                .grant(grant)
                .map(|grant| grant.refresh.iter().map(|entry| entry.generation).collect())
        })
        .unwrap_or_default()
}

/// The secret a keyed token string carries.
fn token_secret(token: &str) -> Secret {
    let (_, ZaSuffix::Secret(secret)) = za_split_bearer(token) else {
        panic!("expected a token secret");
    };
    secret
}

fn refresh_of(response: TokenResponse) -> String {
    granted(response)
        .refresh_token
        .expect("the refresh issued a refresh token")
}

/// A sign-in of `OWNER` and two refreshes: `g(0)`, `g(1)` and `g(2)`, with
/// the grant's stored generation at 1.
async fn refreshed_twice(test: &TestServer, app: &ZaOAuthClient) -> (u64, [String; 3]) {
    let t0 = za_sign_in(test, app, OWNER, STRONG, None).await;
    let g1 = refresh_of(za_refresh(app, &t0.refresh).await);
    let g2 = refresh_of(za_refresh(app, &g1).await);
    let stored = test.server.oauth_grant(t0.grant_id).await.unwrap().unwrap();
    assert_eq!(stored.grant.generation, 1);
    (t0.grant_id, [t0.refresh, g1, g2])
}

async fn refresh_rotates_and_reads(test: &TestServer, app: &ZaOAuthClient) {
    println!("  refresh_rotates_and_reads");
    let id = test.account(OWNER).id().document_id();
    let t0 = za_sign_in(test, app, OWNER, STRONG, None).await;
    let t1 = granted(za_refresh(app, &t0.refresh).await);
    let g1 = t1.refresh_token.as_deref().unwrap();
    assert!(g1 != t0.refresh, "the refresh token did not rotate");
    assert_keyed_pair(&t1.access_token, g1);
    assert_eq!(za_bearer_session(&t1.access_token).await.0, 200);
    // The sign-in's access token lives until it expires.
    assert_eq!(za_bearer_session(&t0.access).await.0, 200);
    assert_eq!(generations(test, id, t0.grant_id).await, [0, 1]);
    let t2 = granted(za_refresh(app, g1).await);
    assert_eq!(generations(test, id, t0.grant_id).await, [1, 2]);
    assert_eq!(
        za_revoke_token(app, t2.refresh_token.as_deref().unwrap()).await,
        200
    );
}

async fn retried_refresh_returns_the_same_token(test: &TestServer, app: &ZaOAuthClient) {
    println!("  retried_refresh_returns_the_same_token");
    let id = test.account(OWNER).id().document_id();
    let t0 = za_sign_in(test, app, OWNER, STRONG, None).await;
    let first = granted(za_refresh(app, &t0.refresh).await);
    let retry = granted(za_refresh(app, &t0.refresh).await);
    let refresh = first.refresh_token.as_deref().unwrap();
    assert!(za_split_token(refresh).1.is_some());
    assert!(
        retry.refresh_token.as_deref() == Some(refresh),
        "a retry returns the same refresh token, secret included"
    );
    assert!(
        first.access_token != retry.access_token,
        "a retry returns a new access token"
    );
    for access in [&first.access_token, &retry.access_token] {
        assert_eq!(za_bearer_session(access).await.0, 200);
    }
    let record = token_record(test, id).await.unwrap();
    assert_eq!(record.grant(t0.grant_id).unwrap().access.len(), 3);
    assert_eq!(za_revoke_token(app, refresh).await, 200);
}

async fn replayed_refresh_revokes(test: &TestServer, app: &ZaOAuthClient) {
    println!("  replayed_refresh_revokes");
    let id = test.account(OWNER).id().document_id();
    let (grant, [g0, _, g2]) = refreshed_twice(test, app).await;
    // Upstream revokes the grant on a replay; its entries leave with it.
    assert_invalid_grant(za_refresh(app, &g0).await);
    assert!(test.server.oauth_grant(grant).await.unwrap().is_none());
    assert!(raw_token_record(test, id).await.is_none());
    assert_invalid_grant(za_refresh(app, &g2).await);
}

/// `g(0)` is never presented here: presenting it would revoke the grant.
async fn superseded_token_opens_nothing(test: &TestServer, app: &ZaOAuthClient) {
    println!("  superseded_token_opens_nothing");
    let id = test.account(OWNER).id().document_id();
    let (grant, [g0, _, g2]) = refreshed_twice(test, app).await;
    let secret0 = token_secret(&g0);
    let record = token_record(test, id).await.unwrap();
    let entries = &record.grant(grant).unwrap().refresh;
    assert!(entries.iter().all(|entry| entry.generation != 0));
    assert!(!matches!(
        record.classify_refresh(id, grant, 0, &secret0, now()),
        ZaRefreshClass::Opened { .. }
    ));
    let next = entries
        .iter()
        .find(|entry| entry.generation == 1)
        .and_then(|entry| entry.next.as_deref())
        .expect("generation 1 holds its sealed next secret");
    assert!(
        unwrap_key(
            next,
            &derive_token_next_key(&secret0),
            &token_next_aad(id, grant, 1)
        )
        .is_err()
    );
    assert_eq!(za_revoke_token(app, &g2).await, 200);
}

async fn refresh_with_missing_entry_deletes_grant(test: &TestServer, app: &ZaOAuthClient) {
    println!("  refresh_with_missing_entry_deletes_grant");
    let id = test.account(OWNER).id().document_id();
    let t0 = za_sign_in(test, app, OWNER, STRONG, None).await;
    test.server
        .za_token_forget_grant(id, t0.grant_id)
        .await
        .unwrap();
    assert!(
        token_record(test, id)
            .await
            .is_none_or(|record| record.grant(t0.grant_id).is_none())
    );
    assert!(
        test.server
            .oauth_grant(t0.grant_id)
            .await
            .unwrap()
            .is_some()
    );
    // Spec 4.5 step 2: the keys are lost, so the grant ends.
    assert_invalid_grant(za_refresh(app, &t0.refresh).await);
    assert!(
        test.server
            .oauth_grant(t0.grant_id)
            .await
            .unwrap()
            .is_none()
    );
}

/// A sign-in of `RACE` with `password`, then its refresh, parked at the
/// pause `set` installs.
async fn park_refresh(
    test: &TestServer,
    app: &Arc<ZaOAuthClient>,
    password: &str,
    set: fn(Option<Arc<test_hooks::Pause>>),
) -> (ZaTokens, HookParked<TokenResponse>) {
    let id = test.account(RACE).id().document_id();
    let tokens = za_sign_in(test, app, RACE, password, None).await;
    let parked = HookParked::start(id, set, {
        let app = app.clone();
        let refresh = tokens.refresh.clone();
        async move { za_refresh(&app, &refresh).await }
    })
    .await;
    (tokens, parked)
}

async fn assert_crossed_refresh_refused(
    test: &TestServer,
    app: &ZaOAuthClient,
    tokens: ZaTokens,
    parked: HookParked<TokenResponse>,
) {
    let id = test.account(RACE).id().document_id();
    assert_invalid_grant(parked.finish().await);
    assert!(raw_token_record(test, id).await.is_none());
    assert_eq!(za_bearer_session(&tokens.access).await.0, 401);
    assert_invalid_grant(za_refresh(app, &tokens.refresh).await);
    // Spec 9.2: the grant object may remain, as any grant of the account
    // does after a credential change. The case removes what it created.
    if test
        .server
        .oauth_grant(tokens.grant_id)
        .await
        .unwrap()
        .is_some()
    {
        za_revoke_grant(&test.server, tokens.grant_id).await;
    }
}

async fn change_race_password(old: &str, new: &str) {
    let reply = za_post(
        "password",
        &json!({ "username": RACE, "password": old, "new_password": new }),
    )
    .await;
    assert_eq!(reply.status, 200);
}

async fn refresh_crossed_by_credential_change(test: &mut TestServer, app: &Arc<ZaOAuthClient>) {
    println!("  refresh_crossed_by_credential_change");
    // Run 1: the grant has rotated; the refresh waits before its first
    // record read.
    let (tokens, parked) =
        park_refresh(test, app, STRONG, test_hooks::set_token_refresh_read_pause).await;
    change_race_password(STRONG, STRONG2).await;
    assert_crossed_refresh_refused(test, app, tokens, parked).await;

    // Run 1b: the same pause, crossed by a recovery.
    let (tokens, parked) =
        park_refresh(test, app, STRONG2, test_hooks::set_token_refresh_read_pause).await;
    let reply = za_post(
        "recover",
        &json!({
            "username": RACE,
            "recovery_key": test.account(RACE).recovery_key.as_deref().unwrap(),
            "new_password": STRONG,
        }),
    )
    .await;
    assert_eq!(reply.status, 200);
    test.accounts.get_mut(RACE).unwrap().recovery_key =
        Some(reply.body["recovery_key"].as_str().unwrap().to_owned());
    assert_crossed_refresh_refused(test, app, tokens, parked).await;

    // Run 2: MK opened and the entries built; the batch then loses its
    // vault assertion.
    let (tokens, parked) =
        park_refresh(test, app, STRONG, test_hooks::set_token_refresh_write_pause).await;
    change_race_password(STRONG, STRONG2).await;
    assert_crossed_refresh_refused(test, app, tokens, parked).await;

    change_race_password(STRONG2, STRONG).await;
}

async fn refresh_crossed_by_other_vault_write(test: &mut TestServer, app: &Arc<ZaOAuthClient>) {
    println!("  refresh_crossed_by_other_vault_write");
    let id = test.account(RACE).id().document_id();
    let pauses: [fn(Option<Arc<test_hooks::Pause>>); 2] = [
        test_hooks::set_token_refresh_read_pause,
        test_hooks::set_token_refresh_write_pause,
    ];
    for set in pauses {
        let before = test.server.za_vault_record(id).await.unwrap().unwrap();
        let (tokens, parked) = park_refresh(test, app, STRONG, set).await;
        rotate_race_recovery_key(test).await;
        let after = test.server.za_vault_record(id).await.unwrap().unwrap();
        assert!(after.record.revision > before.record.revision);
        assert_eq!(
            za_session_binding(&after.record),
            za_session_binding(&before.record)
        );
        // With the write pause the batch asserted the vault bytes of the
        // revision before, so it lost that assertion once and the refresh
        // wrote on a later attempt under the unchanged binding.
        let refreshed = granted(parked.finish().await);
        assert_eq!(generations(test, id, tokens.grant_id).await, [0, 1]);
        assert_eq!(za_bearer_session(&refreshed.access_token).await.0, 200);
        let next = granted(za_refresh(app, refreshed.refresh_token.as_deref().unwrap()).await);
        // Its keys open again at the new revision.
        assert_eq!(za_bearer_session(&tokens.access).await.0, 200);
        assert_eq!(
            za_revoke_token(app, next.refresh_token.as_deref().unwrap()).await,
            200
        );
    }
}

async fn retry_of_g_while_g1_completes(test: &TestServer, app: &Arc<ZaOAuthClient>) {
    println!("  retry_of_g_while_g1_completes");
    let id = test.account(OWNER).id().document_id();
    let pauses: [fn(Option<Arc<test_hooks::Pause>>); 2] = [
        test_hooks::set_token_refresh_read_pause,
        test_hooks::set_token_refresh_write_pause,
    ];
    for set in pauses {
        let t0 = za_sign_in(test, app, OWNER, STRONG, None).await;
        let grant = t0.grant_id;
        let g1 = refresh_of(za_refresh(app, &t0.refresh).await);
        let stored = test.server.oauth_grant(grant).await.unwrap().unwrap();
        assert_eq!(stored.grant.generation, 0);
        assert!(stored.grant.pending);
        assert_eq!(generations(test, id, grant).await, [0, 1]);

        // Upstream accepts the retry of g(0); it waits while g(1) completes.
        let parked = HookParked::start(id, set, {
            let app = app.clone();
            let g0 = t0.refresh.clone();
            async move { za_refresh(&app, &g0).await }
        })
        .await;
        let t2 = granted(za_refresh(app, &g1).await);
        let stored = test.server.oauth_grant(grant).await.unwrap().unwrap();
        assert_eq!(stored.grant.generation, 1);
        assert_eq!(generations(test, id, grant).await, [1, 2]);

        assert_invalid_grant(parked.finish().await);
        let stored = test
            .server
            .oauth_grant(grant)
            .await
            .unwrap()
            .expect("the superseded retry deleted the grant");
        assert_eq!(stored.grant.generation, 1);
        assert_eq!(generations(test, id, grant).await, [1, 2]);
        assert_eq!(za_bearer_session(&t2.access_token).await.0, 200);
        let t3 = granted(za_refresh(app, t2.refresh_token.as_deref().unwrap()).await);
        assert_eq!(
            za_revoke_token(app, t3.refresh_token.as_deref().unwrap()).await,
            200
        );
    }
}

/// R18-1, spec 4.5 step 2: a retry of g(0) whose record snapshot predates the
/// write that added g(1), while the registry has rotated past g(0), is
/// superseded: it deletes no grant and writes nothing. The planted record
/// stands for the stale read.
async fn retry_reading_a_stale_record_keeps_the_grant(test: &TestServer, app: &Arc<ZaOAuthClient>) {
    println!("  retry_reading_a_stale_record_keeps_the_grant");
    let id = test.account(OWNER).id().document_id();
    let t0 = za_sign_in(test, app, OWNER, STRONG, None).await;
    let grant = t0.grant_id;
    let signed_in = raw_token_record(test, id)
        .await
        .expect("the sign-in wrote a record");
    assert_eq!(generations(test, id, grant).await, [0]);
    let g1 = refresh_of(za_refresh(app, &t0.refresh).await);

    let parked = HookParked::start(id, test_hooks::set_token_refresh_read_pause, {
        let app = app.clone();
        let g0 = t0.refresh.clone();
        async move { za_refresh(&app, &g0).await }
    })
    .await;
    let t2 = granted(za_refresh(app, &g1).await);
    let current = raw_token_record(test, id)
        .await
        .expect("the refresh wrote a record");
    set_raw_token_record(test, id, Some(&signed_in)).await;

    assert_invalid_grant(parked.finish().await);
    assert!(
        test.server.oauth_grant(grant).await.unwrap().is_some(),
        "the superseded retry deleted the grant"
    );
    assert!(
        raw_token_record(test, id).await.as_deref() == Some(signed_in.as_slice()),
        "the superseded retry wrote the record"
    );

    set_raw_token_record(test, id, Some(&current)).await;
    let t3 = granted(za_refresh(app, t2.refresh_token.as_deref().unwrap()).await);
    assert_eq!(za_bearer_session(&t3.access_token).await.0, 200);
    assert_eq!(
        za_revoke_token(app, t3.refresh_token.as_deref().unwrap()).await,
        200
    );
}

/// The refresh form through a raw client, whose answer is a server error
/// under `TOKEN_REFRESH_WRITE_FAIL_ONCE`, not a token response.
async fn refresh_write_fails(app: &ZaOAuthClient, id: u32, refresh: &str) {
    test_hooks::set_token_refresh_write_fail_once(Some(id));
    let (status, _) = token_form(&[
        ("grant_type", "refresh_token"),
        ("client_id", app.client_id.as_str()),
        ("refresh_token", refresh),
    ])
    .await;
    assert!(
        !test_hooks::take_token_refresh_write_fail(id),
        "the refresh did not reach its record write"
    );
    assert!((500..600).contains(&status), "expected a server error");
}

async fn failed_write_then_client_retry(test: &TestServer, app: &ZaOAuthClient) {
    println!("  failed_write_then_client_retry");
    let id = test.account(OWNER).id().document_id();
    let t0 = za_sign_in(test, app, OWNER, STRONG, None).await;
    refresh_write_fails(app, id, &t0.refresh).await;
    // The grant rotated; the record did not follow.
    let stored = test.server.oauth_grant(t0.grant_id).await.unwrap().unwrap();
    assert_eq!(stored.grant.generation, 0);
    assert!(stored.grant.pending);
    let record = token_record(test, id).await.unwrap();
    let entries = &record.grant(t0.grant_id).unwrap().refresh;
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.generation)
            .collect::<Vec<_>>(),
        [0]
    );
    assert!(entries[0].next.is_none());

    // The client's retry is upstream's retry case.
    let t1 = granted(za_refresh(app, &t0.refresh).await);
    assert_eq!(za_bearer_session(&t1.access_token).await.0, 200);
    assert_eq!(generations(test, id, t0.grant_id).await, [0, 1]);
    let t2 = granted(za_refresh(app, t1.refresh_token.as_deref().unwrap()).await);
    assert_eq!(
        za_revoke_token(app, t2.refresh_token.as_deref().unwrap()).await,
        200
    );
}

async fn interrupted_refresh_cleared_by_another_grants_write(
    test: &TestServer,
    app: &ZaOAuthClient,
) {
    println!("  interrupted_refresh_cleared_by_another_grants_write");
    let id = test.account(OWNER).id().document_id();
    let a = za_sign_in(test, app, OWNER, STRONG, None).await;
    let g1 = refresh_of(za_refresh(app, &a.refresh).await);
    refresh_write_fails(app, id, &g1).await;
    // Entry 0 is below the stored generation and nobody retries.
    assert_eq!(generations(test, id, a.grant_id).await, [0, 1]);
    let stored = test.server.oauth_grant(a.grant_id).await.unwrap().unwrap();
    assert_eq!(stored.grant.generation, 1);

    // Spec 4.5: any later write of the record drops it.
    let b = za_sign_in(test, app, OWNER, STRONG, None).await;
    assert_eq!(generations(test, id, a.grant_id).await, [1]);
    assert!(
        token_record(test, id)
            .await
            .unwrap()
            .grant(b.grant_id)
            .is_some()
    );
    assert_eq!(za_revoke_token(app, &g1).await, 200);
    assert_eq!(za_revoke_token(app, &b.refresh).await, 200);
}

async fn sign_in_with_short_expiry(test: &TestServer, app: &ZaOAuthClient) -> ZaTokens {
    let _expiry = oauth_expiry(test, Some(3), Some(3), None);
    za_sign_in(test, app, OWNER, STRONG, None).await
}

async fn sweep_removes_expired_entries(
    test: &TestServer,
    admin: &Account,
    app: &ZaOAuthClient,
    id: u32,
) {
    println!("  sweep_removes_expired_entries");
    let tokens = sign_in_with_short_expiry(test, app).await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    let record = token_record(test, id).await.expect("expiry is not a write");
    assert!(record.grant(tokens.grant_id).is_some());
    assert!(
        record
            .open_access(id, tokens.grant_id, &token_secret(&tokens.access), now())
            .is_none()
    );
    assert_eq!(za_bearer_session(&tokens.access).await.0, 401);

    run_purge_maintenance(test, admin).await;
    assert!(
        raw_token_record(test, id).await.is_none(),
        "D8: expired entries survived the sweep"
    );
    za_revoke_grant(&test.server, tokens.grant_id).await;
}

async fn registry_destroyed_grant(
    test: &TestServer,
    admin: &Account,
    owner: &Account,
    app: &ZaOAuthClient,
) {
    println!("  registry_destroyed_grant");
    let id = owner.id().document_id();
    let tokens = za_sign_in(test, app, OWNER, STRONG, None).await;
    let grant = Id::new(tokens.grant_id);
    assert_eq!(za_bearer_session(&tokens.access).await.0, 200);
    owner
        .registry_destroy(ObjectType::OAuthGrant, [grant])
        .await
        .assert_destroyed(&[grant]);
    assert_eq!(za_bearer_session(&tokens.access).await.0, 401);
    assert!(
        token_record(test, id)
            .await
            .unwrap()
            .grant(tokens.grant_id)
            .is_some()
    );

    run_purge_maintenance(test, admin).await;
    assert!(
        raw_token_record(test, id).await.is_none(),
        "D8: a deleted grant survived the sweep"
    );
}

async fn interrupted_refresh_cleared_by_sweep(
    test: &TestServer,
    admin: &Account,
    app: &ZaOAuthClient,
    id: u32,
) {
    println!("  interrupted_refresh_cleared_by_sweep");
    let tokens = za_sign_in(test, app, OWNER, STRONG, None).await;
    let g1 = refresh_of(za_refresh(app, &tokens.refresh).await);
    refresh_write_fails(app, id, &g1).await;
    let grant = tokens.grant_id;
    let stored = test.server.oauth_grant(grant).await.unwrap().unwrap();
    assert_eq!(stored.grant.generation, 1);
    assert_eq!(generations(test, id, grant).await, [0, 1]);

    run_purge_maintenance(test, admin).await;
    assert_eq!(
        generations(test, id, grant).await,
        [1],
        "D8: only the stored generation survives"
    );
    assert!(test.server.oauth_grant(grant).await.unwrap().is_some());
    za_revoke_grant(&test.server, grant).await;
}

async fn sweep_passes_a_broken_record(
    test: &TestServer,
    admin: &Account,
    app: &ZaOAuthClient,
    owner: u32,
    mfa: u32,
    race: u32,
) {
    println!("  sweep_passes_a_broken_record");
    assert!(
        mfa < owner && owner < race,
        "broken records must surround the expired record"
    );
    let tokens = sign_in_with_short_expiry(test, app).await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(raw_token_record(test, owner).await.is_some());
    for id in [mfa, race] {
        set_raw_token_record(test, id, Some(PLANTED)).await;
    }

    run_purge_maintenance(test, admin).await;
    assert!(raw_token_record(test, owner).await.is_none());
    for id in [mfa, race] {
        assert!(raw_token_record(test, id).await.as_deref() == Some(PLANTED));
        set_raw_token_record(test, id, None).await;
    }
    za_revoke_grant(&test.server, tokens.grant_id).await;
}

async fn account_destruction_removes_the_record(
    test: &TestServer,
    admin: &Account,
    app: &ZaOAuthClient,
) {
    println!("  account_destruction_removes_the_record");
    let gone = admin
        .create_key_user_account(GONE, STRONG, "Token Gone", &[], user_permissions())
        .await;
    let id = gone.id().document_id();
    za_sign_in(test, app, GONE, STRONG, None).await;
    assert!(raw_token_record(test, id).await.is_some());

    admin.destroy_account(gone).await;
    test.wait_for_tasks().await;
    assert!(raw_token_record(test, id).await.is_none());
}

/// Answers of the token record's server API that no request picks on a
/// healthy store: taking a hand-off nobody stored; a hand-off put with keys
/// of another account (the sign-in failure, nothing stored); an issue and a
/// refresh for an account without a vault record (`VaultChanged`, no record
/// written); an issue naming a grant that does not exist (an error, the record
/// unchanged); a refresh presenting a secret no entry holds (no keys, the
/// record unchanged); and a sweep over a current record (unchanged).
async fn server_token_answers(test: &TestServer, id: u32, plain_id: u32) {
    println!("  server_token_answers");
    assert!(
        test.server
            .za_handoff_take(&"p".repeat(CODE_LEN), &Secret::random())
            .await
            .unwrap()
            .is_none(),
        "a hand-off nobody stored was taken"
    );

    let keys = za_unlock(test, id, STRONG).await;
    let mfa = test.account(MFA).id().document_id();
    let key_id = "q".repeat(CODE_LEN);
    let refused = test
        .server
        .za_handoff_put(&key_id, mfa, &keys, now() + 60)
        .await
        .err()
        .unwrap_or_else(|| panic!("a hand-off was put with another account's keys"));
    assert_eq!(refused.event_type(), AUTH_FAILED);
    assert!(!handoff_exists(test, &key_id).await);

    assert!(matches!(
        issue(test, plain_id, 1, keys.mk(), 0).await,
        ZaTokenIssue::VaultChanged
    ));
    assert!(matches!(
        test.server
            .za_token_refresh(
                plain_id,
                1,
                0,
                &Secret::random(),
                1,
                now() + HOUR,
                now() + MONTH
            )
            .await
            .unwrap(),
        ZaTokenRefresh::VaultChanged
    ));
    assert!(raw_token_record(test, plain_id).await.is_none());

    let (grant, _, keys, binding) = issued_for(test, OWNER, id).await;
    let raw = raw_token_record(test, id).await;
    assert!(raw.is_some());
    assert!(
        test.server
            .za_token_issue(
                id,
                u64::MAX - 7,
                keys.mk(),
                binding,
                now() + HOUR,
                now() + MONTH
            )
            .await
            .is_err(),
        "an issue naming no grant succeeded"
    );
    assert_eq!(raw_token_record(test, id).await, raw);

    let generation = test
        .server
        .oauth_grant(grant)
        .await
        .unwrap()
        .expect("the issued grant")
        .grant
        .generation;
    let refreshed = test
        .server
        .za_token_refresh(
            id,
            grant,
            generation,
            &Secret::random(),
            generation + 1,
            now() + HOUR,
            now() + MONTH,
        )
        .await
        .unwrap();
    assert!(
        matches!(refreshed, ZaTokenRefresh::Lost | ZaTokenRefresh::Superseded),
        "a refresh with a secret no entry holds was given keys"
    );
    assert_eq!(raw_token_record(test, id).await, raw);

    test.server.za_token_sweep().await.unwrap();
    assert_eq!(raw_token_record(test, id).await, raw);

    // A record under another binding opens no keys for a refresh; the
    // record is left as it is.
    use store::Serialize;
    let mut stale = token_record(test, id).await.unwrap();
    stale.binding ^= 1;
    let archive = store::write::Archiver::with_compression(stale, store::write::Compression::None)
        .serialize()
        .unwrap();
    let planted = [&[TOKEN_RECORD_VERSION][..], &archive].concat();
    set_raw_token_record(test, id, Some(&planted)).await;
    assert!(matches!(
        test.server
            .za_token_refresh(
                id,
                grant,
                generation,
                &Secret::random(),
                generation + 1,
                now() + HOUR,
                now() + MONTH
            )
            .await
            .unwrap(),
        ZaTokenRefresh::VaultChanged
    ));
    assert_eq!(
        raw_token_record(test, id).await.as_deref(),
        Some(planted.as_slice())
    );
    set_raw_token_record(test, id, raw.as_deref()).await;

    // A second issue for a grant the record holds replaces its entries.
    assert!(matches!(
        issue(test, id, grant, keys.mk(), binding).await,
        ZaTokenIssue::Issued(_)
    ));
    assert!(token_record(test, id).await.unwrap().grant(grant).is_some());
    za_revoke_grant(&test.server, grant).await;
}
