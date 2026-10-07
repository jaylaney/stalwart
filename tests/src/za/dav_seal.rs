/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use calcard::{Entry, Parser};
use dav_proto::schema::property::{DavProperty, WebDavProperty};
use groupware::{cache::GroupwareCache, calendar::CalendarEvent};
use hyper::StatusCode;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive},
};
use types::collection::{Collection, SyncCollection};

pub const EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nX-WR-CALNAME:calname-canary\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:STANDARD\r\nDTSTART:19961027T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nTZNAME:tzname-canary\r\nCOMMENT:tzcomment-canary\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:za-event-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART;TZID=Europe/Berlin;X-PARAM=param-canary:20240102T090000\r\nDTEND;TZID=Europe/Berlin:20240102T100000\r\nSUMMARY:summary-canary\r\nDESCRIPTION:description-canary\r\nLOCATION:location-canary\r\nATTENDEE;CN=attendee-canary:mailto:attendee-canary@example.com\r\nX-CUSTOM:xprop-canary\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

pub const CANARIES: &[&str] = &[
    "calname-canary",
    "tzname-canary",
    "tzcomment-canary",
    "param-canary",
    "summary-canary",
    "description-canary",
    "location-canary",
    "attendee-canary",
    "xprop-canary",
];

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");

pub async fn raw_event(
    test: &TestServer,
    account_id: u32,
    path: &str,
) -> (Archive<AlignedBytes>, u32) {
    let resources = test
        .server
        .fetch_dav_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let resource = resources
        .by_path(path)
        .unwrap_or_else(|| panic!("{path} not found"));
    let document_id = resource.document_id();
    let archive = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEvent,
            document_id,
        ))
        .await
        .unwrap()
        .expect("event archive");
    (archive, document_id)
}

pub async fn raw_calendar(test: &TestServer, account_id: u32, path: &str) -> Archive<AlignedBytes> {
    let resources = test
        .server
        .fetch_dav_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let resource = resources
        .by_path(path)
        .unwrap_or_else(|| panic!("{path} not found"));
    test.server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::Calendar,
            resource.document_id(),
        ))
        .await
        .unwrap()
        .expect("calendar archive")
}

/// The VCALENDAR root's `X-ZA-KEY` value (the DEK wrapped for this write),
/// unfolded.
fn key_envelope(text: &str) -> String {
    text.replace("\r\n ", "")
        .lines()
        .find(|line| line.starts_with("X-ZA-KEY:"))
        .unwrap_or_else(|| panic!("no key envelope: {text}"))
        .to_string()
}

/// The account's used quota as reported on a collection. `key1` has no
/// quota limit, so `quota-available-bytes` is not reported; the used bytes
/// carry the same signal.
async fn used_quota(client: &DummyWebDavClient, path: &str) -> u64 {
    client
        .propfind(path, [DavProperty::WebDav(WebDavProperty::QuotaUsedBytes)])
        .await
        .properties(path)
        .get(DavProperty::WebDav(WebDavProperty::QuotaUsedBytes))
        .value()
        .parse()
        .unwrap()
}

fn parse(text: &str) -> calcard::icalendar::ICalendar {
    match Parser::new(text).entry() {
        Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access PUT/GET sealing tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let path = "/dav/cal/key1@example.com/default/sealed-1.ics";
    let quota_path = "/dav/cal/key1%40example.com/default/";

    let created = client
        .request_with_headers("PUT", path, [CONTENT_TYPE], EVENT)
        .await
        .with_status(StatusCode::CREATED);
    let etag1 = created.etag().to_string();

    // Stored record: no canary, carriers present, size is the body length.
    let (archive, _) = raw_event(test, id, "default/sealed-1.ics").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    for canary in CANARIES {
        assert!(
            !raw.contains(canary),
            "{canary} leaked into the stored event"
        );
    }
    let stored = archive.unarchive::<CalendarEvent>().unwrap();
    assert_eq!(stored.size.to_native() as usize, EVENT.len());
    let stored_text = stored.data.event.to_string();
    assert!(
        stored_text.contains("X-ZA-KEY:") && stored_text.contains("X-ZA-SEALED:"),
        "{stored_text}"
    );
    assert!(
        stored_text.contains("TZID:Europe/Berlin")
            && stored_text.contains("DTSTART;TZID=Europe/Berlin:20240102T090000"),
        "visible fields stay visible: {stored_text}"
    );

    // GET: byte-faithful tree, no carrier, same ETag as the PUT.
    let got = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK);
    assert_eq!(got.etag(), etag1);
    let body = got.body.clone().unwrap();
    assert!(!body.contains("X-ZA-"), "{body}");
    for canary in CANARIES {
        assert!(body.contains(canary), "{canary} missing from GET body");
    }
    assert_eq!(
        parse(&body),
        parse(EVENT),
        "GET returns the original tree entry for entry"
    );
    client
        .request("HEAD", path, "")
        .await
        .with_status(StatusCode::OK)
        .with_header("content-length", &EVENT.len().to_string());

    // Unchanged PUT hits the no-change shortcut (compared against the unsealed view).
    client
        .request_with_headers("PUT", path, [CONTENT_TYPE], EVENT)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let (again, _) = raw_event(test, id, "default/sealed-1.ics").await;
    assert_eq!(
        again.version, archive.version,
        "no rewrite on an unchanged PUT"
    );

    // A sealed-only change (SUMMARY) is a real change: new DEK, new ETag.
    let changed = EVENT.replace("summary-canary", "summary-canary-2");
    let updated = client
        .request_with_headers("PUT", path, [CONTENT_TYPE], changed.clone())
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_ne!(updated.etag(), etag1);
    let (after, _) = raw_event(test, id, "default/sealed-1.ics").await;
    assert_ne!(after.version, archive.version);
    let after_text = after
        .unarchive::<CalendarEvent>()
        .unwrap()
        .data
        .event
        .to_string();
    assert_ne!(
        key_envelope(&after_text),
        key_envelope(&stored_text),
        "fresh key envelope"
    );
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("summary-canary-2"));

    // If-Match works against the stored ETag.
    client
        .request_with_headers(
            "PUT",
            path,
            [CONTENT_TYPE, ("if-match", etag1.as_str())],
            EVENT,
        )
        .await
        .with_status(StatusCode::PRECONDITION_FAILED);

    test.wait_for_tasks().await;
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);

    // Repeated PUT and PROPPATCH followed by DELETE: quota back at baseline.
    let baseline = used_quota(&client, quota_path).await;
    for _ in 0..3 {
        client
            .request_with_headers("PUT", path, [CONTENT_TYPE], EVENT)
            .await;
        client
            .proppatch(path, [("D:displayname", "quota-canary")], [], [])
            .await
            .with_status(StatusCode::MULTI_STATUS);
    }
    assert!(
        used_quota(&client, quota_path).await > baseline,
        "sealed events still count against quota"
    );
    test.wait_for_tasks().await;
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        used_quota(&client, quota_path).await,
        baseline,
        "quota returns to baseline after DELETE"
    );
}
