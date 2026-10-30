/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    STRONG,
    tracing::{ws_call, ws_close, ws_connect},
};
use crate::utils::{
    imap::{ImapConnection, Type},
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{Parked, SERVER_URL, assert_nothing_cached, za_grant_token, za_revoke_grant},
};
use ::registry::{
    schema::{prelude::Property, structs::OidcProvider},
    types::duration::Duration as RegistryDuration,
};
use common::{
    Caches, HttpAuthCache,
    auth::{AuthRequest, TokenScope, oauth::scope::OAuthScopes},
    ipc::CacheInvalidation,
};
use http::auth::authenticate::za_test;
use hyper::StatusCode;
use imap_proto::ResponseType;
use serde_json::{Value, json};
use std::{
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use vault::{
    cache::{KeyCache, KeyCacheConfig},
    keys::Secret,
    session::SessionKeys,
};

/// Set up in `setup`, never used by another module, password unchanged.
const NAME: &str = "key3@example.com";
const HOME: &str = "/dav/cal/key3@example.com/";
/// The suite's ordinary account (`za_tests`).
const PLAIN: &str = "plain@example.com";
const ADMIN: &str = "admin@example.com";
const MAIL_USING: [&str; 2] = ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"];

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access cache tests...");
    residency_rules();
    key_cache_only_over_http(test).await;
    live_caches(test).await;
    scoped_cache_hit_preserves_scope(test).await;
    revoked_grant_cannot_repopulate_cache(test).await;
    expired_bearer_cache_entry_is_refused(test).await;
    keyless_key_account_logins_are_not_cached(test).await;
    impersonated_websocket_refresh(test).await;
}

/// Spec 2 and 5, amended by mail spec 3.3: the key cache serves the HTTP
/// layer only. IMAP and POP3 sessions hold their keys for the connection
/// (`mail::login`), but a mail login derives afresh and leaves nothing in
/// the key cache; `Server::authenticate` (the entry point of SMTP and
/// ManageSieve) returns keyless tokens by construction.
async fn key_cache_only_over_http(test: &TestServer) {
    let id = test.account(NAME).id().document_id();
    let caches = &test.server.inner.cache;
    caches.za_keys.clear();
    clear_http_auth(caches);

    let mut imap = ImapConnection::connect(b"_z ").await;
    imap.authenticate(NAME, STRONG).await;
    imap.send("NOOP").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    assert!(
        !caches.za_keys.contains_account(id),
        "an IMAP login puts nothing in the key cache"
    );

    let request = AuthRequest::from_plain(NAME, STRONG, 0, IpAddr::from([127, 0, 0, 1]));
    let token = test
        .server
        .authenticate(&request)
        .await
        .unwrap()
        .access_token;
    assert_eq!(token.account_id(), id);
    assert!(token.session_keys().is_none(), "a keyless token");
    let (authenticated, keys) = test.server.authenticate_with_keys(&request).await.unwrap();
    assert!(
        authenticated.access_token.session_keys().is_none(),
        "keys travel beside the token"
    );
    let keys = keys.expect("the key account's keys");
    assert_eq!(keys.account_id, id);
    assert!(
        !caches.za_keys.contains_account(id),
        "neither entry point makes keys resident"
    );
}

/// Spec 5 residency rules, on a standalone cache with the default
/// configuration and synthetic instants (the server's caches use the clock).
fn residency_rules() {
    let config = KeyCacheConfig::default();
    let cache = KeyCache::new(config);
    let keys = Arc::new(SessionKeys::new(1, 1, Secret::random()));
    let second = Duration::from_secs(1);
    let t0 = Instant::now();

    // Idle timeout: an entry inserted and never looked up again is refused by
    // a lookup once past the timeout, and removed by the next sweep.
    assert_eq!(cache.insert(fp(1), keys.clone(), t0), None);
    assert_eq!(cache.insert(fp(2), keys.clone(), t0), None);
    assert_eq!(
        cache.sweep(t0 + config.idle - second),
        Vec::<[u8; 32]>::new(),
        "nothing idle yet"
    );
    assert!(
        cache.get(&fp(1), t0 + config.idle).is_none(),
        "an idle entry is refused at lookup"
    );
    assert_eq!(cache.len(), 1, "a refused entry is removed");
    assert_eq!(
        cache.sweep(t0 + config.idle),
        vec![fp(2)],
        "the sweep removes the idle entry never looked up, and names it"
    );
    assert!(cache.is_empty());

    // Sliding idle timeout, hard cap from insertion: an entry used just
    // within every idle period outlives the idle timeout but not the cap,
    // both at lookup and at sweep.
    let refused: fn(&KeyCache, Instant) -> bool = |cache, now| cache.get(&fp(3), now).is_none();
    let swept: fn(&KeyCache, Instant) -> bool = |cache, now| cache.sweep(now) == vec![fp(3)];
    for at_cap in [refused, swept] {
        assert_eq!(cache.insert(fp(3), keys.clone(), t0), None);
        let step = config.idle - second;
        let mut now = t0;
        while now + step < t0 + config.max_age {
            now += step;
            assert!(
                cache.get(&fp(3), now).is_some(),
                "a used entry slides past the idle timeout"
            );
        }
        assert!(now > t0 + config.idle);
        assert!(
            at_cap(&cache, t0 + config.max_age),
            "a continuously used entry is dropped at the hard cap"
        );
        assert!(cache.is_empty());
    }

    // Bounded entry count with least recently used eviction.
    for i in 0..config.max_entries {
        assert_eq!(
            cache.insert(fp(i), keys.clone(), t0 + Duration::from_millis(i as u64)),
            None
        );
    }
    let later = t0 + Duration::from_millis(config.max_entries as u64);
    assert!(cache.get(&fp(0), later).is_some());
    assert_eq!(
        cache.insert(fp(config.max_entries), keys.clone(), later + second),
        Some(fp(1)),
        "the evicted fingerprint is returned"
    );
    assert_eq!(cache.len(), config.max_entries, "the bound holds");
    assert!(
        cache.get(&fp(1), later + second).is_none(),
        "the least recently used entry was evicted"
    );
    assert!(cache.get(&fp(0), later + second).is_some());

    // Eviction drops the cache's references, so the keys are zeroed with
    // the last one.
    cache.clear();
    assert_eq!(Arc::strong_count(&keys), 1, "the cache holds no reference");
}

fn fp(i: usize) -> [u8; 32] {
    let mut fp = [0u8; 32];
    fp[..8].copy_from_slice(&(i as u64).to_be_bytes());
    fp
}

/// What the live server shows deterministically: hit and miss, the hit rule
/// (interface sheet 2: a key account's entry is served only with resident
/// keys of its current generation), idle removal and invalidation.
///
/// 1.0 keys `http_auth` on a keyed hash of the token, so an entry cannot be
/// dropped by fingerprint when its keys go; the hit rule refuses it instead.
/// A full verification is observed as new resident keys (a new `Arc`): the
/// entry's expiry has a one-second resolution at 1.0.
async fn live_caches(test: &TestServer) {
    let id = test.account(NAME).id().document_id();
    let client = DummyWebDavClient::new(id, NAME, STRONG, NAME);
    let token = client.credentials.strip_prefix("Basic ").unwrap();
    let caches = &test.server.inner.cache;
    let fp = caches.za_fingerprint(token);
    let generation = test.server.account(id).await.unwrap().za_generation();
    assert_ne!(generation, 0, "a key account");
    caches.za_keys.clear();
    clear_http_auth(caches);

    // Miss: full verification fills both caches, the keys under the
    // fingerprint.
    propfind(&client).await;
    assert_holds_no_credential(caches, &[token]);
    let entry = caches.http_auth.get(token).unwrap();
    assert_eq!(entry.account_id, id);
    let keys = caches.za_keys.get(&fp, Instant::now()).unwrap();
    assert_eq!(keys.account_id, id);
    assert_eq!(keys.generation, generation);
    assert_eq!(caches.za_keys.len(), 1);

    // Hit: served from the caches, nothing re-verified or added.
    propfind(&client).await;
    assert_eq!(http_auth_entries(caches).len(), 1);
    assert_eq!(caches.za_keys.len(), 1, "a hit does not add entries");
    assert_eq!(
        caches.http_auth.get(token).unwrap().expires,
        entry.expires,
        "a hit does not re-verify"
    );
    assert!(
        Arc::ptr_eq(&caches.za_keys.get(&fp, Instant::now()).unwrap(), &keys),
        "a hit serves the resident keys"
    );

    // Idle keys: a sweep once the server's idle timeout has passed removes
    // them. The authentication entry stays (1.0 cannot remove it by
    // fingerprint), but the hit rule refuses it without its keys: the next
    // request re-verifies and is served with fresh keys (spec 5: a
    // fingerprint authenticates no longer than its keys).
    let live = KeyCacheConfig::from_env();
    assert_eq!(caches.za_sweep_keys(Instant::now() + live.idle), 1);
    assert!(caches.za_keys.is_empty());
    assert!(
        caches.http_auth.get(token).is_some(),
        "1.0 keeps the entry; the hit rule refuses it"
    );
    let keys = reverified(caches, &client, token, &keys).await;
    assert_eq!(keys.generation, generation);
    assert_eq!(caches.za_keys.len(), 1);
    assert_holds_no_credential(caches, &[token]);

    // Keys evicted while the entry stays (the bound, or another node's
    // invalidation of the key cache only): the entry is never served
    // keyless (3a's concern 1).
    caches.za_keys.clear();
    let keys = reverified(caches, &client, token, &keys).await;
    assert_eq!(keys.generation, generation);

    // Resident keys of a stale generation are discarded on hit.
    caches.za_insert_keys(
        fp,
        Arc::new(SessionKeys::new(id, generation + 1, Secret::random())),
        Instant::now(),
    );
    let stale = caches.za_keys.get(&fp, Instant::now()).unwrap();
    let keys = reverified(caches, &client, token, &stale).await;
    assert_eq!(keys.generation, generation, "keys rebuilt");
    assert_eq!(caches.za_keys.len(), 1);

    // Resident keys of another account are discarded on hit as well.
    caches.za_insert_keys(
        fp,
        Arc::new(SessionKeys::new(id + 1, generation, Secret::random())),
        Instant::now(),
    );
    let foreign = caches.za_keys.get(&fp, Instant::now()).unwrap();
    let keys = reverified(caches, &client, token, &foreign).await;
    assert_eq!(keys.account_id, id, "keys rebuilt");
    drop(keys);

    // Invalidation drops both caches' entries for the account.
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(id)]);
    assert_nothing_cached(test, id);
    assert!(caches.za_keys.is_empty());
    assert!(http_auth_entries(caches).is_empty());
}

async fn propfind(client: &DummyWebDavClient) {
    client
        .request("PROPFIND", HOME, "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
}

/// Sends a request expected to miss the caches (refused by the hit rule)
/// and returns the keys it made resident, which are not `previous`.
async fn reverified(
    caches: &Caches,
    client: &DummyWebDavClient,
    token: &str,
    previous: &Arc<SessionKeys>,
) -> Arc<SessionKeys> {
    propfind(client).await;
    assert_eq!(http_auth_entries(caches).len(), 1);
    assert!(caches.http_auth.get(token).is_some());
    let keys = caches
        .za_keys
        .get(&caches.za_fingerprint(token), Instant::now())
        .expect("the request re-verified and made its keys resident");
    assert!(!Arc::ptr_eq(&keys, previous), "the request re-verified");
    keys
}

/// Every entry of the HTTP authentication cache (1.0 exposes no iterator;
/// `retain` visits each entry and keeps it).
fn http_auth_entries(caches: &Caches) -> Vec<HttpAuthCache> {
    let entries = Mutex::new(Vec::new());
    caches.http_auth.retain(|entry| {
        entries.lock().unwrap().push(entry.clone());
        true
    });
    entries.into_inner().unwrap()
}

/// Empties the HTTP authentication cache (1.0's only public clear, which
/// also advances the revocation epoch).
fn clear_http_auth(caches: &Caches) {
    caches.grant_revocations.clear(&caches.http_auth);
}

/// Spec 5 and 11: the authentication cache is keyed by 1.0's keyed hash of
/// the token (not reversible, never caller-chosen), its entries carry no
/// password, token or key bytes, and resident keys sit under the keyed
/// fingerprint of a known token, reachable only as `Arc<SessionKeys>`.
/// Failure messages name no secret.
fn assert_holds_no_credential(caches: &Caches, tokens: &[&str]) {
    let secrets: Vec<Vec<u8>> = tokens
        .iter()
        .flat_map(|token| [token.as_bytes().to_vec(), basic_decode(token)])
        .chain([STRONG.as_bytes().to_vec()])
        .collect();

    for entry in http_auth_entries(caches) {
        let debug = format!("{entry:?}");
        for secret in &secrets {
            assert!(
                !contains(debug.as_bytes(), secret),
                "an authentication cache entry holds a credential"
            );
        }
    }

    for token in tokens {
        let fp = caches.za_fingerprint(token);
        for secret in &secrets {
            assert!(
                !contains(secret, &fp) && !contains(&fp, secret),
                "a key cache fingerprint holds credential bytes"
            );
        }
        if let Some(keys) = caches.za_keys.get(&fp, Instant::now()) {
            let debug = format!("{keys:?}");
            for material in [keys.mk().as_bytes(), keys.ewk().as_bytes()] {
                let hex: String = material.iter().map(|b| format!("{b:02x}")).collect();
                assert!(
                    !debug.contains(&hex) && !debug.contains(&format!("{material:?}")),
                    "resident keys print key bytes"
                );
            }
            for secret in &secrets {
                assert!(
                    !contains(debug.as_bytes(), secret),
                    "resident keys print a credential"
                );
            }
        }
    }
}

fn basic_decode(token: &str) -> Vec<u8> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD.decode(token).unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len() && haystack.windows(needle.len()).any(|w| w == needle)
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

/// `GET /jmap/session` with a Bearer token; the status.
async fn session_status(token: &str) -> u16 {
    http_client()
        .get(format!("{SERVER_URL}/jmap/session"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

/// A JMAP call with `authorization` (a whole header value); the status and
/// the first method response.
async fn jmap_call(authorization: &str, method: &str, args: Value) -> (u16, Value) {
    let response = http_client()
        .post(format!("{SERVER_URL}/jmap"))
        .header("Authorization", authorization)
        .header("Content-Type", "application/json")
        .body(json!({ "using": MAIL_USING, "methodCalls": [[method, args, "c0"]] }).to_string())
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let reply: Value = response.json().await.unwrap_or_default();
    (status, reply["methodResponses"][0].clone())
}

fn basic(name: &str, secret: &str) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    format!("Basic {}", STANDARD.encode(format!("{name}:{secret}")))
}

/// C4: an ordinary account's cached OAuth token keeps its grant's scope on a
/// cache hit. The token's grant allows calendars only, so the mail
/// capability is dropped from its requests (`allowed_capabilities`), on the
/// miss and on the hit that `AccessToken::renew` serves with the cached
/// `token_scope`. A Basic login of the same account (unscoped) is the
/// control.
async fn scoped_cache_hit_preserves_scope(test: &TestServer) {
    println!("scoped_cache_hit_preserves_scope");
    let plain = test.account(PLAIN);
    let plain_id = plain.id().document_id();
    let mailbox_get = || json!({ "accountId": plain.id_string(), "ids": null });

    let (status, control) = jmap_call(
        &basic(plain.name(), plain.secret()),
        "Mailbox/get",
        mailbox_get(),
    )
    .await;
    assert_eq!(
        (status, control[0].as_str()),
        (200, Some("Mailbox/get")),
        "control: an unscoped login reads mailboxes: {control}"
    );

    let (grant_id, token) =
        za_grant_token(&test.server, plain.name(), OAuthScopes::CALENDARS).await;
    let bearer = format!("Bearer {token}");
    let caches = &test.server.inner.cache;
    caches.http_auth.remove(&token);

    // Miss: full verification; the entry keeps the grant's scope.
    let (status, refused) = jmap_call(&bearer, "Mailbox/get", mailbox_get()).await;
    assert_eq!(status, 200);
    assert_eq!(refused[0], "error", "the miss is scoped: {refused}");
    let entry = caches
        .http_auth
        .get(&token)
        .expect("an ordinary account's token is cached");
    assert_eq!(entry.account_id, plain_id);
    assert!(
        matches!(
            entry.token_scope,
            Some(TokenScope::Grant { grant_id: id, scopes, .. })
                if id == grant_id && scopes == OAuthScopes::CALENDARS
        ),
        "the entry keeps the grant's scope: {:?}",
        entry.token_scope
    );
    assert!(caches.http_auth.is_live(&entry));

    // Hit: the live entry serves the request, still scoped.
    let (status, refused) = jmap_call(&bearer, "Mailbox/get", mailbox_get()).await;
    assert_eq!(status, 200);
    assert_eq!(refused[0], "error", "the hit is scoped: {refused}");
    assert_eq!(
        caches.http_auth.get(&token).map(|entry| entry.expires),
        Some(entry.expires),
        "served from the cached entry"
    );

    za_revoke_grant(&test.server, grant_id).await;
    assert!(
        caches.http_auth.get(&token).is_none(),
        "evicted by the revocation"
    );
}

/// C4: a verification that started before a grant's revocation cannot write
/// the revoked token back into the cache (1.0's revocation epoch). The
/// request is parked after its verification and before the cache insert
/// (`za_test::set`), the grant is revoked, then the request is released:
/// it completes, but nothing is cached, and the next request with the token
/// is verified afresh and refused.
async fn revoked_grant_cannot_repopulate_cache(test: &TestServer) {
    println!("revoked_grant_cannot_repopulate_cache");
    let plain = test.account(PLAIN);
    let plain_id = plain.id().document_id();
    let (grant_id, token) = za_grant_token(&test.server, plain.name(), OAuthScopes::ALL).await;
    let caches = &test.server.inner.cache;
    caches.http_auth.remove(&token);

    let request_token = token.clone();
    let parked = Parked::start(plain_id, za_test::set, async move {
        session_status(&request_token).await
    })
    .await;
    let epoch = caches.grant_revocations.epoch();
    za_revoke_grant(&test.server, grant_id).await;
    assert_ne!(
        caches.grant_revocations.epoch(),
        epoch,
        "the revocation advanced the epoch"
    );
    let status = parked.finish().await;
    assert!(
        matches!(status, 200 | 401),
        "the request in flight completes: {status}"
    );

    assert!(
        caches.http_auth.get(&token).is_none(),
        "the late loader did not write the revoked token back"
    );
    assert_eq!(
        session_status(&token).await,
        401,
        "the revoked token is refused"
    );
    assert!(caches.http_auth.get(&token).is_none());
}

/// C4: an expired cached Bearer token is refused. The token is minted while
/// the access-token lifetime is `TOKEN_SECS`, then the lifetime is restored,
/// so the cache's own bound (`oauth_expiry_token`) is far longer than the
/// token's: the entry must expire with the token (`expires_in`), and the
/// request after it is verified afresh and refused.
async fn expired_bearer_cache_entry_is_refused(test: &TestServer) {
    const TOKEN_SECS: u64 = 5;
    println!("expired_bearer_cache_entry_is_refused");
    let admin = test.account(ADMIN);
    let plain = test.account(PLAIN);
    let lifetime = test
        .server
        .inner
        .build_server()
        .core
        .oauth
        .oauth_expiry_token;
    assert!(lifetime > 60, "the suite's access tokens outlive this test");

    set_access_token_expiry(admin, TOKEN_SECS).await;
    let server = test.server.inner.build_server();
    assert_eq!(server.core.oauth.oauth_expiry_token, TOKEN_SECS);
    let (grant_id, token) = za_grant_token(&server, plain.name(), OAuthScopes::ALL).await;
    set_access_token_expiry(admin, lifetime).await;
    assert_eq!(
        test.server
            .inner
            .build_server()
            .core
            .oauth
            .oauth_expiry_token,
        lifetime
    );

    let caches = &test.server.inner.cache;
    caches.http_auth.remove(&token);
    assert_eq!(session_status(&token).await, 200);
    let entry = caches
        .http_auth
        .get(&token)
        .expect("an ordinary account's token is cached");
    assert_eq!(session_status(&token).await, 200, "a hit before expiry");

    tokio::time::sleep(Duration::from_secs(TOKEN_SECS + 2)).await;
    assert!(
        !caches.http_auth.is_live(&entry),
        "the entry expired with the token"
    );
    assert_eq!(
        session_status(&token).await,
        401,
        "the expired token is refused"
    );
    assert!(caches.http_auth.get(&token).is_none());

    za_revoke_grant(&test.server, grant_id).await;
}

async fn set_access_token_expiry(admin: &crate::utils::account::Account, secs: u64) {
    admin
        .registry_update_setting(
            OidcProvider {
                access_token_expiry: RegistryDuration::from_millis(secs * 1000),
                ..Default::default()
            },
            &[Property::AccessTokenExpiry],
        )
        .await;
    admin.reload_settings().await;
}

/// C2, C4: a key account's keyless logins over HTTP, its own OAuth Bearer
/// token and an impersonation of it, are refused the key-needing methods,
/// on the first request and on every later one: neither is cached, so no
/// hit can serve them.
async fn keyless_key_account_logins_are_not_cached(test: &TestServer) {
    println!("keyless_key_account_logins_are_not_cached");
    let key = test.account(NAME);
    let admin = test.account(ADMIN);
    let caches = &test.server.inner.cache;
    let (grant_id, token) = za_grant_token(&test.server, NAME, OAuthScopes::ALL).await;
    let impersonation = basic(&format!("{NAME}%{}", admin.name()), admin.secret());
    let impersonation_token = impersonation.strip_prefix("Basic ").unwrap();
    for (authorization, cached) in [
        (format!("Bearer {token}"), token.as_str()),
        (impersonation.clone(), impersonation_token),
    ] {
        for attempt in 0..2 {
            let (status, response) = jmap_call(
                &authorization,
                "Email/get",
                json!({ "accountId": key.id_string(), "ids": [] }),
            )
            .await;
            assert_eq!(status, 200);
            assert_eq!(
                (response[0].as_str(), response[1]["type"].as_str()),
                (Some("error"), Some("forbidden")),
                "attempt {attempt}: {response}"
            );
            assert!(
                caches.http_auth.get(cached).is_none(),
                "a keyless login of a key account is not cached"
            );
        }
    }
    za_revoke_grant(&test.server, grant_id).await;
}

/// Ruling 11: an impersonated JMAP WebSocket keeps R35's marker across a
/// token renewal. The socket's token is renewed on its next request after
/// the account's access token is invalidated (`refresh_access_token`), and
/// `Mailbox/get` on the key account is still refused; a renewal that lost
/// the marker would serve it as the owner's keyless session. An
/// impersonated socket of the ordinary account is the control.
async fn impersonated_websocket_refresh(test: &TestServer) {
    println!("impersonated_websocket_refresh");
    let admin = test.account(ADMIN);
    for (name, refused) in [(NAME, true), (PLAIN, false)] {
        let account = test.account(name);
        let account_id = account.id().document_id();
        let call = json!({
            "@type": "Request",
            "id": "1",
            "using": MAIL_USING,
            "methodCalls": [["Mailbox/get", { "accountId": account.id_string(), "ids": null }, "c0"]]
        })
        .to_string();
        let mut socket = ws_connect(&format!("{name}%{}", admin.name()), admin.secret()).await;

        for phase in ["before the renewal", "after the renewal"] {
            if phase == "after the renewal" {
                let before = test.server.access_token(account_id).await.unwrap();
                test.server
                    .invalidate_local_caches(&[CacheInvalidation::AccessToken(account_id)]);
                assert!(
                    !Arc::ptr_eq(
                        &before,
                        &test.server.access_token(account_id).await.unwrap()
                    ),
                    "the socket's token is stale and is renewed on its next request"
                );
            }
            let reply: Value = serde_json::from_str(&ws_call(&mut socket, &call).await).unwrap();
            let response = &reply["methodResponses"][0];
            if refused {
                assert_eq!(
                    (response[0].as_str(), response[1]["type"].as_str()),
                    (Some("error"), Some("forbidden")),
                    "{name} {phase}: {response}"
                );
            } else {
                assert_eq!(
                    response[0], "Mailbox/get",
                    "control: {name} {phase}: {response}"
                );
            }
        }
        ws_close(socket).await;
    }
}
