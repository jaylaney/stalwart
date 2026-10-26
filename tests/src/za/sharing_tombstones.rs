/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Foreign sync must authorize retained tombstones by source calendar identity.
use super::{STRONG, user_permissions};
use crate::utils::{
    server::TestServer,
    webdav::{DavResponse, DummyWebDavClient},
    za::{HookParked, za_post},
};
use common::za::test_hooks;
use dav_proto::Depth;
use groupware::{cache::GroupwareCache, calendar::Calendar};
use hyper::StatusCode;
use serde_json::json;
use store::{
    IterateParams, Key, LogKey, SUBSPACE_LOGS, ValueKey,
    query::log::Query,
    write::{AlignedBytes, AnyClass, Archive, BatchBuilder, ValueClass},
};
use types::collection::{Collection, SyncCollection, VanishedCollection};

const OWNER: &str = "sharing-tombstone-owner@example.com";
const READER: &str = "sharing-tombstone-reader@example.com";
const HOME: &str = "/dav/cal/sharing-tombstone-owner%40example.com/";
const MKCAL: &str = "<C:mkcalendar xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:set><D:prop><D:displayname>Identity fixture</D:displayname></D:prop></D:set></C:mkcalendar>";

async fn sync(client: &DummyWebDavClient, path: &str, token: &str) -> DavResponse {
    client.request("REPORT",path,format!("<D:sync-collection xmlns:D=\"DAV:\"><D:prop><D:getetag/></D:prop><D:sync-token>{token}</D:sync-token><D:sync-level>infinite</D:sync-level></D:sync-collection>")).await
}
async fn initial(client: &DummyWebDavClient) -> String {
    sync(client, HOME, "")
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .sync_token()
        .to_string()
}
fn token_id(token: &str) -> u64 {
    let value = token.strip_prefix("urn:stalwart:davsync:").unwrap();
    u64::from_str_radix(value.split(':').next().unwrap(), 16).unwrap()
}
fn refused(response: DavResponse) {
    assert_eq!(
        response.status,
        StatusCode::FORBIDDEN,
        "{:?}",
        response.body
    );
    assert!(response.expect_body().contains("valid-sync-token"));
    assert!(!response.expect_body().contains("404 Not Found"));
}
async fn create(client: &DummyWebDavClient, slug: &str) {
    client
        .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
        .await
        .with_status(StatusCode::CREATED);
}
async fn put(client: &DummyWebDavClient, slug: &str, name: &str) {
    client
        .request_with_headers(
            "PUT",
            &format!("{HOME}{slug}/{name}"),
            [("content-type", "text/calendar")],
            super::sharing::sharing_event(&format!("{slug}-{name}")),
        )
        .await
        .with_status(StatusCode::CREATED);
}
async fn delete(client: &DummyWebDavClient, slug: &str, name: &str) {
    client
        .request("DELETE", &format!("{HOME}{slug}/{name}"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}
async fn grant(slug: &str) {
    za_post("shares/grant",&json!({"username":OWNER,"password":STRONG,"calendar":slug,"address":READER,"level":"view"})).await.expect(200);
}
async fn id(test: &TestServer, owner: u32, slug: &str) -> u32 {
    test.server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path(slug)
        .unwrap()
        .document_id()
}
async fn rename_fixture(test: &TestServer, owner: u32, calendar_id: u32, name: &str) {
    let stored: Archive<AlignedBytes> = test
        .server
        .store()
        .get_value(ValueKey::archive(owner, Collection::Calendar, calendar_id))
        .await
        .unwrap()
        .unwrap();
    let mut calendar = stored.deserialize::<Calendar>().unwrap();
    calendar.name = name.to_string();
    let mut batch = BatchBuilder::new();
    calendar
        .update(
            test.server
                .account_info(owner)
                .await
                .unwrap()
                .account_tenant_ids(),
            stored.to_unarchived::<Calendar>().unwrap(),
            owner,
            calendar_id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    // A full cache build establishes both deterministic creation-order winners.
    test.server.inner.cache.events.remove(&owner);
}
async fn raw(
    test: &TestServer,
    owner: u32,
    collection: VanishedCollection,
    after: u64,
) -> Vec<(u64, String)> {
    test.server
        .store()
        .vanished_with_change_id(owner, collection.into(), Query::Since(after))
        .await
        .unwrap()
}
/// Model old/mixed writers and corrupt coverage at the real store boundary.
/// A sync mutation is essential: log-only batches otherwise use change id zero.
async fn seed(
    test: &TestServer,
    owner: u32,
    calendar: u32,
    paths: &[String],
    identities: &[String],
) -> u64 {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(owner)
        .with_document(calendar)
        .log_container_update(SyncCollection::Calendar);
    for path in paths {
        batch.log_vanished_item(VanishedCollection::Calendar, path.clone());
    }
    for identity in identities {
        batch.log_vanished_item(VanishedCollection::CalendarIdentity, identity.clone());
    }
    test.server
        .commit_batch(batch)
        .await
        .unwrap()
        .last_change_id(owner)
        .unwrap()
}
fn identity(id: u32, path: &str) -> String {
    format!("{id:08x}:{path}")
}
async fn bytes(test: &TestServer, owner: u32, collection: u8, change_id: u64) -> Option<Vec<u8>> {
    let key = LogKey {
        account_id: owner,
        collection,
        change_id,
    };
    let mut result = None;
    test.server
        .store()
        .iterate(IterateParams::new(key, key), |_, value| {
            result = Some(value.to_vec());
            Ok(false)
        })
        .await
        .unwrap();
    result
}

pub async fn test(test: &mut TestServer) {
    println!("Running identity-bound calendar tombstone regressions...");
    let mut ids = Vec::new();
    for user in [OWNER, READER] {
        let account = test
            .account("admin@example.com")
            .create_key_user_account(user, STRONG, "Tombstone fixture", &[], user_permissions())
            .await;
        ids.push(account.id().document_id());
        test.insert_account(account);
    }
    let (owner, reader) = (ids[0], ids[1]);
    let alice = DummyWebDavClient::new(owner, OWNER, STRONG, OWNER);
    let bob = DummyWebDavClient::new(reader, READER, STRONG, READER);
    let public = test
        .server
        .za_served_public_key(reader)
        .await
        .unwrap()
        .unwrap();
    super::pinning::pin(OWNER, READER, &vault::keys::display_fingerprint(&public))
        .await
        .expect(200);
    create(&alice, "granted").await;
    grant("granted").await;
    put(&alice, "granted", "anchor.ics").await;
    let granted = id(test, owner, "granted").await;

    collisions(test, owner, &alice, &bob).await;
    controls(test, owner, &alice, &bob, granted).await;
    coverage(test, owner, &alice, &bob, granted).await;
    purge_publication_gap(test, owner, &alice, &bob).await;
    scan_races(test, owner, reader, granted).await;
    retention(test).await;
    ordinary_owner(test).await;
    println!("Identity tombstone collisions, coverage, pagination, races and retention passed");
}

async fn collisions(
    test: &TestServer,
    owner: u32,
    alice: &DummyWebDavClient,
    bob: &DummyWebDavClient,
) {
    // Ascending document scans choose the first created container at a shared raw
    // path. Test both winners and a third current private calendar independently.
    for private_first in [false, true] {
        let slug = if private_first {
            "private-wins"
        } else {
            "grant-wins"
        };
        let private = format!("{slug}-private");
        if private_first {
            create(alice, &private).await;
        }
        create(alice, slug).await;
        if !private_first {
            create(alice, &private).await;
        }
        grant(slug).await;
        let granted = id(test, owner, slug).await;
        let private_id = id(test, owner, &private).await;
        put(alice, slug, "public-anchor.ics").await;
        put(alice, &private, "secret.ics").await;
        let third = format!("{slug}-third");
        create(alice, &third).await;
        let third_id = id(test, owner, &third).await;
        put(alice, &third, "third-secret.ics").await;
        rename_fixture(test, owner, private_id, slug).await;
        rename_fixture(test, owner, third_id, slug).await;
        assert_eq!(
            id(test, owner, slug).await,
            if private_first { private_id } else { granted },
            "fixture must exercise both cache collision winners"
        );
        let token = initial(bob).await;
        delete(alice, slug, "secret.ics").await;
        delete(alice, slug, "third-secret.ics").await;
        let response = sync(bob, HOME, &token)
            .await
            .with_status(StatusCode::MULTI_STATUS);
        assert!(
            !response
                .hrefs()
                .iter()
                .any(|href| href.contains("secret.ics")),
            "private current collider leaked: {:?}",
            response.hrefs()
        );
        // The private source remains ineligible after renaming/removing it.
        rename_fixture(test, owner, private_id, &private).await;
        rename_fixture(test, owner, third_id, &third).await;
        alice
            .request("DELETE", &format!("{HOME}{third}/"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
        let response = sync(bob, &format!("{HOME}{slug}/"), &token)
            .await
            .with_status(StatusCode::MULTI_STATUS);
        assert!(
            !response
                .hrefs()
                .iter()
                .any(|href| href.contains("secret.ics"))
        );

        // DELETE retries with its original path after the private parent renamed.
        put(alice, &private, "stale-private.ics").await;
        rename_fixture(test, owner, private_id, slug).await;
        let token = initial(bob).await;
        let path = format!("{HOME}{slug}/stale-private.ics");
        let request_path = path.clone();
        let parked = HookParked::start(owner, test_hooks::set_delete_commit_pause, async move {
            DummyWebDavClient::new(owner, OWNER, STRONG, OWNER)
                .request("DELETE", &request_path, "")
                .await
                .status
        })
        .await;
        rename_fixture(test, owner, private_id, &private).await;
        assert_eq!(parked.finish().await, StatusCode::NO_CONTENT);
        let paths = raw(test, owner, VanishedCollection::Calendar, token_id(&token)).await;
        let typed = raw(
            test,
            owner,
            VanishedCollection::CalendarIdentity,
            token_id(&token),
        )
        .await;
        assert!(paths.iter().any(|(_, logged)| logged == &path));
        assert!(
            typed
                .iter()
                .any(|(_, logged)| logged == &identity(private_id, &path)),
            "stale retry lost private source identity"
        );
        let response = sync(bob, &format!("{HOME}{slug}/"), &token)
            .await
            .with_status(StatusCode::MULTI_STATUS);
        assert!(!response.hrefs().contains(&path.as_str()));
    }
}

async fn controls(
    test: &TestServer,
    owner: u32,
    alice: &DummyWebDavClient,
    bob: &DummyWebDavClient,
    granted: u32,
) {
    create(alice, "Family%20Trips").await;
    grant("Family%20Trips").await;
    put(alice, "Family%20Trips", "anchor.ics").await;
    let token = initial(bob).await;
    put(alice, "Family%20Trips", "escaped.ics").await;
    delete(alice, "Family%20Trips", "escaped.ics").await;
    let response = sync(bob, &format!("{HOME}Family%20Trips/"), &token)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        response
            .hrefs()
            .contains(&format!("{HOME}Family%20Trips/escaped.ics").as_str())
    );
    let token = initial(bob).await;
    for name in ["page-one.ics", "page-two.ics"] {
        put(alice, "granted", name).await;
        delete(alice, "granted", name).await;
    }
    let mut cursor = token;
    let mut seen = Vec::new();
    for _ in 0..8 {
        let page = bob
            .sync_collection(
                &format!("{HOME}granted/"),
                &cursor,
                Depth::Infinity,
                Some(1),
                ["D:getetag"],
            )
            .await;
        seen.extend(page.hrefs().iter().map(|href| href.to_string()));
        let next = page.sync_token().to_string();
        if !page.expect_body().contains("507 Insufficient Storage") {
            break;
        }
        assert_ne!(cursor, next);
        cursor = next;
    }
    for name in ["page-one.ics", "page-two.ics"] {
        assert_eq!(
            seen.iter()
                .filter(|href| **href == format!("{HOME}granted/{name}"))
                .count(),
            1,
            "pagination must retain each authorized tombstone once"
        );
    }
    let full = sync(bob, HOME, "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // A calendar's missing getetag may legitimately have a 404 propstat.
    // Initial sync must omit the deleted resource hrefs, not every 404 property.
    for deleted in [
        format!("{HOME}granted/page-one.ics"),
        format!("{HOME}granted/page-two.ics"),
        format!("{HOME}Family%20Trips/escaped.ics"),
    ] {
        assert!(!full.hrefs().contains(&deleted.as_str()));
    }
    // Identical hrefs are distinct occurrences; only one belongs to a grant.
    let token = initial(bob).await;
    let path = format!("{HOME}granted/same.ics");
    let commit = seed(
        test,
        owner,
        granted,
        &[path.clone(), path.clone()],
        &[identity(granted, &path), identity(u32::MAX - 2, &path)],
    )
    .await;
    let response = sync(bob, &format!("{HOME}granted/"), &token)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_eq!(
        response
            .hrefs()
            .iter()
            .filter(|href| **href == path)
            .count(),
        1
    );
    // Legacy raw records keep exact bytes, and self-key sync keeps ordinary output.
    let mut expected = path.as_bytes().to_vec();
    expected.push(0);
    expected.extend_from_slice(path.as_bytes());
    expected.push(0);
    assert_eq!(bytes(test, owner, 252, commit).await, Some(expected));
    let own = sync(alice, &format!("{HOME}granted/"), &token)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_eq!(own.hrefs().iter().filter(|href| **href == path).count(), 2);
}

async fn coverage(
    test: &TestServer,
    owner: u32,
    alice: &DummyWebDavClient,
    bob: &DummyWebDavClient,
    granted: u32,
) {
    let scope = format!("{HOME}granted/");
    let path = format!("{scope}legacy.ics");
    let token = initial(bob).await;
    let committed = seed(test, owner, granted, std::slice::from_ref(&path), &[]).await;
    assert!(committed > token_id(&token));
    assert!(
        raw(test, owner, VanishedCollection::Calendar, token_id(&token))
            .await
            .contains(&(committed, path.clone()))
    );
    refused(sync(bob, &scope, &token).await);
    // Ordinary owner behavior retains legacy deletion output.
    assert!(
        sync(alice, &scope, &token)
            .await
            .with_status(StatusCode::MULTI_STATUS)
            .hrefs()
            .contains(&path.as_str())
    );
    // Each new initial cursor consumes the preceding valid sync mutation; a raw
    // tail above that cursor is separately documented as potentially persistent.
    for case in ["cross-commit", "duplicate", "extra", "malformed"] {
        let token = initial(bob).await;
        let path = format!("{scope}{case}.ics");
        match case {
            "cross-commit" => {
                seed(test, owner, granted, std::slice::from_ref(&path), &[]).await;
                seed(test, owner, granted, &[], &[identity(granted, &path)]).await;
            }
            "duplicate" => {
                seed(
                    test,
                    owner,
                    granted,
                    &[path.clone(), path.clone()],
                    &[identity(granted, &path)],
                )
                .await;
            }
            "extra" => {
                seed(
                    test,
                    owner,
                    granted,
                    std::slice::from_ref(&path),
                    &[identity(granted, &path), identity(granted, &path)],
                )
                .await;
            }
            _ => {
                seed(
                    test,
                    owner,
                    granted,
                    std::slice::from_ref(&path),
                    &[format!("not-an-id:{path}")],
                )
                .await;
            }
        }
        refused(sync(bob, &scope, &token).await);
    }
    // Irrelevant legacy and corrupt-private history must not widen direct scope.
    let token = initial(bob).await;
    let public = format!("{scope}healthy.ics");
    let unrelated = format!("{HOME}unrelated/untyped.ics");
    seed(
        test,
        owner,
        granted,
        &[public.clone(), unrelated.clone()],
        &[identity(granted, &public), format!("invalid:{unrelated}")],
    )
    .await;
    let response = sync(bob, &scope, &token)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(response.hrefs().contains(&public.as_str()));
    assert!(!response.hrefs().contains(&unrelated.as_str()));
    // An authorized source id with another current prefix still cannot emit.
    let token = initial(bob).await;
    let other = format!("{HOME}Family%20Trips/wrong-source.ics");
    seed(
        test,
        owner,
        granted,
        std::slice::from_ref(&other),
        &[identity(granted, &other)],
    )
    .await;
    let response = sync(bob, HOME, &token)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(!response.hrefs().contains(&other.as_str()));
}

/// A matched-empty pair is unsafe if pruning has removed a real deletion but
/// has not durably fenced old tokens. This parks retention, not the reader.
async fn purge_publication_gap(
    test: &TestServer,
    owner: u32,
    alice: &DummyWebDavClient,
    bob: &DummyWebDavClient,
) {
    let slug = "purge-publication-gap";
    let scope = format!("{HOME}{slug}/");
    let path = format!("{scope}remembered.ics");
    create(alice, slug).await;
    grant(slug).await;
    put(alice, slug, "remembered.ics").await;
    let calendar = id(test, owner, slug).await;
    let snapshot = sync(bob, &scope, "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        snapshot.hrefs().contains(&path.as_str()),
        "client must have the deleted event in its initial snapshot"
    );
    let token = snapshot.sync_token().to_owned();
    let position = token_id(&token);
    // This token is admitted by the real grant boundary before any history is lost.
    sync(bob, &scope, &token)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    delete(alice, slug, "remembered.ics").await;
    let deleted = test
        .server
        .store()
        .get_last_change_id(owner, SyncCollection::Calendar.into())
        .await
        .unwrap()
        .unwrap();
    for collection in [
        VanishedCollection::Calendar,
        VanishedCollection::CalendarIdentity,
    ] {
        let expected = if collection == VanishedCollection::Calendar {
            path.clone()
        } else {
            identity(calendar, &path)
        };
        assert!(
            raw(test, owner, collection, position)
                .await
                .contains(&(deleted, expected)),
            "real deletion must be retained in both streams before pruning"
        );
    }
    let cutoff = seed(test, owner, calendar, &[], &[]).await;
    let resource_cursor = seed(test, owner, calendar, &[], &[]).await;
    assert!(
        position < deleted && deleted < cutoff && cutoff < resource_cursor,
        "purge fixture requires T < D < C < R"
    );
    let before_purge = sync(bob, &scope, &token)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        before_purge.hrefs().contains(&path.as_str()),
        "token must remain healthy and return the deletion before pruning"
    );
    assert_eq!(token_id(before_purge.sync_token()), resource_cursor);
    let server = test.server.clone();
    let parked = HookParked::start(
        owner,
        test_hooks::set_calendar_history_purge_pause,
        async move {
            // Two newest sync keys are R and C, so max_entries=1 picks cutoff C.
            server.delete_changes(owner, Some(1), None).await
        },
    )
    .await;
    for collection in [250u8, 252] {
        assert!(
            bytes(test, owner, collection, deleted).await.is_none(),
            "purger must have removed deletion D before reader starts either scan"
        );
    }
    assert!(
        bytes(test, owner, 2, deleted).await.is_none(),
        "old sync record D must also be purged"
    );
    let answer = sync(bob, &scope, &token).await;
    // Always release the real purge before asserting the response to avoid
    // leaving a background task parked when the expected RED assertion fires.
    parked.finish().await.unwrap();
    test_hooks::set_calendar_history_purge_pause(None);
    assert_eq!(
        answer.status,
        StatusCode::FORBIDDEN,
        "purged matched-empty history acknowledged a missing deletion: {:?}",
        answer.body
    );
    assert!(answer.expect_body().contains("valid-sync-token"));
}

async fn scan_races(test: &TestServer, owner: u32, reader: u32, granted: u32) {
    let client = move || DummyWebDavClient::new(reader, READER, STRONG, READER);
    let scope = format!("{HOME}granted/");
    let token = initial(&client()).await;
    let request_scope = scope.clone();
    let parked = HookParked::start(owner, test_hooks::set_calendar_sync_log_pause, async move {
        sync(
            &DummyWebDavClient::new(reader, READER, STRONG, READER),
            &request_scope,
            &token,
        )
        .await
    })
    .await;
    let path = format!("{scope}between-reads.ics");
    seed(
        test,
        owner,
        granted,
        std::slice::from_ref(&path),
        &[identity(granted, &path)],
    )
    .await;
    refused(parked.finish().await);
    let token = initial(&client()).await;
    let path = format!("{scope}purged.ics");
    seed(
        test,
        owner,
        granted,
        std::slice::from_ref(&path),
        &[identity(granted, &path)],
    )
    .await;
    let request_scope = scope.clone();
    let parked = HookParked::start(owner, test_hooks::set_calendar_sync_log_pause, async move {
        sync(
            &DummyWebDavClient::new(reader, READER, STRONG, READER),
            &request_scope,
            &token,
        )
        .await
    })
    .await;
    // Real retention removes both streams after raw was already observed.
    test.server
        .delete_changes(owner, Some(0), None)
        .await
        .unwrap();
    refused(parked.finish().await);
    test_hooks::set_calendar_sync_log_pause(None);
}

async fn retention(test: &TestServer) {
    // Real numeric keys prove 250's allocation, unchanged neighbors and both
    // account-prefix destruction and Calendar's exact retention cutoff.
    let account = 900_000_000;
    let mut batch = BatchBuilder::new();
    for owner in [account, account + 1] {
        for collection in [2u8, 250, 251, 252, 253] {
            for change_id in [10, 20, 30] {
                let value = if collection == 2 {
                    vec![0u8; 8]
                } else {
                    b"retained\0".to_vec()
                };
                batch.set(
                    ValueClass::Any(AnyClass {
                        subspace: SUBSPACE_LOGS,
                        key: LogKey {
                            account_id: owner,
                            collection,
                            change_id,
                        }
                        .serialize(0),
                    }),
                    value,
                );
            }
        }
    }
    test.server.store().write(batch.build_all()).await.unwrap();
    test.server
        .delete_changes(account, Some(1), None)
        .await
        .unwrap();
    for collection in [250, 252] {
        assert!(bytes(test, account, collection, 10).await.is_none());
        for change_id in [20, 30] {
            assert!(bytes(test, account, collection, change_id).await.is_some());
        }
    }
    for (owner, collection) in [
        (account, 251),
        (account, 253),
        (account + 1, 250),
        (account + 1, 252),
    ] {
        assert!(
            bytes(test, owner, collection, 10).await.is_some(),
            "purge widened past Calendar history"
        );
    }
    test.server
        .store()
        .danger_destroy_account(account)
        .await
        .unwrap();
    assert!(bytes(test, account, 250, 30).await.is_none());
    assert!(bytes(test, account + 1, 250, 30).await.is_some());
    test.server
        .store()
        .danger_destroy_account(account + 1)
        .await
        .unwrap();
}

async fn ordinary_owner(test: &TestServer) {
    const USER: &str = "sharing-tombstone-plain@example.com";
    const HOME: &str = "/dav/cal/sharing-tombstone-plain%40example.com/";
    let admin = test.account("admin@example.com");
    let account = admin
        .create_user_account(USER, STRONG, "Plain tombstones", &[], user_permissions())
        .await;
    let owner = account.id().document_id();
    let client = DummyWebDavClient::new(owner, USER, STRONG, USER);
    let scope = format!("{HOME}ordinary/");
    client
        .request("MKCALENDAR", &scope, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let path = format!("{scope}ordinary.ics");
    client
        .request_with_headers(
            "PUT",
            &path,
            [("content-type", "text/calendar")],
            super::sharing::sharing_event("ordinary-tombstone"),
        )
        .await
        .with_status(StatusCode::CREATED);
    // This client must first observe the event. Insert+delete after an older
    // token is correctly coalesced away by the ordinary change-log reader.
    let snapshot = sync(&client, &scope, "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        snapshot.hrefs().contains(&path.as_str()),
        "ordinary client must remember the event before its deletion"
    );
    let token = snapshot.sync_token().to_owned();
    client
        .request("DELETE", &path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let deleted = test
        .server
        .store()
        .get_last_change_id(owner, SyncCollection::Calendar.into())
        .await
        .unwrap()
        .unwrap();
    let mut expected = path.as_bytes().to_vec();
    expected.push(0);
    assert_eq!(
        bytes(test, owner, 252, deleted).await,
        Some(expected),
        "ordinary raw path bytes must retain their legacy encoding"
    );
    assert!(
        raw(test, owner, VanishedCollection::Calendar, token_id(&token))
            .await
            .contains(&(deleted, path.clone()))
    );
    let response = sync(&client, &scope, &token)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(response.hrefs().contains(&path.as_str()));
    assert!(response.expect_body().contains("404 Not Found"));
    let calendar = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("ordinary")
        .unwrap()
        .document_id();
    assert!(
        raw(
            test,
            owner,
            VanishedCollection::CalendarIdentity,
            token_id(&token)
        )
        .await
        .iter()
        .any(|(change_id, record)| *change_id == deleted && record == &identity(calendar, &path))
    );
    admin.destroy_account(account).await;
    test.wait_for_tasks().await;
}
