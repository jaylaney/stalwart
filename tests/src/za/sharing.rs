/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Owner read adapters while a first-grant conversion is still a mixed journal.
use super::{STRONG, user_permissions};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{plant_event, za_unlock},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use groupware::{
    cache::GroupwareCache,
    calendar::{
        Calendar, CalendarEvent,
        seal::{rewrap_event, unseal_calendar},
        sharing::{
            format::{OwnerRingHeader, ZaOwnerRing, seal_owner_ring},
            resolve::za_owned_calendar_context,
            test::{calendar_reads, prepare_shared_calendar},
        },
    },
};
use hyper::StatusCode;
use std::collections::BTreeMap;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, BatchBuilder},
};
use types::collection::{Collection, SyncCollection};

const OWNER: &str = "share-owner-read@example.com";
const CAL: &str = "/dav/cal/share-owner-read%40example.com/share-read/";
const MKCAL: &str = "<C:mkcalendar xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:set><D:prop><D:displayname>Shared owner private name</D:displayname></D:prop></D:set></C:mkcalendar>";
const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//share-tests//EN\r\nBEGIN:VEVENT\r\nUID:share-read-UID\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20240102T090000Z\r\nDTEND:20240102T100000Z\r\nSUMMARY:share-read-private-summary\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

async fn event_archive(test: &TestServer, owner_id: u32, event_id: u32) -> Archive<AlignedBytes> {
    test.server
        .store()
        .get_value(ValueKey::archive(
            owner_id,
            Collection::CalendarEvent,
            event_id,
        ))
        .await
        .unwrap()
        .unwrap()
}

async fn update_event(
    test: &TestServer,
    owner_id: u32,
    event_id: u32,
    stored: &Archive<AlignedBytes>,
    event: CalendarEvent,
) {
    let account = test.server.account_info(owner_id).await.unwrap();
    let mut batch = BatchBuilder::new();
    event
        .update(
            account.account_tenant_ids(),
            stored.to_unarchived::<CalendarEvent>().unwrap(),
            owner_id,
            event_id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_task_queue();
}

async fn update_calendar(
    test: &TestServer,
    owner_id: u32,
    calendar_id: u32,
    stored: &Archive<AlignedBytes>,
    calendar: Calendar,
) {
    let account = test.server.account_info(owner_id).await.unwrap();
    let mut batch = BatchBuilder::new();
    calendar
        .update(
            account.account_tenant_ids(),
            stored.to_unarchived::<Calendar>().unwrap(),
            owner_id,
            calendar_id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
}

pub async fn test(test: &mut TestServer) {
    delegated_receiver_consistency(test).await;
    delegated_receiver_rejections_and_reply(test).await;
    delegated_scheduling_tests(test).await;
    sharing_endpoint_tests(test).await;
    println!("Running sharing resolver and owner mixed-state read tests...");
    let owner = test
        .account("admin@example.com")
        .create_key_user_account(OWNER, STRONG, "Sharing owner read", &[], user_permissions())
        .await;
    let owner_id = owner.id().document_id();
    let client = DummyWebDavClient::new(owner_id, OWNER, STRONG, OWNER);
    test.insert_account(owner);
    client
        .request("MKCALENDAR", CAL, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let keys = za_unlock(test, owner_id, STRONG).await;
    let resources = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let calendar_id = resources.by_path("share-read").unwrap().document_id();
    let old_context = za_owned_calendar_context(&test.server, &keys, calendar_id)
        .await
        .unwrap();
    let original_calendar = old_context.stored.deserialize::<Calendar>().unwrap();
    let mut plain = original_calendar.clone();
    unseal_calendar(&mut plain, &keys, owner_id).unwrap();
    let mut event_ids = Vec::new();
    for (name, suffix) in [("share-ewk.ics", "ewk"), ("share-ck.ics", "ck")] {
        let event = EVENT.replace("share-read-UID", &format!("share-read-{suffix}"));
        client
            .request_with_headers(
                "PUT",
                &format!("{CAL}{name}"),
                [("content-type", "text/calendar")],
                &event,
            )
            .await
            .with_status(StatusCode::CREATED);
        let resources = test
            .server
            .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
            .await
            .unwrap();
        event_ids.push(
            resources
                .by_path(&format!("share-read/{name}"))
                .unwrap()
                .document_id(),
        );
    }
    event_ids.push(
        plant_event(
            test,
            owner_id,
            "share-read",
            "share-legacy.ics",
            &EVENT.replace("share-read-UID", "share-read-legacy"),
            None,
        )
        .await,
    );
    let header = OwnerRingHeader {
        calendar_id,
        domain_id: [0x73; 16],
        rename_change_id: 0,
        boundary_pending: false,
        generations: vec![1, 2],
    };
    let ring = ZaOwnerRing {
        version: 1,
        keys: vec![[0x74; 32], [0x75; 32]],
        converted: false,
        reverting: false,
        grants: Vec::new(),
    };
    let encoded = seal_owner_ring(&header, &ring, owner_id, keys.ewk()).unwrap();
    let shared =
        prepare_shared_calendar(&keys, calendar_id, plain, &encoded, &BTreeMap::new()).unwrap();
    update_calendar(test, owner_id, calendar_id, &old_context.stored, shared).await;
    let context = za_owned_calendar_context(&test.server, &keys, calendar_id)
        .await
        .unwrap();
    let stored = event_archive(test, owner_id, event_ids[1]).await;
    let mut event = stored.deserialize::<CalendarEvent>().unwrap();
    rewrap_event(&mut event, &old_context.domain, &context.domain).unwrap();
    update_event(test, owner_id, event_ids[1], &stored, event).await;

    // Task 5 RED: a COPY must not link a shared-domain row into another calendar.
    let copy_cal = "/dav/cal/share-owner-read%40example.com/share-copy/";
    client
        .request("MKCALENDAR", copy_cal, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    client
        .request_with_headers(
            "COPY",
            &format!("{CAL}share-ck.ics"),
            [("destination", format!("{copy_cal}copied.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::FORBIDDEN);

    for name in ["share-ck.ics", "share-ewk.ics", "share-legacy.ics"] {
        let path = format!("{CAL}{name}");
        let got = client
            .request("GET", &path, "")
            .await
            .with_status(StatusCode::OK);
        let body = got.body.as_ref().unwrap();
        assert!(
            body.contains("share-read-private-summary") && !body.contains("X-ZA-"),
            "{body}"
        );
        let head = client
            .request("HEAD", &path, "")
            .await
            .with_status(StatusCode::OK);
        assert_eq!(got.etag(), head.etag());
    }
    let propfind = "<D:propfind xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:displayname/><C:calendar-data/></D:prop></D:propfind>";
    let before = calendar_reads(owner_id, calendar_id);
    let listing = client
        .request_with_headers("PROPFIND", CAL, [("depth", "1")], propfind)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(
        listing.contains("Shared owner private name")
            && listing.matches("share-read-private-summary").count() == 3,
        "{listing}"
    );
    assert_eq!(
        calendar_reads(owner_id, calendar_id) - before,
        1,
        "one calendar resolution for the entire listing"
    );
    let query = "<C:calendar-query xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:prop><C:calendar-data/></D:prop><C:filter><C:comp-filter name=\"VCALENDAR\"><C:comp-filter name=\"VEVENT\"><C:prop-filter name=\"SUMMARY\"><C:text-match>share-read-private</C:text-match></C:prop-filter></C:comp-filter></C:comp-filter></C:filter></C:calendar-query>";
    let before = calendar_reads(owner_id, calendar_id);
    let queried = client
        .request_with_headers("REPORT", CAL, [("depth", "1")], query)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert_eq!(
        queried.matches("share-read-private-summary").count(),
        3,
        "{queried}"
    );
    assert_eq!(calendar_reads(owner_id, calendar_id) - before, 1);
    let hrefs = ["share-ck.ics", "share-ewk.ics", "share-legacy.ics"]
        .map(|name| format!("<D:href>{CAL}{name}</D:href>"))
        .join("");
    let multiget = format!(
        "<C:calendar-multiget xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:prop><C:calendar-data/></D:prop>{hrefs}</C:calendar-multiget>"
    );
    let before = calendar_reads(owner_id, calendar_id);
    let got = client
        .request("REPORT", CAL, multiget)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert_eq!(
        got.matches("share-read-private-summary").count(),
        3,
        "{got}"
    );
    assert_eq!(calendar_reads(owner_id, calendar_id) - before, 1);
    let fb = "<C:free-busy-query xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><C:time-range start=\"20240101T000000Z\" end=\"20240103T000000Z\"/></C:free-busy-query>";
    let got = client
        .request("REPORT", CAL, fb)
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(
        got.contains("FREEBUSY") && got.contains("20240102T090000Z/20240102T100000Z"),
        "{got}"
    );

    // Content corruption is one multistatus item; direct GET still fails 500.
    let healthy = event_archive(test, owner_id, event_ids[1]).await;
    let mut corrupt = healthy.deserialize::<CalendarEvent>().unwrap();
    let key = corrupt.data.event.components[0]
        .entries
        .iter_mut()
        .find(|entry| groupware::calendar::seal::tree::is_carrier(entry, "X-ZA-KEY"))
        .unwrap();
    let calcard::icalendar::ICalendarValue::Text(encoded) = &mut key.values[0] else {
        panic!()
    };
    let mut bytes = STANDARD.decode(&*encoded).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    *encoded = STANDARD.encode(bytes);
    update_event(test, owner_id, event_ids[1], &healthy, corrupt).await;
    client
        .request("GET", &format!("{CAL}share-ck.ics"), "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    let listing = client
        .request_with_headers("PROPFIND", CAL, [("depth", "1")], propfind)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(listing.contains("500 Internal Server Error"), "{listing}");
    assert_eq!(
        listing.matches("share-read-private-summary").count(),
        2,
        "{listing}"
    );
    let corrupt = event_archive(test, owner_id, event_ids[1]).await;
    update_event(
        test,
        owner_id,
        event_ids[1],
        &corrupt,
        healthy.deserialize::<CalendarEvent>().unwrap(),
    )
    .await;

    // The same envelopes remain readable while rollback changes the fresh owner journal.
    let mut reverting = context.stored.deserialize::<Calendar>().unwrap();
    let mut parts = reverting.preferences[0]
        .name
        .strip_prefix("$za$")
        .unwrap()
        .split('|')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut ring = ring.clone();
    ring.reverting = true;
    parts[0] = STANDARD.encode(seal_owner_ring(&header, &ring, owner_id, keys.ewk()).unwrap());
    reverting.preferences[0].name = format!("$za${}", parts.join("|"));
    update_calendar(test, owner_id, calendar_id, &context.stored, reverting).await;
    for name in ["share-ck.ics", "share-ewk.ics", "share-legacy.ics"] {
        let body = client
            .request("GET", &format!("{CAL}{name}"), "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap();
        assert!(body.contains("share-read-private-summary"), "{body}");
    }
    let reverting_context = za_owned_calendar_context(&test.server, &keys, calendar_id)
        .await
        .unwrap();

    // Restore the fixture before upstream deletion: later tasks wire CK writers.
    let stored = event_archive(test, owner_id, event_ids[1]).await;
    let mut event = stored.deserialize::<CalendarEvent>().unwrap();
    rewrap_event(&mut event, &context.domain, &old_context.domain).unwrap();
    update_event(test, owner_id, event_ids[1], &stored, event).await;
    update_calendar(
        test,
        owner_id,
        calendar_id,
        &reverting_context.stored,
        original_calendar,
    )
    .await;
    client
        .request("DELETE", CAL, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .request("DELETE", copy_cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
    conversion_test(test).await;
    conversion_contract_tests(test).await;
    copy_move_test(test).await;
}

/// Install a converted shared form through the ordinary indexed writer. Production
/// domain constructors remain private; all test CKs come from the fixture utility.
pub(super) async fn stage_shared_calendar(
    test: &TestServer,
    owner: u32,
    calendar_id: u32,
    reader: Option<u32>,
) {
    stage_calendar(test, owner, calendar_id, reader, true).await;
}
pub(super) async fn stage_converting_calendar(test: &TestServer, owner: u32, calendar_id: u32) {
    stage_calendar(test, owner, calendar_id, None, false).await;
}
async fn stage_calendar(
    test: &TestServer,
    owner: u32,
    calendar_id: u32,
    reader: Option<u32>,
    converted: bool,
) {
    use groupware::calendar::{
        seal::calendar_view_in_domain,
        sharing::format::{GrantRingHeader, ZaGrant, seal_grant_ring},
    };
    use types::acl::{Acl, AclGrant};
    let keys = za_unlock(test, owner, STRONG).await;
    let context = za_owned_calendar_context(&test.server, &keys, calendar_id)
        .await
        .unwrap();
    let mut plain = calendar_view_in_domain(&context.stored, &context.domain, owner)
        .unwrap()
        .deserialize::<Calendar>()
        .unwrap();
    plain.preferences.retain(|entry| entry.account_id == owner);
    plain.acls.clear();
    let domain = [calendar_id as u8; 16];
    let ck = vault::keys::Secret::from_bytes([calendar_id as u8; 32]);
    let mut grants = Vec::new();
    let mut encoded_grants = BTreeMap::new();
    if let Some(reader) = reader {
        let reader_keys = za_unlock(test, reader, STRONG).await;
        let public =
            vault::keys::derive_public_key(test.server.za_private_key(&reader_keys).await.unwrap());
        let mut preference = plain.preferences[0].clone();
        preference.account_id = reader;
        preference.flags = 0;
        plain.preferences.push(preference);
        plain.acls.push(AclGrant {
            account_id: reader,
            grants: [
                Acl::Read,
                Acl::ReadItems,
                Acl::Modify,
                Acl::AddItems,
                Acl::ModifyItems,
                Acl::RemoveItems,
            ]
            .into_iter()
            .collect(),
        });
        grants.push(ZaGrant {
            account_id: reader,
            address: "share-move-reader@example.com".into(),
            public_key: public,
            level: 2,
            granted_at: 1,
            change_id: 1,
        });
        encoded_grants.insert(
            reader,
            seal_grant_ring(
                &GrantRingHeader {
                    calendar_id,
                    domain_id: domain,
                    grantee_id: reader,
                    grant_change_id: 1,
                    rename_change_id: 0,
                    boundary_pending: false,
                    generations: vec![1],
                },
                &BTreeMap::from([(1, ck)]),
                owner,
                &public,
            )
            .unwrap(),
        );
    }
    let encoded = seal_owner_ring(
        &OwnerRingHeader {
            calendar_id,
            domain_id: domain,
            rename_change_id: 0,
            boundary_pending: false,
            generations: vec![1],
        },
        &ZaOwnerRing {
            version: 1,
            keys: vec![[calendar_id as u8; 32]],
            converted,
            reverting: false,
            grants,
        },
        owner,
        keys.ewk(),
    )
    .unwrap();
    let calendar =
        prepare_shared_calendar(&keys, calendar_id, plain, &encoded, &encoded_grants).unwrap();
    update_calendar(test, owner, calendar_id, &context.stored, calendar).await;
}

async fn resource_id(test: &TestServer, owner: u32, path: &str) -> u32 {
    test.server
        .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path(path)
        .unwrap()
        .document_id()
}
fn payload(mut event: CalendarEvent) -> CalendarEvent {
    event.names.clear();
    event.modified = 0;
    for component in &mut event.data.event.components {
        component
            .entries
            .retain(|entry| !groupware::calendar::seal::tree::is_carrier(entry, "X-ZA-KEY"));
    }
    event
}

async fn move_queue_state(
    test: &TestServer,
    owner: u32,
    reader: u32,
) -> (Vec<u32>, Vec<u32>, Vec<u32>, Vec<u32>, usize, usize) {
    use crate::utils::za::{inbox_ids, itip_tasks_for, pending_ids};
    (
        pending_ids(test, owner).await,
        inbox_ids(test, owner).await,
        pending_ids(test, reader).await,
        inbox_ids(test, reader).await,
        itip_tasks_for(test, owner).await,
        itip_tasks_for(test, reader).await,
    )
}

async fn copy_move_test(test: &mut TestServer) {
    use crate::utils::za::{hold_task_consumer, inbox_ids};
    use common::za::test_hooks;
    const USER: &str = "share-moves@example.com";
    const READER: &str = "share-move-reader@example.com";
    const HOME: &str = "/dav/cal/share-moves%40example.com/";
    let owner = test
        .account("admin@example.com")
        .create_key_user_account(USER, STRONG, "Move owner", &[], user_permissions())
        .await;
    let owner_id = owner.id().document_id();
    test.insert_account(owner);
    let reader = test
        .account("admin@example.com")
        .create_key_user_account(READER, STRONG, "Move reader", &[], user_permissions())
        .await;
    let reader_id = reader.id().document_id();
    test.insert_account(reader);
    let client = DummyWebDavClient::new(owner_id, USER, STRONG, USER);
    let mut ids = Vec::new();
    for slug in ["a", "b", "c", "d"] {
        client
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
        ids.push(resource_id(test, owner_id, slug).await);
    }
    stage_shared_calendar(test, owner_id, ids[2], Some(reader_id)).await;
    stage_shared_calendar(test, owner_id, ids[3], Some(reader_id)).await;
    let keys = za_unlock(test, owner_id, STRONG).await;
    let put_body = |uid: &str| {
        EVENT
            .replace("share-read-UID", uid)
            .replace("20240102", "20990102")
    };
    let put = |path: String, body: String| {
        let client = &client;
        async move {
            client
                .request_with_headers("PUT", &path, [("content-type", "text/calendar")], body)
                .await
                .with_status(StatusCode::CREATED);
        }
    };
    put(format!("{HOME}a/sealed.ics"), put_body("move-sealed")).await;
    let event_id = resource_id(test, owner_id, "a/sealed.ics").await;
    let original = event_archive(test, owner_id, event_id)
        .await
        .deserialize::<CalendarEvent>()
        .unwrap();
    client
        .request_with_headers(
            "COPY",
            &format!("{HOME}a/sealed.ics"),
            [("destination", format!("{HOME}c/forbidden.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}a/sealed.ics"),
            [("destination", format!("{HOME}c/sealed.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    let moved = event_archive(test, owner_id, event_id)
        .await
        .deserialize::<CalendarEvent>()
        .unwrap();
    assert_eq!(moved.names.len(), 1);
    assert_eq!(moved.names[0].parent_id, ids[2]);
    assert_eq!(payload(moved.clone()), payload(original));
    assert!(
        client
            .request("GET", &format!("{HOME}c/sealed.ics"), "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap()
            .contains("share-read-private-summary")
    );
    for (method, from, to) in [
        ("COPY", "c/sealed.ics", "b/copy.ics"),
        ("COPY", "c/", "copy/"),
        ("MOVE", "c/", "d/"),
    ] {
        client
            .request_with_headers(
                method,
                &format!("{HOME}{from}"),
                [("destination", format!("{HOME}{to}").as_str())],
                "",
            )
            .await
            .with_status(StatusCode::FORBIDDEN);
    }
    // Same-domain event rename retains the accepted sealed payload.
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}c/sealed.ics"),
            [("destination", format!("{HOME}c/renamed.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        payload(
            event_archive(test, owner_id, event_id)
                .await
                .deserialize::<CalendarEvent>()
                .unwrap()
        ),
        payload(moved)
    );
    // CK-to-CK and CK-to-EWK preserve ciphertext and the original DEK.
    for (from, to) in [
        ("c/renamed.ics", "d/sealed.ics"),
        ("d/sealed.ics", "b/sealed.ics"),
    ] {
        let before = event_archive(test, owner_id, event_id)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap();
        client
            .request_with_headers(
                "MOVE",
                &format!("{HOME}{from}"),
                [("destination", format!("{HOME}{to}").as_str())],
                "",
            )
            .await
            .with_status(StatusCode::CREATED);
        assert_eq!(
            payload(
                event_archive(test, owner_id, event_id)
                    .await
                    .deserialize::<CalendarEvent>()
                    .unwrap()
            ),
            payload(before)
        );
    }
    // Legacy plaintext has a distinct full-seal path, with the stored event CAS.
    let legacy = plant_event(
        test,
        owner_id,
        "a",
        "legacy.ics",
        &put_body("move-legacy"),
        None,
    )
    .await;
    println!("Task5 consumer hold owner={owner_id} reader={reader_id}");
    test.server.notify_task_queue();
    println!("Task5 legacy event id={legacy}");
    let held = hold_task_consumer(test).await;
    let before = move_queue_state(test, owner_id, reader_id).await;
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}a/legacy.ics"),
            [("destination", format!("{HOME}c/legacy.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    let stored = event_archive(test, owner_id, legacy).await;
    let event = stored.deserialize::<CalendarEvent>().unwrap();
    assert_eq!(event.names.len(), 1);
    assert_eq!(event.names[0].parent_id, ids[2]);
    let text = event.data.event.to_string().replace("\r\n ", "");
    assert!(!text.contains("share-read-private-summary"));
    let envelope = STANDARD
        .decode(
            text.lines()
                .find_map(|line| line.strip_prefix("X-ZA-KEY:"))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(envelope[1], 3);
    assert!(
        client
            .request("GET", &format!("{HOME}c/legacy.ics"), "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap()
            .contains("share-read-private-summary")
    );
    assert_eq!(move_queue_state(test, owner_id, reader_id).await, before);
    drop(held);
    // A pre-conversion COPY produces two names under the EWK; moving either into CK is refused.
    put(format!("{HOME}a/linked.ics"), put_body("move-linked")).await;
    client
        .request_with_headers(
            "COPY",
            &format!("{HOME}a/linked.ics"),
            [("destination", format!("{HOME}b/linked.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    let linked = resource_id(test, owner_id, "a/linked.ics").await;
    let before = event_archive(test, owner_id, linked).await;
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}a/linked.ics"),
            [("destination", format!("{HOME}c/linked.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    assert_eq!(
        event_archive(test, owner_id, linked).await.version,
        before.version
    );
    // Malformed carriers are corruption, not plaintext to reseal.
    let bad = plant_event(
        test,
        owner_id,
        "a",
        "bad.ics",
        &put_body("move-bad").replace("END:VCALENDAR", "X-ZA-KEY:bad\r\nEND:VCALENDAR"),
        None,
    )
    .await;
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}a/bad.ics"),
            [("destination", format!("{HOME}c/bad.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        event_archive(test, owner_id, bad)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap()
            .names[0]
            .parent_id,
        ids[0]
    );
    // A real future scheduling overwrite: returned outer Option is observed even though discarded.
    let scheduled = super::fanout::invite("move-overwritten", "20990102T090000Z", &[READER])
        .replace("key1@example.com", USER);
    put(format!("{HOME}c/overwrite.ics"), scheduled).await;
    put(
        format!("{HOME}a/overwrite-source.ics"),
        put_body("move-overwrite-source"),
    )
    .await;
    let _drain = super::drain::DrainOn::new();
    assert_eq!(crate::utils::za::trigger_drain(READER, STRONG).await, 207);
    let active_copy = crate::utils::za::copy_href(test, reader_id, "move-overwritten").await;
    let recipient = DummyWebDavClient::new(reader_id, READER, STRONG, READER);
    let active_before = recipient
        .request("GET", &active_copy, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    println!(
        "Task5 imported reader event id={}",
        resource_id(test, reader_id, &super::drain::relative(&active_copy)).await
    );
    drop(_drain);
    let target = resource_id(test, owner_id, "c/overwrite.ics").await;
    println!("Task5 consumer hold owner={owner_id} reader={reader_id}");
    test.server.notify_task_queue();
    let held = hold_task_consumer(test).await;
    let before = move_queue_state(test, owner_id, reader_id).await;
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}a/overwrite-source.ics"),
            [("destination", format!("{HOME}c/overwrite.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let overwrite_observation = test_hooks::take_move_overwrite_message(owner_id, target);
    assert_eq!(move_queue_state(test, owner_id, reader_id).await, before);
    drop(held);
    assert_eq!(
        recipient
            .request("GET", &active_copy, "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap(),
        active_before,
        "overwritten attendee copy must stay active"
    );
    println!(
        "Task5 overwrite observation={overwrite_observation:?}; counts unchanged; attendee copy active"
    );
    assert_eq!(
        overwrite_observation,
        Some(false),
        "MOVE overwrite must not build a CANCEL"
    );
    // A reverting calendar still reads old CK ciphertext, even though new
    // writes use EWK. Moving that ciphertext out must open its original CK.
    let context = za_owned_calendar_context(&test.server, &keys, ids[2])
        .await
        .unwrap();
    let mut calendar = context.stored.deserialize::<Calendar>().unwrap();
    let mut parts = calendar.preferences[0]
        .name
        .strip_prefix("$za$")
        .unwrap()
        .split('|')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (header, mut ring) = groupware::calendar::sharing::format::open_owner_ring(
        &STANDARD.decode(&parts[0]).unwrap(),
        owner_id,
        ids[2],
        keys.ewk(),
    )
    .unwrap();
    ring.reverting = true;
    parts[0] = STANDARD.encode(seal_owner_ring(&header, &ring, owner_id, keys.ewk()).unwrap());
    calendar.preferences[0].name = format!("$za${}", parts.join("|"));
    update_calendar(test, owner_id, ids[2], &context.stored, calendar).await;
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}b/sealed.ics"),
            [("destination", format!("{HOME}c/reverting-in.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    let inbound = event_archive(test, owner_id, event_id)
        .await
        .deserialize::<CalendarEvent>()
        .unwrap();
    let text = inbound.data.event.to_string().replace("\r\n ", "");
    assert_eq!(
        STANDARD
            .decode(
                text.lines()
                    .find_map(|line| line.strip_prefix("X-ZA-KEY:"))
                    .unwrap()
            )
            .unwrap()[1],
        1,
        "reverting destination must use EWK"
    );
    client
        .request("GET", &format!("{HOME}c/reverting-in.ics"), "")
        .await
        .with_status(StatusCode::OK);
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}c/legacy.ics"),
            [("destination", format!("{HOME}a/reverting-out.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    client
        .request("GET", &format!("{HOME}a/reverting-out.ics"), "")
        .await
        .with_status(StatusCode::OK);
    println!("Task5 reverting source CK -> EWK and reverting destination passed");
    // Ordinary-account control: upstream MOVE also leaves scheduling tasks and recipient inbox unchanged.
    const PLAIN: &str = "share-move-plain@example.com";
    let plain = test
        .account("admin@example.com")
        .create_user_account(PLAIN, STRONG, "Move plain", &[], user_permissions())
        .await;
    let plain_id = plain.id().document_id();
    test.insert_account(plain);
    let plain_client = DummyWebDavClient::new(plain_id, PLAIN, STRONG, PLAIN);
    let plain_home = "/dav/cal/share-move-plain%40example.com/";
    for slug in ["a", "b"] {
        plain_client
            .request("MKCALENDAR", &format!("{plain_home}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
    }
    println!("Task5 consumer hold owner={owner_id} reader={reader_id}");
    test.server.notify_task_queue();
    plain_client
        .request_with_headers(
            "PUT",
            &format!("{plain_home}b/target.ics"),
            [("content-type", "text/calendar")],
            super::fanout::invite("plain-overwritten", "20990102T090000Z", &[READER])
                .replace("key1@example.com", PLAIN),
        )
        .await
        .with_status(StatusCode::CREATED);
    plain_client
        .request_with_headers(
            "PUT",
            &format!("{plain_home}a/source.ics"),
            [("content-type", "text/calendar")],
            put_body("plain-source"),
        )
        .await
        .with_status(StatusCode::CREATED);
    crate::utils::za::wait_for_delivery(test).await;
    let held = hold_task_consumer(test).await;
    let before = move_queue_state(test, plain_id, reader_id).await;
    plain_client
        .request_with_headers(
            "MOVE",
            &format!("{plain_home}a/source.ics"),
            [("destination", format!("{plain_home}b/target.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(move_queue_state(test, plain_id, reader_id).await, before);
    drop(held);
    crate::utils::za::wait_for_delivery(test).await;
    for slug in ["a", "b", "default"] {
        plain_client
            .request_with_headers(
                "DELETE",
                &format!("{plain_home}{slug}/"),
                [super::fanout::NO_REPLY],
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    // Shared collection rename is a two-step ring-only write with the actual commit boundary.
    let before = za_owned_calendar_context(&test.server, &keys, ids[2])
        .await
        .unwrap();
    client
        .request_with_headers(
            "MOVE",
            &format!("{HOME}c/"),
            [("destination", format!("{HOME}renamed/").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    let after = za_owned_calendar_context(&test.server, &keys, ids[2])
        .await
        .unwrap();
    assert!(!after.boundary.unwrap().pending);
    assert!(Some(after.boundary.unwrap().rename_change_id) > before.stored.version.change_id());
    for (old, new) in before
        .stored
        .deserialize::<Calendar>()
        .unwrap()
        .preferences
        .iter()
        .zip(
            after
                .stored
                .deserialize::<Calendar>()
                .unwrap()
                .preferences
                .iter(),
        )
    {
        assert_eq!(
            old.name.split('|').skip(1).collect::<Vec<_>>(),
            new.name.split('|').skip(1).collect::<Vec<_>>()
        );
    }
    // Delete fixtures without scheduling side effects and remove their private inbox/pins.
    for slug in ["a", "b", "renamed", "d"] {
        client
            .request_with_headers(
                "DELETE",
                &format!("{HOME}{slug}/"),
                [super::fanout::NO_REPLY],
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    recipient
        .request_with_headers(
            "DELETE",
            "/dav/cal/share-move-reader%40example.com/default/",
            [super::fanout::NO_REPLY],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    super::fanout::Party::new(test, READER)
        .delete_items(inbox_ids(test, reader_id).await)
        .await;
    crate::utils::za::set_raw_pins(test, owner_id, None).await;
    crate::utils::za::set_raw_pins(test, reader_id, None).await;
    test.wait_for_tasks().await;
}

// Catches a missing conversion, content reseal, lost tokens or metadata drift.
async fn conversion_test(test: &mut TestServer) {
    use crate::utils::za::plant_sealed_event;
    use groupware::calendar::seal::TokenPairs;
    use groupware::calendar::{
        seal::{event_pairs_in_domain, unseal_event_in_domain},
        sharing::convert::{ZaShareGuards, za_convert_calendar},
    };
    println!("Running direct first-grant conversion tests...");
    let address = "share-converter@example.com";
    let owner = test
        .account("admin@example.com")
        .create_key_user_account(address, STRONG, "Converter", &[], user_permissions())
        .await;
    let owner_id = owner.id().document_id();
    test.insert_account(owner);
    let client = DummyWebDavClient::new(owner_id, address, STRONG, address);
    let path = "/dav/cal/share-converter%40example.com/converter/";
    client
        .request("MKCALENDAR", path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let calendar_id = test
        .server
        .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("converter")
        .unwrap()
        .document_id();
    let mut pairs = TokenPairs::default();
    pairs.insert("attendee@example.org", [0x51; 16]);
    let sealed_id = plant_sealed_event(
        test,
        owner_id,
        "converter",
        "sealed.ics",
        &EVENT.replace("share-read-UID", "converter-sealed"),
        STRONG,
        &pairs,
    )
    .await;
    let plain_id = plant_event(
        test,
        owner_id,
        "converter",
        "plain.ics",
        &EVENT.replace("share-read-UID", "converter-plain"),
        None,
    )
    .await;
    let before = event_archive(test, owner_id, sealed_id)
        .await
        .deserialize::<CalendarEvent>()
        .unwrap();
    let keys = za_unlock(test, owner_id, STRONG).await;
    let guards = ZaShareGuards {
        owner_id,
        verified_generation: keys.generation,
        vault_cas: test
            .server
            .za_vault_record(owner_id)
            .await
            .unwrap()
            .unwrap()
            .cas,
        pin_assertion: None,
    };
    let counts = za_convert_calendar(&test.server, &keys, calendar_id, &guards)
        .await
        .unwrap();
    assert_eq!(counts.rewrapped, 1, "sealed event must be rewrapped");
    assert_eq!(counts.sealed, 1, "plaintext event must be sealed");
    let context = za_owned_calendar_context(&test.server, &keys, calendar_id)
        .await
        .unwrap();
    assert!(context.domain.domain_id().is_some());
    let calendar = context.stored.deserialize::<Calendar>().unwrap();
    assert!(calendar.acls.is_empty());
    let mut after = event_archive(test, owner_id, sealed_id)
        .await
        .deserialize::<CalendarEvent>()
        .unwrap();
    assert_eq!(payload(after.clone()), payload(before.clone()));
    assert_eq!(after.created, before.created);
    assert_eq!(after.modified, before.modified);
    assert!(
        event_pairs_in_domain(
            &event_archive(test, owner_id, sealed_id).await,
            &context.domain
        )
        .unwrap()
        .unwrap()
        .matches("attendee@example.org", &[0x51; 16])
    );
    for id in [sealed_id, plain_id] {
        after = event_archive(test, owner_id, id)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap();
        unseal_event_in_domain(&mut after, &context.domain).unwrap();
        assert!(
            after
                .data
                .event
                .to_string()
                .contains("share-read-private-summary")
        );
    }
    let resumed = za_convert_calendar(&test.server, &keys, calendar_id, &guards)
        .await
        .unwrap();
    assert_eq!(resumed.rewrapped, 0);
    assert_eq!(resumed.sealed, 0);
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    // Provisioning may create defaults when the first collection is requested.
    let default_status = client
        .request(
            "DELETE",
            "/dav/cal/share-converter%40example.com/default/",
            "",
        )
        .await
        .status;
    assert!(matches!(
        default_status,
        StatusCode::NO_CONTENT | StatusCode::NOT_FOUND
    ));
    test.wait_for_tasks().await;
}

struct ConversionHooksReset;
impl Drop for ConversionHooksReset {
    fn drop(&mut self) {
        use common::za::test_hooks::*;
        set_share_precheck_pause(None);
        set_share_rewrap_pause(None);
        set_share_rewrap_fail_once(None);
        set_share_init_assert_fail(None);
        set_share_rewrap_assert_fail(None);
        set_share_revert_assert_fail(None);
        set_share_rollback_assert_fail(None);
        set_share_restore_assert_fail(None);
    }
}
async fn conversion_guards(
    test: &TestServer,
    keys: &vault::session::SessionKeys,
) -> groupware::calendar::sharing::convert::ZaShareGuards {
    groupware::calendar::sharing::convert::ZaShareGuards {
        owner_id: keys.account_id,
        verified_generation: keys.generation,
        vault_cas: test
            .server
            .za_vault_record(keys.account_id)
            .await
            .unwrap()
            .unwrap()
            .cas,
        pin_assertion: None,
    }
}
async fn conversion_ring(
    test: &TestServer,
    keys: &vault::session::SessionKeys,
    id: u32,
) -> Option<(OwnerRingHeader, ZaOwnerRing)> {
    let context = za_owned_calendar_context(&test.server, keys, id)
        .await
        .unwrap();
    let calendar = context.stored.deserialize::<Calendar>().unwrap();
    assert!(
        calendar.acls.is_empty(),
        "conversion must never publish an ACL"
    );
    assert_eq!(calendar.preferences.len(), 1);
    if context.domain.domain_id().is_none() {
        return None;
    }
    let encoded = calendar.preferences[0]
        .name
        .strip_prefix("$za$")
        .unwrap()
        .split('|')
        .next()
        .unwrap();
    let result = groupware::calendar::sharing::format::open_owner_ring(
        &STANDARD.decode(encoded).unwrap(),
        keys.account_id,
        id,
        keys.ewk(),
    )
    .unwrap();
    assert!(!result.1.converted);
    assert!(
        result.1.grants.is_empty(),
        "conversion must never publish a ledger or grant"
    );
    Some(result)
}
fn conversion_conflict(
    result: Result<
        groupware::calendar::sharing::convert::ZaConversionCounts,
        groupware::calendar::sharing::convert::ZaShareError,
    >,
    rewrapped: usize,
    sealed: usize,
    progress: bool,
) {
    use groupware::calendar::sharing::convert::ZaShareError;
    match result {
        Err(ZaShareError::ConversionConflict {
            committed,
            progress_changed,
        }) => {
            assert_eq!(committed.rewrapped, rewrapped);
            assert_eq!(committed.sealed, sealed);
            assert_eq!(progress_changed, progress);
        }
        other => panic!("expected opaque conversion conflict, got {other:?}"),
    }
}
async fn conversion_link(test: &TestServer, owner: u32, event: u32, other_parent: u32, add: bool) {
    let stored = event_archive(test, owner, event).await;
    let mut value = stored.deserialize::<CalendarEvent>().unwrap();
    if add {
        value.names.push(common::DavName {
            name: format!("linked-{event}.ics"),
            parent_id: other_parent,
        });
    } else {
        value.names.retain(|name| name.parent_id != other_parent);
    }
    update_event(test, owner, event, &stored, value).await;
    test.server
        .invalidate_local_caches(&[common::ipc::CacheInvalidation::DavResources(owner)])
        .await;
}
async fn conversion_wait(pause: &common::za::test_hooks::Pause) {
    tokio::time::timeout(std::time::Duration::from_secs(10), pause.arrived.notified())
        .await
        .expect("conversion pause not reached");
}

// Each case names a distinct persistence failure: an injected attempt must not
// write, drop its journal, count uncommitted work, or publish an early grant.
async fn conversion_contract_tests(test: &mut TestServer) {
    use crate::utils::za::{HookParked, plant_sealed_event};
    use common::za::test_hooks::*;
    use groupware::calendar::{
        CalendarPreferences,
        seal::{
            TokenPairs, calendar_view_in_domain, event_pairs_in_domain, seal_calendar,
            unseal_event_in_domain,
        },
        sharing::convert::{ZaConversionCounts, ZaShareError, za_convert_calendar},
    };
    use std::sync::Arc;
    use vault::session::CollectionKeyId;
    let _reset = ConversionHooksReset;
    println!("Running conversion assertion, rollback, resume and cache contracts...");
    let address = "share-conversion-contract@example.com";
    let home = "/dav/cal/share-conversion-contract%40example.com/";
    let owner = test
        .account("admin@example.com")
        .create_key_user_account(
            address,
            STRONG,
            "Conversion contract",
            &[],
            user_permissions(),
        )
        .await;
    let owner_id = owner.id().document_id();
    test.insert_account(owner);
    let client = DummyWebDavClient::new(owner_id, address, STRONG, address);
    let mut keys = za_unlock(test, owner_id, STRONG).await;
    let mut calendars = Vec::new();
    async fn make(
        test: &TestServer,
        client: &DummyWebDavClient,
        owner: u32,
        home: &str,
        slug: &str,
    ) -> u32 {
        client
            .request("MKCALENDAR", &format!("{home}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
        test.server
            .fetch_dav_resources(owner, owner, SyncCollection::Calendar)
            .await
            .unwrap()
            .by_path(slug)
            .unwrap()
            .document_id()
    }
    let other = make(test, &client, owner_id, home, "other").await;
    calendars.push("other".to_string());
    for class in ["init", "rewrap", "revert", "rollback", "restore"] {
        let slug = format!("assert-{class}");
        let id = make(test, &client, owner_id, home, &slug).await;
        calendars.push(slug.clone());
        let mut pairs = TokenPairs::default();
        pairs.insert("token@example.org", [0x62; 16]);
        let event_id = plant_sealed_event(
            test,
            owner_id,
            &slug,
            "ewk.ics",
            &EVENT.replace("share-read-UID", &slug),
            STRONG,
            &pairs,
        )
        .await;
        let current = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap();
        let mut original_plain =
            calendar_view_in_domain(&current.stored, &current.domain, owner_id)
                .unwrap()
                .deserialize::<Calendar>()
                .unwrap();
        use types::dead_property::{DeadElementTag, DeadProperty, DeadPropertyTag as Tag};
        original_plain.dead_properties = DeadProperty(vec![
            Tag::ElementStart(DeadElementTag {
                name: "http://apple.com/ns/ical/ calendar-color".into(),
                attrs: None,
            }),
            Tag::Text("#ABCDEF77".into()),
            Tag::ElementEnd,
            Tag::ElementStart(DeadElementTag {
                name: "http://apple.com/ns/ical/ calendar-order".into(),
                attrs: None,
            }),
            Tag::Text("71".into()),
            Tag::ElementEnd,
        ]);
        let original_properties = original_plain.dead_properties.clone();
        seal_calendar(&mut original_plain, &keys, owner_id).unwrap();
        update_calendar(test, owner_id, id, &current.stored, original_plain).await;
        let before_event = event_archive(test, owner_id, event_id).await;
        let before_calendar = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap()
            .stored;
        let failure = Some(ShareAssertFailure {
            owner_id,
            calendar_id: id,
            remaining: 1,
        });
        if class == "init" || class == "rewrap" {
            if class == "init" {
                set_share_init_assert_fail(failure);
            } else {
                set_share_rewrap_assert_fail(failure);
            }
            let guards = conversion_guards(test, &keys).await;
            conversion_conflict(
                za_convert_calendar(&test.server, &keys, id, &guards).await,
                0,
                0,
                false,
            );
            assert_eq!(
                event_archive(test, owner_id, event_id).await.version,
                before_event.version
            );
            let state = conversion_ring(test, &keys, id).await;
            if class == "init" {
                assert!(state.is_none());
                assert_eq!(
                    za_owned_calendar_context(&test.server, &keys, id)
                        .await
                        .unwrap()
                        .stored
                        .version,
                    before_calendar.version
                );
            } else {
                assert!(!state.unwrap().1.reverting);
            }
            let guards = conversion_guards(test, &keys).await;
            let counts = za_convert_calendar(&test.server, &keys, id, &guards)
                .await
                .unwrap();
            assert_eq!(
                counts,
                ZaConversionCounts {
                    rewrapped: 1,
                    sealed: 0
                }
            );
            let domain = za_owned_calendar_context(&test.server, &keys, id)
                .await
                .unwrap();
            assert!(
                event_pairs_in_domain(
                    &event_archive(test, owner_id, event_id).await,
                    &domain.domain
                )
                .unwrap()
                .unwrap()
                .matches("token@example.org", &[0x62; 16])
            );
            continue;
        }
        // Force the real unfenced precheck window, and create a CK event after
        // phase 1 through the ordinary owner writer before rollback starts.
        let precheck = Arc::new(Pause::new(owner_id));
        let rewrap = Arc::new(Pause::new(owner_id));
        set_share_precheck_pause(Some(precheck.clone()));
        set_share_rewrap_pause(Some(rewrap.clone()));
        let server = test.server.clone();
        let resident = keys.clone();
        let guards = conversion_guards(test, &keys).await;
        let handle =
            tokio::spawn(async move { za_convert_calendar(&server, &resident, id, &guards).await });
        conversion_wait(&precheck).await;
        conversion_link(test, owner_id, event_id, other, true).await;
        precheck.release.notify_one();
        conversion_wait(&rewrap).await;
        let first = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap();
        let first_domain = *first.domain.domain_id().unwrap();
        let first_cache = CollectionKeyId {
            owner_id,
            calendar_id: id,
            domain_id: first_domain,
            generation: 1,
        };
        let first_key = keys.collection_key(&first_cache).unwrap();
        let original = event_archive(test, owner_id, event_id).await;
        client
            .request_with_headers(
                "PUT",
                &format!("{home}{slug}/ck.ics"),
                [("content-type", "text/calendar")],
                &EVENT.replace("share-read-UID", &format!("{slug}-ck")),
            )
            .await
            .with_status(StatusCode::CREATED);
        let ck_id = test
            .server
            .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
            .await
            .unwrap()
            .by_path(&format!("{slug}/ck.ics"))
            .unwrap()
            .document_id();
        let ck_before = event_archive(test, owner_id, ck_id).await;
        match class {
            "revert" => set_share_revert_assert_fail(failure),
            "rollback" => set_share_rollback_assert_fail(failure),
            _ => set_share_restore_assert_fail(failure),
        }
        rewrap.release.notify_one();
        conversion_conflict(
            handle.await.unwrap(),
            usize::from(class == "restore"),
            0,
            false,
        );
        let state = conversion_ring(test, &keys, id).await.unwrap();
        assert_eq!(state.0.domain_id, first_domain);
        assert_eq!(state.1.reverting, class != "revert");
        assert_eq!(
            event_archive(test, owner_id, event_id).await.version,
            original.version,
            "complete walk must precede rewrap"
        );
        let ck_after = event_archive(test, owner_id, ck_id).await;
        if class != "restore" {
            assert_eq!(
                ck_after.version, ck_before.version,
                "injected rollback attempt wrote nothing"
            );
        } else {
            let mut event = ck_after.deserialize::<CalendarEvent>().unwrap();
            groupware::calendar::seal::unseal_event(&mut event, &keys, owner_id).unwrap();
            assert_eq!(
                payload(ck_after.deserialize::<CalendarEvent>().unwrap()),
                payload(ck_before.deserialize::<CalendarEvent>().unwrap())
            );
        }
        if class == "revert" {
            let guards = conversion_guards(test, &keys).await;
            assert!(matches!(
                za_convert_calendar(&test.server, &keys, id, &guards).await,
                Err(ZaShareError::LinkedEvents)
            ));
            assert!(conversion_ring(test, &keys, id).await.is_none());
            let restored = za_owned_calendar_context(&test.server, &keys, id)
                .await
                .unwrap();
            assert_eq!(
                calendar_view_in_domain(&restored.stored, &restored.domain, owner_id)
                    .unwrap()
                    .deserialize::<Calendar>()
                    .unwrap()
                    .dead_properties,
                original_properties
            );
            // Link-preserving writes are ordinary EWK writes again after restore.
            client
                .request_with_headers(
                    "PUT",
                    &format!("{home}{slug}/ewk.ics"),
                    [("content-type", "text/calendar")],
                    &EVENT.replace("share-read-UID", &slug),
                )
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
        conversion_link(test, owner_id, event_id, other, false).await;
        let guards = conversion_guards(test, &keys).await;
        let counts = za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap();
        // T6-N: resumed rollback + new conversion counts both actual writes.
        assert_eq!(counts.rewrapped, if class == "rollback" { 3 } else { 2 });
        assert_eq!(counts.sealed, 0);
        let new = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap();
        let new_domain = *new.domain.domain_id().unwrap();
        assert_ne!(first_domain, new_domain);
        let next_cache = CollectionKeyId {
            domain_id: new_domain,
            ..first_cache
        };
        let next_key = keys.collection_key(&next_cache).unwrap();
        assert!(
            first_key.as_bytes() != next_key.as_bytes(),
            "new domain must publish a fresh CK"
        );
        assert!(
            keys.collection_key(&first_cache).unwrap().as_bytes() == first_key.as_bytes(),
            "old cache identity must retain its key"
        );
        let text = EVENT.replace("share-read-UID", &format!("{slug}-resident"));
        let calcard::Entry::ICalendar(tree) = calcard::Parser::new(&text).entry() else {
            panic!()
        };
        let mut next_alarm = None;
        let mut event = CalendarEvent {
            names: vec![common::DavName {
                name: "resident.ics".into(),
                parent_id: id,
            }],
            data: groupware::calendar::CalendarEventData::new(
                tree,
                calcard::common::timezone::Tz::Floating,
                100,
                &mut next_alarm,
            ),
            size: text.len() as u32,
            ..Default::default()
        };
        groupware::calendar::seal::seal_event_in_domain(&mut event, &new.domain, None).unwrap();
        let resident_id = test
            .server
            .store()
            .assign_document_ids(owner_id, Collection::CalendarEvent, 1)
            .await
            .unwrap();
        let mut batch = BatchBuilder::new();
        new.assert_in(&mut batch);
        event
            .insert(
                test.server
                    .account_info(owner_id)
                    .await
                    .unwrap()
                    .account_tenant_ids(),
                owner_id,
                resident_id,
                next_alarm,
                &mut batch,
            )
            .unwrap();
        test.server.commit_batch(batch).await.unwrap();
        test.server.notify_task_queue();
        test.server
            .invalidate_local_caches(&[common::ipc::CacheInvalidation::DavResources(owner_id)])
            .await;
        let fresh_keys = za_unlock(test, owner_id, STRONG).await;
        let fresh = za_owned_calendar_context(&test.server, &fresh_keys, id)
            .await
            .unwrap();
        let mut event = event_archive(test, owner_id, resident_id)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap();
        unseal_event_in_domain(&mut event, &fresh.domain).unwrap();
        assert!(
            event
                .data
                .event
                .to_string()
                .contains("share-read-private-summary")
        );
    }

    // Repeat every counted write class with both stale vault and absent Account
    // guards in dedicated fixtures, separate from the resident-session oracle.
    for class in ["init", "rewrap", "revert", "rollback", "restore"] {
        let slug = format!("guarded-{class}");
        let id = make(test, &client, owner_id, home, &slug).await;
        calendars.push(slug.clone());
        let event_id = plant_sealed_event(
            test,
            owner_id,
            &slug,
            "ewk.ics",
            &EVENT.replace("share-read-UID", &slug),
            STRONG,
            &TokenPairs::default(),
        )
        .await;
        if class != "init" {
            stage_converting_calendar(test, owner_id, id).await;
        }
        if matches!(class, "revert" | "rollback" | "restore") {
            conversion_link(test, owner_id, event_id, other, true).await;
        }
        if class == "rollback" {
            client
                .request_with_headers(
                    "PUT",
                    &format!("{home}{slug}/ck.ics"),
                    [("content-type", "text/calendar")],
                    &EVENT.replace("share-read-UID", &format!("{slug}-ck")),
                )
                .await
                .with_status(StatusCode::CREATED);
        }
        if matches!(class, "rollback" | "restore") {
            let failure = Some(ShareAssertFailure {
                owner_id,
                calendar_id: id,
                remaining: 1,
            });
            if class == "rollback" {
                set_share_rollback_assert_fail(failure);
            } else {
                set_share_restore_assert_fail(failure);
            }
            let guards = conversion_guards(test, &keys).await;
            conversion_conflict(
                za_convert_calendar(&test.server, &keys, id, &guards).await,
                0,
                0,
                false,
            );
        }
        keys = conversion_guarded_class_loss(test, &keys, id, class).await;
        if matches!(class, "revert" | "rollback" | "restore") {
            conversion_link(test, owner_id, event_id, other, false).await;
        }
        let guards = conversion_guards(test, &keys).await;
        za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap();
    }
    // Unmarked defaults with zero or one preference retain their owner-facing
    // values; actual Apple dead properties remain exact across both domains.
    for empty in [true, false] {
        let slug = if empty {
            "empty-default"
        } else {
            "single-default"
        };
        let id = make(test, &client, owner_id, home, slug).await;
        calendars.push(slug.to_string());
        let context = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap();
        let mut plain = context.stored.deserialize::<Calendar>().unwrap();
        plain.preferences = if empty {
            vec![]
        } else {
            vec![CalendarPreferences {
                account_id: owner_id,
                name: "Plain owner name".into(),
                description: Some("Plain owner description".into()),
                sort_order: 17,
                color: Some("legacy JMAP-only".into()),
                ..Default::default()
            }]
        };
        use types::dead_property::{DeadElementTag, DeadProperty, DeadPropertyTag as Tag};
        plain.dead_properties = DeadProperty(vec![
            Tag::ElementStart(DeadElementTag {
                name: "http://apple.com/ns/ical/ calendar-color".into(),
                attrs: None,
            }),
            Tag::Text("#123456FF".into()),
            Tag::ElementEnd,
            Tag::ElementStart(DeadElementTag {
                name: "http://apple.com/ns/ical/ calendar-order".into(),
                attrs: None,
            }),
            Tag::Text("23".into()),
            Tag::ElementEnd,
        ]);
        let expected = plain.dead_properties.clone();
        update_calendar(test, owner_id, id, &context.stored, plain).await;
        let guards = conversion_guards(test, &keys).await;
        assert_eq!(
            za_convert_calendar(&test.server, &keys, id, &guards)
                .await
                .unwrap(),
            ZaConversionCounts::default()
        );
        let converted = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap();
        let view = calendar_view_in_domain(&converted.stored, &converted.domain, owner_id)
            .unwrap()
            .deserialize::<Calendar>()
            .unwrap();
        assert_eq!(view.dead_properties, expected);
        assert_eq!(
            view.preferences[0].name,
            if empty { "default" } else { "Plain owner name" }
        );
        assert!(
            view.preferences[0].color.is_none(),
            "shared format excludes JMAP personal color"
        );
        if !empty {
            assert_eq!(
                view.preferences[0].description.as_deref(),
                Some("Plain owner description")
            );
            assert_eq!(view.preferences[0].sort_order, 17);
        }
        // Restore a legacy owner field only to exercise a subsequent fresh domain.
        let mut restored = view;
        seal_calendar(&mut restored, &keys, owner_id).unwrap();
        update_calendar(test, owner_id, id, &converted.stored, restored).await;
    }
    // Crash immediately after phase 1 leaves no grants and owner reads work.
    let id = make(test, &client, owner_id, home, "interrupted").await;
    calendars.push("interrupted".to_string());
    let event_id = plant_sealed_event(
        test,
        owner_id,
        "interrupted",
        "ewk.ics",
        &EVENT.replace("share-read-UID", "interrupted"),
        STRONG,
        &TokenPairs::default(),
    )
    .await;
    set_share_rewrap_fail_once(Some(owner_id));
    let guards = conversion_guards(test, &keys).await;
    assert!(
        matches!(za_convert_calendar(&test.server, &keys, id, &guards).await, Err(ZaShareError::Store(error)) if error.matches(trc::EventType::Store(trc::StoreEvent::UnexpectedError)))
    );
    assert!(!conversion_ring(test, &keys, id).await.unwrap().1.reverting);
    let context = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    let mut event = event_archive(test, owner_id, event_id)
        .await
        .deserialize::<CalendarEvent>()
        .unwrap();
    unseal_event_in_domain(&mut event, &context.domain).unwrap();
    assert_eq!(
        za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap()
            .rewrapped,
        1
    );

    // A calendar archive hash change after the precheck must be handed off as
    // opaque progress, including when the supplied vault guard is stale too.
    for stale_vault in [false, true] {
        let slug = if stale_vault {
            "calendar-and-vault"
        } else {
            "calendar-race"
        };
        let id = make(test, &client, owner_id, home, slug).await;
        calendars.push(slug.into());
        let server = test.server.clone();
        let resident = keys.clone();
        let guards = conversion_guards(test, &keys).await;
        let parked = HookParked::start(owner_id, set_share_precheck_pause, async move {
            za_convert_calendar(&server, &resident, id, &guards).await
        })
        .await;
        let current = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap();
        let mut calendar = current.stored.deserialize::<Calendar>().unwrap();
        calendar.supported_components ^= 1;
        update_calendar(test, owner_id, id, &current.stored, calendar).await;
        if stale_vault {
            let vault = test
                .server
                .za_vault_record(owner_id)
                .await
                .unwrap()
                .unwrap();
            let mut next_record = vault.record.clone();
            next_record.revision += 1;
            test.server
                .za_vault_write(owner_id, &next_record, Some(&vault))
                .await
                .unwrap();
        }
        conversion_conflict(parked.finish().await, 0, 0, true);
        assert!(conversion_ring(test, &keys, id).await.is_none());
        keys = za_unlock(test, owner_id, STRONG).await;
        let guards = conversion_guards(test, &keys).await;
        za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap();
    }
    // A cut journal with a raced-in EWK link must refuse both PUT and PROPPATCH
    // while preserving reads, then resume after unlink without starting phase 1 twice.
    let id = make(test, &client, owner_id, home, "linked-cut").await;
    calendars.push("linked-cut".into());
    let event_id = plant_sealed_event(
        test,
        owner_id,
        "linked-cut",
        "ewk.ics",
        &EVENT.replace("share-read-UID", "linked-cut"),
        STRONG,
        &TokenPairs::default(),
    )
    .await;
    let precheck = Arc::new(Pause::new(owner_id));
    set_share_precheck_pause(Some(precheck.clone()));
    set_share_rewrap_fail_once(Some(owner_id));
    let server = test.server.clone();
    let resident = keys.clone();
    let guards = conversion_guards(test, &keys).await;
    let handle =
        tokio::spawn(async move { za_convert_calendar(&server, &resident, id, &guards).await });
    conversion_wait(&precheck).await;
    conversion_link(test, owner_id, event_id, other, true).await;
    precheck.release.notify_one();
    assert!(matches!(handle.await.unwrap(), Err(ZaShareError::Store(_))));
    let old_domain = conversion_ring(test, &keys, id).await.unwrap().0.domain_id;
    for path in [
        format!("{home}linked-cut/ewk.ics"),
        format!("{home}other/linked-{event_id}.ics"),
    ] {
        client
            .request("GET", &path, "")
            .await
            .with_status(StatusCode::OK);
        client
            .request_with_headers(
                "PUT",
                &path,
                [("content-type", "text/calendar")],
                &EVENT.replace("share-read-UID", "linked-cut"),
            )
            .await
            .with_status(StatusCode::CONFLICT);
        client.request("PROPPATCH", &path, "<D:propertyupdate xmlns:D=\"DAV:\" xmlns:X=\"urn:conversion:test\"><D:set><D:prop><X:test>refused</X:test></D:prop></D:set></D:propertyupdate>").await.with_status(StatusCode::CONFLICT);
    }
    conversion_link(test, owner_id, event_id, other, false).await;
    let guards = conversion_guards(test, &keys).await;
    assert_eq!(
        za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap()
            .rewrapped,
        1
    );
    assert_eq!(
        conversion_ring(test, &keys, id).await.unwrap().0.domain_id,
        old_domain
    );

    // Real event assertion losses after one logical 100-row group, including
    // disappearance and simultaneous vault/Account loss, count only the prefix.
    for race in [
        "event-update",
        "event-delete",
        "event-guards",
        "between-cut",
    ] {
        let id = make(test, &client, owner_id, home, race).await;
        calendars.push(race.into());
        let mut ids = Vec::new();
        for index in 0..101 {
            ids.push(
                plant_event(
                    test,
                    owner_id,
                    race,
                    &format!("{index}.ics"),
                    &EVENT.replace("share-read-UID", &format!("{race}-{index}")),
                    None,
                )
                .await,
            );
        }
        let initial = Arc::new(Pause::new(owner_id));
        let between = Arc::new(Pause::new(owner_id));
        set_share_rewrap_pause(Some(initial.clone()));
        let server = test.server.clone();
        let resident = keys.clone();
        let guards = conversion_guards(test, &keys).await;
        let handle =
            tokio::spawn(async move { za_convert_calendar(&server, &resident, id, &guards).await });
        conversion_wait(&initial).await;
        set_share_rewrap_pause(Some(between.clone()));
        initial.release.notify_one();
        conversion_wait(&between).await;
        let context = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap();
        let mut ck_count = 0;
        for event in &ids {
            let stored = event_archive(test, owner_id, *event).await;
            if groupware::calendar::seal::classify_event_storage(
                &stored.deserialize::<CalendarEvent>().unwrap(),
            )
            .unwrap()
                == groupware::calendar::seal::ZaEventStorageKind::Sealed
            {
                ck_count += 1;
            }
        }
        assert_eq!(ck_count, 100, "pause must occur between bounded groups");
        let last = *ids.last().unwrap();
        let mut account_restore = None;
        if race == "event-delete" {
            client
                .request("DELETE", &format!("{home}{race}/100.ics"), "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        } else if race == "between-cut" {
            set_share_rewrap_fail_once(Some(owner_id));
        } else {
            // A concurrent writer seals the last row under the current CK.
            let stored = event_archive(test, owner_id, last).await;
            let mut event = stored.deserialize::<CalendarEvent>().unwrap();
            groupware::calendar::seal::seal_event_in_domain(&mut event, &context.domain, None)
                .unwrap();
            update_event(test, owner_id, last, &stored, event).await;
            if race == "event-guards" {
                let vault = test
                    .server
                    .za_vault_record(owner_id)
                    .await
                    .unwrap()
                    .unwrap();
                let mut record = vault.record.clone();
                record.revision += 1;
                test.server
                    .za_vault_write(owner_id, &record, Some(&vault))
                    .await
                    .unwrap();
                account_restore = Some(conversion_remove_account(test, owner_id).await);
            }
        }
        between.release.notify_one();
        let result = handle.await.unwrap();
        if race == "between-cut" {
            assert!(matches!(result, Err(ZaShareError::Store(_))));
        } else {
            conversion_conflict(result, 0, 100, true);
        }
        if let Some((class, bytes)) = account_restore {
            let mut batch = BatchBuilder::new();
            batch.set(class, bytes);
            test.server.store().write(batch.build_all()).await.unwrap();
        }
        keys = za_unlock(test, owner_id, STRONG).await;
        let guards = conversion_guards(test, &keys).await;
        let counts = za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap();
        assert_eq!(counts.rewrapped, 0);
        assert_eq!(counts.sealed, usize::from(race == "between-cut"));
        conversion_ring(test, &keys, id).await.unwrap();
    }

    // Guard-only losses have unchanged progress and remain opaque. Neither a
    // stale vault CAS, absent Account item nor optional pin assertion can write.
    for lost_guard in ["vault-only", "account-only", "pin-only"] {
        let id = make(test, &client, owner_id, home, lost_guard).await;
        calendars.push(lost_guard.into());
        let server = test.server.clone();
        let resident = keys.clone();
        let mut guards = conversion_guards(test, &keys).await;
        if lost_guard == "pin-only" {
            guards.pin_assertion = Some(store::write::assert::AssertValue::Hash(1));
        }
        let parked = HookParked::start(owner_id, set_share_precheck_pause, async move {
            za_convert_calendar(&server, &resident, id, &guards).await
        })
        .await;
        let mut account_restore = None;
        if lost_guard == "vault-only" {
            let vault = test
                .server
                .za_vault_record(owner_id)
                .await
                .unwrap()
                .unwrap();
            let mut record = vault.record.clone();
            record.revision += 1;
            test.server
                .za_vault_write(owner_id, &record, Some(&vault))
                .await
                .unwrap();
        } else if lost_guard == "account-only" {
            account_restore = Some(conversion_remove_account(test, owner_id).await);
        }
        conversion_conflict(parked.finish().await, 0, 0, false);
        if let Some((class, bytes)) = account_restore {
            let mut batch = BatchBuilder::new();
            batch.set(class, bytes);
            test.server.store().write(batch.build_all()).await.unwrap();
        }
        assert!(conversion_ring(test, &keys, id).await.is_none());
        keys = za_unlock(test, owner_id, STRONG).await;
        let guards = conversion_guards(test, &keys).await;
        za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap();
    }

    // A delayed create or update that read the old calendar retries its actual
    // DAV commit under the real phase-1 ring, rather than leaving an EWK row.
    for updating in [false, true] {
        let slug = if updating {
            "held-update"
        } else {
            "held-create"
        };
        let id = make(test, &client, owner_id, home, slug).await;
        calendars.push(slug.into());
        if updating {
            plant_sealed_event(
                test,
                owner_id,
                slug,
                "held.ics",
                &EVENT.replace("share-read-UID", slug),
                STRONG,
                &TokenPairs::default(),
            )
            .await;
        }
        let before = if updating {
            let event = test
                .server
                .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
                .await
                .unwrap()
                .by_path(&format!("{slug}/held.ics"))
                .unwrap()
                .document_id();
            Some(
                event_archive(test, owner_id, event)
                    .await
                    .deserialize::<CalendarEvent>()
                    .unwrap(),
            )
        } else {
            None
        };
        let request = crate::utils::http::HttpRequest::with_credentials(8899, address, STRONG);
        let path = format!("{home}{slug}/held.ics");
        let text = EVENT
            .replace("share-read-UID", slug)
            .replace("share-read-private-summary", "held conversion write");
        let set = if updating {
            set_update_commit_pause
        } else {
            set_commit_pause
        };
        let held = HookParked::start(owner_id, set, async move {
            request
                .send_full(
                    hyper::Method::PUT,
                    &path,
                    Some(text.into_bytes()),
                    Some("text/calendar"),
                )
                .await
        })
        .await;
        let guards = conversion_guards(test, &keys).await;
        assert_eq!(
            za_convert_calendar(&test.server, &keys, id, &guards)
                .await
                .unwrap()
                .rewrapped,
            usize::from(updating)
        );
        assert_eq!(held.finish().await.status, if updating { 409 } else { 201 });
        let event = test
            .server
            .fetch_dav_resources(owner_id, owner_id, SyncCollection::Calendar)
            .await
            .unwrap()
            .by_path(&format!("{slug}/held.ics"))
            .unwrap()
            .document_id();
        let context = za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap();
        let mut event = event_archive(test, owner_id, event)
            .await
            .deserialize::<CalendarEvent>()
            .unwrap();
        if let Some(before) = before {
            assert_eq!(payload(event.clone()), payload(before.clone()));
            assert_eq!(event.created, before.created);
            assert_eq!(event.modified, before.modified);
        }
        unseal_event_in_domain(&mut event, &context.domain).unwrap();
        if updating {
            assert!(
                !event
                    .data
                    .event
                    .to_string()
                    .contains("held conversion write")
            );
        }
        assert!(event.data.event.to_string().contains(if updating {
            "share-read-private-summary"
        } else {
            "held conversion write"
        }));
    }

    // T6-U: when only the calendar changed, the held UPDATE retries successfully
    // before the converter reads the event, and conversion authenticates/skips it.
    let id = make(test, &client, owner_id, home, "held-calendar-only").await;
    calendars.push("held-calendar-only".into());
    let event_id = plant_sealed_event(
        test,
        owner_id,
        "held-calendar-only",
        "held.ics",
        &EVENT.replace("share-read-UID", "held-calendar-only"),
        STRONG,
        &TokenPairs::default(),
    )
    .await;
    let request = crate::utils::http::HttpRequest::with_credentials(8899, address, STRONG);
    let path = format!("{home}held-calendar-only/held.ics");
    let text = EVENT
        .replace("share-read-UID", "held-calendar-only")
        .replace(
            "share-read-private-summary",
            "calendar-only successful retry",
        );
    let held = HookParked::start(owner_id, set_update_commit_pause, async move {
        request
            .send_full(
                hyper::Method::PUT,
                &path,
                Some(text.into_bytes()),
                Some("text/calendar"),
            )
            .await
    })
    .await;
    let server = test.server.clone();
    let resident = keys.clone();
    let guards = conversion_guards(test, &keys).await;
    let converter = HookParked::start(owner_id, set_share_rewrap_pause, async move {
        za_convert_calendar(&server, &resident, id, &guards).await
    })
    .await;
    assert_eq!(held.finish().await.status, 204);
    assert_eq!(
        converter.finish().await.unwrap(),
        ZaConversionCounts::default()
    );
    let context = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    let mut event = event_archive(test, owner_id, event_id)
        .await
        .deserialize::<CalendarEvent>()
        .unwrap();
    unseal_event_in_domain(&mut event, &context.domain).unwrap();
    assert!(
        event
            .data
            .event
            .to_string()
            .contains("calendar-only successful retry")
    );

    // Calendar absence is progress too: the failed phase-1 write must not
    // resurrect a collection that an ordinary concurrent DELETE removed.
    let id = make(test, &client, owner_id, home, "deleted-calendar").await;
    let server = test.server.clone();
    let resident = keys.clone();
    let guards = conversion_guards(test, &keys).await;
    let converter = HookParked::start(owner_id, set_share_precheck_pause, async move {
        za_convert_calendar(&server, &resident, id, &guards).await
    })
    .await;
    client
        .request("DELETE", &format!("{home}deleted-calendar/"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    conversion_conflict(converter.finish().await, 0, 0, true);
    assert!(
        test.server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                owner_id,
                Collection::Calendar,
                id
            ))
            .await
            .unwrap()
            .is_none()
    );

    // Ring-only journals preserve every generation, key and pending rename flag.
    let id = make(test, &client, owner_id, home, "journal-preservation").await;
    calendars.push("journal-preservation".into());
    let event_id = plant_sealed_event(
        test,
        owner_id,
        "journal-preservation",
        "ewk.ics",
        &EVENT.replace("share-read-UID", "journal-preservation"),
        STRONG,
        &TokenPairs::default(),
    )
    .await;
    let context = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    let plain = calendar_view_in_domain(&context.stored, &context.domain, owner_id)
        .unwrap()
        .deserialize::<Calendar>()
        .unwrap();
    let header = OwnerRingHeader {
        calendar_id: id,
        domain_id: [0x37; 16],
        rename_change_id: 731,
        boundary_pending: true,
        generations: vec![1, 7],
    };
    let ring = ZaOwnerRing {
        version: 1,
        keys: vec![[0x38; 32], [0x39; 32]],
        converted: false,
        reverting: false,
        grants: vec![],
    };
    let encoded = seal_owner_ring(&header, &ring, owner_id, keys.ewk()).unwrap();
    let shared = prepare_shared_calendar(&keys, id, plain, &encoded, &BTreeMap::new()).unwrap();
    update_calendar(test, owner_id, id, &context.stored, shared).await;
    conversion_link(test, owner_id, event_id, other, true).await;
    set_share_restore_assert_fail(Some(ShareAssertFailure {
        owner_id,
        calendar_id: id,
        remaining: 1,
    }));
    let guards = conversion_guards(test, &keys).await;
    conversion_conflict(
        za_convert_calendar(&test.server, &keys, id, &guards).await,
        0,
        0,
        false,
    );
    let (after_header, after_ring) = conversion_ring(test, &keys, id).await.unwrap();
    assert_eq!(after_header, header);
    assert!(after_ring.keys == ring.keys);
    assert!(after_ring.reverting);
    conversion_link(test, owner_id, event_id, other, false).await;
    assert_eq!(
        za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap()
            .rewrapped,
        1
    );

    // Strict storage classification must refuse a partial recognized carrier,
    // while a converted ring dispatch skips all conversion writes entirely.
    let id = make(test, &client, owner_id, home, "partial-carrier").await;
    calendars.push("partial-carrier".into());
    let event = plant_event(
        test,
        owner_id,
        "partial-carrier",
        "bad.ics",
        &EVENT
            .replace("share-read-UID", "partial-carrier")
            .replace("SUMMARY:", "X-ZA-SEALED:malformed\r\nSUMMARY:"),
        None,
    )
    .await;
    let original = event_archive(test, owner_id, event).await;
    let guards = conversion_guards(test, &keys).await;
    assert!(matches!(
        za_convert_calendar(&test.server, &keys, id, &guards).await,
        Err(ZaShareError::Seal(_))
    ));
    assert_eq!(
        event_archive(test, owner_id, event).await.version,
        original.version
    );
    conversion_ring(test, &keys, id).await.unwrap();

    let id = make(test, &client, owner_id, home, "invalid-ck").await;
    calendars.push("invalid-ck".into());
    let event_id = plant_sealed_event(
        test,
        owner_id,
        "invalid-ck",
        "bad.ics",
        &EVENT.replace("share-read-UID", "invalid-ck"),
        STRONG,
        &TokenPairs::default(),
    )
    .await;
    let guards = conversion_guards(test, &keys).await;
    assert_eq!(
        za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap()
            .rewrapped,
        1
    );
    let stored = event_archive(test, owner_id, event_id).await;
    let mut corrupted = stored.deserialize::<CalendarEvent>().unwrap();
    let carrier = corrupted
        .data
        .event
        .components
        .iter_mut()
        .flat_map(|c| &mut c.entries)
        .find(|entry| groupware::calendar::seal::tree::is_carrier(entry, "X-ZA-SEALED"))
        .unwrap();
    let calcard::icalendar::ICalendarValue::Text(encoded) = &mut carrier.values[0] else {
        panic!()
    };
    let mut bytes = STANDARD.decode(&*encoded).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    *encoded = STANDARD.encode(bytes);
    update_event(test, owner_id, event_id, &stored, corrupted).await;
    let before = event_archive(test, owner_id, event_id).await;
    assert!(matches!(
        za_convert_calendar(&test.server, &keys, id, &guards).await,
        Err(ZaShareError::Seal(_))
    ));
    assert_eq!(
        event_archive(test, owner_id, event_id).await.version,
        before.version
    );
    let id = make(test, &client, owner_id, home, "already-converted").await;
    calendars.push("already-converted".into());
    stage_shared_calendar(test, owner_id, id, None).await;
    let before = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap()
        .stored;
    set_share_rewrap_assert_fail(Some(ShareAssertFailure {
        owner_id,
        calendar_id: id,
        remaining: 1,
    }));
    let guards = conversion_guards(test, &keys).await;
    assert_eq!(
        za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap(),
        ZaConversionCounts::default()
    );
    assert_eq!(
        za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap()
            .stored
            .version,
        before.version
    );
    assert!(
        take_share_rewrap_assert_fail(owner_id, id),
        "converted dispatch must not attempt an event write"
    );
    for slug in calendars {
        client
            .request("DELETE", &format!("{home}{slug}/"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    let default_status = client
        .request("DELETE", &format!("{home}default/"), "")
        .await
        .status;
    assert!(matches!(
        default_status,
        StatusCode::NO_CONTENT | StatusCode::NOT_FOUND
    ));
    test.wait_for_tasks().await;
}

pub(super) async fn conversion_remove_account(
    test: &TestServer,
    owner: u32,
) -> (store::write::ValueClass, Vec<u8>) {
    use registry::{schema::prelude::ObjectType, types::EnumImpl};
    use store::write::{RegistryClass, ValueClass};
    let class = ValueClass::Registry(RegistryClass::Item {
        object_id: ObjectType::Account.to_id(),
        item_id: types::id::Id::from(owner).id(),
    });
    let bytes = test
        .server
        .store()
        .get_value::<common::auth::vault::RawValue>(ValueKey::from(class.clone()))
        .await
        .unwrap()
        .unwrap()
        .0;
    let mut batch = BatchBuilder::new();
    batch.clear(class.clone());
    test.server.store().write(batch.build_all()).await.unwrap();
    (class, bytes)
}

// Repeat the exact class from its first-loss journal with both externally
// verified guards now lost. The injected commit shares the real error match;
// changed terminal records cannot be mistaken for a safe calendar-only retry.
async fn conversion_guarded_class_loss(
    test: &TestServer,
    keys: &std::sync::Arc<vault::session::SessionKeys>,
    id: u32,
    class: &str,
) -> std::sync::Arc<vault::session::SessionKeys> {
    use common::za::test_hooks::*;
    use groupware::calendar::sharing::convert::za_convert_calendar;
    let owner = keys.account_id;
    let before = za_owned_calendar_context(&test.server, keys, id)
        .await
        .unwrap()
        .stored;
    let guards = conversion_guards(test, keys).await;
    let vault = test.server.za_vault_record(owner).await.unwrap().unwrap();
    let mut record = vault.record.clone();
    record.revision += 1;
    test.server
        .za_vault_write(owner, &record, Some(&vault))
        .await
        .unwrap();
    let (account_class, account_bytes) = conversion_remove_account(test, owner).await;
    let failure = Some(ShareAssertFailure {
        owner_id: owner,
        calendar_id: id,
        remaining: 1,
    });
    match class {
        "init" => set_share_init_assert_fail(failure),
        "rewrap" => set_share_rewrap_assert_fail(failure),
        "revert" => set_share_revert_assert_fail(failure),
        "rollback" => set_share_rollback_assert_fail(failure),
        "restore" => set_share_restore_assert_fail(failure),
        _ => panic!(),
    }
    let result = za_convert_calendar(&test.server, keys, id, &guards).await;
    let mut batch = BatchBuilder::new();
    batch.set(account_class, account_bytes);
    test.server.store().write(batch.build_all()).await.unwrap();
    conversion_conflict(result, 0, 0, false);
    let fresh = za_unlock(test, owner, STRONG).await;
    assert_eq!(
        za_owned_calendar_context(&test.server, &fresh, id)
            .await
            .unwrap()
            .stored
            .version,
        before.version
    );
    fresh
}

/// Catches missing sharing routes and authentication-before-validation regressions.
async fn sharing_endpoint_tests(test: &mut TestServer) {
    use crate::utils::za::{plant_sealed_event, za_post};
    use serde_json::json;
    use vault::keys::display_fingerprint;
    println!("Running transactional sharing endpoint tests...");
    for (endpoint, extra) in [
        (
            "shares/grant",
            json!({"address":"key2@example.com", "level":"view"}),
        ),
        ("shares/grant", json!({"calendar":"x", "level":"view"})),
        (
            "shares/grant",
            json!({"calendar":"x", "address":"key2@example.com"}),
        ),
        ("shares/revoke", json!({"calendar":"x", "address":null})),
        (
            "shares/revoke",
            json!({"calendar":"x", "address":"mailto:bad@example.com"}),
        ),
        ("shares/revoke", json!({"calendar":"", "grant":1})),
        (
            "shares/grant",
            json!({"calendar":"", "address":"bad", "level":"view"}),
        ),
        ("shares/revoke", json!({"calendar":"x"})),
        ("shares/revoke", json!({"calendar":"x", "grant":null})),
        ("shares/revoke", json!({"calendar":"x", "grant":-1})),
        ("shares/revoke", json!({"calendar":"x", "grant":1.5})),
        (
            "shares/revoke",
            json!({"calendar":"x", "grant":4294967296u64}),
        ),
        (
            "shares/revoke",
            json!({"calendar":"x", "grant":1, "address":"key2@example.com"}),
        ),
    ] {
        let mut request = extra;
        request["username"] = json!("missing-sharing-user@example.com");
        request["password"] = json!("wrong");
        za_post(endpoint, &request).await.expect(400);
    }
    let owner_name = "shares-http-owner@example.com";
    let reader_name = "shares-http-reader@example.com";
    let owner = test
        .account("admin@example.com")
        .create_key_user_account(owner_name, STRONG, "Owner", &[], user_permissions())
        .await;
    let owner_id = owner.id().document_id();
    test.insert_account(owner);
    let reader = test
        .account("admin@example.com")
        .create_key_user_account(reader_name, STRONG, "Reader", &[], user_permissions())
        .await;
    let reader_id = reader.id().document_id();
    test.insert_account(reader);
    let client = DummyWebDavClient::new(owner_id, owner_name, STRONG, owner_name);
    let path = "/dav/cal/shares-http-owner%40example.com/http-sharing/";
    client
        .request("MKCALENDAR", path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    plant_sealed_event(
        test,
        owner_id,
        "http-sharing",
        "sealed.ics",
        &EVENT.replace("share-read-UID", "http-sharing-sealed"),
        STRONG,
        &Default::default(),
    )
    .await;
    plant_event(
        test,
        owner_id,
        "http-sharing",
        "plain.ics",
        &EVENT.replace("share-read-UID", "http-sharing-plain"),
        None,
    )
    .await;
    let request = json!({"username":owner_name,"password":STRONG,"calendar":"http-sharing","address":reader_name,"level":"view"});
    assert_eq!(
        za_post("shares/grant", &request).await.expect(409)["error"],
        "contact not pinned"
    );
    let key = test
        .server
        .za_served_public_key(reader_id)
        .await
        .unwrap()
        .unwrap();
    super::pinning::pin(owner_name, reader_name, &display_fingerprint(&key))
        .await
        .expect(200);
    assert_eq!(
        za_post("shares/grant", &request).await.expect(200),
        json!({"ok":true,"changed":true,"rewrapped":1,"sealed":1})
    );
    assert_eq!(
        za_post("shares/grant", &request).await.expect(200),
        json!({"ok":true,"changed":false,"rewrapped":0,"sealed":0})
    );
    let own_keys = za_unlock(test, owner_id, STRONG).await;
    let raw = za_owned_calendar_context(
        &test.server,
        &own_keys,
        resource_id(test, owner_id, "http-sharing").await,
    )
    .await
    .unwrap()
    .stored
    .deserialize::<Calendar>()
    .unwrap();
    assert_eq!(raw.acls[0].grants.bitmap, 9);
    let listed = za_post("shares", &json!({"username":owner_name,"password":STRONG}))
        .await
        .expect(200);
    let calendar = listed["calendars"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["calendar"] == "http-sharing")
        .unwrap();
    assert_eq!(calendar["shared"], true);
    assert_eq!(calendar["name"], "Shared owner private name");
    assert_eq!(calendar["grants"][0]["grant"], reader_id);
    assert_eq!(calendar["grants"][0]["address"], reader_name);
    assert_eq!(calendar["grants"][0]["state"], "active");
    assert_eq!(calendar["grants"][0]["pin_source"], "confirmed");
    let shared = za_post("shares", &json!({"username":reader_name,"password":STRONG}))
        .await
        .expect(200);
    assert_eq!(shared["shared_with_me"][0]["href"], path);
    assert_eq!(
        shared["shared_with_me"][0]["name"],
        "Shared owner private name"
    );
    sharing_default_and_sort_contracts(test, owner_id, reader_id).await;
    sharing_response_contracts(test, owner_id, reader_id).await;
    sharing_pin_contracts(test, owner_id, reader_id).await;
    sharing_scan_transition_contracts(test, owner_id, reader_id).await;
    sharing_rotation_contracts(test, owner_id, reader_id).await;
    sharing_trace_contracts(test, owner_id, reader_id).await;
    sharing_recovery_contracts(test, owner_id, reader_id).await;
    let mut edit = request.clone();
    edit["level"] = json!("edit");
    assert_eq!(
        za_post("shares/grant", &edit).await.expect(200)["changed"],
        true
    );
    let revoke = json!({"username":owner_name,"password":STRONG,"calendar":"http-sharing","grant":reader_id});
    assert_eq!(
        za_post("shares/revoke", &revoke).await.expect(200),
        json!({"ok":true,"changed":true})
    );
    assert_eq!(
        za_post("shares/revoke", &revoke).await.expect(200),
        json!({"ok":true,"changed":false})
    );
    assert_eq!(za_post("shares/revoke",&json!({"username":owner_name,"password":STRONG,"calendar":"http-sharing","address":"no-sharing-grant@example.com"})).await.expect(200)["changed"],false);
    assert_eq!(
        za_post("shares", &json!({"username":reader_name,"password":STRONG}))
            .await
            .expect(200)["shared_with_me"],
        json!([])
    );
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let _ = client
        .request(
            "DELETE",
            "/dav/cal/shares-http-owner%40example.com/default/",
            "",
        )
        .await;
    test.wait_for_tasks().await;
}

/// Exact pin classifications are read-only, even with an unavailable private wrap.
async fn sharing_pin_contracts(test: &TestServer, owner: u32, reader: u32) {
    use crate::utils::za::*;
    use serde_json::json;
    use vault::{
        keys::display_fingerprint,
        pins::{PinSource, ZaPinRecord},
    };
    const USER: &str = "shares-http-owner@example.com";
    const TARGET: &str = "shares-http-reader@example.com";
    let request = json!({"username":USER,"password":STRONG,"calendar":"http-sharing","address":TARGET,"level":"view"});
    let original_pins = raw_pins(test, owner).await.unwrap();
    let original_wrap = saved_wrap(test, owner).await;
    let original_public = test
        .server
        .za_served_public_key(owner)
        .await
        .unwrap()
        .unwrap();
    let substituted_public = writer_key();
    substitute_public_key(test, owner, &substituted_public, true).await;
    let id = resource_id(test, owner, "http-sharing").await;
    let keys = za_unlock(test, owner, STRONG).await;
    let before = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap()
        .stored;
    for (mark, expected) in [
        ("pinned", "pin record missing"),
        ("legacy", "contact not pinned"),
        ("neither", "vault record missing"),
    ] {
        match mark {
            "pinned" => restore_wrap(test, owner, &original_wrap).await,
            "legacy" => rewrap_legacy(test, owner, STRONG).await,
            _ => restore_wrap(test, owner, &[0; 72]).await,
        }
        let mut wrong_version = ZaPinRecord::new();
        wrong_version.version = 99;
        write_pin_record(test, owner, STRONG, &wrong_version).await;
        let invalid_version = raw_pins(test, owner).await.unwrap();
        let malformed = vault::keys::seal(
            keys.mk(),
            &vault::keys::aad(vault::keys::AAD_PINS, owner),
            &[0; 256],
        );
        for raw in [
            None,
            Some(vec![0x7f; 32]),
            Some(invalid_version),
            Some(malformed),
        ] {
            set_raw_pins(test, owner, raw.as_deref()).await;
            let revision = vault_revision(test, owner).await;
            http::api::shares::test::take_private_outcomes(owner);
            common::za::test_hooks::set_private_key_fail(Some(owner));
            assert_eq!(
                za_post("shares/grant", &request).await.expect(409)["error"],
                expected
            );
            let outcomes = http::api::shares::test::take_private_outcomes(owner);
            assert!(
                !outcomes.is_empty() && outcomes.iter().all(|has_private| !has_private),
                "fresh sharing keys published a lazy private-key outcome"
            );
            assert_eq!(raw_pins(test, owner).await, raw);
            assert_eq!(vault_revision(test, owner).await, revision);
            assert_eq!(
                test.server.za_served_public_key(owner).await.unwrap(),
                Some(substituted_public),
                "grant refusal must not repair the owner public key"
            );
            // This proves no self-check admission, not zero transient mark unwraps.
            assert!(common::za::test_hooks::take_private_key_fail(owner));
            let fresh = za_unlock(test, owner, STRONG).await;
            assert!(fresh.private_key().is_none());
            assert_eq!(
                za_owned_calendar_context(&test.server, &fresh, id)
                    .await
                    .unwrap()
                    .stored
                    .version,
                before.version
            );
        }
    }
    // Usable matching pins need neither the mark nor a private-key self-check.
    restore_wrap(test, owner, &[0; 72]).await;
    for source in [PinSource::FirstUse, PinSource::Confirmed] {
        let mut pins = ZaPinRecord::new();
        pins.set_pin(
            TARGET,
            test.server
                .za_served_public_key(reader)
                .await
                .unwrap()
                .unwrap(),
            source,
            1,
        );
        write_pin_record(test, owner, STRONG, &pins).await;
        common::za::test_hooks::set_private_key_fail(Some(owner));
        assert_eq!(
            za_post("shares/grant", &request).await.expect(200)["changed"],
            false
        );
        assert!(common::za::test_hooks::take_private_key_fail(owner));
    }
    // Revoke has no pin or mark precondition and can rotate using only the ledger.
    set_raw_pins(test, owner, None).await;
    common::za::test_hooks::set_private_key_fail(Some(owner));
    assert_eq!(
        za_post(
            "shares/revoke",
            &json!({"username":USER,"password":STRONG,"calendar":"http-sharing","grant":reader})
        )
        .await
        .expect(200)["changed"],
        true
    );
    assert!(common::za::test_hooks::take_private_key_fail(owner));
    set_raw_pins(test, owner, Some(&original_pins)).await;
    common::za::test_hooks::set_private_key_fail(Some(owner));
    assert_eq!(
        za_post("shares/grant", &request).await.expect(200)["changed"],
        true
    );
    assert!(common::za::test_hooks::take_private_key_fail(owner));
    restore_wrap(test, owner, &original_wrap).await;
    substitute_public_key(test, owner, &original_public, true).await;
    set_raw_pins(test, owner, Some(&original_pins)).await;
    super::pinning::pin(
        USER,
        TARGET,
        &display_fingerprint(
            &test
                .server
                .za_served_public_key(reader)
                .await
                .unwrap()
                .unwrap(),
        ),
    )
    .await
    .expect(200);
    za_post("shares/grant", &request).await.expect(200);
}

async fn sharing_rotation_contracts(test: &mut TestServer, owner: u32, reader: u32) {
    use crate::utils::za::{saved_wrap, substitute_public_key, za_post, za_post_from};
    use groupware::calendar::sharing::format::{open_grant_ring, parse_grant_header};
    use registry::{
        schema::{
            prelude::{ObjectType, Property},
            structs::EmailAlias,
        },
        types::list::List,
    };
    use serde_json::{Value, json};
    use vault::keys::{display_fingerprint, generate_keypair, unwrap_private_key};
    const USER: &str = "shares-http-owner@example.com";
    const READER: &str = "shares-http-reader@example.com";
    const SECOND: &str = "shares-http-second@example.com";
    const ALIAS: &str = "sharing-alias@share-alias.test";
    let second = test
        .account("admin@example.com")
        .create_key_user_account(SECOND, STRONG, "Second reader", &[], user_permissions())
        .await;
    let second_id = second.id().document_id();
    test.insert_account(second);
    let key = test
        .server
        .za_served_public_key(second_id)
        .await
        .unwrap()
        .unwrap();
    super::pinning::pin(USER, SECOND, &display_fingerprint(&key))
        .await
        .expect(200);
    let request = |address: &str, level: &str| json!({"username":USER,"password":STRONG,"calendar":"http-sharing","address":address,"level":level});
    let list = |name: &str| json!({"username":name,"password":STRONG});
    let id = resource_id(test, owner, "http-sharing").await;
    let owner_keys = za_unlock(test, owner, STRONG).await;
    let mut event_versions = Vec::new();
    for name in ["sealed.ics", "plain.ics"] {
        let event_id = resource_id(test, owner, &format!("http-sharing/{name}")).await;
        event_versions.push((event_id, event_archive(test, owner, event_id).await));
    }
    let previous = sharing_http_ring(test, &owner_keys, id).await.unwrap();
    let original = previous
        .1
        .grants
        .iter()
        .find(|g| g.account_id == reader)
        .unwrap()
        .clone();
    // A separately pinned alias identifies the stable account id; address and time stay recorded.
    let domain = test
        .account("admin@example.com")
        .find_or_create_domain("share-alias.test")
        .await;
    let aliases = List::from_iter([EmailAlias {
        name: "sharing-alias".into(),
        domain_id: domain,
        enabled: true,
        description: None,
    }]);
    test.account("admin@example.com")
        .registry_update_object(
            ObjectType::Account,
            test.account(READER).id(),
            json!({Property::Aliases:aliases}),
        )
        .await;
    let reader_key = test
        .server
        .za_served_public_key(reader)
        .await
        .unwrap()
        .unwrap();
    super::pinning::pin(USER, ALIAS, &display_fingerprint(&reader_key))
        .await
        .expect(200);
    assert_eq!(
        za_post("shares/grant", &request(ALIAS, "edit"))
            .await
            .expect(200)["changed"],
        true
    );
    let current = sharing_http_ring(test, &owner_keys, id).await.unwrap();
    let stored = za_owned_calendar_context(&test.server, &owner_keys, id)
        .await
        .unwrap()
        .stored
        .deserialize::<Calendar>()
        .unwrap();
    assert_eq!(
        stored
            .acls
            .iter()
            .find(|a| a.account_id == reader)
            .unwrap()
            .grants
            .bitmap,
        123
    );
    assert_eq!(
        current.0.generations, previous.0.generations,
        "level changes must not rotate"
    );
    let aliased = current
        .1
        .grants
        .iter()
        .find(|g| g.account_id == reader)
        .unwrap();
    assert_eq!(
        (
            aliased.address.as_str(),
            aliased.granted_at,
            aliased.change_id
        ),
        (
            original.address.as_str(),
            original.granted_at,
            original.change_id
        )
    );
    za_post("shares/grant", &request(SECOND, "view"))
        .await
        .expect(200);
    // Reassigned recorded address and a separately granted account must refuse ambiguity.
    let recorded_alias = request(ALIAS, "view");
    // First make the reader's recorded address the alias by revoke and new grant.
    za_post(
        "shares/revoke",
        &json!({"username":USER,"password":STRONG,"calendar":"http-sharing","grant":reader}),
    )
    .await
    .expect(200);
    za_post("shares/grant", &recorded_alias).await.expect(200);
    assert_eq!(
        za_post(
            "shares/revoke",
            &json!({"username":USER,"password":STRONG,"calendar":"http-sharing","address":READER})
        )
        .await
        .expect(200)["changed"],
        true
    );
    za_post("shares/grant", &recorded_alias).await.expect(200);
    test.account("admin@example.com")
        .registry_update_object(
            ObjectType::Account,
            test.account(READER).id(),
            json!({Property::Aliases:List::<EmailAlias>::from_iter([])}),
        )
        .await;
    let aliases = List::from_iter([EmailAlias {
        name: "sharing-alias".into(),
        domain_id: domain,
        enabled: true,
        description: None,
    }]);
    test.account("admin@example.com")
        .registry_update_object(
            ObjectType::Account,
            test.account(SECOND).id(),
            json!({Property::Aliases:aliases}),
        )
        .await;
    let before = sharing_http_ring(test, &owner_keys, id)
        .await
        .unwrap()
        .0
        .generations;
    let ambiguous = za_post(
        "shares/revoke",
        &json!({"username":USER,"password":STRONG,"calendar":"http-sharing","address":ALIAS}),
    )
    .await
    .expect(409);
    assert_eq!(ambiguous["error"], "address is ambiguous, revoke by grant");
    assert_eq!(
        sharing_http_ring(test, &owner_keys, id)
            .await
            .unwrap()
            .0
            .generations,
        before
    );
    // Alias cycle: both grants may legitimately record the same address. A
    // move back to the first ledger row must not make .find() pick that row.
    za_post(
        "shares/revoke",
        &json!({"username":USER,"password":STRONG,"calendar":"http-sharing","grant":second_id}),
    )
    .await
    .expect(200);
    super::pinning::pin(USER, ALIAS, &display_fingerprint(&key))
        .await
        .expect(200);
    za_post("shares/grant", &request(ALIAS, "view"))
        .await
        .expect(200);
    test.account("admin@example.com")
        .registry_update_object(
            ObjectType::Account,
            test.account(SECOND).id(),
            json!({Property::Aliases:List::<EmailAlias>::from_iter([])}),
        )
        .await;
    let aliases = List::from_iter([EmailAlias {
        name: "sharing-alias".into(),
        domain_id: domain,
        enabled: true,
        description: None,
    }]);
    test.account("admin@example.com")
        .registry_update_object(
            ObjectType::Account,
            test.account(READER).id(),
            json!({Property::Aliases:aliases}),
        )
        .await;
    let duplicate_before = za_owned_calendar_context(&test.server, &owner_keys, id)
        .await
        .unwrap()
        .stored
        .version;
    assert_eq!(
        za_post(
            "shares/revoke",
            &json!({"username":USER,"password":STRONG,"calendar":"http-sharing","address":ALIAS})
        )
        .await
        .expect(409)["error"],
        "address is ambiguous, revoke by grant"
    );
    assert_eq!(
        za_owned_calendar_context(&test.server, &owner_keys, id)
            .await
            .unwrap()
            .stored
            .version,
        duplicate_before
    );
    test.account("admin@example.com")
        .registry_update_object(
            ObjectType::Account,
            test.account(READER).id(),
            json!({Property::Aliases:List::<EmailAlias>::from_iter([])}),
        )
        .await;
    let aliases = List::from_iter([EmailAlias {
        name: "sharing-alias".into(),
        domain_id: domain,
        enabled: true,
        description: None,
    }]);
    test.account("admin@example.com")
        .registry_update_object(
            ObjectType::Account,
            test.account(SECOND).id(),
            json!({Property::Aliases:aliases}),
        )
        .await;
    za_post(
        "shares/revoke",
        &json!({"username":USER,"password":STRONG,"calendar":"http-sharing","grant":second_id}),
    )
    .await
    .expect(200);
    za_post("shares/grant", &request(SECOND, "view"))
        .await
        .expect(200);
    // Rotation must use the second grantee's ledger key, even if directory key changed.
    let reader_keys = za_unlock(test, second_id, STRONG).await;
    let original_wrap = saved_wrap(test, second_id).await;
    let (private, original_mark) =
        unwrap_private_key(&original_wrap, reader_keys.mk(), second_id).unwrap();
    let (replacement, replacement_private) = generate_keypair();
    substitute_public_key(test, second_id, &replacement, true).await;
    let listed = za_post("shares", &list(USER)).await.expect(200);
    let grants = listed["calendars"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["calendar"] == "http-sharing")
        .unwrap()["grants"]
        .as_array()
        .unwrap();
    assert_eq!(
        grants.iter().find(|g| g["grant"] == second_id).unwrap()["state"],
        "key changed"
    );
    let revoke =
        json!({"username":USER,"password":STRONG,"calendar":"http-sharing","grant":reader});
    assert_eq!(
        za_post("shares/revoke", &revoke).await.expect(200)["changed"],
        true
    );
    let context = za_owned_calendar_context(&test.server, &owner_keys, id)
        .await
        .unwrap();
    let cal = context.stored.deserialize::<Calendar>().unwrap();
    let entry = cal
        .preferences
        .iter()
        .find(|p| p.account_id == second_id)
        .unwrap();
    let bytes = STANDARD
        .decode(
            entry
                .name
                .strip_prefix("$za$")
                .unwrap()
                .split('|')
                .next()
                .unwrap(),
        )
        .unwrap();
    let (header, _) = open_grant_ring(
        &bytes,
        owner,
        id,
        second_id,
        *context.domain.domain_id().unwrap(),
        &private,
    )
    .unwrap();
    assert_eq!(
        header.generations,
        sharing_http_ring(test, &owner_keys, id)
            .await
            .unwrap()
            .0
            .generations
    );
    // Same-level re-key rotates once, preserves the grant id/time boundary/address,
    // and must evict the grantee's resident keys despite unchanged ACL bits.
    super::pinning::pin(USER, SECOND, &display_fingerprint(&replacement))
        .await
        .expect(200);
    let second_client = DummyWebDavClient::new(second_id, SECOND, STRONG, SECOND);
    second_client
        .request("PROPFIND", "/dav/cal/shares-http-second%40example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_eq!(
        test.server.za_served_public_key(second_id).await.unwrap(),
        Some(key),
        "assisted listing repaired the intentionally mismatched served key"
    );
    assert!(
        !test.server.inner.cache.za_keys.contains_account(second_id),
        "self-check repair invalidates the just-authenticated cache entry"
    );
    // A real reset changes both pieces of the keypair. Seed residency using an
    // owned EWK-only event read, which never tries to open the old incoming ring.
    crate::utils::za::restore_wrap(
        test,
        second_id,
        &vault::keys::wrap_private_key(
            &replacement_private,
            reader_keys.mk(),
            second_id,
            original_mark,
        ),
    )
    .await;
    substitute_public_key(test, second_id, &replacement, true).await;
    let own_path = "/dav/cal/shares-http-second%40example.com/rekey-private/";
    second_client
        .request("MKCALENDAR", own_path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    crate::utils::za::plant_sealed_event(
        test,
        second_id,
        "rekey-private",
        "own.ics",
        &sharing_event("rekey-private"),
        STRONG,
        &Default::default(),
    )
    .await;
    let revision = crate::utils::za::vault_revision(test, second_id).await;
    second_client
        .request("GET", &format!("{own_path}own.ics"), "")
        .await
        .with_status(StatusCode::OK);
    assert_eq!(
        test.server.za_served_public_key(second_id).await.unwrap(),
        Some(replacement)
    );
    assert_eq!(
        crate::utils::za::vault_revision(test, second_id).await,
        revision
    );
    assert!(test.server.inner.cache.za_keys.contains_account(second_id));
    super::pinning::pin(USER, SECOND, &display_fingerprint(&replacement))
        .await
        .expect(200);
    let live_before = vault::session::live::session_keys(second_id);
    let before = sharing_http_ring(test, &owner_keys, id).await.unwrap();
    let old = before
        .1
        .grants
        .iter()
        .find(|g| g.account_id == second_id)
        .unwrap()
        .clone();
    za_post("shares/grant", &request(SECOND, "view"))
        .await
        .expect(200);
    assert!(!test.server.inner.cache.za_keys.contains_account(second_id));
    assert!(vault::session::live::session_keys(second_id) < live_before);
    let after = sharing_http_ring(test, &owner_keys, id).await.unwrap();
    assert_eq!(after.0.generations.len(), before.0.generations.len() + 1);
    let new = after
        .1
        .grants
        .iter()
        .find(|g| g.account_id == second_id)
        .unwrap();
    assert_eq!(
        (new.address.as_str(), new.change_id),
        (old.address.as_str(), old.change_id)
    );
    let cal = za_owned_calendar_context(&test.server, &owner_keys, id)
        .await
        .unwrap()
        .stored
        .deserialize::<Calendar>()
        .unwrap();
    let bytes = STANDARD
        .decode(
            cal.preferences
                .iter()
                .find(|p| p.account_id == second_id)
                .unwrap()
                .name
                .strip_prefix("$za$")
                .unwrap()
                .split('|')
                .next()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        open_grant_ring(
            &bytes,
            owner,
            id,
            second_id,
            after.0.domain_id,
            &replacement_private
        )
        .unwrap()
        .0
        .generations,
        after.0.generations
    );
    assert_eq!(
        parse_grant_header(&bytes).unwrap().grant_change_id,
        old.change_id
    );
    // Business refusals after valid authentication do not spend the fail2ban budget.
    for _ in 0..12 {
        assert_eq!(
            za_post_from(
                "10.0.40.1",
                "shares/grant",
                &request("missing-sharing-contact@example.com", "view")
            )
            .await
            .expect(409)["error"],
            "not a key account"
        );
    }
    za_post_from("10.0.40.1", "shares", &list(USER))
        .await
        .expect(200);
    assert_eq!(
        za_post("shares", &list(SECOND)).await.expect(200)["shared_with_me"][0]["name"],
        "Shared owner private name"
    );
    // Remove the alternate alias grant via the account's primary address.
    assert_eq!(
        za_post(
            "shares/revoke",
            &json!({"username":USER,"password":STRONG,"calendar":"http-sharing","address":SECOND})
        )
        .await
        .expect(200)["changed"],
        true
    );
    second_client
        .request("DELETE", own_path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    crate::utils::za::restore_wrap(test, second_id, &original_wrap).await;
    substitute_public_key(test, second_id, &key, true).await;
    // A fresh later grant receives every surviving generation.
    za_post("shares/grant", &request(READER, "view"))
        .await
        .expect(200);
    let after = sharing_http_ring(test, &owner_keys, id).await.unwrap();
    let cal = za_owned_calendar_context(&test.server, &owner_keys, id)
        .await
        .unwrap()
        .stored
        .deserialize::<Calendar>()
        .unwrap();
    let bytes = STANDARD
        .decode(
            cal.preferences
                .iter()
                .find(|p| p.account_id == reader)
                .unwrap()
                .name
                .strip_prefix("$za$")
                .unwrap()
                .split('|')
                .next()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        parse_grant_header(&bytes).unwrap().generations,
        after.0.generations
    );
    let _: Value = za_post("shares", &list(USER)).await.expect(200);
    for (event_id, before) in event_versions {
        let after = event_archive(test, owner, event_id).await;
        assert_eq!(after.version, before.version);
        assert_eq!(after.as_bytes(), before.as_bytes());
    }
}

pub(super) fn sharing_event(uid: &str) -> String {
    EVENT.replace("share-read-UID", uid)
}

async fn sharing_http_ring(
    test: &TestServer,
    keys: &vault::session::SessionKeys,
    id: u32,
) -> Option<(OwnerRingHeader, ZaOwnerRing)> {
    let context = za_owned_calendar_context(&test.server, keys, id)
        .await
        .unwrap();
    if context.domain.calendar_id().is_none() {
        return None;
    }
    let calendar = context.stored.deserialize::<Calendar>().unwrap();
    let entry = calendar
        .preferences
        .iter()
        .find(|p| p.account_id == keys.account_id)
        .unwrap();
    Some(
        groupware::calendar::sharing::format::open_owner_ring(
            &STANDARD
                .decode(
                    entry
                        .name
                        .strip_prefix("$za$")
                        .unwrap()
                        .split('|')
                        .next()
                        .unwrap(),
                )
                .unwrap(),
            keys.account_id,
            id,
            keys.ewk(),
        )
        .unwrap(),
    )
}

async fn sharing_response_contracts(test: &mut TestServer, owner: u32, reader: u32) {
    use crate::utils::za::{SERVER_URL, code, totp_url, za_app_password, za_post, za_post_from};
    use registry::schema::{enums::Permission, prelude::Property, structs::Sharing};
    use serde_json::json;
    const USER: &str = "shares-http-owner@example.com";
    let credentials = json!({"username":USER,"password":STRONG});
    let base = json!({"username":USER,"password":STRONG,"calendar":"http-sharing","address":"shares-http-reader@example.com","level":"view"});
    for (field, value) in [
        ("calendar", json!("bad/slug")),
        ("calendar", json!("x".repeat(256))),
        ("calendar", json!("é".repeat(128))),
        ("address", json!("mailto:bad@example.com")),
        ("level", json!("owner")),
    ] {
        let mut body = base.clone();
        body[field] = value;
        body["password"] = json!("invalid");
        assert_eq!(
            za_post_from("10.0.40.2", "shares/grant", &body)
                .await
                .expect(400)["status"],
            400
        );
    }
    for slug in [
        "missing".to_owned(),
        "x".repeat(255),
        "é".repeat(127),
        "inbox".to_owned(),
        "outbox".to_owned(),
    ] {
        let mut body = base.clone();
        body["calendar"] = json!(slug);
        assert_eq!(
            za_post("shares/grant", &body).await.expect(409)["error"],
            "unknown calendar"
        );
    }
    for address in ["plain@example.com", "not-known-sharing@example.com"] {
        let mut body = base.clone();
        body["address"] = json!(address);
        assert_eq!(
            za_post("shares/grant", &body).await.expect(409)["error"],
            "not a key account"
        );
    }
    let group = test
        .account("admin@example.com")
        .create_group_account("share-refusal-group@example.com", "Share group", &[])
        .await;
    test.insert_account(group);
    let pending = test
        .account("admin@example.com")
        .create_passwordless_user_account(
            "share-refusal-pending@example.com",
            STRONG,
            "Pending",
            &[],
            user_permissions(),
        )
        .await;
    test.insert_account(pending);
    for address in [
        "share-refusal-group@example.com",
        "share-refusal-pending@example.com",
    ] {
        let mut body = base.clone();
        body["address"] = json!(address);
        assert_eq!(
            za_post("shares/grant", &body).await.expect(409)["error"],
            "not a key account"
        );
    }
    let mut own = base.clone();
    own["address"] = json!(USER);
    assert_eq!(
        za_post("shares/grant", &own).await.expect(409)["error"],
        "cannot share with yourself"
    );
    assert_eq!(
        za_post(
            "shares",
            &json!({"username":"plain@example.com","password":"plain secret with entropy 9"})
        )
        .await
        .expect(409)["error"],
        "not a zero-access account"
    );
    let mut master = credentials.clone();
    master["username"] = json!("admin%shares-http-owner@example.com");
    za_post("shares", &master).await.expect(400);
    let app = za_app_password(USER, STRONG, "sharing request refusal").await;
    let mut app_body = credentials.clone();
    app_body["password"] = json!(app);
    za_post("shares", &app_body).await.expect(400);
    let mut wrong = credentials.clone();
    wrong["password"] = json!("wrong");
    za_post_from("10.0.40.3", "shares", &wrong)
        .await
        .expect(401);
    // TOTP preserves the inherited 402/401 path, including successful current-code login.
    let url = totp_url();
    za_post(
        "totp",
        &json!({"username":USER,"password":STRONG,"otp_auth":url,"confirm":code(&url)}),
    )
    .await
    .expect(200);
    za_post_from("10.0.40.4", "shares", &credentials)
        .await
        .expect(402);
    za_post(
        "shares",
        &json!({"username":USER,"password":STRONG,"totp":code(&url)}),
    )
    .await
    .expect(200);
    za_post(
        "totp",
        &json!({"username":USER,"password":STRONG,"totp":code(&url),"otp_auth":null}),
    )
    .await
    .expect(200);
    let restricted = test
        .account("admin@example.com")
        .create_key_user_account(
            "sharing-no-acl@example.com",
            STRONG,
            "No ACL",
            &[],
            vec![Permission::UnlimitedRequests, Permission::UnlimitedUploads],
        )
        .await;
    test.insert_account(restricted);
    test.account("admin@example.com").registry_update_object(registry::schema::prelude::ObjectType::Account,test.account("sharing-no-acl@example.com").id(),json!({Property::Permissions:registry::schema::structs::Permissions::Merge(registry::schema::structs::PermissionsList {disabled_permissions:registry::types::map::Map::new(vec![Permission::DavCalAcl]),..Default::default()})})).await;

    let mut denied = base.clone();
    denied["username"] = json!("sharing-no-acl@example.com");
    assert_eq!(
        za_post("shares/grant", &denied).await.expect(403)["status"],
        403
    );
    let no_permission = za_post(
        "shares",
        &json!({"username":"sharing-no-acl@example.com","password":STRONG}),
    )
    .await
    .expect(200);
    assert_eq!(
        no_permission["calendars"],
        json!([]),
        "list must not create a default calendar"
    );
    // The anonymous request budget is inherited by all three sharing routes.
    test.reload_core();
    let previous_rate = test.server.core.network.http.rate_anonymous.clone();
    test.account("admin@example.com")
        .registry_update_setting(
            registry::schema::structs::Http {
                rate_limit_anonymous: Some(registry::schema::structs::Rate {
                    count: 1,
                    period: registry::types::duration::Duration::from_millis(60000),
                }),
                ..Default::default()
            },
            &[Property::RateLimitAnonymous],
        )
        .await;
    test.account("admin@example.com").reload_settings().await;
    test.reload_core();
    za_post_from("10.0.40.6", "shares", &credentials)
        .await
        .expect(200);
    assert_eq!(
        za_post_from("10.0.40.6", "shares", &credentials)
            .await
            .expect(429)["status"],
        429
    );
    test.account("admin@example.com")
        .registry_update_setting(
            registry::schema::structs::Http {
                rate_limit_anonymous: previous_rate,
                ..Default::default()
            },
            &[Property::RateLimitAnonymous],
        )
        .await;
    test.account("admin@example.com").reload_settings().await;
    test.reload_core();
    // Existing vault response machinery supplies no-store and both body shapes.
    test.server
        .inner
        .cache
        .set_za_account_page_origin(Some(super::ACCOUNT_PAGE_ORIGIN.parse().unwrap()));
    for (endpoint, body, status) in [
        ("shares", credentials.clone(), 200),
        ("shares/grant", own, 409),
    ] {
        let response = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap()
            .post(format!("{SERVER_URL}/api/vault/{endpoint}"))
            .header("Origin", super::ACCOUNT_PAGE_ORIGIN)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "no-store, no-cache, must-revalidate"
        );
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            super::ACCOUNT_PAGE_ORIGIN
        );
    }
    test.server.inner.cache.set_za_account_page_origin(None);
    // maxShares refuses before conversion, so no journal/DEK work is published.
    let previous = test.server.core.groupware.max_shares_per_item;
    test.account("admin@example.com")
        .registry_update_setting(
            Sharing {
                max_shares: 1,
                ..Default::default()
            },
            &[Property::MaxShares],
        )
        .await;
    test.account("admin@example.com").reload_settings().await;
    test.reload_core();
    let existing = resource_id(test, owner, "http-sharing").await;
    let before = za_owned_calendar_context(
        &test.server,
        &*za_unlock(test, owner, STRONG).await,
        existing,
    )
    .await
    .unwrap()
    .stored
    .version;
    let target = test.account("key2@example.com").id().document_id();
    super::pinning::pin(
        USER,
        "key2@example.com",
        &vault::keys::display_fingerprint(
            &test
                .server
                .za_served_public_key(target)
                .await
                .unwrap()
                .unwrap(),
        ),
    )
    .await
    .expect(200);
    let mut limited = base.clone();
    limited["address"] = json!("key2@example.com");
    assert_eq!(
        za_post("shares/grant", &limited).await.expect(409)["error"],
        "too many shares"
    );
    assert_eq!(
        za_owned_calendar_context(
            &test.server,
            &*za_unlock(test, owner, STRONG).await,
            existing
        )
        .await
        .unwrap()
        .stored
        .version,
        before
    );
    test.account("admin@example.com")
        .registry_update_setting(
            Sharing {
                max_shares: previous as u64,
                ..Default::default()
            },
            &[Property::MaxShares],
        )
        .await;
    test.account("admin@example.com").reload_settings().await;
    test.reload_core();
    let keys = za_unlock(test, owner, STRONG).await;
    // A pending boundary in a list remains pending and leaves the archive untouched.
    let id = resource_id(test, owner, "http-sharing").await;
    let context = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    let mut calendar = context.stored.deserialize::<Calendar>().unwrap();
    groupware::calendar::sharing::boundary::mark_boundary_pending(&mut calendar, &context.domain)
        .unwrap();
    update_calendar(test, owner, id, &context.stored, calendar).await;
    let pending = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    assert!(pending.boundary.unwrap().pending);
    za_post("shares", &credentials).await.expect(200);
    let after = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    assert_eq!(after.stored.version, pending.stored.version);
    assert!(after.boundary.unwrap().pending);
    assert_eq!(
        za_post("shares/grant", &base).await.expect(200)["changed"],
        true,
        "a boundary-only completion is a write"
    );
    assert!(
        !za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap()
            .boundary
            .unwrap()
            .pending
    );
    http::api::shares::test::take_private_outcomes(owner);
    let _ = reader;
}

async fn sharing_trace_contracts(test: &TestServer, owner: u32, reader: u32) {
    use crate::utils::za::{plant_event, za_post, za_post_from};
    use serde_json::json;
    const USER: &str = "shares-http-owner@example.com";
    const TARGET: &str = "shares-http-reader@example.com";
    let client = DummyWebDavClient::new(owner, USER, STRONG, USER);
    let path = "/dav/cal/shares-http-owner%40example.com/trace/";
    client
        .request(
            "MKCALENDAR",
            path,
            MKCAL.replace("Shared owner private name", "share-display-canary-7"),
        )
        .await
        .with_status(StatusCode::CREATED);
    plant_event(
        test,
        owner,
        "trace",
        "old.ics",
        &EVENT
            .replace("share-read-UID", "share-trace-uid-canary-7")
            .replace("share-read-private-summary", "share-trace-summary-canary-7"),
        None,
    )
    .await;
    let request = json!({"username":USER,"password":STRONG,"calendar":"trace","address":TARGET,"level":"view"});
    let captured = super::mail::Traced::start(trc::EventType::variants());
    za_post("shares/grant", &request).await.expect(200);
    za_post(
        "shares/revoke",
        &json!({"username":USER,"password":STRONG,"calendar":"trace","grant":reader}),
    )
    .await
    .expect(200);
    let mut bad = request.clone();
    bad["password"] = json!("share-wrong-password-canary-7");
    za_post_from("10.0.40.5", "shares/grant", &bad)
        .await
        .expect(401);
    let mut events = captured.finish().await;
    super::pinning::forget(USER, Some(TARGET)).await.expect(200);
    let captured = super::mail::Traced::start(trc::EventType::variants());
    assert_eq!(
        za_post("shares/grant", &request).await.expect(409)["error"],
        "contact not pinned"
    );
    events.extend(captured.finish().await);
    let public = test
        .server
        .za_served_public_key(reader)
        .await
        .unwrap()
        .unwrap();
    let fingerprint = vault::keys::display_fingerprint(&public);
    let mut inherited_success = false;
    for event in &events {
        for (key, value) in &event.keys {
            let text = format!("{value:?}");
            if text.contains(USER) {
                assert!(
                    *key == trc::Key::AccountName
                        && matches!(
                            event.typ,
                            trc::EventType::Auth(trc::AuthEvent::Success | trc::AuthEvent::Failed)
                        ),
                    "owner address escaped inherited auth field: {event:?}"
                );
                inherited_success |= event.typ == trc::EventType::Auth(trc::AuthEvent::Success);
            }
        }
        for canary in [
            TARGET,
            "sharing-alias@share-alias.test",
            "share-display-canary-7",
            "share-trace-uid-canary-7",
            "share-trace-summary-canary-7",
            "share-wrong-password-canary-7",
            fingerprint.as_str(),
            STANDARD.encode(public).as_str(),
        ] {
            assert!(
                !event.text.contains(canary),
                "trace exposed {canary}: {event:?}"
            );
        }
    }
    assert!(
        inherited_success,
        "capture must observe inherited auth identity positive control"
    );
    let completed = events
        .iter()
        .find(|e| e.details == "zero-access: calendar sharing completed")
        .unwrap();
    assert_eq!(completed.account_id, Some(owner as u64));
    assert!(
        completed
            .keys
            .iter()
            .any(|(k, v)| *k == trc::Key::Size && matches!(v, trc::Value::UInt(1)))
    );
    assert!(events.iter().any(|e| {
        e.details == "zero-access: calendar share entry committed"
            && e.keys.iter().any(|(k, v)| {
                *k == trc::Key::Id && matches!(v,trc::Value::UInt(id) if *id==reader as u64)
            })
    }));
    super::pinning::pin(USER, TARGET, &fingerprint)
        .await
        .expect(200);
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn sharing_recovery_contracts(test: &mut TestServer, owner: u32, reader: u32) {
    use crate::utils::za::za_post;
    use serde_json::json;
    const USER: &str = "shares-http-owner@example.com";
    const READER: &str = "shares-http-reader@example.com";
    const OTHER: &str = "another sharing long passphrase with 77 numbers";
    let before = za_post("shares", &json!({"username":USER,"password":STRONG}))
        .await
        .expect(200);
    let recovery = test.account(USER).recovery_key.as_ref().unwrap().clone();
    let renewed = za_post(
        "recover",
        &json!({"username":USER,"recovery_key":recovery,"new_password":OTHER}),
    )
    .await
    .str(200, "recovery_key");
    test.accounts.get_mut(USER).unwrap().recovery_key = Some(renewed);
    assert_eq!(
        za_post("shares", &json!({"username":USER,"password":OTHER}))
            .await
            .expect(200),
        before
    );
    let owner_client = DummyWebDavClient::new(owner, USER, OTHER, USER);
    owner_client
        .request(
            "GET",
            "/dav/cal/shares-http-owner%40example.com/http-sharing/sealed.ics",
            "",
        )
        .await
        .with_status(StatusCode::OK);
    za_post(
        "password",
        &json!({"username":READER,"password":STRONG,"new_password":OTHER}),
    )
    .await
    .expect(200);
    let listed = za_post("shares", &json!({"username":READER,"password":OTHER}))
        .await
        .expect(200);
    assert_eq!(
        listed["shared_with_me"][0]["name"],
        "Shared owner private name"
    );
    za_post(
        "password",
        &json!({"username":READER,"password":OTHER,"new_password":STRONG}),
    )
    .await
    .expect(200);
    za_post(
        "password",
        &json!({"username":USER,"password":OTHER,"new_password":STRONG}),
    )
    .await
    .expect(200);
    let _ = reader;
}

async fn sharing_default_and_sort_contracts(test: &TestServer, owner: u32, reader: u32) {
    use crate::utils::za::{plant_event, za_post};
    use serde_json::json;
    const USER: &str = "shares-http-owner@example.com";
    const TARGET: &str = "shares-http-reader@example.com";
    const HOME: &str = "/dav/cal/shares-http-owner%40example.com/";
    let request = |slug: &str| json!({"username":USER,"password":STRONG,"calendar":slug,"address":TARGET,"level":"view"});
    let keys = za_unlock(test, owner, STRONG).await;
    let id = resource_id(test, owner, "default").await;
    let original = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap()
        .stored
        .deserialize::<Calendar>()
        .unwrap();
    assert!(!original.preferences[0].name.starts_with("$za$"));
    plant_event(
        test,
        owner,
        "default",
        "legacy.ics",
        &sharing_event("http-default-legacy"),
        None,
    )
    .await;
    assert_eq!(
        za_post("shares/grant", &request("default"))
            .await
            .expect(200),
        json!({"ok":true,"changed":true,"rewrapped":0,"sealed":1})
    );
    let client = DummyWebDavClient::new(owner, USER, STRONG, USER);
    client
        .request("GET", &format!("{HOME}default/legacy.ics"), "")
        .await
        .with_status(StatusCode::OK);
    za_post(
        "shares/revoke",
        &json!({"username":USER,"password":STRONG,"calendar":"default","grant":reader}),
    )
    .await
    .expect(200);
    let own = za_post("shares", &json!({"username":USER,"password":STRONG}))
        .await
        .expect(200);
    let default = own["calendars"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["calendar"] == "default")
        .unwrap();
    assert_eq!(default["shared"], true);
    assert_eq!(default["grants"], json!([]));
    client
        .request("DELETE", &format!("{HOME}default/legacy.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    // Deliberately create reverse lexical order; listing order cannot pass by id order.
    for slug in ["sort-z", "sort-a", "sort%20space"] {
        client
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
        za_post("shares/grant", &request(slug)).await.expect(200);
    }
    let own = za_post("shares", &json!({"username":USER,"password":STRONG}))
        .await
        .expect(200);
    let names = own["calendars"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["calendar"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "default",
            "http-sharing",
            "sort%20space",
            "sort-a",
            "sort-z"
        ]
    );
    let shared = za_post("shares", &json!({"username":TARGET,"password":STRONG}))
        .await
        .expect(200);
    let names = shared["shared_with_me"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["calendar"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["http-sharing", "sort%20space", "sort-a", "sort-z"]
    );
    assert_eq!(
        shared["shared_with_me"][1]["href"],
        format!("{HOME}sort%20space/")
    );
    for slug in ["sort-z", "sort-a", "sort%20space"] {
        client
            .request("DELETE", &format!("{HOME}{slug}/"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}

async fn sharing_scan_transition_contracts(test: &TestServer, owner: u32, reader: u32) {
    use crate::utils::za::{plant_sealed_event, za_post};
    use groupware::calendar::sharing::{
        convert::ZaShareError,
        mutations::{ZaShareChange, ZaShareLevel, za_share_apply},
    };
    use serde_json::json;
    const USER: &str = "shares-http-owner@example.com";
    const READER: &str = "shares-http-reader@example.com";
    const SLUG: &str = "scan-transition";
    let path = "/dav/cal/shares-http-owner%40example.com/scan-transition/";
    let client = DummyWebDavClient::new(owner, USER, STRONG, USER);
    client
        .request("MKCALENDAR", path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let id = resource_id(test, owner, SLUG).await;
    let event = plant_sealed_event(
        test,
        owner,
        SLUG,
        "history.ics",
        &sharing_event("scan-transition"),
        STRONG,
        &Default::default(),
    )
    .await;
    stage_converting_calendar(test, owner, id).await;
    let keys = za_unlock(test, owner, STRONG).await;
    let context = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    let original = event_archive(test, owner, event).await;
    let mut valid = original.deserialize::<CalendarEvent>().unwrap();
    rewrap_event(&mut valid, &context.domain, &context.domain).unwrap();
    update_event(test, owner, event, &original, valid.clone()).await;
    let damage = |event: &mut CalendarEvent| {
        let carrier = event
            .data
            .event
            .components
            .iter_mut()
            .flat_map(|c| &mut c.entries)
            .find(|entry| groupware::calendar::seal::tree::is_carrier(entry, "X-ZA-SEALED"))
            .unwrap();
        let calcard::icalendar::ICalendarValue::Text(encoded) = &mut carrier.values[0] else {
            panic!()
        };
        let mut bytes = STANDARD.decode(&*encoded).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        *encoded = STANDARD.encode(bytes);
    };
    let good = event_archive(test, owner, event).await;
    let mut invalid = valid.clone();
    damage(&mut invalid);
    update_event(test, owner, event, &good, invalid).await;
    let damaged = event_archive(test, owner, event).await;
    client
        .request("GET", &format!("{path}history.ics"), "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    let vault = test.server.za_vault_record(owner).await.unwrap().unwrap();
    assert_eq!(vault.record.revision, keys.generation);
    let public_key = test
        .server
        .za_served_public_key(reader)
        .await
        .unwrap()
        .unwrap();
    let pins = test.server.za_pin_read(&keys).await.unwrap();
    assert_eq!(
        pins.record
            .as_ref()
            .unwrap()
            .pin(READER)
            .unwrap()
            .public_key,
        public_key
    );
    let mut guards = conversion_guards(test, &keys).await;
    guards.pin_assertion = Some(pins.assertion());
    assert!(
        matches!(
            za_share_apply(
                &test.server,
                &keys,
                id,
                ZaShareChange::Grant {
                    account_id: reader,
                    address: READER.into(),
                    public_key,
                    level: ZaShareLevel::View,
                },
                &guards
            )
            .await,
            Err(ZaShareError::Seal(_))
        ),
        "an invalid unconverted journal cannot publish"
    );
    assert_eq!(
        za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap()
            .stored
            .version,
        context.stored.version
    );
    assert_eq!(
        event_archive(test, owner, event).await.version,
        damaged.version
    );
    conversion_ring(test, &keys, id).await.unwrap();
    update_event(test, owner, event, &damaged, valid.clone()).await;
    let request =
        json!({"username":USER,"password":STRONG,"calendar":SLUG,"address":READER,"level":"view"});
    let first = za_post("shares/grant", &request).await;
    assert_eq!(
        first.status, 200,
        "valid initial publication must succeed: {}",
        first.body
    );
    assert_eq!(
        first.body,
        json!({"ok":true,"changed":true,"rewrapped":0,"sealed":0})
    );
    assert!(
        sharing_http_ring(test, &keys, id)
            .await
            .unwrap()
            .1
            .converted
    );
    let good = event_archive(test, owner, event).await;
    let mut invalid = valid.clone();
    damage(&mut invalid);
    update_event(test, owner, event, &good, invalid).await;
    let damaged = event_archive(test, owner, event).await;
    client
        .request("GET", &format!("{path}history.ics"), "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    let before = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap()
        .stored
        .version;
    println!(
        "round1 scan controls: invalid unconverted apply refused without mutation; restored initial publication200; authenticated converted=true; corrupted historical GET500"
    );
    let noop = za_post("shares/grant", &request).await;
    assert_eq!(
        noop.status, 200,
        "converted same-key same-level no-op must skip historical payload: {}",
        noop.body
    );
    assert_eq!(
        noop.body,
        json!({"ok":true,"changed":false,"rewrapped":0,"sealed":0})
    );
    assert_eq!(
        za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap()
            .stored
            .version,
        before
    );
    let mut edit = request;
    edit["level"] = json!("edit");
    assert_eq!(
        za_post("shares/grant", &edit).await.expect(200),
        json!({"ok":true,"changed":true,"rewrapped":0,"sealed":0})
    );
    let after = event_archive(test, owner, event).await;
    assert_eq!(after.version, damaged.version);
    assert_eq!(after.as_bytes(), damaged.as_bytes());
    assert_eq!(
        za_owned_calendar_context(&test.server, &keys, id)
            .await
            .unwrap()
            .stored
            .deserialize::<Calendar>()
            .unwrap()
            .acls[0]
            .grants
            .bitmap,
        123
    );
    update_event(test, owner, event, &after, valid).await;
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Direct receiver seam: an owner's name authenticated by its writer's SENT-BY.
async fn delegated_receiver_consistency(test: &TestServer) {
    use super::{
        drain::DrainOn,
        fanout::{NO_REPLY, Party, START_A, invite, uid_copies},
    };
    use crate::utils::za::{PlantedItem, copy_href, inbox_ids, plant_inbox_item, trigger_drain};
    let _drain = DrainOn::new();
    let writer = Party::new(test, "key2@example.com");
    let before = inbox_ids(test, writer.id).await;
    let tree = invite("sharing-delegated-direct", START_A, &[writer.name])
        .replace("VERSION:2.0", "VERSION:2.0\r\nMETHOD:REQUEST")
        .replace(
            "ORGANIZER:mailto:",
            "ORGANIZER;SENT-BY=\"mailto:key2@example.com\":mailto:",
        );
    plant_inbox_item(
        test,
        writer.id,
        &tree,
        PlantedItem::Pending {
            pair: Some((writer.name, [0x39; 16])),
            reply_token: None,
        },
        writer.id,
        None,
    )
    .await;
    trigger_drain(writer.name, STRONG).await;
    assert_eq!(
        uid_copies(test, writer.id, "sharing-delegated-direct").await,
        1,
        "a validated owner-named delegated REQUEST must import into the writer inbox"
    );
    let href = copy_href(test, writer.id, "sharing-delegated-direct").await;
    {
        writer
            .client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    writer.delete_items_since(test, &before).await;
}

const DELEGATE_OWNER: &str = "task9-alice@example.com";
const DELEGATE_WRITER: &str = "task9-bob@example.com";
const DELEGATE_ALIAS: &str = "task9-bob@delegation-alias.test";

fn delegated_tree(uid: &str, attendees: &[&str]) -> calcard::icalendar::ICalendar {
    let text = super::fanout::invite(uid, super::fanout::START_A, attendees)
        .replace("key1@example.com", DELEGATE_OWNER);
    match calcard::Parser::new(&text).entry() {
        calcard::Entry::ICalendar(tree) => tree,
        other => panic!("{other:?}"),
    }
}

/// Public producer/builders/combinator/fan-out seam, before Task 10 opens DAV admission.
async fn delegated_run(
    test: &TestServer,
    owner: &common::auth::AccountInfo,
    writer: &common::auth::AccountInfo,
    keys: &vault::session::SessionKeys,
    kind: u8,
    old: Option<&calcard::icalendar::ICalendar>,
    mut new: calcard::icalendar::ICalendar,
    pairs: Option<groupware::calendar::seal::TokenPairs>,
) -> (
    calcard::icalendar::ICalendar,
    groupware::calendar::fanout::ZaFanOutResult,
    Vec<usize>,
) {
    use groupware::{
        calendar::{delegation::*, fanout::*},
        scheduling::{
            ItipError, event_cancel::itip_cancel, event_create::itip_create,
            event_update::itip_update,
        },
    };
    let sender = test
        .server
        .scheduling_account_info(writer.account_id, owner.account_id)
        .await
        .unwrap();
    let operation = match kind {
        0 => ZaSelfOperation::Create,
        1 => ZaSelfOperation::Update,
        _ => ZaSelfOperation::Delete,
    };
    let resolution = za_resolve_delegation(
        &test.server,
        &sender,
        writer,
        owner,
        operation,
        old,
        Some(&new),
    )
    .await
    .unwrap()
    .expect("owner-organized delegated gate");
    let second = match kind {
        0 => za_self_create(&new, writer, owner, &resolution),
        1 => za_self_update(old.unwrap(), &new, writer, owner, &resolution),
        _ => za_self_cancel(old.unwrap(), writer, owner, &resolution),
    };
    let second = match second {
        Ok(second) => second,
        Err(ItipError::NothingToSend) => None,
        Err(err) => panic!("{err:?}"),
    };
    let self_counts = second
        .as_ref()
        .map(|second| {
            second
                .messages()
                .iter()
                .map(|message| message.to.len())
                .collect()
        })
        .unwrap_or_default();
    let mut cloned_finalized_tree = new.clone();
    if let Some(second) = &second
        && second.finalize()
    {
        za_finalize(&mut cloned_finalized_tree, second.increments());
    }
    let first = match kind {
        0 => itip_create(&mut new, sender.addresses()),
        1 => itip_update(&mut new, old.unwrap(), sender.addresses()),
        _ => itip_cancel(old.unwrap(), sender.addresses(), true).map(|message| vec![message]),
    };
    let first = match first {
        Ok(messages) => messages,
        Err(ItipError::NothingToSend) => {
            if let Some(second) = &second
                && second.finalize()
            {
                za_finalize(&mut new, second.increments());
            }
            vec![]
        }
        Err(err) => panic!("{err:?}"),
    };
    if second.as_ref().is_some_and(|second| second.finalize()) {
        assert_eq!(
            new, cloned_finalized_tree,
            "exactly one finalize on the committed clone"
        );
    }
    let messages = za_combine_delegated(first, second, &resolution);
    let result = test
        .server
        .za_fan_out(
            &sender,
            writer,
            owner,
            keys,
            messages,
            pairs,
            &new,
            ZaSource {
                document_id: 900_001,
                uid: "task9-source",
            },
            if kind == 2 {
                ZaFanOutKind::Deletion
            } else {
                ZaFanOutKind::Change
            },
            None,
            Some(&resolution),
        )
        .await
        .unwrap();
    za_set_schedule_status(&mut new, &result.statuses);
    groupware::calendar::sequence::za_set_sequences(&mut new, &result.sequences);
    (new, result, self_counts)
}

struct DelegationLookupReset;
impl Drop for DelegationLookupReset {
    fn drop(&mut self) {
        common::za::test_hooks::set_delegation_lookup_fail_once(None);
    }
}

async fn delegated_scheduling_tests(test: &mut TestServer) {
    use super::{
        drain::{DrainOn, body},
        fanout::{NO_REPLY, Party, uid_copies},
    };
    use crate::utils::za::{
        copy_href, inbox_ids, new_inbox_items, open_pending_item_as, trigger_drain,
    };
    use calcard::icalendar::{ICalendarParameterName, ICalendarProperty};
    use groupware::{
        calendar::{ChangedBy, delegation::*, fanout::*},
        scheduling::event_create::itip_create,
    };
    println!("Running direct delegated scheduling producer and lifecycle fixtures...");
    let admin = test.account("admin@example.com").clone();
    let owner_account = admin
        .create_key_user_account(
            DELEGATE_OWNER,
            STRONG,
            "Task 9 owner",
            &[],
            user_permissions(),
        )
        .await;
    let writer_account = admin
        .create_key_user_account(
            DELEGATE_WRITER,
            STRONG,
            "Task 9 writer",
            &[
                DELEGATE_ALIAS,
                "task9-target-local-canary@task9-target-canary.test",
            ],
            user_permissions(),
        )
        .await;
    test.insert_account(owner_account);
    test.insert_account(writer_account);
    let writer = Party::new(test, DELEGATE_WRITER);
    let owner = test
        .server
        .account_info(test.account(DELEGATE_OWNER).id().document_id())
        .await
        .unwrap();
    let writer_info = test.server.account_info(writer.id).await.unwrap();
    let sender = test
        .server
        .scheduling_account_info(writer.id, owner.account_id)
        .await
        .unwrap();
    let keys = za_unlock(test, writer.id, STRONG).await;
    delegated_owner_exclusion_precedes_eligibility(test, &owner, &writer_info, &keys).await;
    let before = inbox_ids(test, writer.id).await;
    let initial = delegated_tree("task9-sole", &[DELEGATE_WRITER]);
    let resolution = za_resolve_delegation(
        &test.server,
        &sender,
        &writer_info,
        &owner,
        ZaSelfOperation::Create,
        None,
        Some(&initial),
    )
    .await
    .unwrap()
    .unwrap();
    // Public upstream provenance never acquires self-inbox authority merely
    // because a valid request-local resolution is supplied beside it.
    let mut reference = initial.clone();
    let ordinary = itip_create(&mut reference, owner.addresses()).unwrap();
    let refused = test
        .server
        .za_fan_out(
            &sender,
            &writer_info,
            &owner,
            &keys,
            ordinary
                .into_iter()
                .map(ZaFanOutMessage::upstream)
                .collect(),
            None,
            &reference,
            ZaSource {
                document_id: 900_001,
                uid: "task9-source",
            },
            ZaFanOutKind::Change,
            None,
            Some(&resolution),
        )
        .await
        .unwrap();
    assert_eq!(refused.statuses.len(), 1);
    assert_eq!(refused.statuses[0].status, "5.3");
    assert!(new_inbox_items(test, writer.id, &before).await.is_empty());
    let mut pairs = None;
    let mut current = initial;
    let _drain = DrainOn::new();
    for (step, kind) in [
        ("create", 0),
        ("summary", 1),
        ("dtstart", 1),
        ("decline", 1),
        ("delete", 2),
    ] {
        let old = current.clone();
        let mut next = old.clone();
        for component in &mut next.components {
            for entry in &mut component.entries {
                if (step == "summary" && entry.name == ICalendarProperty::Summary)
                    || (step == "dtstart" && entry.name == ICalendarProperty::Dtstart)
                {
                    let text = if step == "summary" {
                        "task9 changed summary"
                    } else {
                        "20990302T100000Z"
                    };
                    if step == "summary" {
                        entry.values = vec![calcard::icalendar::ICalendarValue::Text(text.into())];
                    } else {
                        *entry = delegated_tree("x", &[]).components[1]
                            .entries
                            .iter()
                            .find(|entry| entry.name == ICalendarProperty::Dtstart)
                            .unwrap()
                            .clone();
                        // Parse DTSTART so its date representation remains valid.
                        let source = super::fanout::invite("x", super::fanout::START_B, &[]);
                        let calcard::Entry::ICalendar(parsed) =
                            calcard::Parser::new(&source).entry()
                        else {
                            unreachable!()
                        };
                        *entry = parsed.components[1]
                            .entries
                            .iter()
                            .find(|entry| entry.name == ICalendarProperty::Dtstart)
                            .unwrap()
                            .clone();
                    }
                }
                if step == "decline" && entry.name == ICalendarProperty::Attendee {
                    entry.params.retain(|parameter| {
                        parameter.name != ICalendarParameterName::Partstat
                            && parameter.name != ICalendarParameterName::Rsvp
                    });
                    entry.params.push(calcard::icalendar::ICalendarParameter {
                        name: ICalendarParameterName::Partstat,
                        value: calcard::icalendar::ICalendarParameterValue::Partstat(
                            calcard::icalendar::ICalendarParticipationStatus::Declined,
                        ),
                    });
                    entry.params.push(calcard::icalendar::ICalendarParameter {
                        name: ICalendarParameterName::Rsvp,
                        value: calcard::icalendar::ICalendarParameterValue::Bool(false),
                    });
                }
            }
        }
        let item_start = inbox_ids(test, writer.id).await;
        let (tree, result, counts) = delegated_run(
            test,
            &owner,
            &writer_info,
            &keys,
            kind,
            (kind != 0).then_some(&old),
            next,
            pairs,
        )
        .await;
        assert_eq!(counts, vec![1], "one self attempt for {step}");
        assert_eq!(result.statuses.len(), 1);
        assert_eq!(result.statuses[0].status, "1.2", "{step}");
        assert!(
            result
                .pairs
                .as_ref()
                .unwrap()
                .get(DELEGATE_WRITER)
                .is_some()
        );
        let items = new_inbox_items(test, writer.id, &item_start).await;
        assert_eq!(items.len(), 1, "{step}");
        assert!(matches!(items[0].1.changed_by,ChangedBy::PrincipalId(id) if id == writer.id));
        let (payload, carriers) = open_pending_item_as(test, writer.id, STRONG, items[0].0).await;
        assert_eq!(carriers.pair.as_ref().unwrap().len(), 1);
        let names: Vec<_> = payload
            .components
            .iter()
            .flat_map(|component| &component.entries)
            .filter(|entry| entry.name == ICalendarProperty::Organizer)
            .collect();
        assert!(names.iter().all(|entry| {
            entry.params.iter().any(|parameter| {
                parameter.name == ICalendarParameterName::SentBy
                    && parameter.value.as_text() == Some("mailto:task9-bob@example.com")
            })
        }));
        trigger_drain(writer.name, STRONG).await;
        assert_eq!(uid_copies(test, writer.id, "task9-sole").await, 1);
        let href = copy_href(test, writer.id, "task9-sole").await;
        let personal = body(&writer.client, &href).await;
        if step == "summary" {
            assert!(personal.contains("task9 changed summary"));
        }
        if step == "delete" {
            assert!(personal.contains("STATUS:CANCELLED"));
        }
        pairs = result.pairs;
        current = tree;
    }
    let href = copy_href(test, writer.id, "task9-sole").await;
    writer
        .client
        .request_with_headers("DELETE", &href, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    writer.delete_items_since(test, &before).await;

    // The broader R set removes a sub-address from the first run without
    // creating duplicate items; each listed alias gets its own token carrier.
    let aliases = delegated_tree(
        "task9-aliases",
        &[DELEGATE_WRITER, DELEGATE_ALIAS, "task9-bob+tag@example.com"],
    );
    let start = inbox_ids(test, writer.id).await;
    let (_, result, counts) =
        delegated_run(test, &owner, &writer_info, &keys, 0, None, aliases, None).await;
    assert_eq!(counts, vec![3]);
    assert_eq!(result.statuses.len(), 3);
    assert!(result.statuses.iter().all(|status| status.status == "1.2"));
    assert_eq!(new_inbox_items(test, writer.id, &start).await.len(), 3);
    for (id, _) in new_inbox_items(test, writer.id, &start).await {
        let (_, carriers) = open_pending_item_as(test, writer.id, STRONG, id).await;
        assert_eq!(carriers.pair.unwrap().len(), 1);
    }
    trigger_drain(writer.name, STRONG).await;
    assert_eq!(uid_copies(test, writer.id, "task9-aliases").await, 1);
    let href = copy_href(test, writer.id, "task9-aliases").await;
    writer
        .client
        .request_with_headers("DELETE", &href, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    writer.delete_items_since(test, &before).await;

    let _reset = DelegationLookupReset;
    for (address, suppressed) in [
        (DELEGATE_ALIAS, false),
        (DELEGATE_ALIAS, true),
        ("task9-bob+failure@example.com", false),
    ] {
        common::za::test_hooks::set_delegation_lookup_fail_once(Some((writer.id, address.into())));
        let mut tree = delegated_tree("task9-failure", &[address]);
        if suppressed {
            let text = tree.to_string().replace(
                "PARTSTAT=NEEDS-ACTION;RSVP=TRUE",
                "PARTSTAT=DECLINED;RSVP=FALSE",
            );
            let calcard::Entry::ICalendar(parsed) = calcard::Parser::new(&text).entry() else {
                unreachable!()
            };
            tree = parsed;
        }
        let start = inbox_ids(test, writer.id).await;
        let traced = super::mail::Traced::start(trc::EventType::variants());
        let (_, result, counts) =
            delegated_run(test, &owner, &writer_info, &keys, 0, None, tree, None).await;
        let lookup_traces = traced.finish().await;
        assert_eq!(
            lookup_traces
                .iter()
                .filter(|event| event.details == "zero-access: fan-out recipient lookup failed")
                .count(),
            1,
            "one raw recipient failure is traced once by the producer and reused by fan-out"
        );
        assert_eq!(result.statuses.len(), 1);
        assert_eq!(result.statuses[0].status, "5.1");
        assert_eq!(
            counts,
            if address == DELEGATE_ALIAS {
                vec![1]
            } else {
                vec![]
            }
        );
        assert!(new_inbox_items(test, writer.id, &start).await.is_empty());
        assert!(!common::za::test_hooks::take_delegation_lookup_fail_once(
            writer.id, address
        ));
    }
    common::za::test_hooks::set_delegation_lookup_fail_once(None);
    delegated_identity_and_groups(test, owner.account_id, writer.id).await;
    delegated_schedule_agents_remain_authoritative(test, &owner, &writer_info, &keys).await;
    delegated_attendee_and_alias_edges(test, owner.account_id, writer.id).await;
    delegated_collection_lifecycle(test, owner.account_id, writer.id).await;
    catch_all_resolution_is_quiet(test, owner.account_id, writer.id).await;
    println!(
        "Direct delegated sole-attendee lifecycle, alias/sub-address and failed-lookup fixtures passed"
    );
    writer.delete_items_since(test, &before).await;
    // No source event was stored; remove the receiver's disposable calendar.
    writer
        .client
        .request_with_headers(
            "DELETE",
            "/dav/cal/task9-bob%40example.com/default/",
            [NO_REPLY],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    crate::utils::za::set_raw_pins(test, writer.id, None).await;
    test.wait_for_tasks().await;
}

async fn delegated_identity_and_groups(test: &mut TestServer, owner_id: u32, writer_id: u32) {
    use super::fanout::Party;
    use crate::utils::za::{
        inbox_ids, new_inbox_items, open_pending_item_as, pin_record, trigger_drain,
        write_pin_record,
    };
    use groupware::{calendar::delegation::*, scheduling::event_create::itip_create};
    use registry::{
        schema::prelude::{ObjectType, Property},
        types::EnumImpl,
    };
    use serde_json::json;
    let admin = test.account("admin@example.com").clone();
    let owner = test.server.account_info(owner_id).await.unwrap();
    let writer = test.server.account_info(writer_id).await.unwrap();
    let sender = test
        .server
        .scheduling_account_info(writer_id, owner_id)
        .await
        .unwrap();
    let keys = za_unlock(test, writer_id, STRONG).await;
    let before = inbox_ids(test, writer_id).await;

    // A corrupt dedicated alias-domain record exercises the normal outer
    // preparation error without inventing a second recipient fault hook.
    let domain = admin.find_or_create_domain("delegation-alias.test").await;
    let class = store::write::ValueClass::Registry(store::write::RegistryClass::Item {
        object_id: ObjectType::Domain.to_id(),
        item_id: domain.id(),
    });
    let bytes = test
        .server
        .store()
        .get_value::<common::auth::vault::RawValue>(ValueKey::from(class.clone()))
        .await
        .unwrap()
        .unwrap()
        .0;
    let mut batch = BatchBuilder::new();
    batch.set(class.clone(), vec![0xffu8]);
    test.server.store().write(batch.build_all()).await.unwrap();
    test.server
        .invalidate_local_caches(&[common::ipc::CacheInvalidation::Domain(domain.document_id())])
        .await;
    let tree = delegated_tree("task9-identity-error", &[DELEGATE_ALIAS]);
    let result = za_resolve_delegation(
        &test.server,
        &sender,
        &writer,
        &owner,
        ZaSelfOperation::Create,
        None,
        Some(&tree),
    )
    .await;
    let mut batch = BatchBuilder::new();
    batch.set(class, bytes);
    test.server.store().write(batch.build_all()).await.unwrap();
    test.server
        .invalidate_local_caches(&[common::ipc::CacheInvalidation::Domain(domain.document_id())])
        .await;
    assert!(
        result.is_err(),
        "identity preparation errors are outer errors, not Failed recipient outcomes"
    );

    // Unlisted owner-resolving attendees stay out of the owner's inbox on
    // the gated organizer path, even though the authenticated writer differs.
    let owner_start = inbox_ids(test, owner_id).await;
    let tree = delegated_tree(
        "task9-owner-overlap",
        &["task9-alice+tag@example.com", DELEGATE_WRITER],
    );
    let (_, result, _) = delegated_run(test, &owner, &writer, &keys, 0, None, tree, None).await;
    assert_eq!(
        result
            .statuses
            .iter()
            .find(|status| status.address == "task9-alice+tag@example.com")
            .unwrap()
            .status,
        "5.3"
    );
    assert!(
        new_inbox_items(test, owner_id, &owner_start)
            .await
            .is_empty()
    );
    Party::new(test, DELEGATE_WRITER)
        .delete_items_since(test, &before)
        .await;

    // First-use pins still run, but every source-bound withheld operation is
    // disabled for a delegated event, including a colliding numeric source id.
    let mut record = pin_record(test, writer_id, STRONG).await.unwrap();
    record.refuse(900_001, "task9-source", DELEGATE_ALIAS, 123, None);
    let entry = record
        .entry(900_001, "task9-source", DELEGATE_ALIAS)
        .unwrap()
        .clone();
    write_pin_record(test, writer_id, STRONG, &record).await;
    let tree = delegated_tree("task9-withheld-collision", &[DELEGATE_ALIAS]);
    let (_, result, _) =
        delegated_run(test, &owner, &writer, &keys, 0, None, tree.clone(), None).await;
    assert_eq!(result.statuses[0].status, "1.2");
    assert_eq!(
        new_inbox_items(test, writer_id, &before).await.len(),
        1,
        "no full-state message from the writer's colliding source"
    );
    assert!(
        pin_record(test, writer_id, STRONG).await.unwrap().entry(
            900_001,
            "task9-source",
            DELEGATE_ALIAS
        ) == Some(&entry)
    );
    Party::new(test, DELEGATE_WRITER)
        .delete_items_since(test, &before)
        .await;
    delegated_run(
        test,
        &owner,
        &writer,
        &keys,
        2,
        Some(&tree),
        tree.clone(),
        result.pairs,
    )
    .await;
    assert!(
        pin_record(test, writer_id, STRONG).await.unwrap().entry(
            900_001,
            "task9-source",
            DELEGATE_ALIAS
        ) == Some(&entry)
    );
    Party::new(test, DELEGATE_WRITER)
        .delete_items_since(test, &before)
        .await;
    let mut record = pin_record(test, writer_id, STRONG).await.unwrap();
    let original_key = record.pin(DELEGATE_ALIAS).unwrap().public_key;
    record.set_pin(
        DELEGATE_ALIAS,
        [0x6d; 32],
        vault::pins::PinSource::Confirmed,
        124,
    );
    write_pin_record(test, writer_id, STRONG, &record).await;
    let (_, result, _) = delegated_run(test, &owner, &writer, &keys, 0, None, tree, None).await;
    assert_eq!(result.statuses[0].status, "3.8");
    assert!(new_inbox_items(test, writer_id, &before).await.is_empty());
    let mut record = pin_record(test, writer_id, STRONG).await.unwrap();
    assert!(record.entry(900_001, "task9-source", DELEGATE_ALIAS) == Some(&entry));
    record.set_pin(
        DELEGATE_ALIAS,
        original_key,
        vault::pins::PinSource::Confirmed,
        125,
    );
    record.withheld.clear();
    write_pin_record(test, writer_id, STRONG, &record).await;

    // Two distinct shared groups own the attendee exclusion and organizer
    // gate cases; a third belongs only to the writer and remains an ordinary
    // plain-group target rather than a redirect into its inbox.
    const ATT_GROUP: &str = "task9-attendee-group@example.com";
    const ORG_GROUP: &str = "task9-organizer-group@example.com";
    const WRITER_GROUP: &str = "task9-writer-group@example.com";
    let ag = admin
        .create_group_account(ATT_GROUP, "Task9 attendee group", &[])
        .await;
    let og = admin
        .create_group_account(ORG_GROUP, "Task9 organizer group", &[])
        .await;
    let wg = admin
        .create_group_account(WRITER_GROUP, "Task9 writer group", &[])
        .await;
    let (agid, ogid, wgid) = (ag.id(), og.id(), wg.id());
    test.insert_account(ag);
    test.insert_account(og);
    test.insert_account(wg);
    admin
        .registry_update_object(
            ObjectType::Account,
            test.account(DELEGATE_OWNER).id(),
            json!({Property::MemberGroupIds:{agid:true,ogid:true}}),
        )
        .await;
    admin
        .registry_update_object(
            ObjectType::Account,
            test.account(DELEGATE_WRITER).id(),
            json!({Property::MemberGroupIds:{agid:true,ogid:true,wgid:true}}),
        )
        .await;
    let owner = test.server.account_info(owner_id).await.unwrap();
    let writer = test.server.account_info(writer_id).await.unwrap();
    let sender = test
        .server
        .scheduling_account_info(writer_id, owner_id)
        .await
        .unwrap();
    let tree = delegated_tree("task9-shared-attendee", &[ATT_GROUP]);
    let resolution = za_resolve_delegation(
        &test.server,
        &sender,
        &writer,
        &owner,
        ZaSelfOperation::Create,
        None,
        Some(&tree),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(
        za_self_create(&tree, &writer, &owner, &resolution),
        Err(groupware::scheduling::ItipError::NothingToSend)
    ));
    let (_, result, counts) =
        delegated_run(test, &owner, &writer, &keys, 0, None, tree, None).await;
    assert!(counts.is_empty() && result.statuses.is_empty());
    let tree = delegated_tree("task9-writer-group", &[WRITER_GROUP]);
    let (_, result, counts) =
        delegated_run(test, &owner, &writer, &keys, 0, None, tree, None).await;
    assert_eq!(counts, vec![1]);
    assert_eq!(result.statuses[0].status, "5.3");
    let text = delegated_tree("task9-writer-group-suppressed", &[WRITER_GROUP])
        .to_string()
        .replace(
            "PARTSTAT=NEEDS-ACTION;RSVP=TRUE",
            "PARTSTAT=DECLINED;RSVP=FALSE",
        );
    let calcard::Entry::ICalendar(tree) = calcard::Parser::new(&text).entry() else {
        unreachable!()
    };
    let (_, result, counts) =
        delegated_run(test, &owner, &writer, &keys, 0, None, tree, None).await;
    assert!(counts.is_empty() && result.statuses.is_empty());
    let text = delegated_tree("task9-shared-organizer", &[DELEGATE_WRITER])
        .to_string()
        .replace(DELEGATE_OWNER, ORG_GROUP);
    let calcard::Entry::ICalendar(tree) = calcard::Parser::new(&text).entry() else {
        unreachable!()
    };
    let (_, result, counts) =
        delegated_run(test, &owner, &writer, &keys, 0, None, tree, None).await;
    assert_eq!(counts, vec![1]);
    assert_eq!(result.statuses[0].status, "1.2");
    let items = new_inbox_items(test, writer_id, &before).await;
    assert_eq!(items.len(), 1);
    let (payload, _) = open_pending_item_as(test, writer_id, STRONG, items[0].0).await;
    assert!(
        payload
            .components
            .iter()
            .flat_map(|component| &component.entries)
            .filter(|entry| entry.name == calcard::icalendar::ICalendarProperty::Organizer)
            .all(|entry| entry
                .params
                .iter()
                .all(|parameter| parameter.name
                    != calcard::icalendar::ICalendarParameterName::SentBy))
    );
    trigger_drain(DELEGATE_WRITER, STRONG).await;
    assert_eq!(
        super::fanout::uid_copies(test, writer_id, "task9-shared-organizer").await,
        0,
        "shared group organizer retains ordinary receiver rejection"
    );
    Party::new(test, DELEGATE_WRITER)
        .delete_items_since(test, &before)
        .await;
    admin
        .registry_update_object(
            ObjectType::Account,
            test.account(DELEGATE_OWNER).id(),
            json!({Property::MemberGroupIds:{agid:false,ogid:false}}),
        )
        .await;
    admin
        .registry_update_object(
            ObjectType::Account,
            test.account(DELEGATE_WRITER).id(),
            json!({Property::MemberGroupIds:{agid:false,ogid:false,wgid:false}}),
        )
        .await;
    // An inapplicable writer-organizer gate does no injected attendee lookup.
    let text = delegated_tree("task9-writer-organizer", &[DELEGATE_ALIAS])
        .to_string()
        .replace(DELEGATE_OWNER, DELEGATE_WRITER);
    let calcard::Entry::ICalendar(mut tree) = calcard::Parser::new(&text).entry() else {
        unreachable!()
    };
    common::za::test_hooks::set_delegation_lookup_fail_once(Some((
        writer_id,
        DELEGATE_ALIAS.into(),
    )));
    let _reset = DelegationLookupReset;
    assert!(
        za_resolve_delegation(
            &test.server,
            &sender,
            &writer,
            &owner,
            ZaSelfOperation::Create,
            None,
            Some(&tree)
        )
        .await
        .unwrap()
        .is_none()
    );
    assert!(common::za::test_hooks::take_delegation_lookup_fail_once(
        writer_id,
        DELEGATE_ALIAS
    ));
    let messages = itip_create(&mut tree, sender.addresses());
    assert!(matches!(
        messages,
        Err(groupware::scheduling::ItipError::NothingToSend)
    ));
    // Task10 owns actual foreign DAV admission and stored status/tag assertions.
    println!(
        "Direct delegated identity preparation, owner exclusion, pin collision and shared-group fixtures passed"
    );
}

/// All-event trace canaries exercise both catch-all domains, cold and warm,
/// through recipient classification and the receiver's delegated verification.
async fn catch_all_resolution_is_quiet(test: &TestServer, owner_id: u32, writer_id: u32) {
    use super::{
        fanout::{NO_REPLY, Party, uid_copies},
        mail::Traced,
    };
    use crate::utils::za::{PlantedItem, copy_href, inbox_ids, plant_inbox_item, trigger_drain};
    use common::{auth::EmailAddressRef, ipc::CacheInvalidation};
    use groupware::calendar::delegation::*;
    use registry::schema::{prelude::ObjectType, structs::Domain};
    use serde_json::json;
    const SOURCE_DOMAIN: &str = "task9-source-canary.test";
    const TARGET_DOMAIN: &str = "task9-target-canary.test";
    const SOURCE_LOCAL: &str = "task9-source-local-canary";
    const TARGET_LOCAL: &str = "task9-target-local-canary";
    const SOURCE: &str = "task9-source-local-canary@task9-source-canary.test";
    const TARGET: &str = "task9-target-local-canary@task9-target-canary.test";
    let admin = test.account("admin@example.com").clone();
    let source = admin.find_or_create_domain(SOURCE_DOMAIN).await;
    let target = admin.find_or_create_domain(TARGET_DOMAIN).await;
    let saved_source = test
        .server
        .registry()
        .object::<Domain>(source)
        .await
        .unwrap()
        .unwrap();
    let saved_target = test
        .server
        .registry()
        .object::<Domain>(target)
        .await
        .unwrap()
        .unwrap();
    admin
        .registry_update_object(
            ObjectType::Domain,
            source,
            json!({"catchAllAddress":TARGET,"subAddressing":{"@type":"Disabled"}}),
        )
        .await;
    admin
        .registry_update_object(
            ObjectType::Domain,
            target,
            json!({"catchAllAddress":null,"subAddressing":{"@type":"Disabled"}}),
        )
        .await;
    let owner = test.server.account_info(owner_id).await.unwrap();
    let writer = test.server.account_info(writer_id).await.unwrap();
    let sender = test
        .server
        .scheduling_account_info(writer_id, owner_id)
        .await
        .unwrap();
    let party = Party::new(test, DELEGATE_WRITER);
    let before = inbox_ids(test, writer_id).await;
    let cold = async || {
        test.server
            .invalidate_local_caches(&[
                CacheInvalidation::Domain(source.document_id()),
                CacheInvalidation::Domain(target.document_id()),
                CacheInvalidation::Account(writer_id),
            ])
            .await;
        test.server.invalidate_all_local_negative_caches();
    };
    let text = delegated_tree("task9-quiet-classification", &[SOURCE])
        .to_string()
        .replace(
            "PARTSTAT=NEEDS-ACTION;RSVP=TRUE",
            "PARTSTAT=DECLINED;RSVP=FALSE",
        );
    let calcard::Entry::ICalendar(tree) = calcard::Parser::new(&text).entry() else {
        unreachable!()
    };
    for is_cold in [true, false] {
        if is_cold {
            cold().await;
        }
        let traced = Traced::start(trc::EventType::variants());
        let resolution = za_resolve_delegation(
            &test.server,
            &sender,
            &writer,
            &owner,
            ZaSelfOperation::Create,
            None,
            Some(&tree),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(
            za_self_create(&tree, &writer, &owner, &resolution),
            Err(groupware::scheduling::ItipError::NothingToSend)
        ));
        assert!(
            test.server
                .inner
                .cache
                .emails_negative
                .get(&EmailAddressRef::new(SOURCE_LOCAL, source.document_id()))
                .is_some()
        );
        assert!(
            test.server
                .inner
                .cache
                .emails
                .get(&EmailAddressRef::new(TARGET_LOCAL, target.document_id()))
                .is_some()
        );
        let events = traced.finish().await;
        let all = events
            .iter()
            .map(|event| event.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for canary in [SOURCE_DOMAIN, TARGET_DOMAIN, SOURCE_LOCAL, TARGET_LOCAL] {
            assert!(
                !all.contains(canary),
                "quiet classification leaked {canary}, cold={is_cold}: {all}"
            );
        }
    }
    // Fan-out uses writer primary; explicitly plant catch-all SENT-BY to
    // reach the receiver's independent named-sender verification branch.
    for is_cold in [true, false] {
        let uid = if is_cold {
            "task9-quiet-drain-cold"
        } else {
            "task9-quiet-drain-warm"
        };
        let text = super::fanout::invite(uid, super::fanout::START_A, &[DELEGATE_WRITER])
            .replace("key1@example.com", DELEGATE_OWNER)
            .replace("VERSION:2.0", "VERSION:2.0\r\nMETHOD:REQUEST")
            .replace(
                "ORGANIZER:mailto:",
                &format!("ORGANIZER;SENT-BY=\"mailto:{SOURCE}\":mailto:"),
            );
        plant_inbox_item(
            test,
            writer_id,
            &text,
            PlantedItem::Pending {
                pair: Some((DELEGATE_WRITER, [0x51; 16])),
                reply_token: None,
            },
            writer_id,
            None,
        )
        .await;
        if is_cold {
            cold().await;
        }
        let traced = Traced::start(trc::EventType::variants());
        trigger_drain(DELEGATE_WRITER, STRONG).await;
        let events = traced.finish().await;
        let all = events
            .iter()
            .map(|event| event.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for canary in [SOURCE_DOMAIN, TARGET_DOMAIN, SOURCE_LOCAL, TARGET_LOCAL] {
            assert!(
                !all.contains(canary),
                "quiet sender verification leaked {canary}, cold={is_cold}: {all}"
            );
        }
        assert_eq!(uid_copies(test, writer_id, uid).await, 1);
        let href = copy_href(test, writer_id, uid).await;
        party
            .client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    cold().await;
    for is_cold in [true, false] {
        let traced = Traced::start(trc::EventType::variants());
        assert_eq!(
            test.server
                .account_id_from_email(SOURCE, true)
                .await
                .unwrap(),
            Some(writer_id)
        );
        let events = traced.finish().await;
        let all = events
            .iter()
            .map(|event| event.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for canary in [SOURCE_LOCAL, TARGET_LOCAL] {
            assert!(
                all.contains(canary),
                "ordinary capture missing {canary}, cold={is_cold}: {all}"
            );
        }
        for canary in [SOURCE_DOMAIN, TARGET_DOMAIN] {
            assert_eq!(
                all.contains(canary),
                !is_cold,
                "ordinary domain capture, cold={is_cold}: {all}"
            );
        }
    }
    party.delete_items_since(test, &before).await;
    admin.registry_update_object(ObjectType::Domain,source,json!({"catchAllAddress":saved_source.catch_all_address,"subAddressing":saved_source.sub_addressing})).await;
    admin.registry_update_object(ObjectType::Domain,target,json!({"catchAllAddress":saved_target.catch_all_address,"subAddressing":saved_target.sub_addressing})).await;
    cold().await;
    common::za::test_hooks::set_delegation_lookup_fail_once(None);
    println!(
        "catch_all_resolution_is_quiet: cold/warm classification, receiver and ordinary controls passed"
    );
}

async fn delegated_receiver_rejections_and_reply(test: &TestServer) {
    use super::{
        drain::{DrainOn, body},
        fanout::{NO_REPLY, Party, START_A, invite, uid_copies},
        mail::Traced,
    };
    use crate::utils::za::{
        PlantedItem, copy_href, inbox_ids, inbox_items, plant_inbox_item, plant_sealed_event,
        trigger_drain,
    };
    use groupware::calendar::{ChangedBy, EVENT_NOTIFICATION_ZA_PENDING, seal::TokenPairs};
    let _drain = DrainOn::new();
    let owner = Party::new(test, "key1@example.com");
    let writer = Party::new(test, "key2@example.com");
    let recipient = Party::new(test, "key3@example.com");
    let before = inbox_ids(test, writer.id).await;
    for (label, named, sent_by, changed_by, expected) in [
        ("missing", "key1@example.com", None, writer.id, false),
        (
            "wrong",
            "key1@example.com",
            Some("mailto:key3@example.com"),
            writer.id,
            false,
        ),
        (
            "nonmailto",
            "key1@example.com",
            Some("https:key2@example.com"),
            writer.id,
            false,
        ),
        (
            "malformed",
            "key1@example.com",
            Some("mailto:invalid-address"),
            writer.id,
            false,
        ),
        (
            "data-uri",
            "key1@example.com",
            Some("data:mailto:key2@example.com;base64,YQ=="),
            writer.id,
            false,
        ),
        (
            "same",
            "key2@example.com",
            Some("mailto:key2@example.com"),
            writer.id,
            false,
        ),
        (
            "plain",
            "admin@example.com",
            Some("mailto:key2@example.com"),
            writer.id,
            false,
        ),
        (
            "nonlocal",
            "task9-unknown@not-local.test",
            Some("mailto:key2@example.com"),
            writer.id,
            false,
        ),
        (
            "changed-by",
            "key1@example.com",
            Some("mailto:key2@example.com"),
            owner.id,
            false,
        ),
        (
            "encoded",
            "key1@example.com",
            Some("MAILTO:key%32%40example.com"),
            writer.id,
            true,
        ),
    ] {
        let uid = format!("task9-receiver-{label}");
        let tree = invite(&uid, START_A, &[writer.name])
            .replace("VERSION:2.0", "VERSION:2.0\r\nMETHOD:REQUEST")
            .replace(
                "ORGANIZER:mailto:key1@example.com",
                &format!(
                    "ORGANIZER{}:mailto:{named}",
                    sent_by
                        .map(|sent_by| format!(";SENT-BY=\"{sent_by}\""))
                        .unwrap_or_default()
                ),
            );
        let id = plant_inbox_item(
            test,
            writer.id,
            &tree,
            PlantedItem::Pending {
                pair: Some((writer.name, [0x44; 16])),
                reply_token: None,
            },
            changed_by,
            None,
        )
        .await;
        let traced = Traced::start(trc::EventType::variants());
        trigger_drain(writer.name, STRONG).await;
        let events = traced.finish().await;
        assert_eq!(
            uid_copies(test, writer.id, &uid).await,
            u64::from(expected),
            "{label}"
        );
        let item = inbox_items(test, writer.id)
            .await
            .into_iter()
            .find(|(item_id, _)| *item_id == id)
            .unwrap()
            .1;
        assert_eq!(
            item.flags & EVENT_NOTIFICATION_ZA_PENDING,
            0,
            "{label}: history or imported item is drained"
        );
        assert!(matches!(item.changed_by,ChangedBy::PrincipalId(id) if id==changed_by));
        if !expected && sent_by.is_some() {
            let refused = events
                .iter()
                .filter(|event| event.reason == "zero-access: delegated sender not verified")
                .collect::<Vec<_>>();
            assert_eq!(refused.len(), 1, "{label}: {events:?}");
            for event in refused {
                for forbidden in [
                    named,
                    "key1@example.com",
                    "key2@example.com",
                    "key3@example.com",
                    &uid,
                    "SUMMARY",
                    "BEGIN:VCALENDAR",
                ] {
                    assert!(
                        !event.text.contains(forbidden),
                        "fixed delegated refusal leaks content: {}",
                        event.text
                    );
                }
            }
        }
        if expected {
            let href = copy_href(test, writer.id, &uid).await;
            writer
                .client
                .request_with_headers("DELETE", &href, [NO_REPLY], "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }
    writer.delete_items_since(test, &before).await;

    // A delegated REPLY must merge the named owner's attendee line and match
    // that line's token, while SENT-BY authenticates the writer separately.
    test.server
        .get_or_create_default_calendar(recipient.id, recipient.id)
        .await
        .unwrap();
    let uid = "task9-owner-named-reply";
    let text = invite(uid, START_A, &[owner.name]).replace(
        "ORGANIZER:mailto:key1@example.com",
        "ORGANIZER:mailto:key3@example.com",
    );
    let mut pairs = TokenPairs::new();
    pairs.insert(owner.name, [0x72; 16]);
    plant_sealed_event(
        test,
        recipient.id,
        "default",
        "task9-reply.ics",
        &text,
        STRONG,
        &pairs,
    )
    .await;
    let reply = text
        .replace("VERSION:2.0", "VERSION:2.0\r\nMETHOD:REPLY")
        .replace(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:",
            "ATTENDEE;PARTSTAT=ACCEPTED;SENT-BY=\"mailto:key2@example.com\":",
        );
    let recipient_start = inbox_ids(test, recipient.id).await;
    plant_inbox_item(
        test,
        recipient.id,
        &reply,
        PlantedItem::Pending {
            pair: None,
            reply_token: Some([0x72; 16]),
        },
        writer.id,
        None,
    )
    .await;
    trigger_drain(recipient.name, STRONG).await;
    let href = copy_href(test, recipient.id, uid).await;
    let merged = body(&recipient.client, &href).await;
    assert_eq!(
        super::drain::partstat_of(&recipient.client, &href, owner.name)
            .await
            .as_deref(),
        Some("ACCEPTED")
    );
    assert!(!merged.contains("ATTENDEE;PARTSTAT=ACCEPTED:mailto:key2@example.com"));
    recipient
        .client
        .request_with_headers("DELETE", &href, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    recipient.delete_items_since(test, &recipient_start).await;
    println!(
        "Direct delegated receiver malformed/identity refusals, encoded mailto and owner-named REPLY passed"
    );
}

async fn delegated_collection_lifecycle(test: &TestServer, owner_id: u32, writer_id: u32) {
    use super::{
        drain::body,
        fanout::{MKCALENDAR, NO_REPLY, Party, uid_copies},
    };
    use crate::utils::za::{copy_href, inbox_ids, trigger_drain, za_post};
    use common::auth::BuildAccessToken;
    use serde_json::json;
    const CALENDAR: &str = "task9-lifecycle";
    const PATH: &str = "/dav/cal/task9-alice%40example.com/task9-lifecycle/";
    let owner_client = Party::new(test, DELEGATE_OWNER);
    let writer_client = Party::new(test, DELEGATE_WRITER);
    let outside = Party::new(test, "key3@example.com");
    owner_client
        .client
        .request("MKCALENDAR", PATH, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    let writer_key = test
        .server
        .za_served_public_key(writer_id)
        .await
        .unwrap()
        .unwrap();
    super::pinning::pin(
        DELEGATE_OWNER,
        DELEGATE_WRITER,
        &vault::keys::display_fingerprint(&writer_key),
    )
    .await
    .expect(200);
    let grant = json!({"username":DELEGATE_OWNER,"password":STRONG,"calendar":CALENDAR,"address":DELEGATE_WRITER,"level":"edit"});
    let revoke =
        json!({"username":DELEGATE_OWNER,"password":STRONG,"calendar":CALENDAR,"grant":writer_id});
    za_post("shares/grant", &grant).await.expect(200);
    let owner = test.server.account_info(owner_id).await.unwrap();
    let writer = test.server.account_info(writer_id).await.unwrap();
    let keys = za_unlock(test, writer_id, STRONG).await;
    let before = inbox_ids(test, writer_id).await;
    let tree = delegated_tree("task9-after-revoke", &[DELEGATE_WRITER]);
    let (_, result, _) =
        delegated_run(test, &owner, &writer, &keys, 0, None, tree.clone(), None).await;
    za_post("shares/revoke", &revoke).await.expect(200);
    trigger_drain(DELEGATE_WRITER, STRONG).await;
    assert_eq!(
        uid_copies(test, writer_id, "task9-after-revoke").await,
        1,
        "authorized queued item imports after revoke"
    );
    za_post("shares/grant", &grant).await.expect(200);
    // Store a real source under its current collection key via the owner PUT;
    // the direct sender below queues its CANCEL before that source disappears.
    owner_client
        .client
        .request_with_headers(
            "PUT",
            &format!("{PATH}source.ics"),
            [super::fanout::CONTENT_TYPE],
            tree.to_string(),
        )
        .await
        .with_status(StatusCode::CREATED);
    let (_, _, _) = delegated_run(
        test,
        &owner,
        &writer,
        &keys,
        2,
        Some(&tree),
        tree.clone(),
        result.pairs,
    )
    .await;
    owner_client
        .client
        .request_with_headers("DELETE", &format!("{PATH}source.ics"), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    za_post("shares/revoke", &revoke).await.expect(200);
    trigger_drain(DELEGATE_WRITER, STRONG).await;
    let href = copy_href(test, writer_id, "task9-after-revoke").await;
    assert!(
        body(&writer_client.client, &href)
            .await
            .contains("STATUS:CANCELLED"),
        "queued delegated CANCEL survives source deletion and revoke"
    );
    writer_client
        .client
        .request_with_headers("DELETE", &href, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    writer_client.delete_items_since(test, &before).await;

    // An own-name organizer's reply has no source in the writer account and
    // remains EventNotFound history, the accepted inherited limitation.
    test.server
        .get_or_create_default_calendar(writer_id, writer_id)
        .await
        .unwrap();
    let own = delegated_tree("task9-own-name-reply", &["key3@example.com"])
        .to_string()
        .replace(DELEGATE_OWNER, DELEGATE_WRITER);
    owner_client
        .client
        .request_with_headers(
            "PUT",
            &format!("{PATH}own-name.ics"),
            [super::fanout::CONTENT_TYPE],
            &own,
        )
        .await
        .with_status(StatusCode::CREATED);
    let reply = own
        .replace("VERSION:2.0", "VERSION:2.0\r\nMETHOD:REPLY")
        .replace("PARTSTAT=NEEDS-ACTION", "PARTSTAT=ACCEPTED");
    let own_reply_id = crate::utils::za::plant_inbox_item(
        test,
        writer_id,
        &reply,
        crate::utils::za::PlantedItem::Pending {
            pair: None,
            reply_token: Some([0x32; 16]),
        },
        outside.id,
        None,
    )
    .await;
    trigger_drain(DELEGATE_WRITER, STRONG).await;
    assert_eq!(uid_copies(test, writer_id, "task9-own-name-reply").await, 0);
    let own_reply = crate::utils::za::inbox_items(test, writer_id)
        .await
        .into_iter()
        .find(|(id, _)| *id == own_reply_id)
        .expect("own-name EventNotFound reply remains history")
        .1;
    assert_eq!(
        own_reply.flags & groupware::calendar::EVENT_NOTIFICATION_ZA_PENDING,
        0
    );
    assert!(own_reply.event_id.is_none());
    owner_client
        .client
        .request_with_headers("DELETE", &format!("{PATH}own-name.ics"), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    writer_client.delete_items_since(test, &before).await;

    za_post("shares/grant", &grant).await.expect(200);
    let container_uid = "task9-container-active";
    owner_client
        .client
        .request_with_headers(
            "PUT",
            &format!("{PATH}container.ics"),
            [super::fanout::CONTENT_TYPE],
            delegated_tree(container_uid, &[DELEGATE_WRITER]).to_string(),
        )
        .await
        .with_status(StatusCode::CREATED);
    // Both caches must be resident immediately before the real owner
    // collection DELETE; unrelated key sessions are the precision control.
    writer_client
        .client
        .request("PROPFIND", "/dav/cal/task9-bob%40example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    outside
        .client
        .request("PROPFIND", "/dav/cal/key3%40example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let token = test.server.access_token(writer_id).await.unwrap().build();
    assert!(test.server.inner.cache.za_keys.contains_account(writer_id));
    assert!(
        test.server
            .inner
            .cache
            .access_tokens
            .get(&writer_id)
            .is_some()
    );
    assert!(test.server.inner.cache.za_keys.contains_account(outside.id));
    owner_client
        .client
        .request_with_headers(
            "DELETE",
            PATH,
            [NO_REPLY, ("if-match", "\"task9-not-current\"")],
            "",
        )
        .await
        .with_status(StatusCode::PRECONDITION_FAILED);
    assert!(
        test.server.inner.cache.za_keys.contains_account(writer_id),
        "a rejected collection DELETE must not evict a grantee"
    );
    assert!(
        test.server
            .inner
            .cache
            .access_tokens
            .get(&writer_id)
            .is_some()
    );
    owner_client
        .client
        .request("DELETE", PATH, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        !test.server.inner.cache.za_keys.contains_account(writer_id),
        "owner shared-calendar DELETE must evict resident grantee keys after commit"
    );
    assert!(
        test.server
            .inner
            .cache
            .access_tokens
            .get(&writer_id)
            .is_none(),
        "owner shared-calendar DELETE invalidates grantee access tokens"
    );
    assert!(
        test.server.inner.cache.za_keys.contains_account(outside.id),
        "unaffected account's key cache remains resident"
    );
    trigger_drain(DELEGATE_WRITER, STRONG).await;
    let href = copy_href(test, writer_id, container_uid).await;
    assert!(
        body(&writer_client.client, &href)
            .await
            .contains("STATUS:CANCELLED"),
        "owner container DELETE delivers its queued cancellation after the shared source is gone"
    );
    writer_client
        .client
        .request_with_headers("DELETE", &href, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    drop(token);
    writer_client.delete_items_since(test, &before).await;
    println!(
        "Direct revoke/source-deletion delivery, own-name reply history and owner collection DELETE cache eviction passed"
    );
}

async fn delegated_attendee_and_alias_edges(test: &TestServer, owner_id: u32, writer_id: u32) {
    use super::{
        drain::{body, partstat_of},
        fanout::{NO_REPLY, Party, uid_copies},
    };
    use crate::utils::za::{
        copy_href, inbox_ids, new_inbox_items, open_pending_item_as, plant_sealed_event,
        trigger_drain,
    };
    use calcard::icalendar::{
        ICalendarParameter, ICalendarParameterName, ICalendarParameterValue,
        ICalendarParticipationStatus, ICalendarProperty,
    };
    use groupware::{
        calendar::{delegation::*, fanout::*, itip::ItipSendStatus, seal::TokenPairs},
        scheduling::{ItipError, event_update::itip_update},
    };
    let owner = test.server.account_info(owner_id).await.unwrap();
    let writer = test.server.account_info(writer_id).await.unwrap();
    let sender = test
        .server
        .scheduling_account_info(writer_id, owner_id)
        .await
        .unwrap();
    let keys = za_unlock(test, writer_id, STRONG).await;
    let writer_client = Party::new(test, DELEGATE_WRITER);
    let owner_client = Party::new(test, DELEGATE_OWNER);
    let recipient = Party::new(test, "key3@example.com");
    let before = inbox_ids(test, writer_id).await;
    let set_declined = |tree: &mut calcard::icalendar::ICalendar| {
        for entry in tree
            .components
            .iter_mut()
            .flat_map(|component| &mut component.entries)
            .filter(|entry| entry.name == ICalendarProperty::Attendee)
        {
            entry.params.retain(|parameter| {
                !matches!(
                    parameter.name,
                    ICalendarParameterName::Partstat | ICalendarParameterName::Rsvp
                )
            });
            entry.params.push(ICalendarParameter {
                name: ICalendarParameterName::Partstat,
                value: ICalendarParameterValue::Partstat(ICalendarParticipationStatus::Declined),
            });
            entry.params.push(ICalendarParameter {
                name: ICalendarParameterName::Rsvp,
                value: ICalendarParameterValue::Bool(false),
            });
        }
    };
    for remove in [true, false] {
        let uid = if remove {
            "task9-declined-alias-remove"
        } else {
            "task9-declined-alias-delete"
        };
        let mut tree = delegated_tree(uid, &[DELEGATE_ALIAS]);
        set_declined(&mut tree);
        let (old, result, counts) =
            delegated_run(test, &owner, &writer, &keys, 0, None, tree, None).await;
        assert_eq!(counts, vec![1]);
        assert_eq!(result.statuses[0].status, "1.2");
        trigger_drain(DELEGATE_WRITER, STRONG).await;
        assert_eq!(uid_copies(test, writer_id, uid).await, 1);
        let new = if remove {
            delegated_tree(uid, &[])
        } else {
            old.clone()
        };
        let (_, result, counts) = delegated_run(
            test,
            &owner,
            &writer,
            &keys,
            if remove { 1 } else { 2 },
            Some(&old),
            new,
            result.pairs,
        )
        .await;
        assert_eq!(counts, vec![1]);
        assert_eq!(result.statuses[0].status, "1.2");
        trigger_drain(DELEGATE_WRITER, STRONG).await;
        let href = copy_href(test, writer_id, uid).await;
        assert!(
            body(&writer_client.client, &href)
                .await
                .contains("STATUS:CANCELLED")
        );
        writer_client
            .client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
        writer_client.delete_items_since(test, &before).await;
    }
    let old = delegated_tree("task9-no-message", &[DELEGATE_WRITER]);
    let resolution = za_resolve_delegation(
        &test.server,
        &sender,
        &writer,
        &owner,
        ZaSelfOperation::Update,
        Some(&old),
        Some(&old),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(
        za_self_update(&old, &old, &writer, &owner, &resolution),
        Err(ItipError::NothingToSend)
    ));
    let (_, result, counts) = delegated_run(
        test,
        &owner,
        &writer,
        &keys,
        1,
        Some(&old),
        old.clone(),
        None,
    )
    .await;
    assert!(counts.is_empty() && result.statuses.is_empty());
    assert!(new_inbox_items(test, writer_id, &before).await.is_empty());
    let no_permission = common::auth::AccessToken::from_permissions(writer_id, []);
    assert!(matches!(
        ItipSendStatus::resolve(&test.server, &no_permission, &sender, i64::MAX, true),
        ItipSendStatus::NoPermission
    ));

    // A retained self message still needs the writer's receive permission.
    let admin = test.account("admin@example.com").clone();
    let writer_account_id = test.account(DELEGATE_WRITER).id();
    let registry::schema::structs::Account::User(original) = test
        .server
        .registry()
        .object::<registry::schema::structs::Account>(writer_account_id)
        .await
        .unwrap()
        .unwrap()
    else {
        unreachable!()
    };
    let denied =
        registry::schema::structs::Permissions::Merge(registry::schema::structs::PermissionsList {
            enabled_permissions: registry::types::map::Map::new(user_permissions()),
            disabled_permissions: registry::types::map::Map::new(vec![
                registry::schema::enums::Permission::CalendarSchedulingReceive,
            ]),
        });
    admin
        .registry_update_object(
            registry::schema::prelude::ObjectType::Account,
            writer_account_id,
            serde_json::json!({"permissions":denied}),
        )
        .await;
    let (_, denied_result, counts) = delegated_run(
        test,
        &owner,
        &writer,
        &keys,
        0,
        None,
        delegated_tree("task9-no-receive", &[DELEGATE_WRITER]),
        None,
    )
    .await;
    admin
        .registry_update_object(
            registry::schema::prelude::ObjectType::Account,
            writer_account_id,
            serde_json::json!({"permissions":original.permissions}),
        )
        .await;
    assert_eq!(counts, vec![1]);
    assert_eq!(denied_result.statuses[0].status, "5.3");
    assert!(new_inbox_items(test, writer_id, &before).await.is_empty());

    // Empty attendee-delete fallback must not inspect a colliding writer
    // withheld source or synthesize an additional delegated decline.
    let mut record = crate::utils::za::pin_record(test, writer_id, STRONG)
        .await
        .unwrap();
    record.refuse(900_001, "task9-fallback", "key3@example.com", 22, None);
    let entry = record
        .entry(900_001, "task9-fallback", "key3@example.com")
        .unwrap()
        .clone();
    crate::utils::za::write_pin_record(test, writer_id, STRONG, &record).await;
    let fallback_text = delegated_tree("task9-fallback", &[DELEGATE_OWNER])
        .to_string()
        .replace(
            &format!("ORGANIZER:mailto:{DELEGATE_OWNER}"),
            "ORGANIZER:mailto:key3@example.com",
        );
    let calcard::Entry::ICalendar(fallback) = calcard::Parser::new(&fallback_text).entry() else {
        unreachable!()
    };
    let mut fallback_pairs = TokenPairs::new();
    fallback_pairs.insert(DELEGATE_OWNER, [0x38; 16]);
    let fallback_result = test
        .server
        .za_fan_out(
            &sender,
            &writer,
            &owner,
            &keys,
            vec![],
            Some(fallback_pairs),
            &fallback,
            ZaSource {
                document_id: 900_001,
                uid: "task9-fallback",
            },
            ZaFanOutKind::Deletion,
            None,
            None,
        )
        .await
        .unwrap();
    assert!(fallback_result.statuses.is_empty());
    let mut record = crate::utils::za::pin_record(test, writer_id, STRONG)
        .await
        .unwrap();
    assert!(record.entry(900_001, "task9-fallback", "key3@example.com") == Some(&entry));
    record.withheld.clear();
    crate::utils::za::write_pin_record(test, writer_id, STRONG, &record).await;

    // Combined upstream and delegated-self messages both survive one
    // reconciliation; writer delivery proves its tag stayed on its payload.
    let mixed_uid = "task9-mixed-provenance";
    let old = delegated_tree(mixed_uid, &[DELEGATE_WRITER, "key3@example.com"]);
    let mut changed = old.clone();
    for entry in changed
        .components
        .iter_mut()
        .flat_map(|component| &mut component.entries)
    {
        if entry.name == ICalendarProperty::Summary {
            entry.values = vec![calcard::icalendar::ICalendarValue::Text(
                "task9 mixed changed".into(),
            )];
        }
    }
    let recipient_before = inbox_ids(test, recipient.id).await;
    let (_, mixed_result, counts) =
        delegated_run(test, &owner, &writer, &keys, 1, Some(&old), changed, None).await;
    assert_eq!(counts, vec![1]);
    assert_eq!(mixed_result.statuses.len(), 2);
    assert!(
        mixed_result
            .statuses
            .iter()
            .all(|status| status.status == "1.2")
    );
    assert_eq!(new_inbox_items(test, writer_id, &before).await.len(), 1);
    assert_eq!(
        new_inbox_items(test, recipient.id, &recipient_before)
            .await
            .len(),
        1
    );
    for party in [&writer_client, &recipient] {
        trigger_drain(party.name, STRONG).await;
        let href = copy_href(test, party.id, mixed_uid).await;
        party
            .client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    writer_client.delete_items_since(test, &before).await;
    recipient.delete_items_since(test, &recipient_before).await;

    // The attendee side has no second run. It sends the owner's named REPLY
    // with the writer marked separately, and merges against the owner's pair.
    test.server
        .get_or_create_default_calendar(recipient.id, recipient.id)
        .await
        .unwrap();
    let uid = "task9-attendee-delegation";
    let text = delegated_tree(uid, &[DELEGATE_OWNER]).to_string().replace(
        &format!("ORGANIZER:mailto:{DELEGATE_OWNER}"),
        "ORGANIZER:mailto:key3@example.com",
    );
    let calcard::Entry::ICalendar(old) = calcard::Parser::new(&text).entry() else {
        unreachable!()
    };
    let calcard::Entry::ICalendar(mut new) =
        calcard::Parser::new(&text.replace("PARTSTAT=NEEDS-ACTION", "PARTSTAT=ACCEPTED")).entry()
    else {
        unreachable!()
    };
    assert!(
        za_resolve_delegation(
            &test.server,
            &sender,
            &writer,
            &owner,
            ZaSelfOperation::Update,
            Some(&old),
            Some(&new)
        )
        .await
        .unwrap()
        .is_none()
    );
    let messages = itip_update(&mut new, &old, sender.addresses()).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].from, DELEGATE_OWNER);
    let mut pairs = TokenPairs::new();
    pairs.insert(DELEGATE_OWNER, [0x37; 16]);
    plant_sealed_event(
        test,
        recipient.id,
        "default",
        "task9-attendee.ics",
        &text,
        STRONG,
        &pairs,
    )
    .await;
    let recipient_before = inbox_ids(test, recipient.id).await;
    let result = test
        .server
        .za_fan_out(
            &sender,
            &writer,
            &owner,
            &keys,
            messages
                .into_iter()
                .map(ZaFanOutMessage::upstream)
                .collect(),
            Some(pairs.clone()),
            &new,
            ZaSource {
                document_id: 900_001,
                uid,
            },
            ZaFanOutKind::Change,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.statuses[0].status, "1.2");
    assert!(result.statuses[0].on_organizer);
    let items = new_inbox_items(test, recipient.id, &recipient_before).await;
    assert_eq!(items.len(), 1);
    let (payload, _) = open_pending_item_as(test, recipient.id, STRONG, items[0].0).await;
    assert!(
        payload
            .components
            .iter()
            .flat_map(|component| &component.entries)
            .filter(|entry| entry.name == ICalendarProperty::Attendee
                && entry.values.first().and_then(|value| value.as_text())
                    == Some("mailto:task9-alice@example.com"))
            .all(|entry| entry
                .params
                .iter()
                .any(|parameter| parameter.name == ICalendarParameterName::SentBy
                    && parameter.value.as_text() == Some("mailto:task9-bob@example.com")))
    );
    trigger_drain(recipient.name, STRONG).await;
    let href = copy_href(test, recipient.id, uid).await;
    assert_eq!(
        partstat_of(&recipient.client, &href, DELEGATE_OWNER)
            .await
            .as_deref(),
        Some("ACCEPTED")
    );
    recipient
        .client
        .request_with_headers("DELETE", &href, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    recipient.delete_items_since(test, &recipient_before).await;

    // Accepted fork limitation: attendee-side owner-subaddress delivery is
    // outside the organizer gate. The owner's own send would exclude itself.
    let text = text
        .replace(
            "task9-attendee-delegation",
            "task9-attendee-owner-subaddress",
        )
        .replace(
            "ORGANIZER:mailto:key3@example.com",
            "ORGANIZER:mailto:task9-alice+reply@example.com",
        );
    let calcard::Entry::ICalendar(old) = calcard::Parser::new(&text).entry() else {
        unreachable!()
    };
    let calcard::Entry::ICalendar(mut new) =
        calcard::Parser::new(&text.replace("PARTSTAT=NEEDS-ACTION", "PARTSTAT=ACCEPTED")).entry()
    else {
        unreachable!()
    };
    assert!(
        za_resolve_delegation(
            &test.server,
            &sender,
            &writer,
            &owner,
            ZaSelfOperation::Update,
            Some(&old),
            Some(&new)
        )
        .await
        .unwrap()
        .is_none()
    );
    let messages = itip_update(&mut new, &old, sender.addresses()).unwrap();
    let owner_before = inbox_ids(test, owner_id).await;
    let result = test
        .server
        .za_fan_out(
            &sender,
            &writer,
            &owner,
            &keys,
            messages
                .into_iter()
                .map(ZaFanOutMessage::upstream)
                .collect(),
            Some(pairs.clone()),
            &new,
            ZaSource {
                document_id: 900_001,
                uid: "task9-subaddress",
            },
            ZaFanOutKind::Change,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.statuses[0].status, "1.2");
    assert_eq!(
        new_inbox_items(test, owner_id, &owner_before).await.len(),
        1
    );
    let mut control = new.clone();
    let messages = itip_update(&mut control, &old, owner.addresses()).unwrap();
    let owner_keys = za_unlock(test, owner_id, STRONG).await;
    let result = test
        .server
        .za_fan_out(
            &owner,
            &owner,
            &owner,
            &owner_keys,
            messages
                .into_iter()
                .map(ZaFanOutMessage::upstream)
                .collect(),
            Some(pairs),
            &control,
            ZaSource {
                document_id: 900_001,
                uid: "task9-subaddress",
            },
            ZaFanOutKind::Change,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.statuses[0].status, "5.3");
    owner_client.delete_items_since(test, &owner_before).await;
    println!(
        "Direct declined alias removal/delete, no-message/send permission and attendee-side owner-named/subaddress fixtures passed"
    );
}

async fn delegated_owner_exclusion_precedes_eligibility(
    test: &TestServer,
    owner: &common::auth::AccountInfo,
    writer: &common::auth::AccountInfo,
    keys: &vault::session::SessionKeys,
) {
    use common::auth::BuildAccessToken;
    use groupware::{
        calendar::{delegation::*, fanout::*},
        scheduling::event_create::itip_create,
    };
    const ADDRESS: &str = "task9-alice+store-canary@example.com";
    let sender = test
        .server
        .scheduling_account_info(writer.account_id, owner.account_id)
        .await
        .unwrap();
    let input = delegated_tree("task9-owner-store-failure", &[ADDRESS]);
    let resolution = za_resolve_delegation(
        &test.server,
        &sender,
        writer,
        owner,
        ZaSelfOperation::Create,
        None,
        Some(&input),
    )
    .await
    .unwrap()
    .unwrap();
    // Keep all identity and access-token caches warm. Only the clone's data
    // Store is unavailable; it must never be needed for a known owner target.
    test.server
        .access_token(owner.account_id)
        .await
        .unwrap()
        .build();
    let mut broken = test.server.clone();
    std::sync::Arc::make_mut(&mut broken.core).storage.data = store::Store::None;
    let second = za_self_create(&input, writer, owner, &resolution).unwrap();
    let mut tree = input.clone();
    let first = itip_create(&mut tree, sender.addresses()).unwrap();
    let result = broken
        .za_fan_out(
            &sender,
            writer,
            owner,
            keys,
            za_combine_delegated(first, second, &resolution),
            None,
            &tree,
            ZaSource {
                document_id: 900_001,
                uid: "task9-owner-store-failure",
            },
            ZaFanOutKind::Change,
            None,
            Some(&resolution),
        )
        .await
        .unwrap();
    assert_eq!(result.statuses.len(), 1);
    assert_eq!(
        result.statuses[0].status, "5.3",
        "known owner exclusion must precede receive/key eligibility reads"
    );
    let _reset = DelegationLookupReset;
    common::za::test_hooks::set_delegation_lookup_fail_once(Some((
        writer.account_id,
        ADDRESS.into(),
    )));
    let resolution = za_resolve_delegation(
        &test.server,
        &sender,
        writer,
        owner,
        ZaSelfOperation::Create,
        None,
        Some(&input),
    )
    .await
    .unwrap()
    .unwrap();
    let second = za_self_create(&input, writer, owner, &resolution).unwrap();
    let mut tree = input.clone();
    let first = itip_create(&mut tree, sender.addresses()).unwrap();
    let result = broken
        .za_fan_out(
            &sender,
            writer,
            owner,
            keys,
            za_combine_delegated(first, second, &resolution),
            None,
            &tree,
            ZaSource {
                document_id: 900_001,
                uid: "task9-owner-store-failure",
            },
            ZaFanOutKind::Change,
            None,
            Some(&resolution),
        )
        .await
        .unwrap();
    assert_eq!(
        result.statuses[0].status, "5.1",
        "a raw Failed lookup has no known owner identity and retains failure"
    );
    assert!(
        broken
            .za_fan_out(
                &sender,
                writer,
                writer,
                keys,
                vec![],
                None,
                &tree,
                ZaSource {
                    document_id: 900_001,
                    uid: "task9-owner-store-failure"
                },
                ZaFanOutKind::Change,
                None,
                Some(&resolution)
            )
            .await
            .is_err(),
        "mismatched owner resolution is unauthorized before effects"
    );
    println!(
        "Known owner 5.3 exclusion precedes unavailable eligibility store; unknown Failed remains 5.1"
    );
}

async fn delegated_schedule_agents_remain_authoritative(
    test: &TestServer,
    owner: &common::auth::AccountInfo,
    writer: &common::auth::AccountInfo,
    keys: &vault::session::SessionKeys,
) {
    use super::fanout::{Party, uid_copies};
    use crate::utils::za::{
        PlantedItem, inbox_ids, inbox_items, new_inbox_items, plant_inbox_item, trigger_drain,
    };
    use calcard::icalendar::{
        ICalendarParameter, ICalendarParameterName, ICalendarParameterValue, ICalendarProperty,
        ICalendarScheduleAgentValue,
    };
    let party = Party::new(test, DELEGATE_WRITER);
    let before = inbox_ids(test, writer.account_id).await;
    for (agent, label) in [
        (ICalendarScheduleAgentValue::Client, "client"),
        (ICalendarScheduleAgentValue::None, "none"),
    ] {
        let uid = format!("task9-agent-{label}");
        let mut tree = delegated_tree(&uid, &[DELEGATE_ALIAS]);
        for entry in tree
            .components
            .iter_mut()
            .flat_map(|component| &mut component.entries)
            .filter(|entry| entry.name == ICalendarProperty::Attendee)
        {
            entry.params.push(ICalendarParameter {
                name: ICalendarParameterName::ScheduleAgent,
                value: ICalendarParameterValue::ScheduleAgent(agent.clone()),
            });
        }
        let (_, result, counts) =
            delegated_run(test, owner, writer, keys, 0, None, tree.clone(), None).await;
        assert!(counts.is_empty() && result.statuses.is_empty());
        assert!(
            new_inbox_items(test, writer.account_id, &before)
                .await
                .is_empty()
        );
        let text = tree
            .to_string()
            .replace("VERSION:2.0", "VERSION:2.0\r\nMETHOD:REQUEST")
            .replace(
                "ORGANIZER:mailto:",
                "ORGANIZER;SENT-BY=\"mailto:task9-bob@example.com\":mailto:",
            );
        let id = plant_inbox_item(
            test,
            writer.account_id,
            &text,
            PlantedItem::Pending {
                pair: Some((DELEGATE_ALIAS, [0x41; 16])),
                reply_token: None,
            },
            writer.account_id,
            None,
        )
        .await;
        trigger_drain(DELEGATE_WRITER, STRONG).await;
        assert_eq!(
            uid_copies(test, writer.account_id, &uid).await,
            0,
            "incoming scheduling-agent validation remains unchanged"
        );
        let item = inbox_items(test, writer.account_id)
            .await
            .into_iter()
            .find(|(item_id, _)| *item_id == id)
            .unwrap()
            .1;
        assert_eq!(
            item.flags & groupware::calendar::EVENT_NOTIFICATION_ZA_PENDING,
            0
        );
        party.delete_items_since(test, &before).await;
    }
    println!(
        "Direct CLIENT/NONE listed aliases remain unsent; incoming snapshot validation rejects planted copies"
    );
}
