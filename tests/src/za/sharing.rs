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
