/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Cache snapshots (upgrade Task 6, part 6c). 1.0's swap tier persists two
//! kinds of cache entry, the message cache (`Snapshot::Messages`) and the
//! DAV resource caches (`Snapshot::Resources`), to a local file or to the
//! blob store. Both are built from stored rows only, so a key account's
//! snapshot holds nothing its stored rows do not hold in the clear, and
//! neither kind carries an authority: every restore is revalidated against
//! the change log before it is served.
//!
//! The tier is built once, at boot, so this is a suite of its own: it boots
//! one server per tier (`LocalFile` under the suite's temporary directory,
//! then `BlobStore` on the default blob store) and runs both cases on each.
//!
//! Runtime assumptions:
//! - run alone (`cargo test -p tests cache_swap_tests`), with `STORE` set as
//!   for every suite: the two servers reuse the default listener ports one
//!   after the other;
//! - `BLOB_STORE` unset or naming a store other than the filesystem one:
//!   1.0 refuses a filesystem blob store as the swap tier's backend
//!   (`store/src/build/swap.rs`, `NO_FILESYSTEM`), and the default blob store
//!   then is the data store.

use super::{STRONG, pinning, user_permissions};
use crate::{
    utils::{
        server::{TestServer, TestServerBuilder},
        webdav::DummyWebDavClient,
        za::{HookParked, za_post, za_post_as, za_setup, za_unlock},
    },
    za::mail::{Traced, deliver},
};
use ::registry::{
    schema::{
        prelude::Property,
        structs::{
            BlobStoreSwap, BlobSwapStore, CacheSwap, Expression, Http, LocalFileSwap, MtaStageAuth,
            Rate,
        },
    },
    types::duration::Duration,
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD},
};
use common::{
    GroupwareResources, MessagesCache,
    cache::swap::{SwapKey, SwapPart, blob::BlobSlot},
    za::test_hooks,
};
use email::cache::MessageCacheFetch;
use groupware::cache::GroupwareCache;
use hyper::StatusCode;
use serde_json::json;
use std::{future::Future, path::PathBuf};
use trc::{CacheEvent, EventType};
use types::collection::SyncCollection;

const KEY: &str = "swap-key@example.com";
const PLAIN: &str = "swap-plain@example.com";
const PLAIN_SECRET: &str = "swap plain secret with entropy 7";
const PENDING: &str = "swap-pending@example.com";
const OWNER: &str = "swap-owner@example.com";
const READER: &str = "swap-reader@example.com";
const NEVER: &str = "swap-never@example.com";

/// The event's resource name: visible by design (it is the DAV path), and
/// the positive control of every scan.
const VISIBLE_RES: &str = "swap-visible-res";
const LATER_RES: &str = "swap-later-res";
const SHARED_CAL: &str = "swapshare";

/// Protected text: sealed for a key account, and never held by a cache entry
/// for any account.
const TEXT_CANARIES: &[&str] = &[
    "swap-xwrcal-canary",
    "swap-summary-canary",
    "swap-description-canary",
    "swap-location-canary",
    "swap-comment-canary",
    "swap-xprop-canary",
    "swap-calname-canary",
    "swap-subject-canary",
    "swap-from-canary",
    "swap-body-canary",
];

const MKCAL: &str = "<C:mkcalendar xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:set><D:prop><D:displayname>swap-calname-canary</D:displayname></D:prop></D:set></C:mkcalendar>";

fn event(uid: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//swap//EN\r\n\
         X-WR-CALNAME:swap-xwrcal-canary\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n\
         DTSTAMP:20240101T000000Z\r\nDTSTART:20240102T090000Z\r\n\
         DTEND:20240102T100000Z\r\nSUMMARY:swap-summary-canary\r\n\
         DESCRIPTION:swap-description-canary\r\nLOCATION:swap-location-canary\r\n\
         COMMENT:swap-comment-canary\r\nX-SWAP:swap-xprop-canary\r\n\
         END:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

fn message(to: &str) -> String {
    format!(
        "From: swap-from-canary <sender@example.org>\r\n\
         To: <{to}>\r\n\
         Subject: swap-subject-canary\r\n\
         Message-ID: <{:016x}.swap@example.org>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         \r\n\
         swap-body-canary\r\n",
        store::rand::random::<u64>()
    )
}

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    LocalFile,
    BlobStore,
}

impl Tier {
    fn suite_name(&self) -> &'static str {
        match self {
            Tier::LocalFile => "cache_swap_file",
            Tier::BlobStore => "cache_swap_blob",
        }
    }
}

/// The tier under test and, for `LocalFile`, its directory.
struct Suite {
    tier: Tier,
    root: PathBuf,
}

#[tokio::test(flavor = "multi_thread")]
pub async fn cache_swap_tests() {
    let blob_store = std::env::var("BLOB_STORE").unwrap_or_default();
    assert!(
        !blob_store.eq_ignore_ascii_case("filesystem"),
        "cache_swap_tests: 1.0 refuses a filesystem blob store as the swap tier's \
         backend; run it without BLOB_STORE=FileSystem"
    );

    for tier in [Tier::LocalFile, Tier::BlobStore] {
        let (mut test, suite) = tier_server(tier).await;
        swap_roundtrip_contains_no_protected_canary(&mut test, &suite).await;
        swap_restore_after_revoke_cannot_restore_access(&mut test, &suite).await;

        test.server.inner.cache.swap.stop().await;
        test.shutdown();
        wait_for_listeners_closed().await;
        if test.is_reset() {
            test.temp_dir.delete();
        }
    }
}

/// A server whose swap tier is `tier`, with ruling 20's settings: every
/// account is snapshotted (`min_account_size: 0`), and one change makes a
/// snapshot due (`flush_changes: 1`, no minimum interval).
async fn tier_server(tier: Tier) -> (TestServer, Suite) {
    let name = tier.suite_name();
    // `TempDir::new` puts the suite's directory here (`utils/temp_dir.rs`).
    let root = std::env::temp_dir().join(name).join("swap");
    let swap = match tier {
        Tier::LocalFile => CacheSwap::LocalFile(LocalFileSwap {
            path: root.to_string_lossy().into_owned(),
            flush_changes: 1,
            flush_interval: Duration::from_millis(0),
            min_account_size: 0,
            ..Default::default()
        }),
        Tier::BlobStore => CacheSwap::BlobStore(BlobStoreSwap {
            store: BlobSwapStore::Default,
            flush_changes: 1,
            flush_interval: Duration::from_millis(0),
            min_account_size: 0,
            ..Default::default()
        }),
    };
    let mut test = TestServerBuilder::new(name)
        .await
        .with_default_listeners()
        .await
        .with_cache_swap(swap)
        .await
        .build()
        .await;
    assert!(
        test.server.inner.cache.swap.is_enabled(),
        "{tier:?}: the swap tier did not start"
    );
    assert_eq!(test.server.inner.cache.swap.cadence().min_account_size, 0);
    assert_eq!(test.server.inner.cache.swap.cadence().flush_changes, 1);

    let admin = test.create_admin_account("admin@example.com").await;
    // The vault endpoints are anonymous; the cases send more of them than
    // upstream's default anonymous rate allows.
    admin
        .registry_update_setting(
            Http {
                rate_limit_anonymous: Some(Rate {
                    count: 10000,
                    period: Duration::from_millis(60000),
                }),
                ..Default::default()
            },
            &[Property::RateLimitAnonymous],
        )
        .await;
    // Mail is delivered over the LMTP listener without authenticating
    // (`mail::deliver`), as in the za suite.
    admin
        .registry_update_setting(
            MtaStageAuth {
                require: Expression {
                    else_: "local_port != 25 && local_port != 11200".to_string(),
                    ..Default::default()
                },
                ..Default::default()
            },
            &[Property::Require],
        )
        .await;
    admin.reload_settings().await;
    test.insert_account(admin);

    (test, Suite { tier, root })
}

/// The first server's listeners close on shutdown (`network/listen.rs`); the
/// next server binds the same ports with `SO_REUSEPORT`, so it must not start
/// while the old listeners still accept.
async fn wait_for_listeners_closed() {
    for port in [8899u16, 9991, 9992, 4190, 4110, 11200] {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            assert!(
                std::time::Instant::now() < deadline,
                "the listener on port {port} did not close"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

/// Round trip: no protected text, credential or key reaches a snapshot, raw
/// or decoded, for a key account provisioned before its data, one
/// provisioned while its ordinary-era snapshot is restored, and an ordinary
/// account (upstream's builders read stored rows only); the visible resource
/// name does, as the scans' positive control. Then eviction and fresh loads:
/// a restored snapshot serves the current data, also a stale one written
/// before a later change.
async fn swap_roundtrip_contains_no_protected_canary(test: &mut TestServer, suite: &Suite) {
    println!(
        "Running cache snapshot canary round trip ({:?})...",
        suite.tier
    );
    let admin = test.account("admin@example.com").clone();
    let written_at_start = test.server.inner.cache.swap.snapshots_written();

    let key = admin
        .create_key_user_account(KEY, STRONG, "Swap key", &[], user_permissions())
        .await;
    let plain = admin
        .create_user_account(PLAIN, PLAIN_SECRET, "Swap plain", &[], user_permissions())
        .await;
    let mut pending = admin
        .create_passwordless_user_account(PENDING, STRONG, "Swap pending", &[], user_permissions())
        .await;
    let (key_id, plain_id, pending_id) = (
        key.id().document_id(),
        plain.id().document_id(),
        pending.id().document_id(),
    );
    test.insert_account(key);
    test.insert_account(plain);

    // Provisioning race. The account's snapshot is written while it is
    // ordinary (its default calendar, created by the first build); the
    // provisioning call is parked after its first emptiness check while the
    // snapshot is restored, then completes.
    test.server
        .fetch_groupware_resources(pending_id, pending_id, SyncCollection::Calendar)
        .await
        .unwrap();
    flush(test).await;
    let ordinary_era = load(test, calendar_key(pending_id))
        .await
        .expect("a snapshot of the ordinary account's calendar");
    let admin_request = admin.clone();
    let parked = HookParked::start(pending_id, test_hooks::set_setup_token_pause, async move {
        za_post_as(
            &admin_request,
            "setup-token",
            &json!({ "account": PENDING }),
        )
        .await
    })
    .await;
    test.server.inner.cache.remove_resources(pending_id);
    restored(
        pending_id,
        SyncCollection::Calendar,
        test.server
            .fetch_groupware_resources(pending_id, pending_id, SyncCollection::Calendar),
    )
    .await
    .unwrap();
    let token = parked.finish().await.str(200, "token");
    pending.recovery_key = Some(za_setup(PENDING, &token, STRONG).await);
    test.insert_account(pending);

    // Protected content for each account: a sealed event and a sealed
    // message for the key accounts, plaintext ones for the ordinary account.
    for (name, secret, id) in [
        (KEY, STRONG, key_id),
        (PENDING, STRONG, pending_id),
        (PLAIN, PLAIN_SECRET, plain_id),
    ] {
        DummyWebDavClient::new(id, name, secret, name)
            .request_with_headers(
                "PUT",
                &format!("/dav/cal/{name}/default/{VISIBLE_RES}.ics"),
                [CONTENT_TYPE],
                event(&format!("swap-uid-{id}")),
            )
            .await
            .with_status(StatusCode::CREATED);
        deliver(test, "sender@example.org", name, &message(name)).await;
        build_caches(test, id).await;
    }
    let written = flush(test).await;
    assert!(
        written > written_at_start,
        "{:?}: no snapshot was written ({written})",
        suite.tier
    );

    let mut needles = protected_needles();
    for (name, secret) in [(KEY, STRONG), (PENDING, STRONG), (PLAIN, PLAIN_SECRET)] {
        needles.extend(credential_needles(name, secret));
    }
    for id in [key_id, pending_id] {
        needles.extend(key_needles(&format!("account {id}"), test, id).await);
    }
    let accounts = [key_id, pending_id, plain_id];
    assert_snapshots_clean(test, suite, &accounts, &needles).await;
    for id in accounts {
        assert_visible_snapshot(test, id).await;
    }

    // Eviction, then a fresh load from the snapshot: the sealed event opens
    // with the owner's keys and the message cache holds the message.
    let client = DummyWebDavClient::new(key_id, KEY, STRONG, KEY);
    let path = format!("/dav/cal/{KEY}/default/{VISIBLE_RES}.ics");
    test.server.inner.cache.remove_resources(key_id);
    test.server.inner.cache.remove_messages(key_id);
    let response = restored(
        key_id,
        SyncCollection::Calendar,
        client.request("GET", &path, ""),
    )
    .await
    .with_status(StatusCode::OK);
    assert!(
        response
            .body
            .as_ref()
            .is_ok_and(|body| body.contains("swap-summary-canary")),
        "the restored cache serves the sealed event"
    );
    let messages = restored(
        key_id,
        SyncCollection::Email,
        test.server.get_cached_messages(key_id),
    )
    .await
    .unwrap();
    assert_eq!(messages.emails.len(), 1);

    // Stale snapshots: taken before a later event and message, and stored
    // again after them (a writer whose snapshot predates the change). The
    // fresh load revalidates against the change log and serves both.
    for (id, name, stale_calendar) in [
        (key_id, KEY, None),
        (pending_id, PENDING, Some(ordinary_era)),
    ] {
        let stale_calendar = match stale_calendar {
            Some(bytes) => bytes,
            None => load(test, calendar_key(id)).await.unwrap(),
        };
        let stale_messages = load(test, SwapKey::messages(id)).await.unwrap();
        DummyWebDavClient::new(id, name, STRONG, name)
            .request_with_headers(
                "PUT",
                &format!("/dav/cal/{name}/default/{LATER_RES}.ics"),
                [CONTENT_TYPE],
                event(&format!("swap-later-{id}")),
            )
            .await
            .with_status(StatusCode::CREATED);
        deliver(test, "sender@example.org", name, &message(name)).await;
        build_caches(test, id).await;
        flush(test).await;

        let swap = &test.server.inner.cache.swap;
        swap.store(calendar_key(id), &stale_calendar).await.unwrap();
        swap.store(SwapKey::messages(id), &stale_messages)
            .await
            .unwrap();
        test.server.inner.cache.remove_resources(id);
        test.server.inner.cache.remove_messages(id);
        let resources = restored(
            id,
            SyncCollection::Calendar,
            test.server
                .fetch_groupware_resources(id, id, SyncCollection::Calendar),
        )
        .await
        .unwrap();
        for res in [VISIBLE_RES, LATER_RES] {
            assert!(
                resources.by_path(&format!("default/{res}.ics")).is_some(),
                "account {id}: the fresh load lost {res}"
            );
        }
        let messages = restored(
            id,
            SyncCollection::Email,
            test.server.get_cached_messages(id),
        )
        .await
        .unwrap();
        assert_eq!(messages.emails.len(), 2, "account {id}: the fresh load");
    }
    flush(test).await;
    assert_snapshots_clean(test, suite, &accounts, &needles).await;
}

/// Revocation race: a snapshot of the owner's calendar taken while the
/// reader holds a grant cannot give the reader access back once the grant
/// is revoked, whether the snapshot is restored while the revoke is parked
/// before its commit or written before the revoke and restored after it.
async fn swap_restore_after_revoke_cannot_restore_access(test: &mut TestServer, suite: &Suite) {
    println!(
        "Running cache snapshot revocation tests ({:?})...",
        suite.tier
    );
    let admin = test.account("admin@example.com").clone();
    let mut ids = Vec::new();
    for name in [OWNER, READER, NEVER] {
        let account = admin
            .create_key_user_account(name, STRONG, name, &[], user_permissions())
            .await;
        ids.push(account.id().document_id());
        test.insert_account(account);
    }
    let (owner_id, reader_id, never_id) = (ids[0], ids[1], ids[2]);
    let owner = DummyWebDavClient::new(owner_id, OWNER, STRONG, OWNER);
    let reader = DummyWebDavClient::new(reader_id, READER, STRONG, READER);
    let never = DummyWebDavClient::new(never_id, NEVER, STRONG, NEVER);
    let calendar = format!("/dav/cal/swap-owner%40example.com/{SHARED_CAL}/");
    let path = format!("{calendar}shared.ics");
    owner
        .request("MKCALENDAR", &calendar, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    owner
        .request_with_headers("PUT", &path, [CONTENT_TYPE], event("swap-shared-uid"))
        .await
        .with_status(StatusCode::CREATED);

    // The oracle: what an account that was never granted gets.
    let refused = probe(&never, &path).await;
    assert!(!refused.1, "an ungranted account read the event");
    assert_ne!(refused.0, StatusCode::OK);

    let served = test
        .server
        .za_served_public_key(reader_id)
        .await
        .unwrap()
        .unwrap();
    pinning::pin(OWNER, READER, &vault::keys::display_fingerprint(&served))
        .await
        .expect(200);
    za_post(
        "shares/grant",
        &json!({"username": OWNER, "password": STRONG,
            "calendar": SHARED_CAL, "address": READER, "level": "view"}),
    )
    .await
    .expect(200);
    assert_eq!(probe(&reader, &path).await, (StatusCode::OK, true));

    // The snapshot holds the grant.
    test.server
        .fetch_groupware_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    flush(test).await;
    let granted = load(test, calendar_key(owner_id))
        .await
        .expect("a snapshot of the owner's calendars");
    assert!(
        grants(&granted, reader_id),
        "the snapshot taken while shared holds the reader's grant"
    );

    // The revoke parked before its commit; meanwhile the owner's calendars
    // are evicted and restored from the snapshot, which still answers for
    // the grant. After the commit the restored cache is stale and the
    // reader is refused.
    let parked = HookParked::start(owner_id, test_hooks::set_share_commit_pause, async move {
        za_post(
            "shares/revoke",
            &json!({"username": OWNER, "password": STRONG,
                "calendar": SHARED_CAL, "grant": reader_id}),
        )
        .await
    })
    .await;
    test.server.inner.cache.remove_resources(owner_id);
    let resources = restored(
        owner_id,
        SyncCollection::Calendar,
        test.server
            .fetch_groupware_resources(owner_id, owner_id, SyncCollection::Calendar),
    )
    .await
    .unwrap();
    assert!(
        has_grant(&resources, reader_id),
        "before the revoke commits, the restored cache holds the grant"
    );
    assert_eq!(
        probe(&reader, &path).await,
        (StatusCode::OK, true),
        "before the revoke commits, the grant stands"
    );
    assert_eq!(parked.finish().await.expect(200)["changed"], true);
    assert_eq!(probe(&reader, &path).await, refused);
    // Revalidate the owner's cache in process, so the writer has the
    // post-revoke state to write whether or not the refused request
    // reached it.
    test.server
        .fetch_groupware_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    flush(test).await;
    let current = load(test, calendar_key(owner_id)).await.unwrap();
    assert!(
        !grants(&current, reader_id),
        "the snapshot written after the revoke holds no grant"
    );

    // A snapshot taken before the revoke and stored after it (a late
    // writer) is restored, revalidated and refused, on every reload.
    for round in 0..2 {
        test.server
            .inner
            .cache
            .swap
            .store(calendar_key(owner_id), &granted)
            .await
            .unwrap();
        test.server.inner.cache.remove_resources(owner_id);
        test.server.inner.cache.remove_resources(reader_id);
        // Every load that restores a snapshot revalidates it before it
        // returns (`fetch_groupware_resources_at`), so the grant is gone
        // from what any request is served.
        let resources = restored(
            owner_id,
            SyncCollection::Calendar,
            test.server
                .fetch_groupware_resources(owner_id, owner_id, SyncCollection::Calendar),
        )
        .await
        .unwrap();
        assert!(
            !has_grant(&resources, reader_id),
            "round {round}: a stale snapshot restored the revoked grant"
        );
        assert_eq!(
            probe(&reader, &path).await,
            refused,
            "round {round}: the revoked reader after a stale restore"
        );
        // The same stale snapshot, met only by the reader's request: refused
        // whether or not the request reaches the owner's cache.
        test.server
            .inner
            .cache
            .swap
            .store(calendar_key(owner_id), &granted)
            .await
            .unwrap();
        test.server.inner.cache.remove_resources(owner_id);
        assert_eq!(probe(&reader, &path).await, refused);
        assert_eq!(probe(&owner, &path).await, (StatusCode::OK, true));
    }

    let mut needles = protected_needles();
    for name in [OWNER, READER, NEVER] {
        needles.extend(credential_needles(name, STRONG));
    }
    for id in [owner_id, reader_id] {
        needles.extend(key_needles(&format!("account {id}"), test, id).await);
    }
    // The owner's ring and the calendar's display name live in its
    // preferences, which no cache entry holds.
    needles.push(Needle::text("ring marker", vault::ZA_MARKER));
    assert_snapshots_clean(test, suite, &[owner_id, reader_id, never_id], &needles).await;
}

/// GET as `client`: the status, and whether the body holds the summary.
async fn probe(client: &DummyWebDavClient, path: &str) -> (StatusCode, bool) {
    let response = client.request("GET", path, "").await;
    let leaked = response
        .body
        .as_ref()
        .is_ok_and(|body| body.contains("swap-summary-canary"));
    (response.status, leaked)
}

fn calendar_key(account_id: u32) -> SwapKey {
    SwapKey::new(account_id, SyncCollection::Calendar, SwapPart::Resources)
}

/// Builds or revalidates the account's message and calendar caches, which
/// publishes them to the tier.
async fn build_caches(test: &TestServer, account_id: u32) {
    test.server.get_cached_messages(account_id).await.unwrap();
    test.server
        .fetch_groupware_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap();
}

/// `SwapTier::flush` (ruling 20): every pending snapshot is written before
/// it returns. Returns `snapshots_written()`.
async fn flush(test: &TestServer) -> u64 {
    let swap = &test.server.inner.cache.swap;
    swap.flush().await;
    swap.snapshots_written()
}

/// The snapshot as the tier's own reader returns it.
async fn load(test: &TestServer, key: SwapKey) -> Option<Vec<u8>> {
    test.server.inner.cache.swap.load(key).await.unwrap()
}

/// Runs `load` and asserts that it restored the account's `collection` from
/// its snapshot (`CacheEvent::SwapHit`), so the case does not pass on a
/// rebuild from the store.
async fn restored<T>(
    account_id: u32,
    collection: SyncCollection,
    load: impl Future<Output = T>,
) -> T {
    let traced = Traced::start(&[EventType::Cache(CacheEvent::SwapHit)]);
    let out = load.await;
    let events = traced.finish().await;
    assert!(
        events.iter().any(|event| {
            event.typ == EventType::Cache(CacheEvent::SwapHit)
                && event.account_id == Some(account_id as u64)
                && event.text.contains(collection.as_str())
        }),
        "no {collection:?} snapshot of account {account_id} was restored: {events:?}"
    );
    out
}

/// Whether a resource snapshot grants `account_id` on any container.
fn grants(snapshot: &[u8], account_id: u32) -> bool {
    has_grant(
        &GroupwareResources::from_snapshot(snapshot).expect("a resource snapshot"),
        account_id,
    )
}

fn has_grant(resources: &GroupwareResources, account_id: u32) -> bool {
    resources.resources.iter().any(|resource| {
        resource
            .acls()
            .iter()
            .any(|grant| grant.account_id == account_id)
    })
}

/// A value that must not appear in a snapshot, in each form it could take.
struct Needle {
    label: String,
    forms: Vec<Vec<u8>>,
}

impl Needle {
    fn text(label: &str, text: &str) -> Self {
        Needle {
            label: label.to_string(),
            forms: vec![text.as_bytes().to_vec()],
        }
    }

    fn secret(label: String, bytes: &[u8]) -> Self {
        let hex = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Needle {
            label,
            forms: vec![
                bytes.to_vec(),
                STANDARD.encode(bytes).into_bytes(),
                STANDARD_NO_PAD.encode(bytes).into_bytes(),
                URL_SAFE_NO_PAD.encode(bytes).into_bytes(),
                hex.into_bytes(),
            ],
        }
    }

    fn found_in(&self, bytes: &[u8]) -> bool {
        self.forms.iter().any(|form| {
            bytes
                .windows(form.len())
                .any(|window| window == form.as_slice())
        })
    }
}

fn protected_needles() -> Vec<Needle> {
    TEXT_CANARIES
        .iter()
        .map(|canary| Needle::text(canary, canary))
        .collect()
}

/// The password, and the Basic credential a client sends with it.
fn credential_needles(name: &str, secret: &str) -> Vec<Needle> {
    vec![
        Needle::text(&format!("{name}'s password"), secret),
        Needle::text(
            &format!("{name}'s Basic credential"),
            &STANDARD.encode(format!("{name}:{secret}")),
        ),
    ]
}

/// The account's session keys, unlocked as a login does: the master key and
/// the event and mail wrapping keys derived from it.
async fn key_needles(label: &str, test: &TestServer, account_id: u32) -> Vec<Needle> {
    let keys = za_unlock(test, account_id, STRONG).await;
    vec![
        Needle::secret(format!("{label} mk"), keys.mk().as_bytes()),
        Needle::secret(format!("{label} ewk"), keys.ewk().as_bytes()),
        Needle::secret(format!("{label} mwk"), keys.mwk().as_bytes()),
    ]
}

/// The stored bytes of every snapshot, read without the server: the files
/// under the tier's directory, or each part's pointer and two slots in the
/// blob store.
async fn raw_artifacts(
    test: &TestServer,
    suite: &Suite,
    accounts: &[u32],
) -> Vec<(String, Vec<u8>)> {
    let mut artifacts = Vec::new();
    match suite.tier {
        Tier::LocalFile => {
            let mut directories = vec![suite.root.clone()];
            while let Some(directory) = directories.pop() {
                for entry in std::fs::read_dir(&directory).unwrap() {
                    let path = entry.unwrap().path();
                    if path.is_dir() {
                        directories.push(path);
                    } else {
                        artifacts.push((path.display().to_string(), std::fs::read(&path).unwrap()));
                    }
                }
            }
        }
        Tier::BlobStore => {
            for &account_id in accounts {
                for key in SwapKey::all_parts(account_id) {
                    for (what, hash) in [
                        ("pointer", key.blob_hash()),
                        ("slot 0", key.slot_hash(BlobSlot::First)),
                        ("slot 1", key.slot_hash(BlobSlot::Second)),
                    ] {
                        if let Some(bytes) = test
                            .server
                            .blob_store()
                            .get_blob(hash.as_slice(), 0..usize::MAX)
                            .await
                            .unwrap()
                        {
                            artifacts.push((format!("{key:?} {what}"), bytes));
                        }
                    }
                }
            }
        }
    }
    artifacts
}

/// A snapshot decoded by the shared codecs alone, from its bytes: its
/// `Debug` form, which names every field, plus the string arenas a resource
/// snapshot keeps its names in.
fn decoded(bytes: &[u8]) -> Option<(SwapPart, Vec<u8>)> {
    if let Some(messages) = MessagesCache::from_snapshot(bytes) {
        return Some((SwapPart::Messages, format!("{messages:?}").into_bytes()));
    }
    let resources = GroupwareResources::from_snapshot(bytes)?;
    let mut text = format!("{resources:?}").into_bytes();
    for chunk in &resources.resources.chunks {
        text.push(0);
        text.extend_from_slice(&chunk.bytes);
    }
    for chunk in &resources.paths.chunks {
        text.push(0);
        text.extend_from_slice(&chunk.bytes);
    }
    Some((SwapPart::Resources, text))
}

/// No needle in any stored snapshot, raw or decoded; and both kinds of
/// snapshot were found and decoded.
async fn assert_snapshots_clean(
    test: &TestServer,
    suite: &Suite,
    accounts: &[u32],
    needles: &[Needle],
) {
    let artifacts = raw_artifacts(test, suite, accounts).await;
    let mut violations = Vec::new();
    let (mut messages, mut resources) = (0, 0);
    for (what, raw) in &artifacts {
        for needle in needles {
            if needle.found_in(raw) {
                violations.push(format!("{what} (raw): {}", needle.label));
            }
        }
        if let Some((part, text)) = decoded(raw) {
            match part {
                SwapPart::Messages => messages += 1,
                SwapPart::Resources => resources += 1,
            }
            for needle in needles {
                if needle.found_in(&text) {
                    violations.push(format!("{what} (decoded): {}", needle.label));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "{:?}: protected values in cache snapshots: {violations:#?}",
        suite.tier
    );
    assert!(
        messages > 0 && resources > 0,
        "{:?}: {} stored snapshots, {messages} message and {resources} resource frames decoded",
        suite.tier,
        artifacts.len()
    );
}

/// The positive control: the account's calendar snapshot holds the visible
/// resource name raw and decoded, and its message snapshot decodes to the
/// one delivered message.
async fn assert_visible_snapshot(test: &TestServer, account_id: u32) {
    let control = Needle::text("resource name", VISIBLE_RES);
    let raw = load(test, calendar_key(account_id))
        .await
        .unwrap_or_else(|| panic!("account {account_id}: no calendar snapshot"));
    assert!(control.found_in(&raw), "account {account_id}: raw scan");
    let (part, text) = decoded(&raw).expect("the calendar snapshot decodes");
    assert_eq!(part, SwapPart::Resources);
    assert!(
        control.found_in(&text),
        "account {account_id}: decoded scan"
    );
    assert!(
        GroupwareResources::from_snapshot(&raw)
            .unwrap()
            .by_path(&format!("default/{VISIBLE_RES}.ics"))
            .is_some()
    );

    let raw = load(test, SwapKey::messages(account_id))
        .await
        .unwrap_or_else(|| panic!("account {account_id}: no message snapshot"));
    assert_eq!(
        MessagesCache::from_snapshot(&raw)
            .expect("the message snapshot decodes")
            .len(),
        1,
        "account {account_id}: the message snapshot"
    );
}
