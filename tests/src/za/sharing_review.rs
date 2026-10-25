/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! PR 58 regressions through real DAV/API requests and store-backed race seams.
use super::{STRONG, user_permissions};
use crate::utils::{
    server::TestServer,
    webdav::{DavResponse, DummyWebDavClient},
    za::{HookParked, pin_record, write_pin_record, za_post},
};
use common::za::test_hooks;
use groupware::{
    cache::GroupwareCache,
    calendar::{Calendar, CalendarEvent},
};
use hyper::StatusCode;
use serde_json::json;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, BatchBuilder},
};
use types::collection::{Collection, SyncCollection};

const OWNER: &str = "sharing-review-owner@example.com";
const READER: &str = "sharing-review-reader@example.com";
const HOME: &str = "/dav/cal/sharing-review-owner%40example.com/";
const READER_HOME: &str = "/dav/cal/sharing-review-reader%40example.com/";
const MKCAL: &str = "<C:mkcalendar xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:set><D:prop><D:displayname>Review calendar</D:displayname></D:prop></D:set></C:mkcalendar>";
const PATCH: &str = "<D:propertyupdate xmlns:D=\"DAV:\"><D:set><D:prop><D:displayname>Fresh source metadata</D:displayname></D:prop></D:set></D:propertyupdate>";

async fn id(test: &TestServer, owner: u32, path: &str) -> u32 {
    test.server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path(path)
        .unwrap()
        .document_id()
}
async fn archive(test: &TestServer, owner: u32, calendar: u32) -> Archive<AlignedBytes> {
    test.server
        .store()
        .get_value(ValueKey::archive(owner, Collection::Calendar, calendar))
        .await
        .unwrap()
        .unwrap()
}
async fn sync(client: &DummyWebDavClient, path: &str, token: &str) -> DavResponse {
    client.request("REPORT", path, format!("<D:sync-collection xmlns:D=\"DAV:\"><D:prop><D:getetag/></D:prop><D:sync-token>{token}</D:sync-token><D:sync-level>infinite</D:sync-level></D:sync-collection>")).await.with_status(StatusCode::MULTI_STATUS)
}
async fn put(client: &DummyWebDavClient, path: &str, uid: &str) {
    client
        .request_with_headers(
            "PUT",
            path,
            [("content-type", "text/calendar")],
            super::sharing::sharing_event(uid),
        )
        .await
        .with_status(StatusCode::CREATED);
}
fn display_name(response: &DavResponse) -> Option<&str> {
    response
        .xml
        .iter()
        .find(|(key, _)| key.ends_with(":displayname"))
        .map(|(_, value)| value.as_str())
}
async fn grant(slug: &str, level: &str) -> crate::utils::za::VaultReply {
    za_post(
        "shares/grant",
        &json!({"username":OWNER,"password":STRONG,"calendar":slug,"address":READER,"level":level}),
    )
    .await
}

pub async fn test(test: &mut TestServer) {
    println!("Running PR58 correction regressions...");
    let mut ids = Vec::new();
    for user in [OWNER, READER] {
        let account = test
            .account("admin@example.com")
            .create_key_user_account(user, STRONG, "Review fixture", &[], user_permissions())
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
    let mut failures = Vec::new();
    plain_update_trace(test, &mut failures).await;

    // Incorrect URI encoding used to silently drop these granted 404 tombstones.
    for slug in [
        "family&friends",
        "Family%20Trips",
        "private",
        "pin-conversion",
    ] {
        alice
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
    }
    for slug in ["family&friends", "Family%20Trips"] {
        grant(slug, "view").await.expect(200);
    }
    put(
        &alice,
        &format!("{HOME}private/anchor.ics"),
        "review-after-grant-boundaries",
    )
    .await;
    let token = sync(&bob, HOME, "").await.sync_token().to_string();
    for target in [
        HOME.to_string(),
        format!("{HOME}family&friends/"),
        format!("{HOME}Family%20Trips/"),
    ] {
        // Prove incremental admission on each scope, independently of the deletion oracle.
        sync(&bob, &target, &token).await;
    }
    for slug in ["family&friends", "Family%20Trips", "private"] {
        let path = format!("{HOME}{slug}/deleted.ics");
        put(&alice, &path, &format!("review-{slug}")).await;
        alice
            .request("DELETE", &path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for target in [
        HOME.to_string(),
        format!("{HOME}family&friends/"),
        format!("{HOME}Family%20Trips/"),
    ] {
        let response = sync(&bob, &target, &token).await;
        let hrefs = response.hrefs();
        for slug in ["family&friends", "Family%20Trips"] {
            if target == HOME || target == format!("{HOME}{slug}/") {
                let expected = format!("{HOME}{slug}/deleted.ics");
                if !hrefs.contains(&expected.as_str()) {
                    failures.push(format!(
                        "missing granted tombstone {expected} from {target}: {hrefs:?}"
                    ));
                }
            }
        }
        assert!(
            !hrefs.iter().any(|href| href.contains("/private/")),
            "private tombstone disclosure: {hrefs:?}"
        );
        if !response.expect_body().contains("404") {
            failures.push(format!("missing 404 tombstone status from {target}"));
        }
    }
    // Assisted discovery must reuse each fresh eligible archive, with per-item admission retained.
    let shared = id(test, owner, "family&friends").await;
    let before = groupware::calendar::sharing::test::calendar_reads(owner, shared);
    let discovery = bob
        .request_with_headers(
            "PROPFIND",
            READER_HOME,
            [("depth", "1")],
            "<D:propfind xmlns:D=\"DAV:\"><D:prop><D:displayname/></D:prop></D:propfind>",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        discovery
            .hrefs()
            .contains(&format!("{HOME}family&friends/").as_str())
    );
    assert!(
        !discovery
            .hrefs()
            .iter()
            .any(|href| href.contains("/private/"))
    );
    let delta = groupware::calendar::sharing::test::calendar_reads(owner, shared) - before;
    if delta != 1 {
        failures.push(format!(
            "assisted discovery resolved eligible calendar {delta} times instead of 1"
        ));
    }
    bob.request_with_headers("PROPFIND", &format!("{HOME}private/"), [("depth", "0")], "")
        .await
        .with_status(StatusCode::FORBIDDEN);

    // A pin-only write must not spend a conversion retry; final grant still checks its decision pin.
    put(
        &alice,
        &format!("{HOME}pin-conversion/one.ics"),
        "review-pin-conversion",
    )
    .await;
    test_hooks::take_share_conversion_losses(owner);
    let parked = HookParked::start(
        owner,
        test_hooks::set_share_rewrap_pause,
        grant("pin-conversion", "view"),
    )
    .await;
    let mut pins = pin_record(test, owner, STRONG).await.unwrap();
    pins.revision += 1;
    write_pin_record(test, owner, STRONG, &pins).await;
    let answer = parked.finish().await.expect(200);
    assert_eq!(answer["rewrapped"], 1);
    let losses = test_hooks::take_share_conversion_losses(owner);
    if losses != 0 {
        failures.push(format!("pin-only write caused {losses} conversion losses"));
    }
    let parked = HookParked::start(
        owner,
        test_hooks::set_share_commit_pause,
        grant("pin-conversion", "edit"),
    )
    .await;
    let original = pin_record(test, owner, STRONG).await.unwrap();
    let mut changed = original.clone();
    changed.revision += 1;
    changed
        .pins
        .iter_mut()
        .find(|pin| pin.address == READER)
        .unwrap()
        .public_key = [7; 32];
    write_pin_record(test, owner, STRONG, &changed).await;
    assert_eq!(
        parked.finish().await.status,
        409,
        "final grant must reject a changed pin"
    );
    write_pin_record(test, owner, STRONG, &original).await;
    let listed = za_post("shares", &json!({"username":OWNER,"password":STRONG}))
        .await
        .expect(200);
    let pin_cal = listed["calendars"]
        .as_array()
        .unwrap()
        .iter()
        .find(|cal| cal["calendar"] == "pin-conversion")
        .unwrap();
    assert_eq!(pin_cal["grants"][0]["level"], "view");

    // A later source property B must survive a 409 without publishing a child
    // prefix, with or without the original source If-Match condition.
    for scenario in ["unconditional", "conditional", "rename"] {
        let rename = scenario == "rename";
        let slug = format!("move-{scenario}-source");
        let target = format!("move-{scenario}-target");
        alice
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
        alice
            .request("MKCALENDAR", &format!("{HOME}{target}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
        put(&alice, &format!("{HOME}{slug}/one.ics"), &slug).await;
        let source_id = id(test, owner, &slug).await;
        let before = archive(test, owner, source_id).await.version;
        let source_properties = alice
            .request_with_headers(
                "PROPFIND", &format!("{HOME}{slug}/"), [("depth", "0")],
                "<D:propfind xmlns:D=\"DAV:\"><D:prop><D:getetag/><D:displayname/></D:prop></D:propfind>",
            )
            .await
            .with_status(StatusCode::MULTI_STATUS);
        let etag = source_properties
            .xml
            .iter()
            .find(|(key, _)| key.ends_with(":getetag"))
            .unwrap()
            .1
            .clone();
        assert_eq!(display_name(&source_properties), Some("Review calendar"));
        let move_slug = slug.clone();
        let move_target = target.clone();
        let parked = HookParked::start(owner, test_hooks::set_copy_pause, async move {
            let mut headers = vec![("destination", format!("{HOME}{move_target}/"))];
            if scenario == "conditional" {
                headers.push(("if-match", etag));
            }
            DummyWebDavClient::new(owner, OWNER, STRONG, OWNER)
                .request_with_headers(
                    "MOVE",
                    &format!("{HOME}{move_slug}/"),
                    headers.iter().map(|(name, value)| (*name, value.as_str())),
                    "",
                )
                .await
                .status
        })
        .await;
        if rename {
            alice
                .request_with_headers(
                    "MOVE",
                    &format!("{HOME}{slug}/"),
                    [(
                        "destination",
                        format!("{HOME}renamed-during-move/").as_str(),
                    )],
                    "",
                )
                .await
                .with_status(StatusCode::CREATED);
        } else {
            alice
                .request("PROPPATCH", &format!("{HOME}{slug}/"), PATCH)
                .await
                .with_status(StatusCode::MULTI_STATUS);
        }
        assert_ne!(archive(test, owner, source_id).await.version, before);
        let result = parked.finish().await;
        if result != StatusCode::CONFLICT {
            failures.push(format!(
                "collection MOVE {scenario}: expected 409, got {result}"
            ));
        }
        let resources = test
            .server
            .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
            .await
            .unwrap();
        let retained_source = if rename { "renamed-during-move" } else { &slug };
        assert!(
            resources
                .by_path(&format!("{retained_source}/one.ics"))
                .is_some(),
            "atomic conflict must retain source child"
        );
        assert_eq!(
            resources
                .subtree(&target)
                .filter(|r| !r.is_container())
                .count(),
            0
        );
        if rename {
            assert!(
                resources.by_path("renamed-during-move").is_some(),
                "concurrent rename deleted"
            );
        } else {
            let properties =
                "<D:propfind xmlns:D=\"DAV:\"><D:prop><D:displayname/></D:prop></D:propfind>";
            let source = alice
                .request_with_headers(
                    "PROPFIND",
                    &format!("{HOME}{slug}/"),
                    [("depth", "0")],
                    properties,
                )
                .await;
            if source.status != StatusCode::MULTI_STATUS
                || display_name(&source) != Some("Fresh source metadata")
            {
                failures.push(format!(
                    "collection MOVE {scenario} lost source property B: status{}, name{:?}",
                    source.status,
                    display_name(&source)
                ));
            }
            let destination = alice
                .request_with_headers(
                    "PROPFIND",
                    &format!("{HOME}{target}/"),
                    [("depth", "0")],
                    properties,
                )
                .await
                .with_status(StatusCode::MULTI_STATUS);
            assert_eq!(
                display_name(&destination),
                Some("Review calendar"),
                "destination must retain preflight property A for {scenario}"
            );
        }
    }

    // A source-calendar metadata write after preparation loses the original source CAS.
    for slug in ["move-late-source", "move-late-target"] {
        alice
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
    }
    put(
        &alice,
        &format!("{HOME}move-late-source/one.ics"),
        "review-move-late-source",
    )
    .await;
    let source_id = id(test, owner, "move-late-source").await;
    let before = archive(test, owner, source_id).await.version;
    let parked = HookParked::start(
        owner,
        test_hooks::set_calendar_move_delete_pause,
        async move {
            DummyWebDavClient::new(owner, OWNER, STRONG, OWNER)
                .request_with_headers(
                    "MOVE",
                    &format!("{HOME}move-late-source/"),
                    [("destination", format!("{HOME}move-late-target/").as_str())],
                    "",
                )
                .await
                .status
        },
    )
    .await;
    alice
        .request("PROPPATCH", &format!("{HOME}move-late-source/"), PATCH)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_ne!(archive(test, owner, source_id).await.version, before);
    assert_eq!(
        parked.finish().await,
        StatusCode::CONFLICT,
        "late source write lost its CAS fence"
    );
    let resources = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    assert!(resources.by_path("move-late-source").is_some());
    assert!(resources.by_path("move-late-source/one.ics").is_some());
    assert_eq!(
        resources
            .subtree("move-late-target")
            .filter(|r| !r.is_container())
            .count(),
        0
    );

    // Only a precise missing archive is omitted after enumeration, in both lists.
    for foreign in [false, true] {
        for corrupt in [false, true] {
            let slug = format!(
                "list-{}-{}",
                if foreign { "foreign" } else { "own" },
                if corrupt { "corrupt" } else { "deleted" }
            );
            let path = format!("{HOME}{slug}/");
            alice
                .request("MKCALENDAR", &path, MKCAL)
                .await
                .with_status(StatusCode::CREATED);
            if foreign {
                grant(&slug, "view").await.expect(200);
            }
            let calendar = id(test, owner, &slug).await;
            let caller = if foreign { READER } else { OWNER };
            let caller_id = if foreign { reader } else { owner };
            let hook = if foreign {
                test_hooks::set_share_list_foreign_pause
            } else {
                test_hooks::set_share_list_own_pause
            };
            let parked = HookParked::start(caller_id, hook, async move {
                za_post("shares", &json!({"username":caller,"password":STRONG})).await
            })
            .await;
            let quota_before = if corrupt {
                Some(test.server.get_used_quota_account(owner).await.unwrap())
            } else {
                None
            };
            let original = if corrupt {
                // Keep the rkyv archive structurally valid: corrupt only the authenticated
                // collection entry, so the real resolver reports a non-NotFound failure.
                let stored = archive(test, owner, calendar).await;
                let original = stored.deserialize::<Calendar>().unwrap();
                let mut broken = original.clone();
                broken
                    .preferences
                    .iter_mut()
                    .find(|entry| entry.account_id == owner)
                    .unwrap()
                    .name = "$za$invalid-entry".into();
                let mut batch = BatchBuilder::new();
                broken
                    .update(
                        test.server
                            .account_info(owner)
                            .await
                            .unwrap()
                            .account_tenant_ids(),
                        stored.to_unarchived::<Calendar>().unwrap(),
                        owner,
                        calendar,
                        &mut batch,
                    )
                    .unwrap();
                test.server.commit_batch(batch).await.unwrap();
                Some(original)
            } else {
                alice
                    .request("DELETE", &path, "")
                    .await
                    .with_status(StatusCode::NO_CONTENT);
                None
            };
            let answer = parked.finish().await;
            if corrupt {
                assert_eq!(
                    answer.status, 500,
                    "non-NotFound archive failure was hidden"
                );
                let broken = archive(test, owner, calendar).await;
                let mut batch = BatchBuilder::new();
                original
                    .unwrap()
                    .update(
                        test.server
                            .account_info(owner)
                            .await
                            .unwrap()
                            .account_tenant_ids(),
                        broken.to_unarchived::<Calendar>().unwrap(),
                        owner,
                        calendar,
                        &mut batch,
                    )
                    .unwrap();
                test.server.commit_batch(batch).await.unwrap();
                assert_eq!(
                    test.server.get_used_quota_account(owner).await.unwrap(),
                    quota_before.unwrap(),
                    "corrupt calendar restoration must preserve owner quota"
                );
                alice
                    .request("DELETE", &path, "")
                    .await
                    .with_status(StatusCode::NO_CONTENT);
            } else if answer.status != 200 {
                failures.push(format!(
                    "list foreign={foreign} deleted calendar returned{}: {}",
                    answer.status, answer.body
                ));
            } else {
                let field = if foreign {
                    "shared_with_me"
                } else {
                    "calendars"
                };
                assert!(
                    !answer.body[field]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|cal| cal["calendar"] == slug)
                );
            }
        }
    }
    // Cleanup goes through DAV, including any committed-prefix source still present on RED.
    let resources = test
        .server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let paths = resources
        .tree_with_depth(0)
        .filter(|r| r.is_container())
        .map(|r| resources.format_resource(r).to_string())
        .collect::<Vec<_>>();
    for path in paths {
        alice
            .request("DELETE", &path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    alice
        .request(
            "DELETE",
            "/dav/card/sharing-review-owner%40example.com/default/",
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    bob.delete_default_containers().await;
    test.wait_for_tasks().await;
    assert!(
        failures.is_empty(),
        "PR58 regressions:\n{}",
        failures.join("\n")
    );
}

/// A real plain-account event CAS failure must retain the DAV update call site.
async fn plain_update_trace(test: &TestServer, failures: &mut Vec<String>) {
    use std::time::Duration;
    use trc::{
        Collector, EventType, StoreEvent,
        ipc::subscriber::{Interests, SubscriberBuilder},
    };
    let account = test.account("plain@example.com");
    let owner = account.id().document_id();
    let name = account.name();
    let secret = account.secret();
    let client = DummyWebDavClient::new(owner, name, secret, name);
    let home = format!(
        "/dav/cal/{}/sharing-review-trace/",
        name.replace('@', "%40")
    );
    let path = format!("{home}one.ics");
    client
        .request("MKCALENDAR", &home, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    put(&client, &path, "review-plain-trace").await;
    let body =
        super::sharing::sharing_event("review-plain-trace").replace("SUMMARY:", "SUMMARY:second ");
    client
        .request_with_headers("PUT", &path, [("content-type", "text/calendar")], &body)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let event_type = EventType::Store(StoreEvent::AssertValueFailed);
    let mut interests = Interests::default();
    interests.set(event_type);
    let (_tx, mut rx) = SubscriberBuilder::new("sharing-review-trace".into())
        .set_interests([event_type])
        .with_lossy(false)
        .register();
    Collector::union_interests(interests);
    Collector::reload();
    let request_path = path.clone();
    let parked = HookParked::start(owner, test_hooks::set_update_commit_pause, async move {
        DummyWebDavClient::new(owner, name, secret, name)
            .request_with_headers(
                "PUT",
                &request_path,
                [("content-type", "text/calendar")],
                body.replace("second ", "parked "),
            )
            .await
            .status
    })
    .await;
    let event_id = id(test, owner, "sharing-review-trace/one.ics").await;
    let stored = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            owner,
            Collection::CalendarEvent,
            event_id,
        ))
        .await
        .unwrap()
        .unwrap();
    let mut concurrent = stored.deserialize::<CalendarEvent>().unwrap();
    concurrent.flags ^= 1;
    let mut batch = BatchBuilder::new();
    concurrent
        .update(
            test.server
                .account_info(owner)
                .await
                .unwrap()
                .account_tenant_ids(),
            stored.to_unarchived::<CalendarEvent>().unwrap(),
            owner,
            event_id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    assert_eq!(parked.finish().await, StatusCode::CONFLICT);
    let captured = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(batch) = rx.recv().await {
            for event in batch {
                let this_request = event.keys.iter().any(|(key, value)| {
                    *key == trc::Key::Url
                        && matches!(value, trc::Value::String(url) if url.as_str() == path)
                });
                if this_request {
                    return event;
                }
            }
        }
        panic!("trace subscriber closed before plain CAS failure");
    })
    .await
    .unwrap();
    Collector::remove_subscriber("sharing-review-trace".into());
    Collector::reload();
    assert_eq!(captured.inner.typ, event_type, "trace positive control");
    let located = captured.keys.iter().any(|(key, value)| *key == trc::Key::CausedBy && matches!(value, trc::Value::String(location) if location.contains("crates/dav/src/calendar/update.rs:")));
    if !located {
        failures.push(format!(
            "plain update CAS trace lost DAV location: {:?}",
            captured.keys
        ));
    }
    client
        .request("DELETE", &path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request("DELETE", &home, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}
