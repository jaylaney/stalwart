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
    test.wait_for_tasks().await;
}
