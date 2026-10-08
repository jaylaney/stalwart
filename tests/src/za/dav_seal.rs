/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use calcard::{Entry, Parser};
use dav_proto::Depth;
use dav_proto::schema::property::{DavProperty, WebDavProperty};
use groupware::{
    cache::GroupwareCache,
    calendar::{Calendar, CalendarEvent},
};
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

    // PROPFIND getetag is the PUT's (and GET's) ETag: bound to the stored bytes.
    let propfind = client
        .request_with_headers(
            "PROPFIND",
            path,
            [("depth", "0")],
            "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\"><D:prop><D:getetag/></D:prop></D:propfind>",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    let bare = etag1.trim_matches('"');
    assert!(
        propfind.contains(&format!("getetag>\"{bare}\"<"))
            || propfind.contains(&format!("getetag>&quot;{bare}&quot;<")),
        "PROPFIND getetag differs from PUT ETag {etag1}: {propfind}"
    );
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

pub async fn test_reports(test: &mut TestServer) {
    println!("Running zero-access report tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let path = "/dav/cal/key1@example.com/default/report-1.ics";
    let other = "/dav/cal/key1@example.com/default/report-2.ics";
    client
        .request_with_headers("PUT", path, [CONTENT_TYPE], EVENT)
        .await
        .with_status(StatusCode::CREATED);
    client
        .request_with_headers(
            "PUT",
            other,
            [CONTENT_TYPE],
            EVENT
                .replace("za-event-1", "za-event-2")
                .replace("summary-canary", "other-canary"),
        )
        .await
        .with_status(StatusCode::CREATED);

    // PROPFIND with calendar-data on the collection.
    let response = client
        .request_with_headers(
            "PROPFIND",
            "/dav/cal/key1@example.com/default/",
            [("depth", "1")],
            "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/><A:calendar-data/></D:prop></D:propfind>",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let body = response.body.clone().unwrap();
    assert!(
        body.contains("summary-canary") && !body.contains("X-ZA-"),
        "{body}"
    );

    // calendar-query with a text match on a sealed property.
    let query = "<?xml version=\"1.0\"?><A:calendar-query xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><D:prop><A:calendar-data/></D:prop><A:filter><A:comp-filter name=\"VCALENDAR\"><A:comp-filter name=\"VEVENT\"><A:prop-filter name=\"SUMMARY\"><A:text-match>summary-canary</A:text-match></A:prop-filter></A:comp-filter></A:comp-filter></A:filter></A:calendar-query>";
    let body = client
        .request_with_headers(
            "REPORT",
            "/dav/cal/key1@example.com/default/",
            [("depth", "1")],
            query,
        )
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(
        body.contains("report-1.ics")
            && !body.contains("report-2.ics")
            && body.contains("location-canary")
            && !body.contains("X-ZA-"),
        "{body}"
    );
    let miss = query.replace("summary-canary", "no-such-summary");
    let body = client
        .request_with_headers(
            "REPORT",
            "/dav/cal/key1@example.com/default/",
            [("depth", "1")],
            miss,
        )
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(!body.contains("report-1.ics"), "{body}");

    // multiget and sync-collection.
    let body = client
        .multiget_calendar("/dav/cal/key1@example.com/default/", &[path])
        .await
        .response
        .body
        .unwrap();
    assert!(
        body.contains("description-canary") && !body.contains("X-ZA-"),
        "{body}"
    );
    let body = client
        .sync_collection(
            "/dav/cal/key1@example.com/default/",
            "",
            Depth::One,
            None,
            ["A:calendar-data"],
        )
        .await
        .body
        .unwrap();
    assert!(
        body.contains("summary-canary") && !body.contains("X-ZA-"),
        "{body}"
    );

    // free-busy by the owner.
    let fb = "<?xml version=\"1.0\"?><A:free-busy-query xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><A:time-range start=\"20240101T000000Z\" end=\"20240103T000000Z\"/></A:free-busy-query>";
    let body = client
        .request("REPORT", "/dav/cal/key1@example.com/default/", fb)
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(
        body.contains("FREEBUSY") && body.contains("20240102T080000Z/20240102T090000Z"),
        "{body}"
    );

    // A corrupted stored record fails that one item with 500 and leaves the rest readable.
    let (archive, document_id) = raw_event(test, id, "default/report-1.ics").await;
    let mut broken = archive.deserialize::<CalendarEvent>().unwrap();
    let root = &mut broken.data.event.components[0];
    let last = root.entries.last_mut().unwrap();
    last.values = vec![calcard::icalendar::ICalendarValue::Text("AQI=".into())];
    let account_info = test.server.account_info(id).await.unwrap();
    let mut batch = store::write::BatchBuilder::new();
    broken
        .update(
            account_info.account_tenant_ids(),
            archive.to_unarchived::<CalendarEvent>().unwrap(),
            id,
            document_id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    let body = client
        .multiget_calendar("/dav/cal/key1@example.com/default/", &[path, other])
        .await
        .response
        .body
        .unwrap();
    assert!(
        body.contains("HTTP/1.1 500")
            && body.contains("other-canary")
            && !body.contains("summary-canary")
            && !body.contains("X-ZA-"),
        "{body}"
    );
    let body = client
        .request_with_headers(
            "PROPFIND",
            "/dav/cal/key1@example.com/default/",
            [("depth", "1")],
            "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/><A:calendar-data/></D:prop></D:propfind>",
        )
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(
        body.contains("HTTP/1.1 500") && body.contains("other-canary"),
        "{body}"
    );

    test.wait_for_tasks().await;
    for path in [path, other] {
        client
            .request("DELETE", path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}

/// MKCALENDAR with the Apple `calendar-color` dead property; the helper in
/// `utils/webdav.rs` does not declare the `C:` prefix for MKCOL bodies.
pub(super) fn mkcalendar_body(props: &[(&str, &str)]) -> String {
    let mut body = concat!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
        "<A:mkcalendar xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\" ",
        "xmlns:C=\"http://calendarserver.org/ns/\"><D:set><D:prop>"
    )
    .to_string();
    for (key, value) in props {
        body.push_str(&format!("<{key}>{value}</{key}>"));
    }
    body.push_str("</D:prop></D:set></A:mkcalendar>");
    body
}

pub async fn test_collections(test: &mut TestServer) {
    println!("Running zero-access collection sealing tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let cal = "/dav/cal/key1%40example.com/work/";
    let collection_canaries = [
        "displayname-canary",
        "coldesc-canary",
        "#aabbcc-canary",
        "tzname-canary",
        "tzcomment-canary",
    ];

    client
        .request(
            "MKCALENDAR",
            cal,
            mkcalendar_body(&[
                ("D:displayname", "Work displayname-canary"),
                ("A:calendar-description", "coldesc-canary"),
                ("C:calendar-color", "#aabbcc-canary"),
            ]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let archive = raw_calendar(test, id, "work").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    assert!(raw.contains("$za$"), "no collection marker");
    for canary in collection_canaries {
        assert!(
            !raw.contains(canary),
            "{canary} leaked into the stored collection"
        );
    }
    let stored = archive.unarchive::<Calendar>().unwrap();
    assert_eq!(stored.name, "work", "slug visible");
    let pref = stored.preferences(id);
    assert!(pref.name.starts_with("$za$"));
    assert!(pref.description.is_none());
    assert!(stored.dead_properties.0.is_empty());
    let props = client
        .propfind(
            cal,
            [
                "D:displayname",
                "A:calendar-description",
                "C:calendar-color",
            ],
        )
        .await;
    props
        .properties(cal)
        .get("D:displayname")
        .with_values(["Work displayname-canary"]);
    props
        .properties(cal)
        .get("A:calendar-description")
        .with_values(["coldesc-canary"]);
    props
        .properties(cal)
        .get("calendar-color")
        .with_values(["#aabbcc-canary", "[xmlns]:http://calendarserver.org/ns/"]);

    // Custom timezone: calculation rules visible, names and comments sealed,
    // round trip intact.
    let tz = crate::webdav::TEST_VTIMEZONE_1
        .replace("Eastern Standard Time (US Canada)", "tzname-canary")
        .replace(
            "LAST-MODIFIED:19870101T000000Z\n",
            "LAST-MODIFIED:19870101T000000Z\nX-LIC-LOCATION:America/New_York\n",
        )
        .replace(
            "TZOFFSETTO:-0500\n",
            "TZOFFSETTO:-0500\nCOMMENT:tzcomment-canary\n",
        )
        .replace('\n', "\r\n");
    client
        .proppatch(cal, [("A:calendar-timezone", tz.as_str())], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let archive = raw_calendar(test, id, "work").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    assert!(raw.contains("$za$"), "no collection marker");
    for canary in collection_canaries {
        assert!(
            !raw.contains(canary),
            "{canary} leaked into the stored collection"
        );
    }
    let stored = archive.unarchive::<Calendar>().unwrap();
    let groupware::calendar::ArchivedTimezone::Custom(stored_tz) =
        &stored.preferences(id).time_zone
    else {
        panic!("custom timezone expected")
    };
    let dump = stored_tz.to_string();
    assert!(
        dump.contains("TZID:US-Eastern")
            && dump.contains("TZOFFSETFROM:-0400")
            && dump.contains("RRULE:")
            && dump.contains("X-LIC-LOCATION:America/New_York")
            && !dump.contains("COMMENT"),
        "{dump}"
    );
    let back = client.propfind(cal, ["A:calendar-timezone"]).await;
    let value = back
        .properties(cal)
        .get("A:calendar-timezone")
        .value()
        .to_string();
    assert!(
        value.contains("tzname-canary")
            && value.contains("tzcomment-canary")
            && value.contains("X-LIC-LOCATION:America/New_York")
            && value.contains("TZID:US-Eastern")
            && !value.contains("X-ZA-"),
        "{value}"
    );

    // Clearing the last ordinary value keeps the timezone readable.
    client
        .proppatch(cal, [], ["A:calendar-description", "C:calendar-color"], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let back = client
        .propfind(cal, ["A:calendar-timezone", "D:displayname"])
        .await;
    assert!(
        back.properties(cal)
            .get("A:calendar-timezone")
            .value()
            .contains("tzname-canary")
    );
    back.properties(cal)
        .get("D:displayname")
        .with_values(["Work displayname-canary"]);

    // The server-created default calendar: plaintext until the owner first
    // writes a property, sealed afterwards.
    let default = "/dav/cal/key1%40example.com/default/";
    let archive = raw_calendar(test, id, "default").await;
    assert!(
        !archive
            .unarchive::<Calendar>()
            .unwrap()
            .preferences(id)
            .name
            .starts_with("$za$"),
        "the default calendar starts in plaintext"
    );
    client
        .proppatch(
            default,
            [("D:displayname", "Default displayname-canary")],
            [],
            [],
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let archive = raw_calendar(test, id, "default").await;
    assert!(
        archive
            .unarchive::<Calendar>()
            .unwrap()
            .preferences(id)
            .name
            .starts_with("$za$")
    );
    assert!(!String::from_utf8_lossy(archive.as_bytes()).contains("displayname-canary"));
    client
        .propfind(default, ["D:displayname"])
        .await
        .properties(default)
        .get("D:displayname")
        .with_values(["Default displayname-canary"]);

    // Event PROPPATCH: display name and dead properties go into X-ZA-EXTRA.
    let path = "/dav/cal/key1%40example.com/work/evt.ics";
    client
        .request_with_headers("PUT", path, [CONTENT_TYPE], EVENT)
        .await
        .with_status(StatusCode::CREATED);
    client
        .proppatch(
            path,
            [
                ("D:displayname", "evtname-canary"),
                ("C:za-dead", "dead-canary"),
            ],
            [],
            [],
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let (archive, _) = raw_event(test, id, "work/evt.ics").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    for canary in CANARIES.iter().chain(&["evtname-canary", "dead-canary"]) {
        assert!(
            !raw.contains(canary),
            "{canary} leaked into the stored event"
        );
    }
    let stored = archive.unarchive::<CalendarEvent>().unwrap();
    assert!(stored.display_name.is_none() && stored.dead_properties.0.is_empty());
    assert!(stored.data.event.to_string().contains("X-ZA-EXTRA:"));
    let props = client.propfind(path, ["D:displayname", "C:za-dead"]).await;
    props
        .properties(path)
        .get("D:displayname")
        .with_values(["evtname-canary"]);
    props
        .properties(path)
        .get("za-dead")
        .with_values(["dead-canary", "[xmlns]:http://calendarserver.org/ns/"]);
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(
        !body.contains("X-ZA-") && body.contains("summary-canary"),
        "{body}"
    );

    // Collection COPY within the account: copied as stored, readable at the
    // new id.
    let copy = "/dav/cal/key1%40example.com/work-copy/";
    test.wait_for_tasks().await;
    client
        .request_with_headers(
            "COPY",
            cal,
            [("destination", copy), ("depth", "infinity")],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    client
        .propfind(copy, ["D:displayname"])
        .await
        .properties(copy)
        .get("D:displayname")
        .with_values(["Work displayname-canary"]);
    let body = client
        .request("GET", "/dav/cal/key1%40example.com/work-copy/evt.ics", "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("summary-canary"));

    // A copied collection is editable: its bundle unseals and reseals.
    client
        .proppatch(copy, [("D:displayname", "stale-name")], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    client
        .propfind(copy, ["D:displayname"])
        .await
        .properties(copy)
        .get("D:displayname")
        .with_values(["stale-name"]);

    // COPY over an existing collection (Overwrite: T): destination replaced,
    // still readable. The destination holds no copy of the source's events:
    // upstream answers 409 when it does, for any account (both batch updates
    // hit the same event document).
    let dest = "/dav/cal/key1%40example.com/work-dest/";
    client
        .request(
            "MKCALENDAR",
            dest,
            mkcalendar_body(&[("D:displayname", "stale-name")]),
        )
        .await
        .with_status(StatusCode::CREATED);
    test.wait_for_tasks().await;
    client
        .request_with_headers(
            "COPY",
            cal,
            [
                ("destination", dest),
                ("depth", "infinity"),
                ("overwrite", "T"),
            ],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    client
        .propfind(dest, ["D:displayname"])
        .await
        .properties(dest)
        .get("D:displayname")
        .with_values(["Work displayname-canary"]);
    let body = client
        .request("GET", "/dav/cal/key1%40example.com/work-dest/evt.ics", "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(
        body.contains("summary-canary") && !body.contains("X-ZA-"),
        "{body}"
    );

    test.wait_for_tasks().await;
    for path in [dest, copy, cal] {
        client
            .request("DELETE", path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}
