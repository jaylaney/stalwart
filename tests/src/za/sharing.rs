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

async fn conversion_remove_account(
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
