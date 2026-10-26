/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Bounded collection overwrite MOVE uses one physical transaction and a membership fence.
use super::{STRONG, user_permissions};
use crate::utils::{server::TestServer, webdav::DummyWebDavClient, za::HookParked};
use common::za::test_hooks;
use groupware::{cache::GroupwareCache, calendar::CalendarEvent};
use hyper::StatusCode;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, ArchiveVersion},
};
use types::collection::{Collection, SyncCollection};

const USER: &str = "sharing-move-atomic@example.com";
const HOME: &str = "/dav/cal/sharing-move-atomic%40example.com/";
const MKCAL: &str = "<C:mkcalendar xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:set><D:prop><D:displayname>Atomic calendar</D:displayname></D:prop></D:set></C:mkcalendar>";
const PATCH: &str = "<D:propertyupdate xmlns:D=\"DAV:\"><D:set><D:prop><D:displayname>Concurrent metadata B</D:displayname></D:prop></D:set></D:propertyupdate>";

async fn create(client: &DummyWebDavClient, slug: &str) {
    client
        .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
        .await
        .with_status(StatusCode::CREATED);
}
async fn put(client: &DummyWebDavClient, slug: &str, name: &str, uid: &str) {
    client
        .request_with_headers(
            "PUT",
            &format!("{HOME}{slug}/{name}"),
            [("content-type", "text/calendar")],
            super::sharing::sharing_event(uid),
        )
        .await
        .with_status(StatusCode::CREATED);
}
async fn moving(owner: u32, source: &str, target: &str) -> StatusCode {
    DummyWebDavClient::new(owner, USER, STRONG, USER)
        .request_with_headers(
            "MOVE",
            &format!("{HOME}{source}/"),
            [("destination", format!("{HOME}{target}/").as_str())],
            "",
        )
        .await
        .status
}
async fn archive(
    test: &TestServer,
    owner: u32,
    collection: Collection,
    id: u32,
) -> Option<Archive<AlignedBytes>> {
    test.server
        .store()
        .get_value(ValueKey::archive(owner, collection, id))
        .await
        .unwrap()
}
async fn id(test: &TestServer, owner: u32, path: &str) -> u32 {
    test.server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path(path)
        .unwrap()
        .document_id()
}
async fn snapshot(test: &TestServer, owner: u32) -> Vec<(u8, u32, ArchiveVersion, Vec<u8>)> {
    let mut result = Vec::new();
    for collection in [Collection::Calendar, Collection::CalendarEvent] {
        test.server
            .archives(owner, collection, &(), |id, stored| {
                result.push((
                    collection.into(),
                    id,
                    stored.version,
                    stored.as_bytes().to_vec(),
                ));
                Ok(true)
            })
            .await
            .unwrap();
    }
    result
}
async fn readable(client: &DummyWebDavClient, slug: &str, name: &str, uid: &str) -> bool {
    let result = client
        .request("GET", &format!("{HOME}{slug}/{name}"), "")
        .await;
    result.status == StatusCode::OK && result.body.as_ref().is_ok_and(|body| body.contains(uid))
}

pub async fn test(test: &mut TestServer) {
    println!("Running atomic collection MOVE regressions...");
    let account = test
        .account("admin@example.com")
        .create_key_user_account(USER, STRONG, "Atomic MOVE", &[], user_permissions())
        .await;
    let owner = account.id().document_id();
    test.insert_account(account);
    let client = DummyWebDavClient::new(owner, USER, STRONG, USER);
    let mut failures = Vec::new();

    // Two identical unconditional requests must never erase a committed child prefix.
    for slug in ["metadata-source", "metadata-target"] {
        create(&client, slug).await;
    }
    put(
        &client,
        "metadata-source",
        "one.ics",
        "atomic-metadata-source",
    )
    .await;
    put(
        &client,
        "metadata-target",
        "old.ics",
        "atomic-metadata-target",
    )
    .await;
    let target_id = id(test, owner, "metadata-target").await;
    let target_before = archive(test, owner, Collection::Calendar, target_id)
        .await
        .unwrap();
    let old_id = id(test, owner, "metadata-target/old.ics").await;
    let old_before = archive(test, owner, Collection::CalendarEvent, old_id)
        .await
        .unwrap();
    let parked = HookParked::start(
        owner,
        test_hooks::set_copy_pause,
        moving(owner, "metadata-source", "metadata-target"),
    )
    .await;
    client
        .request("PROPPATCH", &format!("{HOME}metadata-source/"), PATCH)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let quota = test.server.get_used_quota_account(owner).await.unwrap();
    let status = parked.finish().await;
    if status != StatusCode::CONFLICT {
        failures.push(format!("metadata race returned {status}"));
    }
    if !readable(
        &client,
        "metadata-source",
        "one.ics",
        "atomic-metadata-source",
    )
    .await
        || !readable(
            &client,
            "metadata-target",
            "old.ics",
            "atomic-metadata-target",
        )
        .await
        || archive(test, owner, Collection::Calendar, target_id)
            .await
            .map(|s| s.version)
            != Some(target_before.version)
        || archive(test, owner, Collection::CalendarEvent, old_id)
            .await
            .map(|s| s.version)
            != Some(old_before.version)
        || test.server.get_used_quota_account(owner).await.unwrap() != quota
    {
        failures.push("metadata loss published a destination deletion or moved prefix".into());
    }
    let retry = moving(owner, "metadata-source", "metadata-target").await;
    if retry != StatusCode::NO_CONTENT
        || !readable(
            &client,
            "metadata-target",
            "one.ics",
            "atomic-metadata-source",
        )
        .await
    {
        failures.push(format!(
            "identical unconditional retry lost source event: {retry}"
        ));
    }

    let properties = client
        .request_with_headers(
            "PROPFIND",
            &format!("{HOME}metadata-target/"),
            [("depth", "0")],
            "<D:propfind xmlns:D=\"DAV:\"><D:prop><D:displayname/></D:prop></D:propfind>",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    if !properties
        .xml
        .iter()
        .any(|(key, value)| key.ends_with(":displayname") && value == "Concurrent metadata B")
    {
        failures.push("identical retry lost the concurrent source metadata B".into());
    }

    // A create after the actual membership scan must abort every MOVE mutation.
    for slug in ["membership-source", "membership-target"] {
        create(&client, slug).await;
    }
    put(
        &client,
        "membership-source",
        "one.ics",
        "atomic-membership-original",
    )
    .await;
    let source_id = id(test, owner, "membership-source").await;
    let parked = HookParked::start(
        owner,
        test_hooks::set_calendar_move_delete_pause,
        moving(owner, "membership-source", "membership-target"),
    )
    .await;
    put(
        &client,
        "membership-source",
        "late.ics",
        "atomic-membership-late",
    )
    .await;
    let after_create = snapshot(test, owner).await;
    let quota = test.server.get_used_quota_account(owner).await.unwrap();
    let status = parked.finish().await;
    if status != StatusCode::CONFLICT
        || archive(test, owner, Collection::Calendar, source_id)
            .await
            .is_none()
        || snapshot(test, owner).await != after_create
        || test.server.get_used_quota_account(owner).await.unwrap() != quota
        || !readable(
            &client,
            "membership-source",
            "late.ics",
            "atomic-membership-late",
        )
        .await
    {
        failures.push(format!(
            "post-scan source create escaped membership fence: {status}"
        ));
    }
    assert!(
        failures.is_empty(),
        "atomic MOVE regressions:\n{}",
        failures.join("\n")
    );

    // Additional quiet/race/budget controls follow after the RED atomicity oracle.
    controls(test, owner, &client).await;
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
        client
            .request("DELETE", &path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    client
        .request(
            "DELETE",
            "/dav/card/sharing-move-atomic%40example.com/default/",
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
}

async fn controls(test: &TestServer, owner: u32, client: &DummyWebDavClient) {
    // Metadata/event/other-parent conditions all retain the competing write.
    for kind in ["target", "other-parent", "unrelated", "event", "link"] {
        let source = format!("condition-{kind}-source");
        let target = format!("condition-{kind}-target");
        let other = format!("condition-{kind}-other");
        for slug in [&source, &target, &other] {
            create(client, slug).await;
        }
        put(client, &source, "one.ics", &format!("condition-{kind}-one")).await;
        put(client, &target, "old.ics", &format!("condition-{kind}-old")).await;
        if kind == "other-parent" {
            client
                .request_with_headers(
                    "COPY",
                    &format!("{HOME}{source}/one.ics"),
                    [("destination", format!("{HOME}{other}/linked.ics").as_str())],
                    "",
                )
                .await
                .with_status(StatusCode::CREATED);
        }
        if kind == "link" {
            put(client, &other, "new.ics", "late-copy-membership").await;
        }
        let from = source.clone();
        let to = target.clone();
        let parked = HookParked::start(
            owner,
            test_hooks::set_calendar_move_delete_pause,
            async move { moving(owner, &from, &to).await },
        )
        .await;
        match kind {
            "target" => {
                client
                    .request("PROPPATCH", &format!("{HOME}{target}/"), PATCH)
                    .await
                    .with_status(StatusCode::MULTI_STATUS);
            }
            "other-parent" => {
                client
                    .request("PROPPATCH", &format!("{HOME}{other}/"), PATCH)
                    .await
                    .with_status(StatusCode::MULTI_STATUS);
            }
            "unrelated" => put(client, &other, "new.ics", "unrelated-indexed-change").await,
            "event" => {
                client
                    .request_with_headers(
                        "PUT",
                        &format!("{HOME}{source}/one.ics"),
                        [("content-type", "text/calendar")],
                        super::sharing::sharing_event(&format!("condition-{kind}-one"))
                            .replace("SUMMARY:", "SUMMARY:updated "),
                    )
                    .await
                    .with_status(StatusCode::NO_CONTENT);
            }
            "link" => {
                client
                    .request_with_headers(
                        "COPY",
                        &format!("{HOME}{other}/new.ics"),
                        [("destination", format!("{HOME}{source}/new.ics").as_str())],
                        "",
                    )
                    .await
                    .with_status(StatusCode::CREATED);
            }
            _ => unreachable!(),
        }
        let after_competing = snapshot(test, owner).await;
        let quota = test.server.get_used_quota_account(owner).await.unwrap();
        assert_eq!(parked.finish().await, StatusCode::CONFLICT, "{kind}");
        assert_eq!(
            snapshot(test, owner).await,
            after_competing,
            "{kind} changed archives on failure"
        );
        assert_eq!(
            test.server.get_used_quota_account(owner).await.unwrap(),
            quota,
            "{kind} changed quota on failure"
        );
    }

    // Already-admitted create commits after MOVE: its original parent fence must fail.
    for slug in ["held-source", "held-target"] {
        create(client, slug).await;
    }
    put(client, "held-source", "one.ics", "held-original").await;
    let creator = HookParked::start(owner, test_hooks::set_commit_pause, async move {
        DummyWebDavClient::new(owner, USER, STRONG, USER)
            .request_with_headers(
                "PUT",
                &format!("{HOME}held-source/late.ics"),
                [("content-type", "text/calendar")],
                super::sharing::sharing_event("held-late"),
            )
            .await
            .status
    })
    .await;
    assert_eq!(
        moving(owner, "held-source", "held-target").await,
        StatusCode::NO_CONTENT
    );
    let committed = snapshot(test, owner).await;
    let result = creator.finish().await;
    assert!(
        result.is_client_error(),
        "held creator unexpectedly succeeded: {result}"
    );
    assert_eq!(
        snapshot(test, owner).await,
        committed,
        "held creator published an orphan"
    );
    assert!(readable(client, "held-target", "one.ics", "held-original").await);

    // One event in source, destination and a third collection is updated exactly once.
    for slug in ["linked-source", "linked-target", "linked-other"] {
        create(client, slug).await;
    }
    put(
        client,
        "linked-source",
        "source-name.ics",
        "coalesced-three-links",
    )
    .await;
    let event_id = id(test, owner, "linked-source/source-name.ics").await;
    let target_id = id(test, owner, "linked-target").await;
    let other_id = id(test, owner, "linked-other").await;
    for (slug, name) in [
        ("linked-target", "old-target-name.ics"),
        ("linked-other", "other-name.ics"),
    ] {
        client
            .request_with_headers(
                "COPY",
                &format!("{HOME}linked-source/source-name.ics"),
                [("destination", format!("{HOME}{slug}/{name}").as_str())],
                "",
            )
            .await
            .with_status(StatusCode::CREATED);
    }
    assert_eq!(
        moving(owner, "linked-source", "linked-target").await,
        StatusCode::NO_CONTENT
    );
    let stored = archive(test, owner, Collection::CalendarEvent, event_id)
        .await
        .unwrap()
        .deserialize::<CalendarEvent>()
        .unwrap();
    assert_eq!(stored.names.len(), 2);
    assert!(
        stored
            .names
            .iter()
            .any(|name| name.parent_id == target_id && name.name == "source-name.ics")
    );
    assert!(
        stored
            .names
            .iter()
            .any(|name| name.parent_id == other_id && name.name == "other-name.ics")
    );
    assert!(
        readable(
            client,
            "linked-target",
            "source-name.ics",
            "coalesced-three-links"
        )
        .await
    );
    assert!(
        readable(
            client,
            "linked-other",
            "other-name.ics",
            "coalesced-three-links"
        )
        .await
    );

    // Empty-source overwrite still intentionally clears a populated destination.
    for slug in ["empty-source", "empty-target"] {
        create(client, slug).await;
    }
    put(client, "empty-target", "old.ics", "empty-overwrite-old").await;
    let old_id = id(test, owner, "empty-target/old.ics").await;
    assert_eq!(
        moving(owner, "empty-source", "empty-target").await,
        StatusCode::NO_CONTENT
    );
    assert!(
        archive(test, owner, Collection::CalendarEvent, old_id)
            .await
            .is_none()
    );

    // Source request conditions are checked before planning and remain binding.
    for slug in ["conditional-source", "conditional-target"] {
        create(client, slug).await;
    }
    put(client, "conditional-source", "one.ics", "stale-if-match").await;
    let before = snapshot(test, owner).await;
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}conditional-source/"),
            [
                ("destination", format!("{HOME}conditional-target/").as_str()),
                ("if-match", "\"not-current\""),
            ],
            "",
        )
        .await
        .with_status(StatusCode::PRECONDITION_FAILED);
    assert_eq!(snapshot(test, owner).await, before);

    // Clear the default atomically, and never overwrite a concurrent replacement pointer.
    for concurrent in [false, true] {
        let source = format!("default-{concurrent}-source");
        let target = format!("default-{concurrent}-target");
        for slug in [&source, &target] {
            create(client, slug).await;
        }
        let source_id = id(test, owner, &source).await;
        let target_id = id(test, owner, &target).await;
        set_default(test, owner, source_id).await;
        let from = source.clone();
        let to = target.clone();
        let parked = HookParked::start(
            owner,
            test_hooks::set_calendar_move_delete_pause,
            async move { moving(owner, &from, &to).await },
        )
        .await;
        if concurrent {
            set_default(test, owner, target_id).await;
        }
        let before_commit = snapshot(test, owner).await;
        let quota = test.server.get_used_quota_account(owner).await.unwrap();
        assert_eq!(
            parked.finish().await,
            if concurrent {
                StatusCode::CONFLICT
            } else {
                StatusCode::NO_CONTENT
            }
        );
        let default = test
            .server
            .store()
            .get_value::<u32>(ValueKey {
                account_id: owner,
                collection: Collection::Principal.into(),
                document_id: 0,
                class: store::write::ValueClass::Property(
                    types::field::PrincipalField::DefaultCalendarId.into(),
                ),
            })
            .await
            .unwrap();
        assert_eq!(default, if concurrent { Some(target_id) } else { None });
        if concurrent {
            assert_eq!(snapshot(test, owner).await, before_commit);
            assert_eq!(
                test.server.get_used_quota_account(owner).await.unwrap(),
                quota
            );
        }
    }

    // Cross each cap independently; a small source control still commits one slice.
    for (label, count, padding) in [
        ("operations", 360, 0),
        ("bytes", 30, 230_000),
        ("split", 999, 0),
    ] {
        let source = format!("budget-{label}-source");
        let target = format!("budget-{label}-target");
        for slug in [&source, &target] {
            create(client, slug).await;
        }
        put(client, &target, "old.ics", &format!("budget-{label}-old")).await;
        for n in 0..count {
            let mut event = super::sharing::sharing_event(&format!("budget-{label}-{n}"));
            if padding > 0 {
                event = event.replace(
                    "END:VEVENT",
                    &format!("DESCRIPTION:{}\r\nEND:VEVENT", "p".repeat(padding)),
                );
            }
            client
                .request_with_headers(
                    "PUT",
                    &format!("{HOME}{source}/{n}.ics"),
                    [("content-type", "text/calendar")],
                    event,
                )
                .await
                .with_status(StatusCode::CREATED);
        }
        test_hooks::take_calendar_move_plans(owner);
        let before = snapshot(test, owner).await;
        let quota = test.server.get_used_quota_account(owner).await.unwrap();
        assert_eq!(
            moving(owner, &source, &target).await,
            StatusCode::CONFLICT,
            "{label} oversize"
        );
        assert_eq!(
            snapshot(test, owner).await,
            before,
            "{label} refusal mutated archives"
        );
        assert_eq!(
            test.server.get_used_quota_account(owner).await.unwrap(),
            quota
        );
        let plans = test_hooks::take_calendar_move_plans(owner);
        assert_eq!(plans.len(), 1);
        let (operations, bytes, slices) = plans[0];
        println!(
            "Atomic MOVE {label} admission: operations={operations}, bytes={bytes}, physical_slices={slices}"
        );
        if label == "operations" {
            assert!(operations > 1000 && bytes < 5_000_000);
        } else if label == "split" {
            assert!(operations > 1000 && bytes < 5_000_000 && slices > 1);
        } else {
            assert!(operations <= 1000 && bytes > 5_000_000 && slices == 1);
        }
    }
    test_hooks::take_calendar_move_plans(owner);
    for slug in ["bounded-source", "bounded-target"] {
        create(client, slug).await;
    }
    put(client, "bounded-source", "one.ics", "bounded-control").await;
    assert_eq!(
        moving(owner, "bounded-source", "bounded-target").await,
        StatusCode::NO_CONTENT
    );
    let plans = test_hooks::take_calendar_move_plans(owner);
    assert_eq!(plans.len(), 1);
    assert!(plans[0].0 <= 1000 && plans[0].1 <= 5_000_000 && plans[0].2 == 1);

    // Every stored event name still has a live parent after all race/control cases.
    let mut calendars = std::collections::BTreeSet::new();
    test.server
        .archives(owner, Collection::Calendar, &(), |id, _| {
            calendars.insert(id);
            Ok(true)
        })
        .await
        .unwrap();
    test.server
        .archives(owner, Collection::CalendarEvent, &(), |_, stored| {
            for name in stored.unarchive::<CalendarEvent>()?.names.iter() {
                assert!(
                    calendars.contains(&name.parent_id.to_native()),
                    "event name has a deleted parent"
                );
            }
            Ok(true)
        })
        .await
        .unwrap();
}

async fn set_default(test: &TestServer, owner: u32, id: u32) {
    use store::SerializeInfallible;
    let mut batch = store::write::BatchBuilder::new();
    batch
        .with_account_id(owner)
        .with_collection(Collection::Principal)
        .with_document(0)
        .set(
            types::field::PrincipalField::DefaultCalendarId,
            id.serialize(),
        );
    test.server.store().write(batch.build_all()).await.unwrap();
}
