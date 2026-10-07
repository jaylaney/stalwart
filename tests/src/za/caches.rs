/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{server::TestServer, webdav::DummyWebDavClient, za::assert_nothing_cached};
use common::{Caches, ipc::CacheInvalidation};
use hyper::StatusCode;
use std::{
    sync::Arc,
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

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access cache tests...");
    residency_rules();
    live_caches(test).await;
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
    cache.insert(fp(1), keys.clone(), t0);
    cache.insert(fp(2), keys.clone(), t0);
    assert_eq!(
        cache.sweep(t0 + config.idle - second),
        0,
        "nothing idle yet"
    );
    assert!(
        cache.get(&fp(1), t0 + config.idle).is_none(),
        "an idle entry is refused at lookup"
    );
    assert_eq!(cache.len(), 1, "a refused entry is removed");
    assert_eq!(
        cache.sweep(t0 + config.idle),
        1,
        "the sweep removes the idle entry never looked up"
    );
    assert!(cache.is_empty());

    // Sliding idle timeout, hard cap from insertion: an entry used just
    // within every idle period outlives the idle timeout but not the cap,
    // both at lookup and at sweep.
    let refused: fn(&KeyCache, Instant) -> bool = |cache, now| cache.get(&fp(3), now).is_none();
    let swept: fn(&KeyCache, Instant) -> bool = |cache, now| cache.sweep(now) == 1;
    for at_cap in [refused, swept] {
        cache.insert(fp(3), keys.clone(), t0);
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
        cache.insert(fp(i), keys.clone(), t0 + Duration::from_millis(i as u64));
    }
    let later = t0 + Duration::from_millis(config.max_entries as u64);
    assert!(cache.get(&fp(0), later).is_some());
    cache.insert(fp(config.max_entries), keys.clone(), later + second);
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

/// What the live server shows deterministically: hit and miss, the
/// generation fence on stale entries, idle removal and invalidation.
async fn live_caches(test: &TestServer) {
    let id = test.account(NAME).id().document_id();
    let client = DummyWebDavClient::new(id, NAME, STRONG, NAME);
    let token = client.credentials.strip_prefix("Basic ").unwrap();
    let caches = &test.server.inner.cache;
    let fp = caches.za_fingerprint(token);
    let generation = test.server.account(id).await.unwrap().za_generation;
    assert_ne!(generation, 0, "a key account");
    caches.za_keys.clear();
    caches.http_auth.clear();

    // Miss: full verification fills both caches under the fingerprint.
    propfind(&client).await;
    assert_holds_no_credential(caches, &[(fp, token)]);
    let entry = caches.http_auth.peek(&fp).unwrap();
    assert_eq!(entry.account_id, id);
    assert_eq!(entry.generation, generation);
    let keys = caches.za_keys.get(&fp, Instant::now()).unwrap();
    assert_eq!(keys.account_id, id);
    assert_eq!(keys.generation, generation);
    assert_eq!(caches.za_keys.len(), 1);

    // Hit: served from the caches, nothing re-verified or added.
    propfind(&client).await;
    assert_eq!(caches.http_auth.inner().len(), 1);
    assert_eq!(caches.za_keys.len(), 1, "a hit does not add entries");
    assert!(
        caches.http_auth.peek(&fp).unwrap().expires == entry.expires,
        "a hit does not re-verify"
    );
    assert!(
        Arc::ptr_eq(&caches.za_keys.get(&fp, Instant::now()).unwrap(), &keys),
        "a hit serves the resident keys"
    );
    drop(keys);

    // Idle keys: a sweep once the server's idle timeout has passed removes
    // them, and the next request re-verifies (new entry, same count).
    let live = KeyCacheConfig::from_env();
    assert_eq!(caches.za_keys.sweep(Instant::now() + live.idle), 1);
    assert!(caches.za_keys.is_empty());
    let entry = reverified(caches, &client, fp, entry.expires).await;
    assert_eq!(entry.generation, generation);
    assert_eq!(caches.za_keys.len(), 1);
    assert_holds_no_credential(caches, &[(fp, token)]);

    // A stale generation in the authentication cache forces full
    // verification; the entry is rebuilt with the current generation.
    let mut stale = entry.clone();
    stale.generation += 1;
    caches.http_auth.insert(fp, stale);
    let entry = reverified(caches, &client, fp, entry.expires).await;
    assert_eq!(entry.generation, generation, "entry rebuilt");
    assert_eq!(
        caches.za_keys.get(&fp, Instant::now()).unwrap().generation,
        generation
    );

    // Resident keys of a stale generation are discarded on hit as well.
    caches.za_keys.insert(
        fp,
        Arc::new(SessionKeys::new(id, generation + 1, Secret::random())),
        Instant::now(),
    );
    let entry = reverified(caches, &client, fp, entry.expires).await;
    assert_eq!(entry.generation, generation);
    assert_eq!(
        caches.za_keys.get(&fp, Instant::now()).unwrap().generation,
        generation,
        "keys rebuilt"
    );
    assert_eq!(caches.za_keys.len(), 1);

    // Invalidation drops both caches' entries for the account.
    test.server
        .invalidate_local_caches(&[CacheInvalidation::AccessToken(id)])
        .await;
    assert_nothing_cached(test, id);
    assert!(caches.za_keys.is_empty());
    assert_eq!(caches.http_auth.inner().len(), 0);
}

async fn propfind(client: &DummyWebDavClient) {
    client
        .request("PROPFIND", HOME, "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
}

/// Sends a request expected to miss the caches and returns the new entry,
/// inserted after the one that expired at `previous`.
async fn reverified(
    caches: &Caches,
    client: &DummyWebDavClient,
    fp: [u8; 32],
    previous: Instant,
) -> common::HttpAuthCache {
    propfind(client).await;
    assert_eq!(caches.http_auth.inner().len(), 1);
    let entry = caches.http_auth.peek(&fp).unwrap();
    assert!(entry.expires > previous, "the request re-verified");
    entry
}

/// Spec 5 and 11: every authentication cache key is the keyed fingerprint of
/// a known Authorization token, neither cache's entries carry the password,
/// the token or key bytes, and resident keys are reachable only as
/// `Arc<SessionKeys>`. Failure messages name no secret.
fn assert_holds_no_credential(caches: &Caches, tokens: &[([u8; 32], &str)]) {
    let secrets: Vec<Vec<u8>> = tokens
        .iter()
        .flat_map(|(_, token)| [token.as_bytes().to_vec(), basic_decode(token)])
        .chain([STRONG.as_bytes().to_vec()])
        .collect();

    for (key, entry) in caches.http_auth.inner().iter() {
        assert!(
            tokens
                .iter()
                .any(|(fp, token)| { *fp == key && caches.za_fingerprint(token) == key }),
            "an authentication cache key is not the fingerprint of a known token"
        );
        for secret in &secrets {
            assert!(
                !contains(secret, &key) && !contains(&key, secret),
                "an authentication cache key holds credential bytes"
            );
        }
        let debug = format!("{entry:?}");
        for secret in &secrets {
            assert!(
                !contains(debug.as_bytes(), secret),
                "an authentication cache entry holds a credential"
            );
        }

        if let Some(keys) = caches.za_keys.get(&key, Instant::now()) {
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
