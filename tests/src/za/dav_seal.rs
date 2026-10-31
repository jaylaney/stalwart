/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::STRONG;
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{StoredEvent, plant_event, shared_container, stored_event},
};
use calcard::{Entry, Parser};
use dav_proto::Depth;
use dav_proto::schema::property::{DavProperty, WebDavProperty};
use groupware::{
    SizeWriter,
    cache::GroupwareCache,
    calendar::{Calendar, CalendarEvent, CalendarEventContent, seal::is_sealed_container},
};
use hyper::StatusCode;
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes},
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

/// The document id of the account's resource at `path` (`<calendar>/<name>`).
pub async fn resource_id(test: &TestServer, account_id: u32, path: &str) -> u32 {
    test.server
        .fetch_groupware_resources(account_id, account_id, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path(path)
        .unwrap_or_else(|| panic!("{path} not found"))
        .document_id()
}

/// The event at `path`: its content archive (the stored tree, which 0.16
/// read from the one event archive) and its document id. At 1.0 the row is
/// a separate record (`raw_event_records`).
pub async fn raw_event(
    test: &TestServer,
    account_id: u32,
    path: &str,
) -> (Archive<ArchiveBytes>, u32) {
    let (records, document_id) = raw_event_records(test, account_id, path).await;
    (records.content, document_id)
}

/// Both stored records of the event at `path`, and its document id.
pub async fn raw_event_records(
    test: &TestServer,
    account_id: u32,
    path: &str,
) -> (StoredEvent, u32) {
    let document_id = resource_id(test, account_id, path).await;
    (
        stored_event(test, account_id, document_id).await,
        document_id,
    )
}

pub async fn raw_calendar(test: &TestServer, account_id: u32, path: &str) -> Archive<ArchiveBytes> {
    let document_id = resource_id(test, account_id, path).await;
    test.server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            account_id,
            Collection::Calendar,
            document_id,
        ))
        .await
        .unwrap()
        .expect("calendar archive")
}

/// The stored size of a key account's event at 1.0: upstream's builder
/// derives the row's `size` from the content it stores
/// (`refresh_from_content`), the sealed tree (expectation ledger, part
/// 4c). Readers still serve the plaintext's length (HEAD, PROPFIND).
fn sealed_size(content: &Archive<ArchiveBytes>) -> usize {
    SizeWriter::ical(
        &content
            .deserialize::<CalendarEventContent>()
            .unwrap()
            .data
            .event,
    )
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

    // Stored records: no canary in the row or the content, carriers
    // present. At 1.0 the row's size is upstream's measure of the stored
    // (sealed) tree; HEAD below still serves the body length.
    let (records, sealed_id) = raw_event_records(test, id, "default/sealed-1.ics").await;
    let archive = records.content.clone();
    // Per-user event data (calendar spec, near line 717): a key account's
    // PUT stores no preferences.
    crate::utils::za::assert_no_event_preferences(test, id, sealed_id).await;
    let raw = records.bytes();
    for canary in CANARIES {
        assert!(
            !raw.contains(canary),
            "{canary} leaked into the stored event"
        );
    }
    let row = records.meta.unarchive::<CalendarEvent>().unwrap();
    assert_eq!(row.size.to_native() as usize, sealed_size(&archive));
    assert!(
        row.display_name.is_none(),
        "ruling 15: the row holds no name"
    );
    let stored = archive.unarchive::<CalendarEventContent>().unwrap();
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
        .unarchive::<CalendarEventContent>()
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
    let (records, document_id) = raw_event_records(test, id, "default/report-1.ics").await;
    let mut broken = records.za_event();
    let root = &mut broken.content.data.event.components[0];
    let last = root.entries.last_mut().unwrap();
    last.values = vec![calcard::icalendar::ICalendarValue::Text("AQI=".into())];
    crate::utils::za::write_stored_event(test, id, document_id, &records, broken).await;
    client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    // HEAD shares GET's handler, and a PUT over an existing event unseals it
    // to compare: both fail the same way, before any conditional header is
    // looked at, and the failed PUT leaves the record as it was.
    let (tampered, _) = raw_event_records(test, id, "default/report-1.ics").await;
    client
        .request("HEAD", path, "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    client
        .request_with_headers(
            "PUT",
            path,
            [CONTENT_TYPE],
            EVENT.replace("summary-canary", "tampered-put-canary"),
        )
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    let (after, _) = raw_event_records(test, id, "default/report-1.ics").await;
    assert_eq!(after.bytes(), tampered.bytes(), "a failed PUT wrote");
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

fn time_range_query(start: &str, end: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\" ?><C:calendar-query xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/></D:prop><C:filter><C:comp-filter name=\"VCALENDAR\"><C:comp-filter name=\"VEVENT\"><C:time-range start=\"{start}\" end=\"{end}\"/></C:comp-filter></C:comp-filter></C:filter></C:calendar-query>"
    )
}

/// Whether a depth-1 calendar-query on `cal` returns `floating.ics`.
async fn reports_floating(client: &DummyWebDavClient, cal: &str, query: &str) -> bool {
    let response = client
        .request_with_headers("REPORT", cal, [("depth", "1")], query)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    response
        .hrefs()
        .iter()
        .any(|href| href.ends_with("/floating.ics"))
}

/// A floating event: 23:00 local on 10 January 2099.
const FLOATING: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:za-floating-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990110T230000\r\nDTEND:20990110T233000\r\nSUMMARY:floating\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

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
    let pref = stored.preferences(id).unwrap();
    assert!(pref.name.starts_with("$za$"));
    assert!(pref.description.is_none());
    // At 1.0 `calendar-color` is a dead property in the calendar's shared
    // metadata container, which MKCALENDAR sealed (ruling 16).
    let work_id = resource_id(test, id, "work").await;
    let container = shared_container(test, id, Collection::Calendar, work_id)
        .await
        .expect("the calendar's dead-property container");
    assert!(is_sealed_container(&container), "an unsealed container");
    let container_text = String::from_utf8_lossy(&container).to_string();
    for canary in collection_canaries.iter().chain(&["calendar-color"]) {
        assert!(
            !container_text.contains(canary),
            "{canary} in the stored container"
        );
    }
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
        &stored.preferences(id).unwrap().time_zone
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

    // The cleared values are gone, and the record stays sealed.
    let gone = client
        .propfind(cal, ["A:calendar-description", "C:calendar-color"])
        .await;
    gone.properties(cal)
        .get("A:calendar-description")
        .with_status(StatusCode::NOT_FOUND);
    gone.properties(cal)
        .get("calendar-color")
        .with_status(StatusCode::NOT_FOUND);
    let archive = raw_calendar(test, id, "work").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    assert!(raw.contains("$za$"), "no collection marker");
    for canary in collection_canaries {
        assert!(!raw.contains(canary), "{canary} in the stored collection");
    }

    // A PROPPATCH that sets only creationdate still writes a sealed bundle.
    client
        .proppatch(cal, [("D:creationdate", "2000-01-01T00:00:00Z")], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let archive = raw_calendar(test, id, "work").await;
    let raw = String::from_utf8_lossy(archive.as_bytes()).to_string();
    for canary in collection_canaries {
        assert!(!raw.contains(canary), "{canary} in the stored collection");
    }
    let stored = archive.unarchive::<Calendar>().unwrap();
    assert!(stored.preferences(id).unwrap().name.starts_with("$za$"));
    assert_eq!(stored.created.to_native(), 946684800);
    client
        .propfind(cal, ["D:displayname"])
        .await
        .properties(cal)
        .get("D:displayname")
        .with_values(["Work displayname-canary"]);

    // Time-range REPORT in a calendar with a sealed custom timezone: the
    // query resolves the calendar timezone by name (here its
    // X-LIC-LOCATION:America/New_York) from the stored, sealed record, so a
    // floating event lands where it does on an ordinary calendar with the
    // same timezone. calcard never reads the STANDARD/DAYLIGHT rules for
    // this; their visibility is covered by the TZOFFSETFROM dump check
    // above. 23:00 floating is 04:00Z the next day in US-Eastern (UTC-5
    // in January) and 23:00Z if the timezone were lost.
    //
    // The query first drops events by the range cached at write time, which
    // reads floating times as UTC (23:00Z), before any timezone is applied
    // (`is_resource_in_time_range`). So the positive window spans both the
    // cached 23:00Z interval and the Eastern 04:00Z one; the narrow UTC
    // window passes that prefilter and is then decided by the timezone. A
    // lost timezone would give (true, true).
    let plain = test.account("plain@example.com").clone();
    let plain_client = DummyWebDavClient::new(
        plain.id().document_id(),
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    let plain_cal = "/dav/cal/plain%40example.com/tz/";
    plain_client
        .request("MKCALENDAR", plain_cal, mkcalendar_body(&[]))
        .await
        .with_status(StatusCode::CREATED);
    plain_client
        .proppatch(plain_cal, [("A:calendar-timezone", tz.as_str())], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    for (client, cal) in [(&client, cal), (&plain_client, plain_cal)] {
        client
            .request_with_headers(
                "PUT",
                &format!("{cal}floating.ics"),
                [CONTENT_TYPE],
                FLOATING,
            )
            .await
            .with_status(StatusCode::CREATED);
    }
    let eastern = time_range_query("20990110T223000Z", "20990111T043000Z");
    let utc = time_range_query("20990110T223000Z", "20990110T233000Z");
    let key = (
        reports_floating(&client, cal, &eastern).await,
        reports_floating(&client, cal, &utc).await,
    );
    let ordinary = (
        reports_floating(&plain_client, plain_cal, &eastern).await,
        reports_floating(&plain_client, plain_cal, &utc).await,
    );
    println!("time-range REPORT: key={key:?} ordinary={ordinary:?}");
    assert_eq!(
        key, ordinary,
        "the sealed timezone changes time-range results"
    );
    assert_eq!(
        ordinary,
        (true, false),
        "the calendar timezone places the event"
    );
    test.wait_for_tasks().await;
    for (client, cal) in [(&client, cal), (&plain_client, plain_cal)] {
        client
            .request("DELETE", &format!("{cal}floating.ics"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    plain_client
        .request("DELETE", plain_cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);

    // The server-created default calendar: plaintext until the owner first
    // writes a property, sealed afterwards.
    let default = "/dav/cal/key1%40example.com/default/";
    let archive = raw_calendar(test, id, "default").await;
    assert!(
        !archive
            .unarchive::<Calendar>()
            .unwrap()
            .preferences(id)
            .unwrap()
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
            .unwrap()
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

    // Event PROPPATCH: the display name goes into X-ZA-EXTRA (ruling 15);
    // at 1.0 the dead property goes into the event's shared metadata
    // container, sealed whole (ruling 16).
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
    let (records, evt_id) = raw_event_records(test, id, "work/evt.ics").await;
    let raw = records.bytes();
    for canary in CANARIES.iter().chain(&["evtname-canary", "dead-canary"]) {
        assert!(
            !raw.contains(canary),
            "{canary} leaked into the stored event"
        );
    }
    assert_dead_container_sealed(test, id, evt_id, &["dead-canary", "za-dead"]).await;
    let row = records.meta.unarchive::<CalendarEvent>().unwrap();
    assert_eq!(
        row.size.to_native() as usize,
        sealed_size(&records.content),
        "the row measures the stored tree"
    );
    assert!(row.display_name.is_none());
    assert!(records.tree().to_string().contains("X-ZA-EXTRA:"));
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

    // A PROPPATCH that sets only creationdate still seals the event, and the
    // extra properties written above survive it.
    client
        .proppatch(path, [("D:creationdate", "2000-01-01T00:00:00Z")], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let (records, _) = raw_event_records(test, id, "work/evt.ics").await;
    let raw = records.bytes();
    for canary in CANARIES.iter().chain(&["evtname-canary", "dead-canary"]) {
        assert!(
            !raw.contains(canary),
            "{canary} leaked into the stored event"
        );
    }
    assert_dead_container_sealed(test, id, evt_id, &["dead-canary", "za-dead"]).await;
    assert!(groupware::calendar::seal::archived_event_is_sealed(
        records.content.unarchive::<CalendarEventContent>().unwrap()
    ));
    let row = records.meta.unarchive::<CalendarEvent>().unwrap();
    assert_eq!(row.created.to_native(), 946684800);
    assert_eq!(row.size.to_native() as usize, sealed_size(&records.content));
    assert!(row.display_name.is_none());
    assert!(records.tree().to_string().contains("X-ZA-EXTRA:"));
    let props = client.propfind(path, ["D:displayname", "C:za-dead"]).await;
    props
        .properties(path)
        .get("D:displayname")
        .with_values(["evtname-canary"]);
    props
        .properties(path)
        .get("za-dead")
        .with_values(["dead-canary", "[xmlns]:http://calendarserver.org/ns/"]);

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

/// What the conditional cases answer, from code reading of upstream's
/// `validate_headers`; the plain account is the oracle.
const CONDITIONAL_EXPECTED: [(&str, StatusCode); 8] = [
    ("GET If-None-Match current", StatusCode::NOT_MODIFIED),
    ("GET If-None-Match *", StatusCode::NOT_MODIFIED),
    ("GET If-Match current", StatusCode::OK),
    ("GET If-Match stale", StatusCode::PRECONDITION_FAILED),
    (
        "PUT If-None-Match * existing",
        StatusCode::PRECONDITION_FAILED,
    ),
    ("PUT If-Match stale", StatusCode::PRECONDITION_FAILED),
    ("PUT If-Match current", StatusCode::NO_CONTENT),
    ("PUT If-None-Match * new", StatusCode::CREATED),
];

/// Runs the conditional cases against `path` (which holds `EVENT`) and a
/// not-yet-existing `fresh` path; returns each case's status.
async fn conditional_statuses(
    client: &DummyWebDavClient,
    path: &str,
    fresh: &str,
) -> Vec<(&'static str, StatusCode)> {
    let etag = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .etag()
        .to_string();
    let stale = "\"stale-etag\"";
    let changed = EVENT.replace("summary-canary", "conditional-canary");
    let mut out = Vec::new();
    for (case, header, value) in [
        ("GET If-None-Match current", "if-none-match", etag.as_str()),
        ("GET If-None-Match *", "if-none-match", "*"),
        ("GET If-Match current", "if-match", etag.as_str()),
        ("GET If-Match stale", "if-match", stale),
    ] {
        let status = client
            .request_with_headers("GET", path, [(header, value)], "")
            .await
            .status;
        out.push((case, status));
    }
    for (case, header, value) in [
        ("PUT If-None-Match * existing", "if-none-match", "*"),
        ("PUT If-Match stale", "if-match", stale),
        ("PUT If-Match current", "if-match", etag.as_str()),
    ] {
        let status = client
            .request_with_headers(
                "PUT",
                path,
                [CONTENT_TYPE, (header, value)],
                changed.clone(),
            )
            .await
            .status;
        out.push((case, status));
    }
    // A UID of its own: a second event with `EVENT`'s UID in the same
    // calendar is refused (no-uid-conflict: 403 at 1.0, ruling 50) before
    // the condition is looked at.
    let status = client
        .request_with_headers(
            "PUT",
            fresh,
            [CONTENT_TYPE, ("if-none-match", "*")],
            EVENT.replace("za-event-1", "za-cond-new"),
        )
        .await
        .status;
    out.push(("PUT If-None-Match * new", status));
    out
}

/// 304, If-Match and If-None-Match on sealed events answer exactly as on
/// ordinary ones: they compare against the stored record's ETag.
pub async fn test_conditional(test: &mut TestServer) {
    println!("Running zero-access conditional request tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let key_client = DummyWebDavClient::new(id, name, STRONG, name);
    let plain = test.account("plain@example.com").clone();
    let plain_client = DummyWebDavClient::new(
        plain.id().document_id(),
        plain.name(),
        plain.secret(),
        plain.name(),
    );
    // Plain's side runs in a calendar this module owns, so nothing here
    // depends on whether plain's default calendar exists.
    let plain_cal = "/dav/cal/plain@example.com/cond/";
    plain_client
        .request("MKCALENDAR", plain_cal, "")
        .await
        .with_status(StatusCode::CREATED);
    let mut results = Vec::new();
    let mut before = Vec::new();
    for (client, base) in [
        (&key_client, "/dav/cal/key1@example.com/default/"),
        (&plain_client, plain_cal),
    ] {
        let path = format!("{base}cond.ics");
        client
            .request_with_headers("PUT", &path, [CONTENT_TYPE], EVENT)
            .await
            .with_status(StatusCode::CREATED);
        let etag_before = client
            .request("GET", &path, "")
            .await
            .with_status(StatusCode::OK)
            .etag()
            .to_string();
        let bytes_before = if base.contains("key1") {
            Some(
                raw_event(test, id, "default/cond.ics")
                    .await
                    .0
                    .as_bytes()
                    .to_vec(),
            )
        } else {
            None
        };
        results.push(conditional_statuses(client, &path, &format!("{base}cond-new.ics")).await);

        // The accepted If-Match PUT stored its edit: the read-back shows it
        // under a new ETag.
        let got = client
            .request("GET", &path, "")
            .await
            .with_status(StatusCode::OK);
        assert_ne!(got.etag(), etag_before, "{base}: ETag unchanged");
        let body = got.body.unwrap();
        assert!(body.contains("conditional-canary"), "{base}: {body}");
        assert!(!body.contains("X-ZA-"), "{base}: {body}");
        before.push(bytes_before);
    }
    assert_eq!(results[0], results[1], "sealed events answer differently");
    assert_eq!(results[1], CONDITIONAL_EXPECTED.to_vec(), "plain oracle");

    // The stored archive is still sealed, holds no plaintext canary, and is
    // not the bytes it held before the If-Match write.
    let (archive, _) = raw_event(test, id, "default/cond.ics").await;
    assert_ne!(
        Some(archive.as_bytes().to_vec()),
        before[0],
        "stored archive unchanged by the accepted If-Match PUT"
    );
    assert!(groupware::calendar::seal::archived_event_is_sealed(
        archive.unarchive::<CalendarEventContent>().unwrap()
    ));
    let (records, _) = raw_event_records(test, id, "default/cond.ics").await;
    assert!(!records.bytes().contains("conditional-canary"));

    test.wait_for_tasks().await;
    for (client, base) in [
        (&key_client, "/dav/cal/key1@example.com/default/"),
        (&plain_client, plain_cal),
    ] {
        for name in ["cond.ics", "cond-new.ics"] {
            client
                .request("DELETE", &format!("{base}{name}"), "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }
    // `plain` outlives this module: drop the calendar it created above.
    plain_client
        .request("DELETE", plain_cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Spec 7: an event stored before the account held keys is read as it is,
/// and the next write that changes it seals it.
pub async fn test_legacy(test: &mut TestServer) {
    println!("Running zero-access legacy event tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let path = "/dav/cal/key1@example.com/default/legacy.ics";
    let legacy = EVENT.replace("za-event-1", "za-legacy-1");
    plant_event(test, id, "default", "legacy.ics", &legacy, None).await;
    let is_sealed = |archive: &Archive<ArchiveBytes>| {
        groupware::calendar::seal::archived_event_is_sealed(
            archive.unarchive::<CalendarEventContent>().unwrap(),
        )
    };
    let (archive, _) = raw_event(test, id, "default/legacy.ics").await;
    assert!(!is_sealed(&archive));

    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(!body.contains("X-ZA-"), "{body}");
    assert_eq!(parse(&body), parse(&legacy), "legacy event read as stored");

    // An unchanged PUT is the no-change shortcut: no write, still plaintext.
    client
        .request_with_headers("PUT", path, [CONTENT_TYPE], legacy.clone())
        .await
        .with_status(StatusCode::NO_CONTENT);
    let (archive, _) = raw_event(test, id, "default/legacy.ics").await;
    assert!(!is_sealed(&archive));

    // A changed PUT seals it.
    let changed = legacy.replace("summary-canary", "legacy-rewritten-canary");
    client
        .request_with_headers("PUT", path, [CONTENT_TYPE], changed)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let (records, _) = raw_event_records(test, id, "default/legacy.ics").await;
    assert!(is_sealed(&records.content));
    let raw = records.bytes();
    for canary in CANARIES.iter().chain(&["legacy-rewritten-canary"]) {
        assert!(!raw.contains(canary), "{canary} left in the clear");
    }
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(body.contains("legacy-rewritten-canary") && !body.contains("X-ZA-"));

    test.wait_for_tasks().await;
    client
        .request("DELETE", path, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

// ---------------------------------------------------------------------------
// Upgrade part 4c: dead properties, split metadata-only updates and the
// per-user preference rule on 1.0's split records.
// ---------------------------------------------------------------------------

/// Canaries of the dead-property cases: a sensitive namespace and name, its
/// value, and the Apple colour and order properties' namespace and name.
const DEAD_CANARIES: &[&str] = &[
    "deadprop-name-canary",
    "x-deadprop-ns-canary",
    "deadprop-value-canary",
    "apple.com/ns/ical",
    "calendar-order",
];

/// A PROPPATCH setting the sensitive custom property to `value`, with the
/// Apple colour and order.
fn dead_proppatch(value: &str) -> String {
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
            "<D:propertyupdate xmlns:D=\"DAV:\" xmlns:X=\"urn:x-deadprop-ns-canary\" ",
            "xmlns:I=\"http://apple.com/ns/ical/\"><D:set><D:prop>",
            "<X:deadprop-name-canary>{value}</X:deadprop-name-canary>",
            "<I:calendar-color>#C0FFEEFF</I:calendar-color>",
            "<I:calendar-order>3</I:calendar-order>",
            "</D:prop></D:set></D:propertyupdate>"
        ),
        value = value
    )
}

/// A PROPFIND of the three dead properties.
const DEAD_PROPFIND: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
    "<D:propfind xmlns:D=\"DAV:\" xmlns:X=\"urn:x-deadprop-ns-canary\" ",
    "xmlns:I=\"http://apple.com/ns/ical/\"><D:prop><X:deadprop-name-canary/>",
    "<I:calendar-color/><I:calendar-order/></D:prop></D:propfind>"
);

/// The dead properties of `path` as `client` reads them: the response
/// body, which must hold every value.
async fn assert_dead_round_trip(client: &DummyWebDavClient, path: &str, value: &str) {
    let body = client
        .request_with_headers("PROPFIND", path, [("depth", "0")], DEAD_PROPFIND)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(
        body.contains(value)
            && body.contains("deadprop-name-canary")
            && body.contains("urn:x-deadprop-ns-canary")
            && body.contains("#C0FFEEFF")
            && body.contains("calendar-order")
            && !body.contains(groupware::calendar::seal::container::CONTAINER_NAMESPACE),
        "{path}: {body}"
    );
}

/// The canaries found in the raw stored container of a document: its keys
/// hold only ids (`store/src/write/key.rs`), so the value is all a reader
/// of the store sees of it.
fn container_leaks(container: &[u8], canaries: &[&str]) -> Vec<String> {
    let text = String::from_utf8_lossy(container);
    canaries
        .iter()
        .filter(|canary| text.contains(**canary))
        .map(|canary| canary.to_string())
        .collect()
}

/// Ruling 16 at the store: a key account's document holds a sealed
/// container whose stored bytes carry none of `canaries`.
pub(super) async fn assert_dead_container_sealed(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
    canaries: &[&str],
) {
    let container = shared_container(test, account_id, Collection::CalendarEvent, document_id)
        .await
        .unwrap_or_else(|| panic!("event {document_id} has no dead-property container"));
    assert!(
        is_sealed_container(&container),
        "event {document_id}: an unsealed container"
    );
    assert_eq!(
        container_leaks(&container, canaries),
        Vec::<String>::new(),
        "event {document_id}: the container leaks"
    );
}

/// The raw stored value of a document's shared container (compressed and
/// trailed as upstream stores it), for the keyless readers.
async fn raw_container(
    test: &TestServer,
    account_id: u32,
    collection: Collection,
    document_id: u32,
) -> Option<Vec<u8>> {
    test.server
        .store()
        .get_value::<common::auth::vault::RawValue>(ValueKey {
            account_id,
            collection: collection.into(),
            document_id,
            class: store::write::ValueClass::Metadata(
                store::write::metadata::MetadataClass::Shared,
            ),
        })
        .await
        .unwrap()
        .map(|raw| raw.0)
}

/// Every canary of `canaries` found in an event's stored row and content
/// and in its stored container: the scan of the split metadata case.
async fn event_leaks(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
    canaries: &[&str],
) -> Vec<String> {
    let records = stored_event(test, account_id, document_id).await;
    let mut text = records.bytes();
    if let Some(container) =
        shared_container(test, account_id, Collection::CalendarEvent, document_id).await
    {
        text.push_str(&String::from_utf8_lossy(&container));
    }
    canaries
        .iter()
        .filter(|canary| text.contains(**canary))
        .map(|canary| canary.to_string())
        .collect()
}

/// Shares the owner's calendar `slug` with `reader` at `level` through the
/// account API (pin first, as every grant requires).
pub(super) async fn share_calendar(
    test: &TestServer,
    owner: &str,
    reader: &str,
    reader_id: u32,
    slug: &str,
    level: &str,
) {
    let key = test
        .server
        .za_served_public_key(reader_id)
        .await
        .unwrap()
        .unwrap();
    super::pinning::pin(owner, reader, &vault::keys::display_fingerprint(&key))
        .await
        .expect(200);
    crate::utils::za::za_post(
        "shares/grant",
        &serde_json::json!({
            "username": owner,
            "password": STRONG,
            "calendar": slug,
            "address": reader,
            "level": level,
        }),
    )
    .await
    .expect(200);
}

/// The suite's key account pair for a module, created and registered.
async fn key_pair(test: &mut TestServer, owner: &'static str, reader: &'static str) -> (u32, u32) {
    let mut ids = Vec::new();
    for name in [owner, reader] {
        let account = test
            .account("admin@example.com")
            .create_key_user_account(name, STRONG, name, &[], super::user_permissions())
            .await;
        ids.push(account.id().document_id());
        test.insert_account(account);
    }
    (ids[0], ids[1])
}

const DEAD_OWNER: &str = "deadprop-owner@example.com";
const DEAD_READER: &str = "deadprop-reader@example.com";
const DEAD_HOME: &str = "/dav/cal/deadprop-owner%40example.com/";

/// Design "Dead properties and object metadata" (rulings 16, 23 and 29): a
/// key account's dead properties round-trip sealed for the owner and a
/// grantee, survive COPY and MOVE, leave upstream's viewer counters alone,
/// and every keyless reader of a sealed container runs without keys.
pub async fn test_dead_properties(test: &mut TestServer) {
    println!("Running zero-access dead-property tests...");
    let (owner_id, reader_id) = key_pair(test, DEAD_OWNER, DEAD_READER).await;
    let owner = DummyWebDavClient::new(owner_id, DEAD_OWNER, STRONG, DEAD_OWNER);
    let reader = DummyWebDavClient::new(reader_id, DEAD_READER, STRONG, DEAD_READER);
    for slug in ["dead-a", "dead-b", "dead-c"] {
        owner
            .request(
                "MKCALENDAR",
                &format!("{DEAD_HOME}{slug}/"),
                mkcalendar_body(&[]),
            )
            .await
            .with_status(StatusCode::CREATED);
    }
    for (path, uid) in [
        ("dead-a/evt.ics", "deadprop-evt"),
        ("dead-a/control.ics", "deadprop-control"),
        ("dead-b/copy-src.ics", "deadprop-copy"),
        ("dead-b/move-src.ics", "deadprop-move"),
        ("dead-b/shared-in.ics", "deadprop-shared-in"),
    ] {
        owner
            .request_with_headers(
                "PUT",
                &format!("{DEAD_HOME}{path}"),
                [CONTENT_TYPE],
                EVENT.replace("za-event-1", uid),
            )
            .await
            .with_status(StatusCode::CREATED);
    }

    // 1. Owner round trips: an event's and a calendar's custom (sensitive
    // namespace and name), colour and order properties.
    let evt = format!("{DEAD_HOME}dead-a/evt.ics");
    let cal_a = format!("{DEAD_HOME}dead-a/");
    for (path, value) in [
        (&evt, "deadprop-value-canary"),
        (&cal_a, "deadprop-value-canary-cal"),
    ] {
        owner
            .request("PROPPATCH", path, dead_proppatch(value))
            .await
            .with_status(StatusCode::MULTI_STATUS);
        assert_dead_round_trip(&owner, path, value).await;
    }
    let evt_id = resource_id(test, owner_id, "dead-a/evt.ics").await;
    let cal_a_id = resource_id(test, owner_id, "dead-a").await;
    assert_dead_container_sealed(test, owner_id, evt_id, DEAD_CANARIES).await;
    let cal_container = shared_container(test, owner_id, Collection::Calendar, cal_a_id)
        .await
        .expect("the calendar's container");
    assert!(is_sealed_container(&cal_container));
    assert!(container_leaks(&cal_container, DEAD_CANARIES).is_empty());
    // The row's presence bit says a DAV container exists (rewrite_presence).
    let row = stored_event(test, owner_id, evt_id).await;
    assert_eq!(
        row.meta
            .unarchive::<CalendarEvent>()
            .unwrap()
            .metadata_kinds(),
        types::metadata::MetadataKinds::DAV
    );
    assert!(row.bytes().contains("X-ZA-KEY:"));
    assert!(container_leaks(row.bytes().as_bytes(), DEAD_CANARIES).is_empty());

    // Negative control: a planted plaintext container on another event is
    // what the scan above must catch.
    let control_id = resource_id(test, owner_id, "dead-a/control.ics").await;
    crate::utils::za::plant_container(
        test,
        owner_id,
        Collection::CalendarEvent,
        control_id,
        Some(crate::utils::za::plain_dav_container(&[(
            "urn:x-deadprop-ns-canary",
            "deadprop-name-canary",
            "deadprop-value-canary",
        )])),
    )
    .await;
    let planted = shared_container(test, owner_id, Collection::CalendarEvent, control_id)
        .await
        .unwrap();
    assert!(!is_sealed_container(&planted), "the control is plaintext");
    assert!(
        !container_leaks(&planted, DEAD_CANARIES).is_empty(),
        "the scan misses a plaintext container"
    );
    crate::utils::za::plant_container(test, owner_id, Collection::CalendarEvent, control_id, None)
        .await;

    // A corrupted sealed container fails its item (500) and nothing else:
    // GET reads the content record, which is intact.
    let original = shared_container(test, owner_id, Collection::CalendarEvent, evt_id)
        .await
        .unwrap();
    let view = types::metadata::MetadataView::new(&original).unwrap();
    let (_, value) = view.dav().next().unwrap();
    let text = value.text().unwrap();
    let mut payload =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, text.as_bytes())
            .unwrap();
    *payload.last_mut().unwrap() ^= 1;
    let corrupt = crate::utils::za::plain_dav_container(&[(
        groupware::calendar::seal::container::CONTAINER_NAMESPACE,
        groupware::calendar::seal::container::CONTAINER_NAME,
        &base64::Engine::encode(&base64::engine::general_purpose::STANDARD, payload),
    )]);
    crate::utils::za::plant_container(
        test,
        owner_id,
        Collection::CalendarEvent,
        evt_id,
        Some(corrupt),
    )
    .await;
    let body = owner
        .request_with_headers("PROPFIND", &evt, [("depth", "0")], DEAD_PROPFIND)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(
        body.contains("500") && !body.contains("deadprop-value-canary"),
        "{body}"
    );
    owner
        .request("GET", &evt, "")
        .await
        .with_status(StatusCode::OK);
    crate::utils::za::plant_container(
        test,
        owner_id,
        Collection::CalendarEvent,
        evt_id,
        types::metadata::EncodedMetadata::from_view(&view),
    )
    .await;
    assert_dead_round_trip(&owner, &evt, "deadprop-value-canary").await;

    // 2. Viewer and Owner counters (upstream's 12-byte `ViewerState`, never
    // sealed): planted for the pair, merged once, then left as they were by
    // every sealed write below.
    use store::{
        SerializeInfallible,
        write::metadata::{MetadataClass, ViewerState},
    };
    let planted_state = ViewerState {
        change_id: 1,
        containers: 0,
    };
    let mut batch = store::write::BatchBuilder::new();
    batch
        .with_account_id(owner_id)
        .with_collection(Collection::CalendarEvent)
        .set(
            MetadataClass::Viewer { viewer: reader_id },
            planted_state.serialize(),
        )
        .with_account_id(reader_id)
        .with_collection(Collection::CalendarEvent)
        .set(
            MetadataClass::Owner { owner: owner_id },
            planted_state.serialize(),
        );
    test.server.store().write_batch(&mut batch).await.unwrap();
    let mut batch = store::write::BatchBuilder::new();
    batch
        .with_account_id(owner_id)
        .with_collection(Collection::CalendarEvent)
        .merge_fnc(MetadataClass::Viewer { viewer: reader_id }, |_, current| {
            let mut state = current
                .map(<ViewerState as store::Deserialize>::deserialize)
                .transpose()?
                .unwrap_or_default();
            state.change_id = state.change_id.max(5);
            Ok(store::write::MergeResult::Update(state.serialize()))
        });
    test.server.store().write_batch(&mut batch).await.unwrap();
    let merged = ViewerState {
        change_id: 5,
        containers: 0,
    };

    // 3. A grantee's round trip: the shared calendar's containers move to
    // the collection key with the ring (ruling 29) and the grantee reads
    // and writes them.
    share_calendar(test, DEAD_OWNER, DEAD_READER, reader_id, "dead-a", "edit").await;
    assert_dead_round_trip(&reader, &evt, "deadprop-value-canary").await;
    assert_dead_round_trip(&reader, &cal_a, "deadprop-value-canary-cal").await;
    reader
        .request(
            "PROPPATCH",
            &evt,
            dead_proppatch("deadprop-value-canary-reader"),
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert_dead_round_trip(&owner, &evt, "deadprop-value-canary-reader").await;
    assert_dead_round_trip(&reader, &evt, "deadprop-value-canary-reader").await;
    let container = shared_container(test, owner_id, Collection::CalendarEvent, evt_id)
        .await
        .unwrap();
    assert_eq!(
        groupware::calendar::seal::container_is_ck(&container),
        Some(true),
        "a shared calendar's event container is under its collection key"
    );
    assert!(container_leaks(&container, DEAD_CANARIES).is_empty());

    // 4. COPY and MOVE between two of the account's calendars, then a
    // PROPFIND at the destination; and a MOVE into the shared calendar,
    // which reseals the container under its collection key.
    for name in ["copy-src.ics", "move-src.ics", "shared-in.ics"] {
        owner
            .request(
                "PROPPATCH",
                &format!("{DEAD_HOME}dead-b/{name}"),
                dead_proppatch("deadprop-value-canary"),
            )
            .await
            .with_status(StatusCode::MULTI_STATUS);
    }
    owner
        .request_with_headers(
            "COPY",
            &format!("{DEAD_HOME}dead-b/copy-src.ics"),
            [(
                "destination",
                format!("{DEAD_HOME}dead-c/copied.ics").as_str(),
            )],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    owner
        .request_with_headers(
            "MOVE",
            &format!("{DEAD_HOME}dead-b/move-src.ics"),
            [(
                "destination",
                format!("{DEAD_HOME}dead-c/moved.ics").as_str(),
            )],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    owner
        .request_with_headers(
            "MOVE",
            &format!("{DEAD_HOME}dead-b/shared-in.ics"),
            [(
                "destination",
                format!("{DEAD_HOME}dead-a/shared-in.ics").as_str(),
            )],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    for path in [
        "dead-c/copied.ics",
        "dead-b/copy-src.ics",
        "dead-c/moved.ics",
        "dead-a/shared-in.ics",
    ] {
        assert_dead_round_trip(
            &owner,
            &format!("{DEAD_HOME}{path}"),
            "deadprop-value-canary",
        )
        .await;
        let id = resource_id(test, owner_id, path).await;
        assert_dead_container_sealed(test, owner_id, id, DEAD_CANARIES).await;
        // Per-user event data: no COPY or MOVE writes preferences.
        crate::utils::za::assert_no_event_preferences(test, owner_id, id).await;
    }
    let shared_in = resource_id(test, owner_id, "dead-a/shared-in.ics").await;
    assert_eq!(
        groupware::calendar::seal::container_is_ck(
            &shared_container(test, owner_id, Collection::CalendarEvent, shared_in)
                .await
                .unwrap()
        ),
        Some(true),
        "the MOVE into the shared calendar resealed the container"
    );
    assert_dead_round_trip(
        &reader,
        &format!("{DEAD_HOME}dead-a/shared-in.ics"),
        "deadprop-value-canary",
    )
    .await;

    // The counters decode and kept the merged value through every sealed
    // write above.
    let viewers = test.server.all_metadata_viewers(owner_id).await.unwrap();
    assert_eq!(viewers.len(), 1, "{viewers:?}");
    assert_eq!(viewers[0].viewer_id, reader_id);
    assert_eq!(viewers[0].collection, Collection::CalendarEvent);
    assert_eq!(viewers[0].state, merged);
    assert_eq!(
        test.server.metadata_owners(reader_id).await.unwrap(),
        vec![(owner_id, Collection::CalendarEvent)]
    );

    // 5. Keyless readers over the sealed containers, with no session keys:
    // `MetadataBuf::read` and its kinds (DAV's `container_kinds`), the
    // used-quota scan, then the cleanup of a deleted event and calendar and
    // the account-destruction paths.
    for (collection, id) in [
        (Collection::CalendarEvent, evt_id),
        (Collection::Calendar, cal_a_id),
    ] {
        let raw = raw_container(test, owner_id, collection, id).await.unwrap();
        let read = store::write::metadata::MetadataBuf::read(&raw).unwrap();
        assert_eq!(read.kinds(), types::metadata::MetadataKinds::DAV);
        assert_eq!(read.view().len(), 1, "one sealed wrapper entry");
        assert_eq!(read.stored_len() as usize, raw.len());
    }
    let mut stored = 0i64;
    for collection in [Collection::Calendar, Collection::CalendarEvent] {
        test.server
            .all_metadata_containers(owner_id, collection, |_, _, container| {
                stored += container.size as i64;
                Ok(true)
            })
            .await
            .unwrap();
    }
    assert!(stored > 0);
    assert_eq!(
        test.server.metadata_used_quota(owner_id).await.unwrap(),
        stored,
        "the keyless quota scan counts every sealed container"
    );
    owner
        .request("DELETE", &evt, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        raw_container(test, owner_id, Collection::CalendarEvent, evt_id)
            .await
            .is_none()
    );
    test.wait_for_tasks().await;
    owner
        .request("DELETE", &cal_a, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
    assert!(
        raw_container(test, owner_id, Collection::Calendar, cal_a_id)
            .await
            .is_none()
    );
    assert!(
        raw_container(test, owner_id, Collection::CalendarEvent, shared_in)
            .await
            .is_none()
    );
    test.server
        .purge_private_metadata(owner_id, Some(1))
        .await
        .unwrap();
    test.server.destroy_owner_metadata(owner_id).await.unwrap();
    assert!(
        test.server
            .all_metadata_viewers(owner_id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        test.server
            .metadata_owners(reader_id)
            .await
            .unwrap()
            .is_empty()
    );

    test.wait_for_tasks().await;
    for slug in ["dead-b", "dead-c"] {
        owner
            .request("DELETE", &format!("{DEAD_HOME}{slug}/"), "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    test.wait_for_tasks().await;
}

const SPLIT_OWNER: &str = "split-meta-owner@example.com";
const SPLIT_READER: &str = "split-meta-reader@example.com";
const SPLIT_CAL: &str = "/dav/cal/split-meta-owner%40example.com/split/";

/// What the owner and the grantee read of the event at `path`: each GET's
/// ETag is the stored row's (the view keeps the stored version's identity)
/// and its body the plaintext; a GET conditional on that ETag answers 304.
async fn split_reads(
    test: &TestServer,
    owner_id: u32,
    document_id: u32,
    clients: [&DummyWebDavClient; 2],
    path: &str,
) {
    let etag = stored_event(test, owner_id, document_id).await.etag();
    for client in clients {
        let got = client
            .request("GET", path, "")
            .await
            .with_status(StatusCode::OK);
        assert_eq!(got.etag(), etag, "{path}: the view's ETag is the row's");
        let body = got.body.unwrap();
        assert!(!body.contains("X-ZA-"), "{body}");
        for canary in CANARIES {
            assert!(body.contains(canary), "{canary} missing from {path}");
        }
        client
            .request_with_headers("GET", path, [("if-none-match", etag.as_str())], "")
            .await
            .with_status(StatusCode::NOT_MODIFIED);
    }
}

/// Upgrade part 4c: a metadata-only write on 1.0's split records (a
/// rename, and a dead-property PROPPATCH whose presence bit
/// `rewrite_presence` writes to the row through a store-side merge) leaves
/// the sealed content record byte for byte, and neither row nor content nor
/// container holds a protected value afterwards; owner and grantee read the
/// event as before.
pub async fn split_metadata_only_update_keeps_sealed_content(test: &mut TestServer) {
    println!("Running zero-access split metadata-only update tests...");
    let (owner_id, reader_id) = key_pair(test, SPLIT_OWNER, SPLIT_READER).await;
    let owner = DummyWebDavClient::new(owner_id, SPLIT_OWNER, STRONG, SPLIT_OWNER);
    let reader = DummyWebDavClient::new(reader_id, SPLIT_READER, STRONG, SPLIT_READER);
    owner
        .request("MKCALENDAR", SPLIT_CAL, mkcalendar_body(&[]))
        .await
        .with_status(StatusCode::CREATED);
    let path = format!("{SPLIT_CAL}evt.ics");
    owner
        .request_with_headers(
            "PUT",
            &path,
            [CONTENT_TYPE],
            EVENT.replace("za-event-1", "split-meta-1"),
        )
        .await
        .with_status(StatusCode::CREATED);
    share_calendar(test, SPLIT_OWNER, SPLIT_READER, reader_id, "split", "edit").await;
    let id = resource_id(test, owner_id, "split/evt.ics").await;
    let canaries: Vec<&str> = CANARIES
        .iter()
        .copied()
        .chain(["split-dead-canary", "split-name-canary", "x-split-ns"])
        .collect();
    let base = stored_event(test, owner_id, id).await;
    assert_eq!(
        event_leaks(test, owner_id, id, &canaries).await,
        Vec::<String>::new()
    );
    split_reads(test, owner_id, id, [&owner, &reader], &path).await;

    // 1. A dead-property PROPPATCH: the container is written sealed and the
    // presence bit is set on the row alone.
    let proppatch = concat!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
        "<D:propertyupdate xmlns:D=\"DAV:\" xmlns:X=\"urn:x-split-ns\"><D:set><D:prop>",
        "<X:split-dead>split-dead-canary</X:split-dead>",
        "</D:prop></D:set></D:propertyupdate>"
    );
    owner
        .request("PROPPATCH", &path, proppatch)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let flipped = stored_event(test, owner_id, id).await;
    assert_eq!(
        flipped.content.as_bytes(),
        base.content.as_bytes(),
        "a presence-bit write rewrote the sealed content"
    );
    assert_eq!(flipped.content.version, base.content.version);
    assert_ne!(
        flipped.meta.version, base.meta.version,
        "the row was not rewritten"
    );
    let row = flipped.meta.unarchive::<CalendarEvent>().unwrap();
    assert_eq!(row.metadata_kinds(), types::metadata::MetadataKinds::DAV);
    assert_ne!(
        row.flags.to_native() & groupware::calendar::EVENT_META_DAV,
        0
    );
    assert!(row.display_name.is_none());
    assert_eq!(
        event_leaks(test, owner_id, id, &canaries).await,
        Vec::<String>::new()
    );
    assert!(is_sealed_container(
        &shared_container(test, owner_id, Collection::CalendarEvent, id)
            .await
            .unwrap()
    ));
    split_reads(test, owner_id, id, [&owner, &reader], &path).await;

    // 2. A rename: MOVE within the calendar is upstream's `update_meta`
    // (the row's names only).
    let renamed = format!("{SPLIT_CAL}evt-renamed.ics");
    test.wait_for_tasks().await;
    owner
        .request_with_headers("MOVE", &path, [("destination", renamed.as_str())], "")
        .await
        .with_status(StatusCode::CREATED);
    let moved = stored_event(test, owner_id, id).await;
    assert_eq!(moved.content.as_bytes(), base.content.as_bytes());
    assert_eq!(moved.content.version, base.content.version);
    assert_eq!(
        moved.meta.unarchive::<CalendarEvent>().unwrap().names[0]
            .name
            .as_str(),
        "evt-renamed.ics"
    );
    assert_eq!(
        event_leaks(test, owner_id, id, &canaries).await,
        Vec::<String>::new()
    );
    split_reads(test, owner_id, id, [&owner, &reader], &renamed).await;
    // Per-user event data: neither metadata write added preferences.
    crate::utils::za::assert_no_event_preferences(test, owner_id, id).await;

    // Negative control (a flipped field): a row-only writer that put a
    // protected value in the row is what the scan must catch. Written with
    // upstream's `update_meta` and restored the same way.
    let mut leaky = moved.meta.deserialize::<CalendarEvent>().unwrap();
    leaky.display_name = Some("split-name-canary".into());
    let changed_by = test
        .server
        .account_info(owner_id)
        .await
        .unwrap()
        .changed_by();
    let mut batch = store::write::BatchBuilder::new();
    leaky
        .update_meta(
            changed_by,
            moved.meta.to_unarchived::<CalendarEvent>().unwrap(),
            owner_id,
            id,
            None,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    assert_eq!(
        event_leaks(test, owner_id, id, &canaries).await,
        vec!["split-name-canary".to_string()],
        "the scan misses a protected value in the row"
    );
    let planted = stored_event(test, owner_id, id).await;
    let mut restored = planted.meta.deserialize::<CalendarEvent>().unwrap();
    restored.display_name = None;
    let mut batch = store::write::BatchBuilder::new();
    restored
        .update_meta(
            changed_by,
            planted.meta.to_unarchived::<CalendarEvent>().unwrap(),
            owner_id,
            id,
            None,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server
        .invalidate_local_caches(&[common::ipc::CacheInvalidation::GroupwareResources(owner_id)]);
    assert_eq!(
        event_leaks(test, owner_id, id, &canaries).await,
        Vec::<String>::new()
    );

    // 3. Removing the dead property clears the container and the presence
    // bit, again on the row alone.
    owner
        .request(
            "PROPPATCH",
            &renamed,
            concat!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>",
                "<D:propertyupdate xmlns:D=\"DAV:\" xmlns:X=\"urn:x-split-ns\"><D:remove>",
                "<D:prop><X:split-dead/></D:prop></D:remove></D:propertyupdate>"
            ),
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let cleared = stored_event(test, owner_id, id).await;
    assert_eq!(cleared.content.as_bytes(), base.content.as_bytes());
    assert_eq!(
        cleared
            .meta
            .unarchive::<CalendarEvent>()
            .unwrap()
            .metadata_kinds(),
        types::metadata::MetadataKinds::NONE
    );
    assert!(
        shared_container(test, owner_id, Collection::CalendarEvent, id)
            .await
            .is_none()
    );
    assert_eq!(
        event_leaks(test, owner_id, id, &canaries).await,
        Vec::<String>::new()
    );
    split_reads(test, owner_id, id, [&owner, &reader], &renamed).await;

    test.wait_for_tasks().await;
    owner
        .request("DELETE", &renamed, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    owner
        .request("DELETE", SPLIT_CAL, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
}
