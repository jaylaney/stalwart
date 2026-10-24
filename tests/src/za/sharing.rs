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

// The calendar and generation a stored event's root `X-ZA-KEY` names when
// it is a CK envelope (spec 3.4: policy version, `0x03`, calendar (4),
// generation (4), wrapped DEK (72)); `None` for any other wrap or shape.
fn ck_envelope(event: &CalendarEvent) -> Option<(u32, u32)> {
    use groupware::calendar::{
        seal::tree::{KEY_PROP, entry_text, is_carrier},
        sharing::format::WRAP_CK,
    };
    let entry = event.data.event.components.first()?.entries.last()?;
    if !is_carrier(entry, KEY_PROP) {
        return None;
    }
    let bytes = STANDARD.decode(entry_text(entry)?.trim()).ok()?;
    (bytes.len() == 82 && bytes[..2] == [1, WRAP_CK]).then(|| {
        (
            u32::from_be_bytes(bytes[2..6].try_into().unwrap()),
            u32::from_be_bytes(bytes[6..10].try_into().unwrap()),
        )
    })
}

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
    grantee_dav_admission(test).await;
    sharing_lifecycle_differential(test).await;
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
    DummyWebDavClient::new(reader_id, READER, STRONG, READER)
        .request("GET", &format!("{HOME}c/legacy.ics"), "")
        .await
        .with_status(StatusCode::OK);
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
    // Bob's independently tagged future overwrite exercises the outer Option even
    // when the inner CANCEL would be empty for the unmerged writer identity.
    put(
        format!("{HOME}c/bob-overwrite.ics"),
        super::fanout::invite("move-bob-overwritten", "20990102T090000Z", &[READER])
            .replace("key1@example.com", USER),
    )
    .await;
    put(
        format!("{HOME}d/bob-source.ics"),
        put_body("move-bob-source"),
    )
    .await;
    let _drain = super::drain::DrainOn::new();
    assert_eq!(crate::utils::za::trigger_drain(READER, STRONG).await, 207);
    let bob_copy = crate::utils::za::copy_href(test, reader_id, "move-bob-overwritten").await;
    let bob_copy_before = recipient
        .request("GET", &bob_copy, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    drop(_drain);
    let bob_target = resource_id(test, owner_id, "c/bob-overwrite.ics").await;
    test.server.notify_task_queue();
    let held = hold_task_consumer(test).await;
    let before = move_queue_state(test, owner_id, reader_id).await;
    recipient
        .request_with_headers(
            "MOVE",
            &format!("{HOME}d/bob-source.ics"),
            [("destination", format!("{HOME}c/bob-overwrite.ics").as_str())],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        test_hooks::take_move_overwrite_message(owner_id, bob_target),
        Some(false),
        "Bob future scheduled overwrite outer Option"
    );
    assert_eq!(move_queue_state(test, owner_id, reader_id).await, before);
    drop(held);
    assert_eq!(
        recipient
            .request("GET", &bob_copy, "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap(),
        bob_copy_before
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
    // Mutation34: the earlier sole-parent EWK row must remain untouched when
    // the complete post-phase-1 walk finds a later linked row. Observe before
    // the injected revert failure can be retried or rollback can hide a write.
    {
        let id = make(test, &client, owner_id, home, "walk-before-write").await;
        calendars.push("walk-before-write".into());
        let early = plant_sealed_event(
            test,
            owner_id,
            "walk-before-write",
            "early.ics",
            &EVENT.replace("share-read-UID", "walk-early"),
            STRONG,
            &TokenPairs::default(),
        )
        .await;
        let late = plant_sealed_event(
            test,
            owner_id,
            "walk-before-write",
            "late.ics",
            &EVENT.replace("share-read-UID", "walk-late"),
            STRONG,
            &TokenPairs::default(),
        )
        .await;
        assert!(
            early < late,
            "archive walk fixture must place the unlinked candidate first"
        );
        let before = event_archive(test, owner_id, early).await;
        let pause = Arc::new(Pause::new(owner_id));
        set_share_precheck_pause(Some(pause.clone()));
        set_share_revert_assert_fail(Some(ShareAssertFailure {
            owner_id,
            calendar_id: id,
            remaining: 1,
        }));
        let server = test.server.clone();
        let resident = keys.clone();
        let guards = conversion_guards(test, &keys).await;
        let handle =
            tokio::spawn(async move { za_convert_calendar(&server, &resident, id, &guards).await });
        conversion_wait(&pause).await;
        conversion_link(test, owner_id, late, other, true).await;
        pause.release.notify_one();
        let result = handle.await.unwrap();
        let after = event_archive(test, owner_id, early).await;
        assert_eq!(
            after.version, before.version,
            "mutation34: earlier unlinked EWK candidate changed before failed revert"
        );
        let raw = after.deserialize::<CalendarEvent>().unwrap();
        let key = raw.data.event.components[0].entries.last().unwrap().values[0]
            .as_text()
            .unwrap();
        let envelope = STANDARD.decode(key).unwrap();
        assert!(
            envelope.len() == 74 && envelope[..2] == [1, 1],
            "mutation34: earlier candidate must still have account envelope at failed revert"
        );
        conversion_conflict(result, 0, 0, false);
        println!(
            "Mutation34 oracle: earlier unlinked candidate archive unchanged and account envelope at failed revert, before rollback"
        );
        set_share_precheck_pause(None);
        set_share_revert_assert_fail(None);
        conversion_link(test, owner_id, late, other, false).await;
        let guards = conversion_guards(test, &keys).await;
        za_convert_calendar(&test.server, &keys, id, &guards)
            .await
            .unwrap();
    }
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
        // Spec 11.2 case 21: every event ends 0x03. The unseal below also
        // opens an EWK (0x01) row through the owner's legacy read path, so it
        // cannot tell a held write that committed under the pre-conversion
        // key domain: the stored envelope itself must be a CK envelope naming
        // this calendar and a generation its owner ring lists.
        let listed = sharing_http_ring(test, &keys, id)
            .await
            .expect("case 21: the converted calendar holds an owner ring")
            .0
            .generations;
        assert_eq!(
            crate::utils::za::envelope_wrap(&event.data.event),
            Some(groupware::calendar::sharing::format::WRAP_CK),
            "{slug}: case 21 requires the held write's event to end under a 0x03 envelope"
        );
        let (calendar, generation) =
            ck_envelope(&event).unwrap_or_else(|| panic!("{slug}: malformed 0x03 envelope"));
        assert_eq!(
            calendar, id,
            "{slug}: the 0x03 envelope must name this calendar"
        );
        assert!(
            listed.contains(&generation),
            "{slug}: envelope generation {generation} is not listed in {listed:?}"
        );
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
    let reader_client = DummyWebDavClient::new(reader_id, reader_name, STRONG, reader_name);
    for name in ["sealed.ics", "plain.ics"] {
        let body = reader_client
            .request("GET", &format!("{path}{name}"), "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap();
        assert!(body.contains("share-read-private-summary") && !body.contains("X-ZA-"));
    }
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
    // Spec 11.2 case 6: keep the reader's grant ring as stored before its
    // revoke, beside the owner ring's generations at that moment.
    let generations_before = sharing_http_ring(test, &owner_keys, id)
        .await
        .unwrap()
        .0
        .generations;
    let reader_calendar = za_owned_calendar_context(&test.server, &owner_keys, id)
        .await
        .unwrap()
        .stored
        .deserialize::<Calendar>()
        .unwrap();
    let saved_reader_ring = STANDARD
        .decode(
            reader_calendar
                .preferences
                .iter()
                .find(|p| p.account_id == reader)
                .expect("case 6: the reader holds a grant before its revoke")
                .name
                .strip_prefix("$za$")
                .unwrap()
                .split('|')
                .next()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(
        parse_grant_header(&saved_reader_ring).unwrap().generations,
        generations_before,
        "case 6: the reader's ring lists every generation before its revoke"
    );
    let revoke =
        json!({"username":USER,"password":STRONG,"calendar":"http-sharing","grant":reader});
    assert_eq!(
        za_post("shares/revoke", &revoke).await.expect(200)["changed"],
        true
    );
    // The revoke rotates in its own write (spec 7.2, invariant 37): exactly one
    // new generation, every earlier one kept, and the revoked reader's saved
    // ring never lists the new one.
    let generations_after = sharing_http_ring(test, &owner_keys, id)
        .await
        .unwrap()
        .0
        .generations;
    assert_eq!(
        generations_after.len(),
        generations_before.len() + 1,
        "case 6: a revoke must add exactly one generation"
    );
    assert_eq!(
        generations_after[..generations_before.len()],
        generations_before[..],
        "case 6: a revoke keeps every earlier generation"
    );
    let newest = *generations_after.last().unwrap();
    assert!(
        !parse_grant_header(&saved_reader_ring)
            .unwrap()
            .generations
            .contains(&newest),
        "case 6: the revoked reader's saved ring must not list the new generation"
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
    // Spec 11.2 case 6: an owner event written after the revoke is sealed under
    // the new generation, which the revoked reader never held.
    let owner_client = DummyWebDavClient::new(owner, USER, STRONG, USER);
    let post_revoke = "/dav/cal/shares-http-owner%40example.com/http-sharing/post-revoke.ics";
    owner_client
        .request_with_headers(
            "PUT",
            post_revoke,
            [("content-type", "text/calendar")],
            sharing_event("share-rotation-post-revoke"),
        )
        .await
        .with_status(StatusCode::CREATED);
    let post_revoke_id = resource_id(test, owner, "http-sharing/post-revoke.ics").await;
    assert_eq!(
        ck_envelope(
            &event_archive(test, owner, post_revoke_id)
                .await
                .deserialize::<CalendarEvent>()
                .unwrap()
        ),
        Some((id, newest)),
        "case 6: the owner event written after the revoke must carry the new generation"
    );
    owner_client
        .request("DELETE", post_revoke, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
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
    for name in ["sealed.ics", "plain.ics"] {
        second_client
            .request(
                "GET",
                &format!("/dav/cal/shares-http-owner%40example.com/http-sharing/{name}"),
                "",
            )
            .await
            .with_status(StatusCode::OK);
    }
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
    DummyWebDavClient::new(reader, READER, OTHER, READER)
        .request(
            "GET",
            "/dav/cal/shares-http-owner%40example.com/http-sharing/sealed.ics",
            "",
        )
        .await
        .with_status(StatusCode::OK);
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

/// Public endpoint grant followed by real DAV reads: this must fail at the old owner-only gate.
async fn grantee_dav_admission(test: &mut TestServer) {
    use crate::utils::za::za_post;
    use serde_json::json;
    use vault::keys::display_fingerprint;
    println!("Running Task 10 grantee DAV admission and method matrix...");
    const ALICE: &str = "sharing-dav-alice@example.com";
    const BOB: &str = "sharing-dav-bob@example.com";
    const HOME: &str = "/dav/cal/sharing-dav-alice%40example.com/";
    const SHARED: &str = "/dav/cal/sharing-dav-alice%40example.com/shared/";
    let mut ids = Vec::new();
    for name in [ALICE, BOB] {
        let account = test
            .account("admin@example.com")
            .create_key_user_account(name, STRONG, name, &[], user_permissions())
            .await;
        ids.push(account.id().document_id());
        test.insert_account(account);
    }
    let alice = DummyWebDavClient::new(ids[0], ALICE, STRONG, ALICE);
    let bob = DummyWebDavClient::new(ids[1], BOB, STRONG, BOB);
    for slug in ["shared", "private"] {
        alice
            .request("MKCALENDAR", &format!("{HOME}{slug}/"), MKCAL)
            .await
            .with_status(StatusCode::CREATED);
    }
    let event = EVENT.replace("share-read-UID", "sharing-dav-admission");
    alice
        .request_with_headers(
            "PUT",
            &format!("{SHARED}event.ics"),
            [("content-type", "text/calendar")],
            &event,
        )
        .await
        .with_status(StatusCode::CREATED);
    let key = test
        .server
        .za_served_public_key(ids[1])
        .await
        .unwrap()
        .unwrap();
    super::pinning::pin(ALICE, BOB, &display_fingerprint(&key))
        .await
        .expect(200);
    let mut grant = json!({"username": ALICE, "password": STRONG, "calendar": "shared", "address": BOB, "level": "view"});
    za_post("shares/grant", &grant).await.expect(200);
    // Behavior RED: the reviewed collection grant exists, but the old URI gate answers 403.
    let got = bob
        .request("GET", &format!("{SHARED}event.ics"), "")
        .await
        .with_status(StatusCode::OK);
    assert!(got.body.unwrap().contains("share-read-private-summary"));
    for method in ["HEAD", "PROPFIND"] {
        bob.request(method, &format!("{SHARED}event.ics"), "")
            .await
            .with_status(if method == "HEAD" {
                StatusCode::OK
            } else {
                StatusCode::MULTI_STATUS
            });
    }
    for prop in ["<D:allprop/>", "<D:propname/>"] {
        let body = format!("<D:propfind xmlns:D=\"DAV:\">{prop}</D:propfind>");
        for home in [HOME, "/dav/cal/sharing-dav-bob%40example.com/"] {
            let response = bob
                .request_with_headers("PROPFIND", home, [("depth", "1")], &body)
                .await
                .with_status(StatusCode::MULTI_STATUS);
            let body = response.body.unwrap();
            assert!(
                body.contains("/shared/"),
                "missing eligible calendar: {body}"
            );
            assert!(
                !body.contains("/private/"),
                "private calendar leaked: {body}"
            );
        }
    }
    for method in ["PUT", "DELETE", "PROPPATCH"] {
        bob.request_with_headers(method, &format!("{SHARED}event.ics"), [("content-type", if method == "PROPPATCH" { "application/xml" } else { "text/calendar" })], if method == "PUT" { event.as_str() } else if method == "PROPPATCH" { "<D:propertyupdate xmlns:D=\"DAV:\"><D:set><D:prop><D:displayname>Bob event</D:displayname></D:prop></D:set></D:propertyupdate>" } else { "" }).await.with_status(StatusCode::FORBIDDEN);
    }
    for target in [
        format!("{HOME}private/"),
        "/dav/itip/sharing-dav-alice%40example.com/inbox/".into(),
        "/dav/itip/sharing-dav-alice%40example.com/outbox/".into(),
    ] {
        bob.request("PROPFIND", &target, "")
            .await
            .with_status(StatusCode::FORBIDDEN);
    }
    let patch = "<D:propertyupdate xmlns:D=\"DAV:\" xmlns:A=\"http://apple.com/ns/ical/\"><D:set><D:prop><D:displayname>Bob personal name</D:displayname><A:calendar-color>#123456FF</A:calendar-color></D:prop></D:set></D:propertyupdate>";
    bob.request("PROPPATCH", SHARED, patch)
        .await
        .with_status(StatusCode::FORBIDDEN);
    for report in [
        "<C:calendar-query xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/><C:calendar-data/></D:prop><C:filter><C:comp-filter name=\"VCALENDAR\"><C:comp-filter name=\"VEVENT\"/></C:comp-filter></C:filter></C:calendar-query>".to_string(),
        format!("<C:calendar-multiget xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/><C:calendar-data/></D:prop><D:href>{SHARED}event.ics</D:href></C:calendar-multiget>"),
    ] {
        let response = bob.request("REPORT", SHARED, report).await.with_status(StatusCode::MULTI_STATUS);
        assert!(response.body.unwrap().contains("share-read-private-summary"));
    }
    let freebusy = bob.request("REPORT", SHARED, "<C:free-busy-query xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><C:time-range start=\"20240101T000000Z\" end=\"20240103T000000Z\"/></C:free-busy-query>").await.with_status(StatusCode::OK);
    assert!(!freebusy.body.unwrap().contains("FREEBUSY:"));
    bob.request_with_headers(
        "PUT",
        &format!("{SHARED}view-create.ics"),
        [("content-type", "text/calendar")],
        event.replace("sharing-dav-admission", "view-refused-create"),
    )
    .await
    .with_status(StatusCode::FORBIDDEN);
    bob.request_with_headers(
        "MOVE",
        &format!("{SHARED}event.ics"),
        [("destination", format!("{SHARED}view-move.ics").as_str())],
        "",
    )
    .await
    .with_status(StatusCode::FORBIDDEN);
    grant["level"] = json!("edit");
    za_post("shares/grant", &grant).await.expect(200);
    bob.request_with_headers(
        "PUT",
        &format!("{SHARED}event.ics"),
        [("content-type", "text/calendar")],
        event.replace("private-summary", "bob-edit"),
    )
    .await
    .with_status(StatusCode::NO_CONTENT);
    alice
        .request("GET", &format!("{SHARED}event.ics"), "")
        .await
        .with_status(StatusCode::OK);
    for (method, body) in [
        ("ACL", "<D:acl xmlns:D=\"DAV:\"/>"),
        (
            "LOCK",
            "<D:lockinfo xmlns:D=\"DAV:\"><D:lockscope><D:exclusive/></D:lockscope><D:locktype><D:write/></D:locktype></D:lockinfo>",
        ),
        ("UNLOCK", ""),
        ("DELETE", ""),
    ] {
        bob.request_with_headers(
            method,
            SHARED,
            [(
                "lock-token",
                "<urn:uuid:00000000-0000-0000-0000-000000000000>",
            )],
            body,
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    }
    for method in ["MKCOL", "MKCALENDAR"] {
        bob.request(method, &format!("{HOME}bob-created/"), "")
            .await
            .with_status(StatusCode::FORBIDDEN);
    }
    let properties = "<D:propfind xmlns:D=\"DAV:\" xmlns:A=\"http://apple.com/ns/ical/\"><D:prop><D:displayname/><A:calendar-color/></D:prop></D:propfind>";
    let owner_before = alice
        .request_with_headers("PROPFIND", SHARED, [("depth", "0")], properties)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    bob.request("PROPPATCH", SHARED, patch)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let owner_after = alice
        .request_with_headers("PROPFIND", SHARED, [("depth", "0")], properties)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    let bob_after = bob
        .request_with_headers("PROPFIND", SHARED, [("depth", "0")], properties)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(owner_before.contains("Shared owner private name"));
    assert!(owner_after.contains("Shared owner private name"));
    assert!(!owner_after.contains("Bob personal name"));
    assert!(bob_after.contains("Bob personal name"));
    assert!(owner_after.contains("#123456FF") && bob_after.contains("#123456FF"));
    bob.request_with_headers(
        "PUT",
        &format!("{SHARED}new.ics"),
        [("content-type", "text/calendar")],
        event.replace("sharing-dav-admission", "sharing-dav-new"),
    )
    .await
    .with_status(StatusCode::CREATED);
    bob.request("PROPPATCH", &format!("{SHARED}new.ics"), "<D:propertyupdate xmlns:D=\"DAV:\"><D:set><D:prop><D:displayname>Bob event</D:displayname></D:prop></D:set></D:propertyupdate>").await.with_status(StatusCode::MULTI_STATUS);
    bob.request("DELETE", &format!("{SHARED}new.ics"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    for method in ["COPY", "MOVE"] {
        bob.request_with_headers(
            method,
            &format!("{SHARED}event.ics"),
            [(
                "destination",
                "/dav/cal/sharing-dav-bob%40example.com/default/cross.ics",
            )],
            "",
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    }
    for (source, destination) in [
        (format!("{SHARED}event.ics"), format!("{SHARED}copied.ics")),
        (SHARED.to_string(), format!("{HOME}copied/")),
    ] {
        bob.request_with_headers("COPY", &source, [("destination", destination.as_str())], "")
            .await
            .with_status(StatusCode::FORBIDDEN);
    }
    grantee_pending_grant_and_slug_reuse(test, &alice, &bob, HOME).await;
    grantee_sync_boundaries(test, &alice, &bob, HOME, SHARED).await;
    grantee_unrelated_corruption(test, &alice, &bob, HOME, SHARED).await;
    grantee_gate_controls(test, &alice, &bob, HOME, SHARED).await;
    za_post(
        "shares/revoke",
        &json!({"username": ALICE, "password": STRONG, "calendar": "shared", "grant": ids[1]}),
    )
    .await
    .expect(200);
    bob.request("GET", &format!("{SHARED}event.ics"), "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    for slug in ["shared", "private", "default"] {
        alice
            .request("DELETE", &format!("{HOME}{slug}/"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    let _ = bob
        .request(
            "DELETE",
            "/dav/cal/sharing-dav-bob%40example.com/default/",
            "",
        )
        .await;
    test.wait_for_tasks().await;
}

/// Grant/rename fencing and tombstone filtering use the real REPORT handler.
async fn grantee_sync_boundaries(
    test: &TestServer,
    alice: &DummyWebDavClient,
    bob: &DummyWebDavClient,
    home: &str,
    shared: &str,
) {
    use crate::utils::za::HookParked;
    use common::za::test_hooks;
    use dav_proto::Depth;
    use groupware::calendar::sharing::boundary::mark_boundary_pending;
    use std::sync::Arc;
    println!("Running Task 10 sync boundary, rename and private tombstone controls...");
    let owner = alice.account_id;
    let calendar = resource_id(test, owner, "shared").await;
    let keys = za_unlock(test, owner, STRONG).await;
    let context = za_owned_calendar_context(&test.server, &keys, calendar)
        .await
        .unwrap();
    let token = format!(
        "urn:stalwart:davsync:{:x}",
        context.stored.version.change_id().unwrap()
    );
    let old = sharing_sync_report(
        bob,
        shared,
        "urn:stalwart:davsync:0",
        Depth::Infinity,
        None,
        ["D:getetag"],
    )
    .await
    .with_status(StatusCode::FORBIDDEN);
    assert!(old.body.unwrap().contains("valid-sync-token"));
    let mut pending = context.stored.deserialize::<Calendar>().unwrap();
    mark_boundary_pending(&mut pending, &context.domain).unwrap();
    update_calendar(test, owner, calendar, &context.stored, pending).await;
    for target in [shared, home] {
        for token in [&token, "urn:stalwart:davsync:fffffffffffffffe"] {
            let response =
                sharing_sync_report(bob, target, token, Depth::Infinity, None, ["D:getetag"])
                    .await
                    .with_status(StatusCode::FORBIDDEN);
            assert!(response.body.unwrap().contains("valid-sync-token"));
        }
        sharing_sync_report(bob, target, "", Depth::Infinity, None, ["D:getetag"])
            .await
            .with_status(StatusCode::MULTI_STATUS);
    }
    assert!(
        za_owned_calendar_context(&test.server, &keys, calendar)
            .await
            .unwrap()
            .boundary
            .unwrap()
            .pending,
        "grantee must never repair the pending boundary"
    );
    alice
        .request("PROPFIND", shared, "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        !za_owned_calendar_context(&test.server, &keys, calendar)
            .await
            .unwrap()
            .boundary
            .unwrap()
            .pending
    );
    println!("Task10 sync after repair / rename: initial token");
    let later = sharing_sync_report(bob, home, "", Depth::Infinity, None, ["D:getetag"])
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .sync_token()
        .to_string();
    for slug in ["shared", "private"] {
        let path = format!("{home}{slug}/deleted.ics");
        alice
            .request_with_headers(
                "PUT",
                &path,
                [("content-type", "text/calendar")],
                EVENT.replace("share-read-UID", &format!("sync-{slug}")),
            )
            .await
            .with_status(StatusCode::CREATED);
        alice
            .request("DELETE", &path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    println!("Task10 sync filtered tombstones token={later}");
    for target in [home, shared] {
        let response =
            sharing_sync_report(bob, target, &later, Depth::Infinity, Some(1), ["D:getetag"])
                .await
                .with_status(StatusCode::MULTI_STATUS);
        let hrefs = response.hrefs();
        assert!(
            hrefs
                .iter()
                .any(|path| path == &format!("{shared}deleted.ics")),
            "missing granted tombstone: {hrefs:?}"
        );
        assert!(
            !hrefs.iter().any(|path| path.contains("/private/")),
            "private tombstone counted or emitted: {hrefs:?}"
        );
        // R11-29: a counted private tombstone exceeds the limit and truncates this
        // response into a continuation, even when the one slot went to the granted one.
        let next = response.sync_token();
        assert!(
            next.strip_prefix("urn:stalwart:davsync:")
                .is_some_and(|id| !id.contains(':')),
            "limit-1 sync of {target} answered continuation {next}: private tombstone counted"
        );
        assert!(
            !response.expect_body().contains("HTTP/1.1 507"),
            "limit-1 sync of {target} reported truncation: private tombstone counted"
        );
        let unlimited =
            sharing_sync_report(bob, target, &later, Depth::Infinity, None, ["D:getetag"])
                .await
                .with_status(StatusCode::MULTI_STATUS);
        let hrefs = unlimited.hrefs();
        assert!(
            hrefs
                .iter()
                .any(|path| path == &format!("{shared}deleted.ics")),
            "unlimited sync of {target} is missing the granted tombstone: {hrefs:?}"
        );
        assert!(
            !hrefs.iter().any(|path| path.contains("/private/")),
            "unlimited sync of {target} emitted a private tombstone: {hrefs:?}"
        );
    }
    // The destination is vacant when rename pauses. Its entire private history is
    // then committed before the rename: only the rename's own assigned id may fence it.
    let home_owned = home.to_string();
    let shared_owned = shared.to_string();
    // R11-31, case 1's rename into a former slug, unpaused: bob keeps a token of the
    // granted calendar, alice deletes an event of a private calendar and then that
    // calendar, and renames the granted calendar into the freed slug. The rename's
    // boundary must fence the kept token; the slug is restored before the paused rename.
    println!("Task11 rename into a former slug, unpaused");
    let former = format!("{home}former-slug/");
    let former_event = format!("{former}former-history.ics");
    alice
        .request("MKCALENDAR", &former, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    alice
        .request_with_headers(
            "PUT",
            &former_event,
            [("content-type", "text/calendar")],
            EVENT.replace("share-read-UID", "sync-former-history"),
        )
        .await
        .with_status(StatusCode::CREATED);
    let kept = sharing_sync_report(bob, shared, "", Depth::Infinity, None, ["D:getetag"])
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .sync_token()
        .to_string();
    alice
        .request("DELETE", &former_event, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    alice
        .request("DELETE", &former, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    alice
        .request_with_headers("MOVE", shared, [("destination", former.as_str())], "")
        .await
        .with_status(StatusCode::CREATED);
    for target in [home, former.as_str()] {
        let response =
            sharing_sync_report(bob, target, &kept, Depth::Infinity, None, ["D:getetag"])
                .await
                .with_status(StatusCode::FORBIDDEN);
        assert!(
            response.body.unwrap().contains("valid-sync-token"),
            "{target}: a token kept before a rename into a former slug must be fenced"
        );
        let response = sharing_sync_report(bob, target, "", Depth::Infinity, None, ["D:getetag"])
            .await
            .with_status(StatusCode::MULTI_STATUS);
        let hrefs = response.hrefs();
        assert!(
            !hrefs.iter().any(|path| path.contains("former-history")),
            "{target}: full sync shows the former slug's private calendar: {hrefs:?}"
        );
    }
    alice
        .request_with_headers("MOVE", &former, [("destination", shared)], "")
        .await
        .with_status(StatusCode::CREATED);
    let parked = HookParked::start(owner, test_hooks::set_share_rename_pause, async move {
        DummyWebDavClient::new(
            owner,
            "sharing-dav-alice@example.com",
            STRONG,
            "sharing-dav-alice@example.com",
        )
        .request_with_headers(
            "MOVE",
            &shared_owned,
            [("destination", format!("{home_owned}reused/").as_str())],
            "",
        )
        .await
        .status
    })
    .await;
    let prior = test
        .server
        .store()
        .get_last_change_id(owner, SyncCollection::Calendar.into())
        .await
        .unwrap()
        .unwrap_or_default();
    let above_prior = prior.checked_add(1).unwrap();
    let precommit_token = format!("urn:stalwart:davsync:{above_prior:x}");
    let reused = format!("{home}reused/");
    alice
        .request("MKCALENDAR", &reused, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    alice
        .request_with_headers(
            "PUT",
            &format!("{reused}private-history.ics"),
            [("content-type", "text/calendar")],
            EVENT.replace("share-read-UID", "sync-private-history"),
        )
        .await
        .with_status(StatusCode::CREATED);
    alice
        .request("DELETE", &reused, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let private_history = test
        .server
        .store()
        .get_last_change_id(owner, SyncCollection::Calendar.into())
        .await
        .unwrap()
        .unwrap();
    assert!(above_prior > prior && above_prior < private_history);
    let completion = Arc::new(test_hooks::Pause::new(owner));
    test_hooks::set_share_boundary_pause(Some(completion.clone()));
    let finishing = tokio::spawn(parked.finish());
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        completion.arrived.notified(),
    )
    .await
    .unwrap();
    for target in [home, reused.as_str()] {
        let response = sharing_sync_report(
            bob,
            target,
            "urn:stalwart:davsync:fffffffffffffffe",
            Depth::Infinity,
            None,
            ["D:getetag"],
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
        assert!(response.body.unwrap().contains("valid-sync-token"));
        let response = sharing_sync_report(bob, target, "", Depth::Infinity, None, ["D:getetag"])
            .await
            .with_status(StatusCode::MULTI_STATUS);
        assert!(
            !response
                .hrefs()
                .iter()
                .any(|path| path.contains("private-history"))
        );
    }
    completion.release.notify_one();
    assert_eq!(finishing.await.unwrap(), StatusCode::CREATED);
    test_hooks::set_share_boundary_pause(None);
    let renamed = za_owned_calendar_context(&test.server, &keys, calendar)
        .await
        .unwrap();
    assert!(renamed.boundary.unwrap().rename_change_id > private_history);
    for target in [home, reused.as_str()] {
        let response = sharing_sync_report(
            bob,
            target,
            &precommit_token,
            Depth::Infinity,
            None,
            ["D:getetag"],
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
        assert!(
            response.body.unwrap().contains("valid-sync-token"),
            "numeric token above all pre-rename IDs must still be fenced after completion"
        );
    }
    let refused = sharing_sync_report(bob, home, &later, Depth::Infinity, None, ["D:getetag"])
        .await
        .with_status(StatusCode::FORBIDDEN);
    assert!(refused.body.unwrap().contains("valid-sync-token"));
    println!("Task10 sync after repair / rename: initial token");
    let later = sharing_sync_report(bob, home, "", Depth::Infinity, None, ["D:getetag"])
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .sync_token()
        .to_string();
    println!("Task10 sync after rename later token={later}");
    let response = sharing_sync_report(bob, home, &later, Depth::Infinity, None, ["D:getetag"])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        !response
            .hrefs()
            .iter()
            .any(|path| path.contains("private-history"))
    );
    // Restore the fixture's public slug for the caller's revoke and cleanup.
    alice
        .request_with_headers("MOVE", &reused, [("destination", shared)], "")
        .await
        .with_status(StatusCode::CREATED);
}

const LIFE_ALICE: &str = "sharing-life-alice@example.com";
const LIFE_BOB: &str = "sharing-life-bob@example.com";
const LIFE_ALIAS: &str = "sharing-life-work@example.com";
const LIFE_CAROL: &str = "sharing-life-carol@example.com";
const LIFE_PATH: &str = "/dav/cal/sharing-life-alice%40example.com/lifecycle/";

fn sharing_lifecycle_body(
    uid: &str,
    summary: &str,
    start: &str,
    master: &[&str],
    overrides: &[(&str, &[&str])],
) -> String {
    let mut result = format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//sharing-lifecycle//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:{start}\r\nDTEND:20990301T120000Z\r\nRRULE:FREQ=DAILY;COUNT=5\r\nSUMMARY:{summary}\r\nORGANIZER:mailto:{LIFE_ALICE}\r\n"
    );
    for address in master {
        result.push_str(&format!(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{address}\r\n"
        ));
    }
    result.push_str("END:VEVENT\r\n");
    for (rid, attendees) in overrides {
        result.push_str(&format!("BEGIN:VEVENT\r\nUID:{uid}\r\nRECURRENCE-ID:{rid}\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:{rid}\r\nDTEND:{}\r\nSUMMARY:override-{rid}\r\nORGANIZER:mailto:{LIFE_ALICE}\r\n", rid.replace("100000", "120000")));
        for address in *attendees {
            result.push_str(&format!(
                "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{address}\r\n"
            ));
        }
        result.push_str("END:VEVENT\r\n");
    }
    result.push_str("END:VCALENDAR\r\n");
    result
}

async fn sharing_drain_step(test: &TestServer, party: &super::fanout::Party) {
    use crate::utils::za::{pending_ids, trigger_drain};
    assert_eq!(trigger_drain(party.name, STRONG).await, 207);
    if !pending_ids(test, party.id).await.is_empty() {
        // A transient pass defers the next pass for30s. Do not mistake that deferral
        // for completed delivery or use the alarm-task wait_for_delivery helper.
        tokio::time::sleep(std::time::Duration::from_secs(31)).await;
        assert_eq!(trigger_drain(party.name, STRONG).await, 207);
    }
    assert!(
        pending_ids(test, party.id).await.is_empty(),
        "pending pass after explicit retry"
    );
}

/// Same accepted HTTP lifecycle, two UIDs. No expected copy shape is rewritten to
/// manufacture parity: only the spec's timestamps/UID/delegated SENT-BY are normalized.
async fn sharing_compare_lifecycle(
    test: &TestServer,
    label: &str,
    templates: &[Option<String>],
    addresses: &[&str],
    expect_equal: bool,
) {
    use super::{
        drain::{body, relative},
        fanout::{NO_REPLY, Party, stored_pairs, uid_copies},
    };
    use crate::utils::za::{
        copy_href, inbox_ids, inbox_items, item_attendees, new_inbox_items, normalize_sharing_copy,
        open_pending_item_as, schedule_statuses,
    };
    use calcard::{
        Entry, Parser,
        icalendar::{ICalendarParameterName, ICalendarProperty},
    };
    use std::collections::BTreeSet;
    let alice = Party::new(test, LIFE_ALICE);
    let bob = Party::new(test, LIFE_BOB);
    let carol = Party::new(test, LIFE_CAROL);
    let keys = za_unlock(test, bob.id, STRONG).await;
    let uids = [
        format!("sharing-{label}-owner"),
        format!("sharing-{label}-delegated"),
    ];
    let paths = [
        format!("{LIFE_PATH}{label}-owner.ics"),
        format!("{LIFE_PATH}{label}-delegated.ics"),
    ];
    let mut created = [false; 2];
    for (step, template) in templates.iter().enumerate() {
        let rejection_reason = match label {
            "shared-organizer" => Some("SenderIsNotOrganizerNorAttendee"),
            "subaddress-only" => Some("NotOrganizerNorAttendee"),
            _ => None,
        };
        let rejection_trace =
            rejection_reason.map(|_| super::mail::Traced::start(trc::EventType::variants()));
        let mut rejected_items = Vec::new();
        let mut messages = Vec::new();
        let mut source_statuses = Vec::new();
        let mut source_pair_sets = Vec::new();
        for run in 0..2 {
            if label.starts_with("failed-direct-alias") && run == 1 {
                common::za::test_hooks::set_delegation_lookup_fail_once(Some((
                    bob.id,
                    LIFE_ALIAS.into(),
                )));
            }
            let before = inbox_ids(test, bob.id).await;
            let owner_inbox_before = inbox_ids(test, alice.id).await;
            let client = if run == 0 { &alice.client } else { &bob.client };
            if let Some(template) = template {
                let calendar = template.replace("LIFECYCLE-UID", &uids[run]);
                client
                    .request_with_headers(
                        "PUT",
                        &paths[run],
                        [super::fanout::CONTENT_TYPE],
                        calendar,
                    )
                    .await
                    .with_status(if created[run] {
                        StatusCode::NO_CONTENT
                    } else {
                        StatusCode::CREATED
                    });
                created[run] = true;
            } else {
                client
                    .request("DELETE", &paths[run], "")
                    .await
                    .with_status(StatusCode::NO_CONTENT);
                created[run] = false;
            }
            common::za::test_hooks::set_delegation_lookup_fail_once(None);
            if label == "owner-unlisted" {
                assert_eq!(
                    inbox_ids(test, alice.id).await,
                    owner_inbox_before,
                    "an owner-resolving unlisted attendee must get5.3 without an owner inbox item"
                );
            }
            let mut sent = Vec::new();
            let pending = new_inbox_items(test, bob.id, &before).await;
            assert!(
                pending.len() < 32,
                "each step must fit within the drain cap"
            );
            if rejection_reason.is_some() {
                assert_eq!(pending.len(), 1, "{label} step{step} run{run}: delivery");
                rejected_items.push(pending[0].0);
            }
            for (id, _) in pending {
                let (tree, _) = open_pending_item_as(test, bob.id, STRONG, id).await;
                if rejection_reason.is_some() {
                    let organizers = tree
                        .components
                        .iter()
                        .flat_map(|component| &component.entries)
                        .filter(|entry| entry.name == ICalendarProperty::Organizer)
                        .flat_map(|entry| &entry.values)
                        .map(|value| value.as_text().unwrap().to_string())
                        .collect::<Vec<_>>();
                    assert_eq!(
                        organizers,
                        vec![format!(
                            "mailto:{}",
                            if label == "shared-organizer" {
                                "sharing-life-shared-organizer@example.com"
                            } else {
                                LIFE_ALICE
                            }
                        )],
                        "{label}: delivered organizer identity"
                    );
                    assert_eq!(
                        item_attendees(&tree),
                        vec![if label == "shared-organizer" {
                            LIFE_BOB.to_string()
                        } else {
                            "sharing-life-bob+x@example.com".to_string()
                        }],
                        "{label}: delivered attendee identity"
                    );
                }
                for organizer in tree
                    .components
                    .iter()
                    .flat_map(|component| &component.entries)
                    .filter(|entry| {
                        entry.name == ICalendarProperty::Organizer
                            && entry
                                .values
                                .first()
                                .and_then(|value| value.as_text())
                                .is_some_and(|address| {
                                    address.trim_start_matches("mailto:") == LIFE_ALICE
                                })
                    })
                {
                    let sent_by = organizer
                        .params
                        .iter()
                        .find(|parameter| parameter.name == ICalendarParameterName::SentBy)
                        .and_then(|parameter| parameter.value.as_text());
                    assert_eq!(
                        sent_by,
                        if run == 1 {
                            Some("mailto:sharing-life-bob@example.com")
                        } else {
                            None
                        },
                        "HTTP delegated ORGANIZER must carry its writer's SENT-BY"
                    );
                }
                if label.contains("subaddress") {
                    println!("Case25 exported {label} step{step} run{run}:\n{tree}");
                }
                if label == "subaddress"
                    && (step == 3 || step == 5)
                    && crate::utils::za::item_method(&tree)
                        == Some(calcard::icalendar::ICalendarMethod::Cancel)
                {
                    let attendee_addresses = item_attendees(&tree);
                    assert!(
                        attendee_addresses
                            .iter()
                            .any(|address| address == "sharing-life-bob+x@example.com")
                    );
                    assert_eq!(
                        attendee_addresses.iter().any(|address| address == LIFE_BOB),
                        step == 5,
                        "partial removal CANCEL lacks the listed line; override deletion includes it"
                    );
                }
                if label == "shared-organizer" {
                    assert!(
                        !tree
                            .components
                            .iter()
                            .flat_map(|component| &component.entries)
                            .filter(|entry| entry.name == ICalendarProperty::Organizer)
                            .flat_map(|entry| &entry.params)
                            .any(|parameter| parameter.name == ICalendarParameterName::SentBy)
                    );
                }
                sent.push((
                    item_attendees(&tree),
                    normalize_sharing_copy(tree, "same-uid"),
                ));
            }
            sent.sort_unstable_by_key(|(addresses, tree)| (addresses.clone(), tree.to_string()));
            messages.push(sent);
            if created[run] {
                let source = body(&alice.client, &paths[run]).await;
                let statuses = addresses
                    .iter()
                    .map(|address| {
                        (
                            address.to_string(),
                            schedule_statuses(&source, ICalendarProperty::Attendee, address),
                        )
                    })
                    .collect::<Vec<_>>();
                source_statuses.push(Some(statuses));
                let id = resource_id(
                    test,
                    alice.id,
                    &format!(
                        "lifecycle/{label}-{}.ics",
                        if run == 0 { "owner" } else { "delegated" }
                    ),
                )
                .await;
                let raw = event_archive(test, alice.id, id)
                    .await
                    .deserialize::<CalendarEvent>()
                    .unwrap();
                let owner_keys = za_unlock(test, alice.id, STRONG).await;
                let calendar_id = resource_id(test, alice.id, "lifecycle").await;
                let context = za_owned_calendar_context(&test.server, &owner_keys, calendar_id)
                    .await
                    .unwrap();
                let stored = event_archive(test, alice.id, id).await;
                let pairs =
                    groupware::calendar::seal::event_pairs_in_domain(&stored, &context.domain)
                        .unwrap();
                let present = pairs.as_ref().map(|pairs| {
                    let addresses = addresses
                        .iter()
                        .filter(|address| pairs.get(address).is_some())
                        .map(|address| address.to_string())
                        .collect::<BTreeSet<_>>();
                    assert_eq!(
                        pairs.len(),
                        addresses.len(),
                        "source pair addresses must be exhaustively enumerated"
                    );
                    (pairs.len(), addresses)
                });
                source_pair_sets.push(Some(present));
                if label.contains("sole-group") {
                    assert!(
                        statuses_for(&source_statuses[run], "sharing-life-team@example.com")
                            .iter()
                            .all(|status| status == "5.3")
                    );
                    assert!(
                        !statuses_for(&source_statuses[run], "sharing-life-team@example.com")
                            .is_empty()
                    );
                }
                if label == "owner-unlisted" {
                    assert_eq!(
                        statuses_for(&source_statuses[run], "sharing-life-alice+y@example.com"),
                        vec!["5.3".to_string()]
                    );
                }
                if label.starts_with("failed-direct-alias") {
                    assert_eq!(
                        statuses_for(&source_statuses[run], LIFE_ALIAS),
                        if run == 0 {
                            Vec::<String>::new()
                        } else {
                            vec!["5.1".to_string()]
                        }
                    );
                }
                if label.starts_with("negative-") {
                    for address in addresses {
                        assert!(
                            statuses_for(&source_statuses[run], address).is_empty(),
                            "suppressed negative recipient must have no attempted status"
                        );
                    }
                }
                if label.contains("force") || label.contains("sole-group") {
                    assert!(
                        raw.schedule_tag.is_some(),
                        "self-only scheduling must finalize and set a tag"
                    );
                    let Entry::ICalendar(parsed) = Parser::new(&source).entry() else {
                        panic!("source iCalendar")
                    };
                    assert!(
                        !parsed
                            .components
                            .iter()
                            .flat_map(|component| &component.entries)
                            .flat_map(|entry| &entry.params)
                            .any(|parameter| parameter.name
                                == ICalendarParameterName::ScheduleForceSend)
                    );
                }
            } else {
                source_statuses.push(None);
                source_pair_sets.push(None);
                alice
                    .client
                    .request("GET", &paths[run], "")
                    .await
                    .with_status(StatusCode::NOT_FOUND);
            }
        }
        if expect_equal {
            assert_eq!(
                messages[0], messages[1],
                "{label} step{step}: inbox payload/address multisets"
            );
            assert_eq!(
                source_pair_sets[0], source_pair_sets[1],
                "{label} step{step}: owner-stored token pair presence"
            );
            assert_eq!(
                source_statuses[0], source_statuses[1],
                "{label} step{step}: owner-stored recipient statuses"
            );
        }
        sharing_drain_step(test, &bob).await;
        sharing_drain_step(test, &carol).await;
        if let Some(trace) = rejection_trace {
            let events = trace.finish().await;
            let history = inbox_items(test, bob.id).await;
            assert_eq!(rejected_items.len(), 2);
            for id in rejected_items {
                let item = &history
                    .iter()
                    .find(|(item_id, _)| *item_id == id)
                    .unwrap()
                    .1;
                assert_eq!(item.flags, 0, "{label}: rejected item is drained history");
                assert_eq!(item.event_id, None, "{label}: rejected item has no copy");
                assert_eq!(
                    events
                        .iter()
                        .filter(|event| {
                            event.account_id == Some(u64::from(bob.id))
                                && event.reason == rejection_reason.unwrap()
                                && event.keys.iter().any(|(key, value)| {
                                    *key == trc::Key::DocumentId
                                        && matches!(value, trc::Value::UInt(value) if *value == u64::from(id))
                                })
                        })
                        .count(),
                    1,
                    "{label} step{step}: each incoming item must hit its exact rejection"
                );
            }
        }
        let mut copies = Vec::new();
        let mut token_sets = Vec::new();
        for uid in &uids {
            if uid_copies(test, bob.id, uid).await == 0 {
                copies.push(None);
                token_sets.push(None);
            } else {
                let href = copy_href(test, bob.id, uid).await;
                let Entry::ICalendar(copy) = Parser::new(&body(&bob.client, &href).await).entry()
                else {
                    panic!("recipient iCalendar")
                };
                copies.push(Some(normalize_sharing_copy(copy, "same-uid")));
                let pairs = stored_pairs(test, &keys, bob.id, &relative(&href)).await;
                let present = addresses
                    .iter()
                    .filter(|address| {
                        pairs
                            .as_ref()
                            .is_some_and(|pairs| pairs.get(address).is_some())
                    })
                    .map(|address| address.to_string())
                    .collect::<BTreeSet<_>>();
                let count = pairs.as_ref().map_or(0, |pairs| pairs.len());
                assert_eq!(
                    count,
                    present.len(),
                    "fixture must enumerate every token address"
                );
                token_sets.push(pairs.as_ref().map(|_| (count, present)));
            }
        }
        if label.starts_with("agent-")
            || label.starts_with("negative-")
            || label.contains("sole-group")
            || label == "shared-attendee"
            || label == "shared-organizer"
            || label == "subaddress-only"
            || label == "writer-unlisted-organizer"
            || label.starts_with("failed-direct-alias")
        {
            assert!(
                copies.iter().all(Option::is_none),
                "{label}: no self copy may be manufactured"
            );
        }
        if label.starts_with("failed-direct-alias")
            || label.starts_with("negative-")
            || label.starts_with("agent-")
            || label == "shared-attendee"
            || label.contains("sole-group")
        {
            assert!(
                messages.iter().all(Vec::is_empty),
                "{label}: must create no Bob inbox item, including refused items"
            );
        }
        if label.starts_with("exception-") {
            assert!(
                copies[0].is_none(),
                "ordinary owner run suppresses direct listed aliases"
            );
            let copy = copies[1]
                .as_ref()
                .expect("delegated exception delivers the direct listed alias");
            let expected = label.strip_prefix("exception-").unwrap();
            assert_eq!(
                crate::utils::za::item_partstat(copy, LIFE_ALIAS).as_deref(),
                Some(expected)
            );
            assert!(messages[0].is_empty());
            assert!(!messages[1].is_empty());
        }
        if label.contains("force") {
            for copy in copies.iter().flatten() {
                assert!(
                    !copy
                        .components
                        .iter()
                        .flat_map(|component| &component.entries)
                        .flat_map(|entry| &entry.params)
                        .any(
                            |parameter| parameter.name == ICalendarParameterName::ScheduleForceSend
                        )
                );
            }
        }
        if expect_equal {
            assert_eq!(copies[0], copies[1], "{label} step{step}: recipient copies");
            assert_eq!(
                token_sets[0], token_sets[1],
                "{label} step{step}: token-address presence"
            );
        }
        println!(
            "Case25 {label} step{step}: owner/delegated copies {:?}, messages {:?}, source statuses {:?}",
            copies.iter().map(Option::is_some).collect::<Vec<_>>(),
            messages.iter().map(Vec::len).collect::<Vec<_>>(),
            source_statuses
        );
    }
    for (run, uid) in uids.iter().enumerate() {
        if created[run] {
            alice
                .client
                .request_with_headers("DELETE", &paths[run], [NO_REPLY], "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
        for party in [&bob, &carol] {
            if uid_copies(test, party.id, uid).await != 0 {
                party
                    .client
                    .request_with_headers(
                        "DELETE",
                        &copy_href(test, party.id, uid).await,
                        [NO_REPLY],
                        "",
                    )
                    .await
                    .with_status(StatusCode::NO_CONTENT);
            }
        }
    }
    for party in [&alice, &bob, &carol] {
        party.delete_items(inbox_ids(test, party.id).await).await;
    }
}

async fn sharing_lifecycle_differential(test: &mut TestServer) {
    use super::drain::DrainOn;
    use crate::utils::za::za_post;
    use serde_json::json;
    println!("Running case25 HTTP owner/delegated lifecycle differential...");
    // The differential oracle must retain an exact SEQUENCE difference of n+2.
    let n = sharing_tree(&EVENT.replace("SUMMARY:", "SEQUENCE:7\r\nSUMMARY:"));
    let n_plus_two = sharing_tree(&EVENT.replace("SUMMARY:", "SEQUENCE:9\r\nSUMMARY:"));
    assert!(
        crate::utils::za::normalize_sharing_copy(n, "same-uid")
            != crate::utils::za::normalize_sharing_copy(n_plus_two, "same-uid"),
        "mutation45: normalization must preserve an n+2 SEQUENCE difference"
    );

    let _drain = DrainOn::new();
    let admin = test.account("admin@example.com").clone();
    for name in [LIFE_ALICE, LIFE_BOB, LIFE_CAROL] {
        let account = admin
            .create_key_user_account(
                name,
                STRONG,
                name,
                if name == LIFE_BOB {
                    &[LIFE_ALIAS, "sharing-life-custom@sharing-neg-custom.test"][..]
                } else {
                    &[]
                },
                user_permissions(),
            )
            .await;
        test.insert_account(account);
    }
    let alice = super::fanout::Party::new(test, LIFE_ALICE);
    let bob = super::fanout::Party::new(test, LIFE_BOB);
    alice
        .client
        .request("MKCALENDAR", LIFE_PATH, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let key = test
        .server
        .za_served_public_key(bob.id)
        .await
        .unwrap()
        .unwrap();
    super::pinning::pin(
        LIFE_ALICE,
        LIFE_BOB,
        &vault::keys::display_fingerprint(&key),
    )
    .await
    .expect(200);
    za_post("shares/grant", &json!({"username":LIFE_ALICE,"password":STRONG,"calendar":"lifecycle","address":LIFE_BOB,"level":"edit"})).await.expect(200);
    let both = [LIFE_BOB, LIFE_CAROL];
    let carol = [LIFE_CAROL];
    let rid1 = "20990302T100000Z";
    let rid2 = "20990303T100000Z";
    let body = |summary, start, master: &[&str], overrides: &[(&str, &[&str])]| {
        Some(sharing_lifecycle_body(
            "LIFECYCLE-UID",
            summary,
            start,
            master,
            overrides,
        ))
    };
    let main = [
        body("create", "20990301T100000Z", &both, &[(rid1, &both)]),
        body("summary", "20990301T100000Z", &both, &[(rid1, &both)]),
        body("summary", "20990301T110000Z", &both, &[(rid1, &both)]),
        body(
            "summary",
            "20990301T110000Z",
            &both,
            &[(rid1, &both), (rid2, &both)],
        ),
        body(
            "summary",
            "20990301T110000Z",
            &both,
            &[(rid1, &carol), (rid2, &both)],
        ),
        body(
            "summary-again",
            "20990301T110000Z",
            &both,
            &[(rid1, &carol), (rid2, &both)],
        ),
        body(
            "summary-again",
            "20990301T110000Z",
            &both,
            &[(rid1, &both), (rid2, &both)],
        ),
        body("summary-again", "20990301T110000Z", &both, &[(rid2, &both)]),
        body(
            "summary-again",
            "20990301T110000Z",
            &carol,
            &[(rid2, &both)],
        ),
        body("summary-again", "20990301T110000Z", &both, &[(rid2, &both)]),
        None,
    ];
    sharing_compare_lifecycle(
        test,
        "eleven",
        &main,
        &[LIFE_BOB, LIFE_ALIAS, LIFE_CAROL],
        true,
    )
    .await;
    let alias = [LIFE_ALIAS, LIFE_CAROL];
    sharing_compare_lifecycle(
        test,
        "listed-alias",
        &[
            body("alias", "20990301T100000Z", &both, &[(rid1, &alias)]),
            body(
                "alias-summary",
                "20990301T100000Z",
                &both,
                &[(rid1, &alias)],
            ),
            body(
                "alias-summary",
                "20990301T100000Z",
                &carol,
                &[(rid1, &alias)],
            ),
            body(
                "alias-summary",
                "20990301T100000Z",
                &alias,
                &[(rid1, &alias)],
            ),
            None,
        ],
        &[LIFE_BOB, LIFE_ALIAS, LIFE_CAROL],
        true,
    )
    .await;
    let sub = "sharing-life-bob+x@example.com";
    assert_eq!(
        test.server.account_id_from_email(sub, true).await.unwrap(),
        Some(bob.id)
    );
    let sub_attendees = [LIFE_BOB, sub, LIFE_CAROL];
    sharing_compare_lifecycle(
        test,
        "subaddress",
        &[
            body("sub", "20990301T100000Z", &both, &[(rid1, &sub_attendees)]),
            body(
                "sub-summary",
                "20990301T100000Z",
                &both,
                &[(rid1, &sub_attendees)],
            ),
            body(
                "sub-summary",
                "20990301T110000Z",
                &both,
                &[(rid1, &sub_attendees)],
            ),
            body("sub-summary", "20990301T110000Z", &both, &[(rid1, &both)]),
            body(
                "sub-summary",
                "20990301T110000Z",
                &both,
                &[(rid1, &sub_attendees)],
            ),
            body("sub-summary", "20990301T110000Z", &both, &[]),
            None,
        ],
        &[LIFE_BOB, LIFE_ALIAS, sub, LIFE_CAROL],
        true,
    )
    .await;
    sharing_compare_lifecycle(
        test,
        "subaddress-only",
        &[body("sub-only", "20990301T100000Z", &[sub], &[]), None],
        &[sub],
        true,
    )
    .await;
    for agent in ["CLIENT", "NONE"] {
        let value = sharing_lifecycle_body(
            "LIFECYCLE-UID",
            "agent",
            "20990301T100000Z",
            &[LIFE_BOB],
            &[],
        )
        .replace("ATTENDEE;", &format!("ATTENDEE;SCHEDULE-AGENT={agent};"));
        sharing_compare_lifecycle(
            test,
            &format!("agent-{agent}"),
            &[Some(value), None],
            &[LIFE_BOB],
            true,
        )
        .await;
    }
    let value = sharing_lifecycle_body(
        "LIFECYCLE-UID",
        "force",
        "20990301T100000Z",
        &[LIFE_BOB],
        &[],
    )
    .replace("ATTENDEE;", "ATTENDEE;SCHEDULE-FORCE-SEND=REQUEST;");
    sharing_compare_lifecycle(test, "force-sole", &[Some(value), None], &[LIFE_BOB], true).await;
    sharing_http_schedule_contracts(test).await;
    sharing_lifecycle_negative_and_exception(test).await;
    alice
        .client
        .request("DELETE", LIFE_PATH, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    for name in [LIFE_ALICE, LIFE_BOB, LIFE_CAROL] {
        super::fanout::Party::new(test, name)
            .client
            .delete_default_containers()
            .await;
    }
    test.wait_for_tasks().await;
}

/// Raw sync REPORT used for both success and negative precondition assertions.
/// DummyWebDavClient::sync_collection asserts207 internally and cannot inspect403.
async fn sharing_sync_report(
    client: &DummyWebDavClient,
    path: &str,
    token: &str,
    depth: dav_proto::Depth,
    limit: Option<usize>,
    _properties: [&str; 1],
) -> crate::utils::webdav::DavResponse {
    let limit = limit
        .map(|limit| format!("<D:limit><D:nresults>{limit}</D:nresults></D:limit>"))
        .unwrap_or_default();
    client.request("REPORT", path, format!("<D:sync-collection xmlns:D=\"DAV:\"><D:prop><D:getetag/></D:prop><D:sync-token>{token}</D:sync-token><D:sync-level>{}</D:sync-level>{limit}</D:sync-collection>", if depth == dav_proto::Depth::Infinity { "infinite" } else { "1" })).await
}

fn statuses_for(statuses: &Option<Vec<(String, Vec<String>)>>, address: &str) -> Vec<String> {
    statuses
        .as_ref()
        .and_then(|statuses| statuses.iter().find(|(key, _)| key == address))
        .map(|(_, values)| values.clone())
        .unwrap_or_default()
}

async fn sharing_lifecycle_negative_and_exception(test: &mut TestServer) {
    use common::ipc::CacheInvalidation;
    use registry::schema::{
        prelude::{ObjectType, Property},
        structs::{Domain, Expression, ExpressionMatch, SubAddressing, SubAddressingCustom},
    };
    use registry::types::list::List;
    use serde_json::json;
    let admin = test.account("admin@example.com").clone();
    let owner = test.account(LIFE_ALICE).id();
    let writer = test.account(LIFE_BOB).id();
    let shared_attendee = admin
        .create_group_account(
            "sharing-life-shared-attendee@example.com",
            "Shared attendee",
            &[],
        )
        .await;
    let shared_organizer = admin
        .create_group_account(
            "sharing-life-shared-organizer@example.com",
            "Shared organizer",
            &[],
        )
        .await;
    let team = admin
        .create_group_account("sharing-life-team@example.com", "Writer only", &[])
        .await;
    let (ag, og, wg) = (shared_attendee.id(), shared_organizer.id(), team.id());
    test.insert_account(shared_attendee);
    test.insert_account(shared_organizer);
    test.insert_account(team);
    admin
        .registry_update_object(
            ObjectType::Account,
            owner,
            json!({Property::MemberGroupIds:{ag:true,og:true}}),
        )
        .await;
    admin
        .registry_update_object(
            ObjectType::Account,
            writer,
            json!({Property::MemberGroupIds:{ag:true,og:true,wg:true}}),
        )
        .await;
    let make = |addresses: &[&str]| {
        sharing_lifecycle_body("LIFECYCLE-UID", "edge", "20990301T100000Z", addresses, &[])
    };
    sharing_compare_lifecycle(
        test,
        "shared-attendee",
        &[
            Some(make(&["sharing-life-shared-attendee@example.com"])),
            None,
        ],
        &["sharing-life-shared-attendee@example.com"],
        true,
    )
    .await;
    let organized =
        make(&[LIFE_BOB]).replace(LIFE_ALICE, "sharing-life-shared-organizer@example.com");
    sharing_compare_lifecycle(
        test,
        "shared-organizer",
        &[Some(organized), None],
        &[LIFE_BOB],
        true,
    )
    .await;
    println!(
        "Case25 shared-group ORGANIZER: delegated gate, no SENT-BY, inherited receiver group-id/changed_by rejection preserved"
    );
    sharing_compare_lifecycle(
        test,
        "sole-group",
        &[Some(make(&["sharing-life-team@example.com"])), None],
        &["sharing-life-team@example.com"],
        true,
    )
    .await;
    sharing_compare_lifecycle(
        test,
        "owner-unlisted",
        &[
            Some(make(&[LIFE_BOB, "sharing-life-alice+y@example.com"])),
            None,
        ],
        &[LIFE_BOB, "sharing-life-alice+y@example.com"],
        true,
    )
    .await;
    let unlisted = make(&[LIFE_BOB]).replace(LIFE_ALICE, "sharing-life-bob+x@example.com");
    sharing_compare_lifecycle(
        test,
        "writer-unlisted-organizer",
        &[Some(unlisted), None],
        &[LIFE_BOB],
        true,
    )
    .await;
    for (expected, flags) in [
        ("DECLINED", "PARTSTAT=DECLINED;RSVP=TRUE"),
        ("NEEDS-ACTION", "PARTSTAT=NEEDS-ACTION;RSVP=FALSE"),
        ("ACCEPTED", "PARTSTAT=ACCEPTED;RSVP=TRUE"),
    ] {
        let initial = make(&[LIFE_ALIAS]).replace("PARTSTAT=NEEDS-ACTION;RSVP=TRUE", flags);
        let added = sharing_lifecycle_body(
            "LIFECYCLE-UID",
            "edge",
            "20990301T100000Z",
            &[LIFE_ALIAS],
            &[("20990302T100000Z", &[LIFE_ALIAS])],
        )
        .replace("PARTSTAT=NEEDS-ACTION;RSVP=TRUE", flags);
        sharing_compare_lifecycle(
            test,
            &format!("exception-{expected}"),
            &[Some(initial), Some(added), None],
            &[LIFE_ALIAS],
            false,
        )
        .await;
        if expected != "ACCEPTED" {
            let failed = make(&[LIFE_ALIAS]).replace("PARTSTAT=NEEDS-ACTION;RSVP=TRUE", flags);
            sharing_compare_lifecycle(
                test,
                &format!("failed-direct-alias-{expected}"),
                &[Some(failed), None],
                &[LIFE_ALIAS],
                false,
            )
            .await;
        }
        for (label, address) in [
            ("group", "sharing-life-team@example.com"),
            ("unlisted", "sharing-life-bob+x@example.com"),
        ] {
            let initial = make(&[address]).replace("PARTSTAT=NEEDS-ACTION;RSVP=TRUE", flags);
            sharing_compare_lifecycle(
                test,
                &format!("negative-{label}-{expected}"),
                &[Some(initial), None],
                &[address],
                true,
            )
            .await;
        }
        // Isolated domains exercise the broader resolver without making the
        // resulting address a directly owned listed alias. Restore both policy
        // fields and positive/negative caches after every individual fixture.
        for (kind, domain_name, address) in [
            (
                "custom",
                "sharing-neg-custom.test",
                "sharing-life-custom.topic@sharing-neg-custom.test",
            ),
            (
                "catchall",
                "sharing-neg-catchall.test",
                "nobody@sharing-neg-catchall.test",
            ),
        ] {
            let domain = admin.find_or_create_domain(domain_name).await;
            let saved = test
                .server
                .registry()
                .object::<Domain>(domain)
                .await
                .unwrap()
                .unwrap();
            let settings = if kind == "custom" {
                json!({"catchAllAddress":null,"subAddressing":SubAddressing::Custom(SubAddressingCustom{custom_rule:Expression{else_:"false".into(),match_:List::from_iter([ExpressionMatch{if_:"matches('^([^.]+)\\.([^.]+)', rcpt)".into(),then:"$1".into()}])}})})
            } else {
                json!({"catchAllAddress":LIFE_BOB,"subAddressing":{"@type":"Disabled"}})
            };
            admin
                .registry_update_object(ObjectType::Domain, domain, settings)
                .await;
            test.server
                .invalidate_local_caches(&[
                    CacheInvalidation::Domain(domain.document_id()),
                    CacheInvalidation::Account(writer.document_id()),
                ])
                .await;
            test.server.invalidate_all_local_negative_caches();
            assert_eq!(
                test.server
                    .account_id_from_email(address, true)
                    .await
                    .unwrap(),
                Some(writer.document_id())
            );
            let initial = make(&[address]).replace("PARTSTAT=NEEDS-ACTION;RSVP=TRUE", flags);
            sharing_compare_lifecycle(
                test,
                &format!("negative-{kind}-{expected}"),
                &[Some(initial), None],
                &[address],
                true,
            )
            .await;
            admin.registry_update_object(ObjectType::Domain, domain, json!({"catchAllAddress":saved.catch_all_address,"subAddressing":saved.sub_addressing})).await;
            test.server
                .invalidate_local_caches(&[
                    CacheInvalidation::Domain(domain.document_id()),
                    CacheInvalidation::Account(writer.document_id()),
                ])
                .await;
            test.server.invalidate_all_local_negative_caches();
        }
    }
}

async fn grantee_gate_controls(
    test: &mut TestServer,
    alice: &DummyWebDavClient,
    bob: &DummyWebDavClient,
    home: &str,
    shared: &str,
) {
    use crate::utils::za::za_post;
    use common::auth::oauth::GrantType;
    use registry::schema::{
        enums::Permission,
        prelude::{ObjectType, Property},
    };
    use serde_json::json;
    use types::acl::{Acl, AclGrant};
    const GRACE: &str = "sharing-gate-grace@example.com";
    const IMP: &str = "sharing-gate-imp@example.com";
    let admin = test.account("admin@example.com").clone();
    let grace = admin
        .create_key_user_account(GRACE, STRONG, "Group member", &[], user_permissions())
        .await;
    let grace_id = grace.id();
    test.insert_account(grace);
    let mut permissions = user_permissions();
    permissions.push(Permission::Impersonate);
    let imp = admin
        .create_key_user_account(IMP, STRONG, "Impersonate permission", &[], permissions)
        .await;
    let imp_id = imp.id().document_id();
    test.insert_account(imp);
    let grace = DummyWebDavClient::new(grace_id.document_id(), GRACE, STRONG, GRACE);
    let imp = DummyWebDavClient::new(imp_id, IMP, STRONG, IMP);
    grace
        .request("GET", &format!("{shared}event.ics"), "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    let bearer = test
        .server
        .encode_access_token(
            GrantType::AccessToken,
            bob.account_id,
            bob.name,
            3600,
            None,
            None,
        )
        .await
        .unwrap();
    let mut keyless = DummyWebDavClient::new(bob.account_id, bob.name, STRONG, bob.name);
    keyless.credentials = format!("Bearer {bearer}");
    keyless
        .request("PROPFIND", &format!("/dav/card/{}/", bob.name), "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    keyless
        .request("GET", &format!("{shared}event.ics"), "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    let master: &'static str = Box::leak(format!("{}%{}", bob.name, admin.name()).into_boxed_str());
    DummyWebDavClient::new(bob.account_id, master, admin.secret(), bob.name)
        .request("PROPFIND", shared, "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    let group = admin
        .create_group_account("sharing-gate-group@example.com", "Planted group grant", &[])
        .await;
    let group_id = group.id();
    test.insert_account(group);
    admin
        .registry_update_object(
            ObjectType::Account,
            grace_id,
            json!({Property::MemberGroupIds:{group_id:true}}),
        )
        .await;
    let keys = za_unlock(test, alice.account_id, STRONG).await;
    let private_id = resource_id(test, alice.account_id, "private").await;
    let context = za_owned_calendar_context(&test.server, &keys, private_id)
        .await
        .unwrap();
    let mut calendar = context.stored.deserialize::<Calendar>().unwrap();
    calendar.acls.push(AclGrant {
        account_id: group_id.document_id(),
        grants: [Acl::Read, Acl::ReadItems].into_iter().collect(),
    });
    update_calendar(
        test,
        alice.account_id,
        private_id,
        &context.stored,
        calendar,
    )
    .await;
    grace
        .request("PROPFIND", &format!("{home}private/"), "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    grace
        .request("PROPFIND", home, "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // Case11: group membership is not an own-id grant. Assisted propname must
    // omit the foreign calendar even though upstream shared_containers finds it.
    let grace_home = format!("/dav/cal/{}/", GRACE.replace('@', "%40"));
    let discovered = grace
        .request_with_headers(
            "PROPFIND",
            &grace_home,
            [("depth", "1")],
            "<D:propfind xmlns:D=\"DAV:\"><D:propname/></D:propfind>",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        discovered
            .hrefs()
            .contains(&format!("{grace_home}default/").as_str()),
        "case11: assisted propname retains Grace's own calendar: {:?}",
        discovered.body
    );
    assert!(
        !discovered
            .hrefs()
            .contains(&format!("{home}private/").as_str()),
        "case11: assisted propname must omit the group-only foreign calendar: {:?}",
        discovered.body
    );
    let key = test
        .server
        .za_served_public_key(imp_id)
        .await
        .unwrap()
        .unwrap();
    super::pinning::pin(alice.name, IMP, &vault::keys::display_fingerprint(&key))
        .await
        .expect(200);
    za_post("shares/grant", &json!({"username":alice.name,"password":STRONG,"calendar":"shared","address":IMP,"level":"edit"})).await.expect(200);
    imp.request("PROPFIND", "/dav/cal/sharing-gate-imp%40example.com/", "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // A downstream event resolver also refuses these tokens. Check the URI
    // admission seam itself on the foreign home root and its fixed trace.
    for (client, label) in [(&keyless, "keyless"), (&imp, "Impersonate")] {
        let traced = super::mail::Traced::start(trc::EventType::variants());
        let response = client.request("PROPFIND", home, "").await;
        let events = traced.finish().await;
        assert_eq!(
            response.status,
            StatusCode::FORBIDDEN,
            "{label}: foreign home must be refused at URI gate"
        );
        assert!(
            events
                .iter()
                .any(|event| event.account_id == Some(alice.account_id as u64)
                    && event.details == "zero-access: calendar access without session keys"),
            "{label}: URI gate refusal must precede collection resolution"
        );
    }

    for (method, target, body) in [
        ("GET", format!("{shared}event.ics"), ""),
        (
            "PROPFIND",
            shared.into(),
            "<D:propfind xmlns:D=\"DAV:\"><D:propname/></D:propfind>",
        ),
        (
            "LOCK",
            shared.into(),
            "<D:lockinfo xmlns:D=\"DAV:\"><D:lockscope><D:exclusive/></D:lockscope><D:locktype><D:write/></D:locktype></D:lockinfo>",
        ),
        ("UNLOCK", shared.into(), ""),
        ("COPY", shared.into(), ""),
        ("MOVE", shared.into(), ""),
    ] {
        imp.request_with_headers(
            method,
            &target,
            [
                ("destination", format!("{home}forbidden/").as_str()),
                ("lock-token", "<urn:stalwart:davlock:1234>"),
            ],
            body,
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    }
    // Owner EWK/personal bundle and the other grantee's entire entry survive Bob's patch.
    let id = resource_id(test, alice.account_id, "shared").await;
    let before = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap()
        .stored
        .deserialize::<Calendar>()
        .unwrap();
    bob.request("PROPPATCH", shared, "<D:propertyupdate xmlns:D=\"DAV:\"><D:set><D:prop><D:displayname>Bob final personal name</D:displayname></D:prop></D:set></D:propertyupdate>").await.with_status(StatusCode::MULTI_STATUS);
    let after = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap()
        .stored
        .deserialize::<Calendar>()
        .unwrap();
    for personal in before
        .preferences
        .iter()
        .filter(|entry| entry.account_id != bob.account_id)
    {
        assert_eq!(
            after
                .preferences
                .iter()
                .find(|entry| entry.account_id == personal.account_id),
            Some(personal),
            "unopened personal/ring entry must be byte-for-byte unchanged"
        );
    }
    // Spec 5.2: the resolver checks the grant's own bits on every operation.
    // Upstream's container check reads the request's DAV resource snapshot, so
    // only a level change made after that snapshot leaves the resolver's fresh
    // read as the deciding check: park Bob's update right after the snapshot
    // (PUT_PAUSE), downgrade him to view, and require the refusal with the
    // stored event unchanged.
    {
        use crate::utils::{http::HttpRequest, za::HookParked};
        let event_id = resource_id(test, alice.account_id, "shared/event.ics").await;
        let stored = event_archive(test, alice.account_id, event_id).await;
        let request = HttpRequest::with_credentials(8899, bob.name, STRONG);
        let path = format!("{shared}event.ics");
        let text = EVENT
            .replace("share-read-UID", "sharing-dav-admission")
            .replace("share-read-private-summary", "gate-downgraded-write");
        let held = HookParked::start(
            alice.account_id,
            common::za::test_hooks::set_put_pause,
            async move {
                request
                    .send_full(
                        hyper::Method::PUT,
                        &path,
                        Some(text.into_bytes()),
                        Some("text/calendar"),
                    )
                    .await
            },
        )
        .await;
        let mut level = json!({"username":alice.name,"password":STRONG,"calendar":"shared","address":bob.name,"level":"view"});
        assert_eq!(
            za_post("shares/grant", &level).await.expect(200)["changed"],
            true
        );
        assert_eq!(
            held.finish().await.status,
            403,
            "case 11: a grant downgraded after the request snapshot must be refused by the resolver's bit check"
        );
        let unchanged = event_archive(test, alice.account_id, event_id).await;
        assert_eq!(unchanged.version, stored.version);
        assert_eq!(
            unchanged.as_bytes(),
            stored.as_bytes(),
            "case 11: the refused update must leave the event archive byte-identical"
        );
        level["level"] = json!("edit");
        assert_eq!(
            za_post("shares/grant", &level).await.expect(200)["changed"],
            true
        );
    }
    let assisted_before = test.server.core.groupware.assisted_discovery;
    for enabled in [false, true] {
        admin
            .registry_update_setting(
                registry::schema::structs::WebDav {
                    enable_assisted_discovery: enabled,
                    ..Default::default()
                },
                &[Property::EnableAssistedDiscovery],
            )
            .await;
        admin.reload_settings().await;
        test.reload_core();
        for client in [bob, &grace, &imp] {
            for prop in ["<D:allprop/>", "<D:propname/>"] {
                let body = format!("<D:propfind xmlns:D=\"DAV:\">{prop}</D:propfind>");
                for path in [
                    format!("/dav/cal/{}/", client.name.replace('@', "%40")),
                    "/dav/cal/".into(),
                ] {
                    let result = client
                        .request_with_headers("PROPFIND", &path, [("depth", "1")], &body)
                        .await
                        .with_status(StatusCode::MULTI_STATUS)
                        .body
                        .unwrap();
                    if client.account_id != bob.account_id {
                        assert!(
                            !result.contains("sharing-dav-alice"),
                            "ineligible discovery: {result}"
                        );
                    } else if enabled && path != "/dav/cal/" {
                        assert!(result.contains("/shared/"));
                    } else if !enabled && path != "/dav/cal/" {
                        assert!(!result.contains("sharing-dav-alice"));
                        assert!(!result.contains("/shared/"));
                    }
                    assert!(!result.contains("/private/"));
                }
                if !enabled && client.account_id == bob.account_id {
                    let foreign = client
                        .request_with_headers("PROPFIND", home, [("depth", "1")], &body)
                        .await
                        .with_status(StatusCode::MULTI_STATUS)
                        .body
                        .unwrap();
                    assert!(foreign.contains("/shared/"));
                    assert!(!foreign.contains("/private/"));
                }
            }
            let response = client.request("PROPFIND", &format!("/dav/pal/{}/", client.name.replace('@',"%40")), "<D:propfind xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:prop><C:calendar-home-set/></D:prop></D:propfind>").await.with_status(StatusCode::MULTI_STATUS).body.unwrap();
            assert_eq!(
                response.contains("sharing-dav-alice"),
                client.account_id == bob.account_id && !enabled
            );
        }
    }
    admin
        .registry_update_setting(
            registry::schema::structs::WebDav {
                enable_assisted_discovery: assisted_before,
                ..Default::default()
            },
            &[Property::EnableAssistedDiscovery],
        )
        .await;
    admin.reload_settings().await;
    test.reload_core();
    // Eligible corrupt events stay visible as500 items, rather than disappearing.
    let event_id = resource_id(test, alice.account_id, "shared/event.ics").await;
    let stored = event_archive(test, alice.account_id, event_id).await;
    let valid = stored.deserialize::<CalendarEvent>().unwrap();
    let mut corrupt = valid.clone();
    let carrier = corrupt.data.event.components[0].entries.last_mut().unwrap();
    let calcard::icalendar::ICalendarValue::Text(encoded) = &mut carrier.values[0] else {
        panic!("envelope")
    };
    *encoded = "not-base64".into();
    update_event(test, alice.account_id, event_id, &stored, corrupt).await;
    bob.request("GET", &format!("{shared}event.ics"), "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    let response = bob
        .request_with_headers("PROPFIND", home, [("depth", "infinity")], "")
        .await;
    // Home infinity is the upstream finite-depth refusal. A calendar depth1 visits the corrupt item.
    assert_eq!(response.status, StatusCode::FORBIDDEN);
    let response = bob
        .request_with_headers("PROPFIND", shared, [("depth", "1")], "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        response
            .hrefs()
            .iter()
            .any(|path| path.ends_with("event.ics"))
    );
    assert!(response.body.unwrap().contains("500 Internal Server Error"));
    let damaged = event_archive(test, alice.account_id, event_id).await;
    update_event(test, alice.account_id, event_id, &damaged, valid).await;
    let current = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            alice.account_id,
            Collection::Calendar,
            private_id,
        ))
        .await
        .unwrap()
        .unwrap();
    let original_private = context.stored.deserialize::<Calendar>().unwrap();
    update_calendar(
        test,
        alice.account_id,
        private_id,
        &current,
        original_private,
    )
    .await;
    // Root discovery materializes the ordinary group's default calendar.
    // Group membership grants the same cleanup authority as the ordinary DAV controls.
    grace
        .request(
            "DELETE",
            "/dav/cal/sharing-gate-group%40example.com/default/",
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    for client in [&grace, &imp] {
        client.delete_default_containers().await;
    }
}

async fn grantee_pending_grant_and_slug_reuse(
    _test: &TestServer,
    alice: &DummyWebDavClient,
    bob: &DummyWebDavClient,
    home: &str,
) {
    use crate::utils::za::{HookParked, za_post};
    use dav_proto::Depth;
    use serde_json::json;
    let pending_path = format!("{home}pending-grant/");
    alice
        .request("MKCALENDAR", &pending_path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    let request = json!({"username":alice.name,"password":STRONG,"calendar":"pending-grant","address":bob.name,"level":"view"});
    let parked = HookParked::start(
        alice.account_id,
        common::za::test_hooks::set_share_boundary_pause,
        async move { za_post("shares/grant", &request).await },
    )
    .await;
    for path in [&pending_path, home] {
        for token in [
            "urn:stalwart:davsync:0",
            "urn:stalwart:davsync:fffffffffffffffe",
        ] {
            let result =
                sharing_sync_report(bob, path, token, Depth::Infinity, None, ["D:getetag"])
                    .await
                    .with_status(StatusCode::FORBIDDEN);
            assert!(result.body.unwrap().contains("valid-sync-token"));
        }
        sharing_sync_report(bob, path, "", Depth::Infinity, None, ["D:getetag"])
            .await
            .with_status(StatusCode::MULTI_STATUS);
    }
    parked.finish().await.expect(200);
    alice
        .request("DELETE", &pending_path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let old = sharing_sync_report(bob, home, "", Depth::Infinity, None, ["D:getetag"])
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .sync_token()
        .to_string();
    let path = format!("{home}reused-grant/");
    alice
        .request("MKCALENDAR", &path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    alice
        .request_with_headers(
            "PUT",
            &format!("{path}private-history.ics"),
            [("content-type", "text/calendar")],
            EVENT.replace("share-read-UID", "grant-private-history"),
        )
        .await
        .with_status(StatusCode::CREATED);
    alice
        .request("DELETE", &path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    alice
        .request("MKCALENDAR", &path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    za_post("shares/grant",&json!({"username":alice.name,"password":STRONG,"calendar":"reused-grant","address":bob.name,"level":"view"})).await.expect(200);
    let refused = sharing_sync_report(bob, &path, &old, Depth::Infinity, None, ["D:getetag"])
        .await
        .with_status(StatusCode::FORBIDDEN);
    assert!(refused.body.unwrap().contains("valid-sync-token"));
    let initial = sharing_sync_report(bob, &path, "", Depth::Infinity, None, ["D:getetag"])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(
        !initial
            .hrefs()
            .iter()
            .any(|href| href.contains("private-history"))
    );
    alice
        .request("DELETE", &path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// A direct healthy request must not open another granted calendar's corrupt ring.
async fn grantee_unrelated_corruption(
    test: &TestServer,
    alice: &DummyWebDavClient,
    bob: &DummyWebDavClient,
    home: &str,
    shared: &str,
) {
    use crate::utils::za::za_post;
    use serde_json::json;
    let path = format!("{home}other-eligible/");
    alice
        .request("MKCALENDAR", &path, MKCAL)
        .await
        .with_status(StatusCode::CREATED);
    za_post("shares/grant",&json!({"username":alice.name,"password":STRONG,"calendar":"other-eligible","address":bob.name,"level":"view"})).await.expect(200);
    let id = resource_id(test, alice.account_id, "other-eligible").await;
    let keys = za_unlock(test, alice.account_id, STRONG).await;
    let context = za_owned_calendar_context(&test.server, &keys, id)
        .await
        .unwrap();
    let valid = context.stored.deserialize::<Calendar>().unwrap();
    let mut corrupted = valid.clone();
    let entry = corrupted
        .preferences
        .iter_mut()
        .find(|entry| entry.account_id == bob.account_id)
        .unwrap();
    let mut parts = entry
        .name
        .strip_prefix("$za$")
        .unwrap()
        .split('|')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    parts[0] = "invalid-grant-ring".into();
    entry.name = format!("$za${}", parts.join("|"));
    update_calendar(test, alice.account_id, id, &context.stored, corrupted).await;
    println!(
        "Task10 requested scope: healthy calendar with separately eligible corrupt grant ring"
    );
    bob.request("PROPFIND", shared, "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    bob.request("PROPFIND", &path, "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    bob.request("PROPFIND", home, "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    // Corruption remains an error in either valid DAV packaging: a request500
    // or a207 with a500 item for this exact eligible calendar (spec10).
    assert!(
        test.server.core.groupware.assisted_discovery,
        "the suite runs with assisted discovery on"
    );
    let own_home = format!("/dav/cal/{}/", bob.name.replace('@', "%40"));
    for prop in ["<D:allprop/>", "<D:prop><D:displayname/></D:prop>"] {
        let response = bob
            .request_with_headers(
                "PROPFIND",
                &own_home,
                [("depth", "1")],
                format!("<D:propfind xmlns:D=\"DAV:\">{prop}</D:propfind>"),
            )
            .await;
        if response.status != StatusCode::INTERNAL_SERVER_ERROR {
            response
                .with_status(StatusCode::MULTI_STATUS)
                .into_propfind_response(None)
                .properties(&path)
                .with_status(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
    let current = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            alice.account_id,
            Collection::Calendar,
            id,
        ))
        .await
        .unwrap()
        .unwrap();
    update_calendar(test, alice.account_id, id, &current, valid).await;
    alice
        .request("DELETE", &path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

fn sharing_tree(text: &str) -> calcard::icalendar::ICalendar {
    let calcard::Entry::ICalendar(tree) = calcard::Parser::new(text).entry() else {
        panic!("expected iCalendar")
    };
    tree
}

fn sharing_sent_by(
    tree: &calcard::icalendar::ICalendar,
    property: calcard::icalendar::ICalendarProperty,
    address: &str,
) -> Vec<Option<String>> {
    use calcard::icalendar::ICalendarParameterName;
    tree.components
        .iter()
        .flat_map(|component| &component.entries)
        .filter(|entry| {
            entry.name == property
                && entry
                    .values
                    .first()
                    .and_then(|value| value.as_text())
                    .is_some_and(|value| value.trim_start_matches("mailto:") == address)
        })
        .map(|entry| {
            entry
                .params
                .iter()
                .find(|parameter| parameter.name == ICalendarParameterName::SentBy)
                .and_then(|parameter| parameter.value.as_text())
                .map(str::to_string)
        })
        .collect()
}

/// Cases9/10 exercise the actual foreign-DAV scheduling caller and ordinary receiver.
async fn sharing_http_schedule_contracts(test: &mut TestServer) {
    use super::{
        drain::{body, partstat_of, set_partstat},
        fanout::{CONTENT_TYPE, NO_REPLY, Party, stored_pairs, uid_copies},
    };
    use crate::utils::za::{
        copy_href, inbox_ids, item_method, item_partstat, item_sequence, new_inbox_items,
        open_pending_item_as, za_post,
    };
    use calcard::icalendar::{ICalendarMethod, ICalendarProperty};
    use groupware::calendar::ChangedBy;
    use serde_json::json;
    const DAVE: &str = "sharing-life-dave@example.com";
    let admin = test.account("admin@example.com").clone();
    let account = admin
        .create_key_user_account(DAVE, STRONG, "No grant", &[], user_permissions())
        .await;
    test.insert_account(account);
    let alice = Party::new(test, LIFE_ALICE);
    let bob = Party::new(test, LIFE_BOB);
    let carol = Party::new(test, LIFE_CAROL);
    let dave = Party::new(test, DAVE);
    let carol_key = test
        .server
        .za_served_public_key(carol.id)
        .await
        .unwrap()
        .unwrap();
    super::pinning::pin(
        LIFE_BOB,
        LIFE_CAROL,
        &vault::keys::display_fingerprint(&carol_key),
    )
    .await
    .expect(200);
    let make = |uid, attendees: &[&str]| {
        sharing_lifecycle_body(uid, "HTTP schedule", "20990301T100000Z", attendees, &[])
    };
    let writer_marker = Some(format!("mailto:{LIFE_BOB}"));
    let mut uids = Vec::new();
    // Delegated create replaces a client-supplied false SENT-BY, and records actual changed_by.
    let uid = "case9-delegated";
    uids.push(uid);
    let path = format!("{LIFE_PATH}{uid}.ics");
    let before = inbox_ids(test, carol.id).await;
    bob.client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            make(uid, &[LIFE_CAROL]).replace(
                "ORGANIZER:mailto:",
                &format!("ORGANIZER;SENT-BY=\"mailto:{DAVE}\":mailto:"),
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    let delivered = new_inbox_items(test, carol.id, &before).await;
    assert_eq!(delivered.len(), 1);
    assert!(matches!(delivered[0].1.changed_by,ChangedBy::PrincipalId(id) if id==bob.id));
    let (message, _) = open_pending_item_as(test, carol.id, STRONG, delivered[0].0).await;
    assert_eq!(
        sharing_sent_by(&message, ICalendarProperty::Organizer, LIFE_ALICE),
        vec![writer_marker.clone()]
    );
    sharing_drain_step(test, &carol).await;
    let carol_copy = copy_href(test, carol.id, uid).await;
    assert_eq!(
        sharing_sent_by(
            &sharing_tree(&body(&carol.client, &carol_copy).await),
            ICalendarProperty::Organizer,
            LIFE_ALICE
        ),
        vec![writer_marker.clone()]
    );
    let before = inbox_ids(test, carol.id).await;
    let changed = body(&bob.client, &path)
        .await
        .replace("20990301T100000Z", "20990301T110000Z");
    bob.client
        .request_with_headers("PUT", &path, [CONTENT_TYPE], changed)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let delivered = new_inbox_items(test, carol.id, &before).await;
    assert_eq!(delivered.len(), 1);
    let (message, _) = open_pending_item_as(test, carol.id, STRONG, delivered[0].0).await;
    assert_eq!(
        sharing_sent_by(&message, ICalendarProperty::Organizer, LIFE_ALICE),
        vec![writer_marker.clone()]
    );
    sharing_drain_step(test, &carol).await;
    assert_eq!(
        sharing_sent_by(
            &sharing_tree(&body(&carol.client, &carol_copy).await),
            ICalendarProperty::Organizer,
            LIFE_ALICE
        ),
        vec![writer_marker.clone()]
    );
    set_partstat(&carol.client, &carol_copy, LIFE_CAROL, "ACCEPTED").await;
    sharing_drain_step(test, &alice).await;
    assert_eq!(
        partstat_of(&bob.client, &path, LIFE_CAROL).await.as_deref(),
        Some("ACCEPTED")
    );
    bob.client
        .request("DELETE", &path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    sharing_drain_step(test, &carol).await;
    assert!(
        body(&carol.client, &carol_copy)
            .await
            .contains("STATUS:CANCELLED")
    );
    // Cross-origin cancellation must replace, rather than inherit, organizer markers.
    for delegated_create in [false, true] {
        let uid = if delegated_create {
            "case9-cancel-loss"
        } else {
            "case9-cancel-gain"
        };
        uids.push(uid);
        let path = format!("{LIFE_PATH}{uid}.ics");
        let creator = if delegated_create {
            &bob.client
        } else {
            &alice.client
        };
        creator
            .request_with_headers("PUT", &path, [CONTENT_TYPE], make(uid, &[LIFE_CAROL]))
            .await
            .with_status(StatusCode::CREATED);
        sharing_drain_step(test, &carol).await;
        let copy = copy_href(test, carol.id, uid).await;
        assert_eq!(
            sharing_sent_by(
                &sharing_tree(&body(&carol.client, &copy).await),
                ICalendarProperty::Organizer,
                LIFE_ALICE
            ),
            vec![if delegated_create {
                writer_marker.clone()
            } else {
                None
            }]
        );
        let deleter = if delegated_create {
            &alice.client
        } else {
            &bob.client
        };
        deleter
            .request("DELETE", &path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
        sharing_drain_step(test, &carol).await;
        let tree = sharing_tree(&body(&carol.client, &copy).await);
        assert!(tree.to_string().contains("STATUS:CANCELLED"));
        assert_eq!(
            sharing_sent_by(&tree, ICalendarProperty::Organizer, LIFE_ALICE),
            vec![if delegated_create {
                None
            } else {
                writer_marker.clone()
            }]
        );
    }
    // A new override gets Bob's marker; a master imported from Alice keeps Alice's line.
    let uid = "case9-per-component";
    uids.push(uid);
    let path = format!("{LIFE_PATH}{uid}.ics");
    alice
        .client
        .request_with_headers("PUT", &path, [CONTENT_TYPE], make(uid, &[LIFE_CAROL]))
        .await
        .with_status(StatusCode::CREATED);
    sharing_drain_step(test, &carol).await;
    let mut changed = body(&bob.client, &path).await;
    let override_body = format!(
        "BEGIN:VEVENT\r\nUID:{uid}\r\nRECURRENCE-ID:20990302T100000Z\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20990302T100000Z\r\nDTEND:20990302T120000Z\r\nSUMMARY:New override\r\nORGANIZER:mailto:{LIFE_ALICE}\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{LIFE_CAROL}\r\nEND:VEVENT\r\nEND:VCALENDAR"
    );
    changed = changed.replace("END:VCALENDAR", &override_body);
    bob.client
        .request_with_headers("PUT", &path, [CONTENT_TYPE], changed)
        .await
        .with_status(StatusCode::NO_CONTENT);
    sharing_drain_step(test, &carol).await;
    let tree = sharing_tree(&body(&carol.client, &copy_href(test, carol.id, uid).await).await);
    for component in tree.components.iter().filter(|component| {
        component
            .entries
            .iter()
            .any(|entry| entry.name == ICalendarProperty::Organizer)
    }) {
        let is_override = component
            .entries
            .iter()
            .any(|entry| entry.name == ICalendarProperty::RecurrenceId);
        let organizer = component
            .entries
            .iter()
            .find(|entry| entry.name == ICalendarProperty::Organizer)
            .unwrap();
        let sent_by = organizer
            .params
            .iter()
            .find(|parameter| parameter.name == calcard::icalendar::ICalendarParameterName::SentBy)
            .and_then(|parameter| parameter.value.as_text());
        assert_eq!(
            sent_by,
            if is_override {
                Some("mailto:sharing-life-bob@example.com")
            } else {
                None
            }
        );
    }
    // External invitation held in Alice's shared calendar: Bob sends a REPLY as Alice.
    let uid = "case9-attendee-reply";
    uids.push(uid);
    let external = format!("/dav/cal/sharing-life-carol%40example.com/default/{uid}.ics");
    carol
        .client
        .request_with_headers(
            "PUT",
            &external,
            [CONTENT_TYPE],
            make(uid, &[LIFE_ALICE])
                .replace(LIFE_ALICE, LIFE_CAROL)
                .replace(
                    &format!("ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{LIFE_CAROL}"),
                    &format!("ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{LIFE_ALICE}"),
                ),
        )
        .await
        .with_status(StatusCode::CREATED);
    sharing_drain_step(test, &alice).await;
    let imported = copy_href(test, alice.id, uid).await;
    let held = format!("{LIFE_PATH}{uid}.ics");
    alice
        .client
        .request_with_headers("MOVE", &imported, [("destination", held.as_str())], "")
        .await
        .with_status(StatusCode::CREATED);
    // Oracle 46 (RFC 6638 3.2.10): an attendee's changes to its own participation
    // do not change the Schedule-Tag its scheduling object resource is served with.
    let served_tag = async |client: &DummyWebDavClient| {
        client
            .request("GET", &held, "")
            .await
            .with_status(StatusCode::OK)
            .headers
            .get("schedule-tag")
            .cloned()
    };
    let held_tag = served_tag(&alice.client).await;
    assert!(
        held_tag.is_some(),
        "oracle46: the held invitation is served a Schedule-Tag"
    );
    assert_eq!(
        served_tag(&bob.client).await,
        held_tag,
        "oracle46: the writer is served the owner's Schedule-Tag"
    );
    let before = inbox_ids(test, carol.id).await;
    let bob_before = inbox_ids(test, bob.id).await;
    set_partstat(&bob.client, &held, LIFE_ALICE, "ACCEPTED").await;
    assert_eq!(
        served_tag(&alice.client).await,
        held_tag,
        "oracle46: Bob's ACCEPTED write as Alice keeps the owner's Schedule-Tag"
    );
    assert_eq!(
        served_tag(&bob.client).await,
        held_tag,
        "oracle46: Bob's ACCEPTED write as Alice keeps the writer's Schedule-Tag"
    );
    let replies = new_inbox_items(test, carol.id, &before).await;
    assert_eq!(replies.len(), 1);
    let (reply, carriers) = open_pending_item_as(test, carol.id, STRONG, replies[0].0).await;
    assert_eq!(item_method(&reply), Some(ICalendarMethod::Reply));
    assert!(carriers.reply_token.is_some());
    assert_eq!(
        sharing_sent_by(&reply, ICalendarProperty::Attendee, LIFE_ALICE),
        vec![writer_marker.clone()]
    );
    assert_eq!(inbox_ids(test, bob.id).await, bob_before);
    sharing_drain_step(test, &carol).await;
    assert_eq!(
        partstat_of(&carol.client, &external, LIFE_ALICE)
            .await
            .as_deref(),
        Some("ACCEPTED")
    );
    let before = inbox_ids(test, carol.id).await;
    let changed = body(&bob.client, &held).await.replace(
        "PARTSTAT=ACCEPTED",
        "PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:sharing-life-bob@example.com\"",
    );
    bob.client
        .request_with_headers("PUT", &held, [CONTENT_TYPE], changed)
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        served_tag(&alice.client).await,
        held_tag,
        "oracle46: Bob's DELEGATED write as Alice keeps the owner's Schedule-Tag"
    );
    assert_eq!(
        served_tag(&bob.client).await,
        held_tag,
        "oracle46: Bob's DELEGATED write as Alice keeps the writer's Schedule-Tag"
    );
    let replies = new_inbox_items(test, carol.id, &before).await;
    assert_eq!(replies.len(), 1);
    assert_eq!(
        inbox_ids(test, bob.id).await,
        bob_before,
        "attendee-side delegation has no self run"
    );
    let (reply, _) = open_pending_item_as(test, carol.id, STRONG, replies[0].0).await;
    assert_eq!(
        item_partstat(&reply, LIFE_ALICE).as_deref(),
        Some("DELEGATED")
    );
    sharing_drain_step(test, &carol).await;
    assert_eq!(
        partstat_of(&carol.client, &external, LIFE_ALICE)
            .await
            .as_deref(),
        Some("DELEGATED")
    );
    // Bob organizes under his own address from Alice's calendar: Carol replies to Bob's history.
    let uid = "case9-own-organizer";
    uids.push(uid);
    let path = format!("{LIFE_PATH}{uid}.ics");
    bob.client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            make(uid, &[LIFE_CAROL]).replace(LIFE_ALICE, LIFE_BOB),
        )
        .await
        .with_status(StatusCode::CREATED);
    sharing_drain_step(test, &carol).await;
    let copy = copy_href(test, carol.id, uid).await;
    assert_eq!(
        sharing_sent_by(
            &sharing_tree(&body(&carol.client, &copy).await),
            ICalendarProperty::Organizer,
            LIFE_BOB
        ),
        vec![None]
    );
    let before = inbox_ids(test, bob.id).await;
    set_partstat(&carol.client, &copy, LIFE_CAROL, "ACCEPTED").await;
    assert_eq!(new_inbox_items(test, bob.id, &before).await.len(), 1);
    sharing_drain_step(test, &bob).await;
    assert_eq!(uid_copies(test, bob.id, uid).await, 0);
    assert_eq!(
        partstat_of(&alice.client, &path, LIFE_CAROL)
            .await
            .as_deref(),
        Some("NEEDS-ACTION")
    );
    println!(
        "Case9 own-name organizer: reply stays Bob inbox history, source in Alice remains unchanged (inherited EventNotFound)"
    );
    // Dave cannot acquire delegate authority by spelling Alice's organizer in his own calendar.
    let uid = "case9-spoof";
    uids.push(uid);
    let before = inbox_ids(test, carol.id).await;
    dave.client
        .request_with_headers(
            "PUT",
            &format!("/dav/cal/sharing-life-dave%40example.com/default/{uid}.ics"),
            [CONTENT_TYPE],
            make(uid, &[LIFE_CAROL]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert!(new_inbox_items(test, carol.id, &before).await.is_empty());
    dave.client
        .request("PROPFIND", LIFE_PATH, "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    // Owner-organized replies and subsequent shared writes keep Bob's self token usable.
    let uid = "case10-participation";
    uids.push(uid);
    let path = format!("{LIFE_PATH}{uid}.ics");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            make(uid, &[LIFE_BOB, LIFE_CAROL]),
        )
        .await
        .with_status(StatusCode::CREATED);
    sharing_drain_step(test, &bob).await;
    sharing_drain_step(test, &carol).await;
    let own = copy_href(test, bob.id, uid).await;
    set_partstat(&bob.client, &own, LIFE_BOB, "ACCEPTED").await;
    sharing_drain_step(test, &alice).await;
    assert_eq!(
        partstat_of(&bob.client, &path, LIFE_BOB).await.as_deref(),
        Some("ACCEPTED")
    );
    set_partstat(&bob.client, &path, LIFE_BOB, "DECLINED").await;
    sharing_drain_step(test, &bob).await;
    sharing_drain_step(test, &carol).await;
    assert_eq!(
        partstat_of(&bob.client, &own, LIFE_BOB).await.as_deref(),
        Some("DECLINED")
    );
    assert_eq!(
        item_sequence(&sharing_tree(&body(&bob.client, &own).await)),
        item_sequence(&sharing_tree(&body(&alice.client, &path).await))
    );
    let owner_keys = za_unlock(test, alice.id, STRONG).await;
    let context = za_owned_calendar_context(
        &test.server,
        &owner_keys,
        resource_id(test, alice.id, "lifecycle").await,
    )
    .await
    .unwrap();
    let archive = event_archive(
        test,
        alice.id,
        resource_id(test, alice.id, "lifecycle/case10-participation.ics").await,
    )
    .await;
    assert!(
        groupware::calendar::seal::event_pairs_in_domain(&archive, &context.domain)
            .unwrap()
            .unwrap()
            .get(LIFE_BOB)
            .is_some()
    );
    set_partstat(&bob.client, &own, LIFE_BOB, "ACCEPTED").await;
    sharing_drain_step(test, &alice).await;
    assert_eq!(
        partstat_of(&alice.client, &path, LIFE_BOB).await.as_deref(),
        Some("ACCEPTED")
    );
    // Alarm-only edits and event properties schedule nothing through the actual Bob handler.
    let before = (
        inbox_ids(test, alice.id).await,
        inbox_ids(test, bob.id).await,
        inbox_ids(test, carol.id).await,
    );
    let alarm=body(&bob.client,&path).await.replacen("END:VEVENT","BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nDESCRIPTION:Bob personal alarm\r\nEND:VALARM\r\nEND:VEVENT",1);
    bob.client
        .request_with_headers("PUT", &path, [CONTENT_TYPE], alarm)
        .await
        .with_status(StatusCode::NO_CONTENT);
    bob.client.request("PROPPATCH",&path,"<D:propertyupdate xmlns:D=\"DAV:\"><D:set><D:prop><D:displayname>Bob event property</D:displayname></D:prop></D:set></D:propertyupdate>").await.with_status(StatusCode::MULTI_STATUS);
    assert_eq!(
        (
            inbox_ids(test, alice.id).await,
            inbox_ids(test, bob.id).await,
            inbox_ids(test, carol.id).await
        ),
        before
    );
    let uid = "case9-no-attendees";
    uids.push(uid);
    bob.client
        .request_with_headers(
            "PUT",
            &format!("{LIFE_PATH}{uid}.ics"),
            [CONTENT_TYPE],
            make(uid, &[]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        (
            inbox_ids(test, alice.id).await,
            inbox_ids(test, bob.id).await,
            inbox_ids(test, carol.id).await
        ),
        before
    );
    // Both aliases reply after self import and after REQUEST token replacement.
    let uid = "case10-alias-replies";
    uids.push(uid);
    let path = format!("{LIFE_PATH}{uid}.ics");
    bob.client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            sharing_lifecycle_body(
                uid,
                "alias replies",
                "20990301T100000Z",
                &[LIFE_BOB],
                &[("20990302T100000Z", &[LIFE_ALIAS])],
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    sharing_drain_step(test, &bob).await;
    let own = copy_href(test, bob.id, uid).await;
    let bob_keys = za_unlock(test, bob.id, STRONG).await;
    let pairs = stored_pairs(test, &bob_keys, bob.id, &super::drain::relative(&own))
        .await
        .unwrap();
    assert!(pairs.get(LIFE_BOB).is_some() && pairs.get(LIFE_ALIAS).is_some());
    for round in 0..2 {
        if round == 1 {
            // Normal REQUESTs retain source tokens. Remove both aliases, then
            // re-add them so this HTTP round really exercises newly minted pairs.
            bob.client
                .request_with_headers(
                    "PUT",
                    &path,
                    [CONTENT_TYPE],
                    sharing_lifecycle_body(
                        uid,
                        "retoken prune",
                        "20990301T100000Z",
                        &[LIFE_CAROL],
                        &[],
                    ),
                )
                .await
                .with_status(StatusCode::NO_CONTENT);
            sharing_drain_step(test, &bob).await;
            bob.client
                .request_with_headers(
                    "PUT",
                    &path,
                    [CONTENT_TYPE],
                    sharing_lifecycle_body(
                        uid,
                        "alias replies retoken",
                        "20990301T100000Z",
                        &[LIFE_BOB],
                        &[("20990302T100000Z", &[LIFE_ALIAS])],
                    ),
                )
                .await
                .with_status(StatusCode::NO_CONTENT);
            sharing_drain_step(test, &bob).await;
            sharing_drain_step(test, &carol).await;
            let replaced = stored_pairs(test, &bob_keys, bob.id, &super::drain::relative(&own))
                .await
                .unwrap();
            for address in [LIFE_BOB, LIFE_ALIAS] {
                assert!(
                    replaced.get(address) != pairs.get(address),
                    "remove/re-add must mint a fresh token for {address}"
                );
            }
        }
        for address in [LIFE_BOB, LIFE_ALIAS] {
            set_partstat(
                &bob.client,
                &own,
                address,
                if round == 0 { "ACCEPTED" } else { "DECLINED" },
            )
            .await;
            sharing_drain_step(test, &alice).await;
            assert_eq!(
                partstat_of(&alice.client, &path, address).await.as_deref(),
                Some(if round == 0 { "ACCEPTED" } else { "DECLINED" })
            );
        }
    }
    // Sole attendee paths: ordinary decline and Bob create/summary/delete.
    // The delegated create uses ForceSend so second-only finalize has a stored effect.
    for delegated_create in [false, true] {
        let uid = if delegated_create {
            "case10-sole-delegated"
        } else {
            "case10-sole-decline"
        };
        uids.push(uid);
        let path = format!("{LIFE_PATH}{uid}.ics");
        let creator = if delegated_create {
            &bob.client
        } else {
            &alice.client
        };
        let source = make(uid, &[LIFE_BOB]);
        let source = if delegated_create {
            source.replace("ATTENDEE;", "ATTENDEE;SCHEDULE-FORCE-SEND=REQUEST;")
        } else {
            source
        };
        creator
            .request_with_headers("PUT", &path, [CONTENT_TYPE], source)
            .await
            .with_status(StatusCode::CREATED);
        if delegated_create {
            use calcard::icalendar::ICalendarParameterName;
            let stored_source = sharing_tree(&body(&bob.client, &path).await);
            assert!(
                !stored_source
                    .components
                    .iter()
                    .flat_map(|component| &component.entries)
                    .flat_map(|entry| &entry.params)
                    .any(|parameter| parameter.name == ICalendarParameterName::ScheduleForceSend),
                "case10-sole-delegated: second-only finalize removes stored SCHEDULE-FORCE-SEND"
            );
        }
        sharing_drain_step(test, &bob).await;
        let own = copy_href(test, bob.id, uid).await;
        if !delegated_create {
            set_partstat(&bob.client, &path, LIFE_BOB, "DECLINED").await;
            sharing_drain_step(test, &bob).await;
            assert_eq!(
                partstat_of(&bob.client, &own, LIFE_BOB).await.as_deref(),
                Some("DECLINED")
            );
        }
        let changed = body(&bob.client, &path)
            .await
            .replace("SUMMARY:HTTP schedule", "SUMMARY:sole updated");
        bob.client
            .request_with_headers("PUT", &path, [CONTENT_TYPE], changed)
            .await
            .with_status(StatusCode::NO_CONTENT);
        sharing_drain_step(test, &bob).await;
        assert!(
            body(&bob.client, &own)
                .await
                .contains("SUMMARY:sole updated")
        );
        bob.client
            .request("DELETE", &path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
        sharing_drain_step(test, &bob).await;
        assert!(body(&bob.client, &own).await.contains("STATUS:CANCELLED"));
    }
    let uid = "case10-remove-self";
    uids.push(uid);
    let path = format!("{LIFE_PATH}{uid}.ics");
    alice
        .client
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            make(uid, &[LIFE_BOB, LIFE_CAROL]),
        )
        .await
        .with_status(StatusCode::CREATED);
    sharing_drain_step(test, &bob).await;
    sharing_drain_step(test, &carol).await;
    let own = copy_href(test, bob.id, uid).await;
    let changed = body(&bob.client, &path)
        .await
        .replace("\r\n ", "")
        .split("\r\n")
        .filter(|line| {
            !(line.starts_with("ATTENDEE") && line.ends_with(&format!("mailto:{LIFE_BOB}")))
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    bob.client
        .request_with_headers("PUT", &path, [CONTENT_TYPE], changed)
        .await
        .with_status(StatusCode::NO_CONTENT);
    sharing_drain_step(test, &bob).await;
    sharing_drain_step(test, &carol).await;
    assert!(body(&bob.client, &own).await.contains("STATUS:CANCELLED"));
    assert!(
        !crate::utils::za::item_attendees(&sharing_tree(
            &body(&carol.client, &copy_href(test, carol.id, uid).await).await
        ))
        .iter()
        .any(|address| address == LIFE_BOB)
    );
    // Delivery authority is fixed by the authorized write, not a later grant lookup in drain.
    let uid = "case9-queued-revoke";
    uids.push(uid);
    let path = format!("{LIFE_PATH}{uid}.ics");
    common::za::test_hooks::set_drain_skip(carol.id);
    bob.client
        .request_with_headers("PUT", &path, [CONTENT_TYPE], make(uid, &[LIFE_CAROL]))
        .await
        .with_status(StatusCode::CREATED);
    za_post(
        "shares/revoke",
        &json!({"username":LIFE_ALICE,"password":STRONG,"calendar":"lifecycle","grant":bob.id}),
    )
    .await
    .expect(200);
    common::za::test_hooks::clear_drain_skip(carol.id);
    sharing_drain_step(test, &carol).await;
    assert_eq!(uid_copies(test, carol.id, uid).await, 1);
    za_post("shares/grant",&json!({"username":LIFE_ALICE,"password":STRONG,"calendar":"lifecycle","address":LIFE_BOB,"level":"edit"})).await.expect(200);
    // All synthetic source/copy/inbox rows are removed before the legacy leak scan.
    for uid in uids {
        for party in [&alice, &bob, &carol, &dave] {
            if uid_copies(test, party.id, uid).await > 0 {
                party
                    .client
                    .request_with_headers(
                        "DELETE",
                        &copy_href(test, party.id, uid).await,
                        [NO_REPLY],
                        "",
                    )
                    .await
                    .with_status(StatusCode::NO_CONTENT);
            }
        }
    }
    for party in [&alice, &bob, &carol, &dave] {
        party.delete_items(inbox_ids(test, party.id).await).await;
    }
    dave.client.delete_default_containers().await;
    println!(
        "Cases9/10 actual HTTP delegated requests, component markers, cross-origin cancellations, replies, alias retokens, no-message writes and queued revoke passed"
    );
}
