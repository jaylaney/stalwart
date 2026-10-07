/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Spec 11: decodes every stored record and asserts the sealing boundary.
//! A regression test, not a proof.

use super::{
    STRONG,
    dav_seal::{CANARIES, EVENT, mkcalendar_body},
};
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use calcard::icalendar::{ICalendar, ICalendarProperty};
use email::cache::MessageCacheFetch;
use groupware::{
    cache::GroupwareCache,
    calendar::{
        Calendar, CalendarEvent, CalendarEventData, Timezone,
        seal::{
            collection::COLLECTION_MARKER,
            policy::{is_visible_parameter, is_visible_property},
            tree::{EXTRA_PROP, KEY_PROP, SEALED_PROP},
        },
    },
};
use hyper::StatusCode;
use mail_parser::{DateTime, MessageParser};
use store::{
    BlobStore, IterateParams, SUBSPACE_ACL, SUBSPACE_BLOB_LINK, SUBSPACE_BLOBS, SUBSPACE_COUNTER,
    SUBSPACE_DELETED_ITEMS, SUBSPACE_DIRECTORY, SUBSPACE_IN_MEMORY_COUNTER,
    SUBSPACE_IN_MEMORY_VALUE, SUBSPACE_INDEXES, SUBSPACE_LOGS, SUBSPACE_PROPERTY,
    SUBSPACE_QUEUE_EVENT, SUBSPACE_QUEUE_MESSAGE, SUBSPACE_QUOTA, SUBSPACE_REGISTRY,
    SUBSPACE_REGISTRY_IDX, SUBSPACE_REGISTRY_PK, SUBSPACE_REPORT_IN, SUBSPACE_REPORT_OUT,
    SUBSPACE_SEARCH_INDEX, SUBSPACE_SPAM_SAMPLES, SUBSPACE_TASK_QUEUE, SUBSPACE_TELEMETRY_METRIC,
    SUBSPACE_TELEMETRY_SPAN, U32_LEN,
    write::{AlignedBytes, AnyKey, Archive},
};
use types::{
    collection::{Collection, SyncCollection},
    field::Field,
};

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
const EVENT_PATH: &str = "/dav/cal/key2%40example.com/leak/e.ics";

/// Canaries written by this module, on top of `dav_seal::CANARIES` (which
/// `EVENT` carries).
const EXTRA_CANARIES: &[&str] = &[
    "leakcal-canary",
    "leakdesc-canary",
    "leakcolor-canary",
    "leaktz-canary",
    "leaktzcomment-canary",
    "leakevtname-canary",
    "leakdead-canary",
    "alarm-event-canary",
    "alarm-summary-canary",
    "alarm-description-canary",
    "alarm-location-canary",
    "alarm-attendee-canary",
    "todo-canary",
    "todo-description-canary",
    "negative-canary",
];

/// Archive marker bits (`store/src/write/serialize.rs`, private there).
const ARCHIVE_MAGIC_MARKER: u8 = 1 << 7;
const ARCHIVE_LZ4_COMPRESSED: u8 = 1 << 4;
/// Largest decompressed size the scanner accepts for a value that merely
/// looks like an LZ4 archive, so arbitrary bytes cannot ask for gigabytes.
const MAX_DECODED_LEN: usize = 64 * 1024 * 1024;

/// Every subspace `assert_is_empty` knows about, plus the change log.
const SUBSPACES: &[u8] = &[
    SUBSPACE_ACL,
    SUBSPACE_TASK_QUEUE,
    SUBSPACE_IN_MEMORY_VALUE,
    SUBSPACE_IN_MEMORY_COUNTER,
    SUBSPACE_PROPERTY,
    SUBSPACE_QUEUE_MESSAGE,
    SUBSPACE_QUEUE_EVENT,
    SUBSPACE_REPORT_OUT,
    SUBSPACE_REPORT_IN,
    SUBSPACE_DELETED_ITEMS,
    SUBSPACE_SPAM_SAMPLES,
    SUBSPACE_BLOB_LINK,
    SUBSPACE_BLOBS,
    SUBSPACE_COUNTER,
    SUBSPACE_QUOTA,
    SUBSPACE_INDEXES,
    SUBSPACE_TELEMETRY_SPAN,
    SUBSPACE_TELEMETRY_METRIC,
    SUBSPACE_SEARCH_INDEX,
    SUBSPACE_REGISTRY,
    SUBSPACE_REGISTRY_IDX,
    SUBSPACE_REGISTRY_PK,
    SUBSPACE_DIRECTORY,
    SUBSPACE_LOGS,
];

/// What one scan saw. The counters let the caller prove the scanner was
/// not blind: a wrong key layout would decode nothing and find nothing.
#[derive(Debug, Default)]
pub struct Scan {
    pub violations: Vec<String>,
    pub records: usize,
    pub archives: usize,
    pub blobs: usize,
    pub events: usize,
    pub calendars: usize,
    pub emails: usize,
}

fn all_canaries() -> Vec<&'static str> {
    CANARIES
        .iter()
        .chain(EXTRA_CANARIES.iter())
        .copied()
        .collect()
}

fn find_canaries(bytes: &[u8], what: &str, how: &str, violations: &mut Vec<String>) {
    let text = String::from_utf8_lossy(bytes);
    for canary in all_canaries() {
        if text.contains(canary) {
            violations.push(format!("{what}: {how} contains {canary}"));
        }
    }
}

/// The three carriers the sealer writes; any other `X-ZA-*` property is an
/// ordinary `X-` property and must have been sealed.
fn is_carrier_name(name: &ICalendarProperty) -> bool {
    matches!(name, ICalendarProperty::Other(n)
        if [SEALED_PROP, KEY_PROP, EXTRA_PROP].iter().any(|c| n.eq_ignore_ascii_case(c)))
}

fn check_tree(tree: &ICalendar, what: &str, require_key: bool, violations: &mut Vec<String>) {
    for (index, component) in tree.components.iter().enumerate() {
        for entry in component.entries.iter() {
            if is_carrier_name(&entry.name) {
                continue;
            }
            if !is_visible_property(&component.component_type, &entry.name) {
                violations.push(format!(
                    "{what}: component {index} ({:?}) has sealed-class property {:?}",
                    component.component_type, entry.name
                ));
            }
            for param in entry.params.iter() {
                if !is_visible_parameter(&param.name) {
                    violations.push(format!(
                        "{what}: component {index} {:?} has sealed-class parameter {:?}",
                        entry.name, param.name
                    ));
                }
            }
        }
    }
    if require_key
        && !tree
            .components
            .first()
            .and_then(|c| c.entries.last())
            .is_some_and(|e| matches!(&e.name, ICalendarProperty::Other(n) if n == KEY_PROP))
    {
        violations.push(format!("{what}: missing {KEY_PROP}"));
    }
}

fn check_event(event: &CalendarEvent, what: &str, violations: &mut Vec<String>) {
    if event.display_name.is_some() {
        violations.push(format!("{what}: display_name not empty"));
    }
    if !event.dead_properties.0.is_empty() {
        violations.push(format!("{what}: dead_properties not empty"));
    }
    check_tree(&event.data.event, what, true, violations);
}

fn check_calendar(calendar: &Calendar, account_id: u32, what: &str, violations: &mut Vec<String>) {
    if calendar.preferences.is_empty() {
        violations.push(format!("{what}: collection without preferences"));
        return;
    }
    let pref = calendar.preferences(account_id);
    if !pref.name.starts_with(COLLECTION_MARKER) {
        // Plaintext collections are allowed only if nothing user-supplied is in them.
        if calendar.preferences.iter().any(|p| {
            p.description.is_some()
                || p.color.is_some()
                || matches!(p.time_zone, Timezone::Custom(_))
        }) || !calendar.dead_properties.0.is_empty()
        {
            violations.push(format!("{what}: unsealed collection with user data"));
        }
        return;
    }
    // Spec 7.2: a sealed collection holds only the owner's entry.
    if calendar.preferences.len() != 1 || calendar.preferences[0].account_id != account_id {
        violations.push(format!(
            "{what}: sealed collection with preferences other than the owner's"
        ));
    }
    if pref.description.is_some() || pref.color.is_some() || !calendar.dead_properties.0.is_empty()
    {
        violations.push(format!("{what}: sealed collection with plaintext fields"));
    }
    if let Timezone::Custom(tz) = &pref.time_zone {
        check_tree(tz, what, false, violations);
    }
}

/// Decodes a value as a stored archive if it carries the archive marker.
/// An LZ4 archive starts with its decompressed size (little endian u32);
/// values that only look like one and claim a huge size are skipped.
fn try_archive(value: &[u8]) -> Option<Archive<AlignedBytes>> {
    let marker = *value.last()?;
    if marker & ARCHIVE_MAGIC_MARKER == 0 {
        return None;
    }
    if marker & ARCHIVE_LZ4_COMPRESSED != 0 {
        let size = u32::from_le_bytes(value.get(..U32_LEN)?.try_into().ok()?) as usize;
        if size > MAX_DECODED_LEN {
            return None;
        }
    }
    <Archive<AlignedBytes> as store::Deserialize>::deserialize(value).ok()
}

/// Walks every subspace and the account's mail. Returns what it saw and the
/// violations found for `account_id`.
pub async fn scan(test: &TestServer, account_id: u32) -> Scan {
    let store = test.server.store().clone();
    let blob_store = BlobStore::Store(store.clone());
    let archive_field = u8::from(Field::ARCHIVE);
    let mut scan = Scan::default();

    for &subspace in SUBSPACES {
        if subspace == SUBSPACE_SEARCH_INDEX && store.is_pg_or_mysql() {
            continue;
        }
        let from = AnyKey {
            subspace,
            key: vec![0u8],
        };
        let to = AnyKey {
            subspace,
            key: vec![u8::MAX; 64],
        };
        let mut records: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        store
            .iterate(IterateParams::new(from, to), |key, value| {
                records.push((key.to_vec(), value.to_vec()));
                Ok(true)
            })
            .await
            .unwrap();
        for (key, value) in records {
            scan.records += 1;
            let what = format!("subspace {:?} key {:?}", char::from(subspace), key);
            find_canaries(&key, &what, "key", &mut scan.violations);
            find_canaries(&value, &what, "raw value", &mut scan.violations);

            // Blobs are stored compressed; decode them the way readers do.
            if subspace == SUBSPACE_BLOBS
                && let Some(blob) = blob_store
                    .get_blob(&key, 0..usize::MAX)
                    .await
                    .ok()
                    .flatten()
            {
                scan.blobs += 1;
                find_canaries(&blob, &what, "decoded blob", &mut scan.violations);
            }

            let Some(archive) = try_archive(&value) else {
                continue;
            };
            scan.archives += 1;
            find_canaries(
                archive.as_bytes(),
                &what,
                "decoded archive",
                &mut scan.violations,
            );

            // Property key: account_id (u32 BE) | collection | property | document_id (u32 BE).
            if subspace != SUBSPACE_PROPERTY
                || key.len() != U32_LEN + 2 + U32_LEN
                || key[U32_LEN + 1] != archive_field
                || u32::from_be_bytes(key[..U32_LEN].try_into().unwrap()) != account_id
            {
                continue;
            }
            let collection = key[U32_LEN];
            if collection == Collection::CalendarEvent as u8 {
                scan.events += 1;
                match archive.deserialize::<CalendarEvent>() {
                    Ok(event) => check_event(&event, &what, &mut scan.violations),
                    Err(err) => scan
                        .violations
                        .push(format!("{what}: event archive does not decode: {err:?}")),
                }
            } else if collection == Collection::Calendar as u8 {
                scan.calendars += 1;
                match archive.deserialize::<Calendar>() {
                    Ok(calendar) => {
                        check_calendar(&calendar, account_id, &what, &mut scan.violations)
                    }
                    Err(err) => scan
                        .violations
                        .push(format!("{what}: calendar archive does not decode: {err:?}")),
                }
            }
        }
    }

    // Generated alarm emails, decoded: transfer encodings would hide a
    // canary from the raw scan.
    if let Ok(messages) = test.server.get_cached_messages(account_id).await {
        for message in messages.emails.items.iter() {
            scan.emails += 1;
            let what = format!("email {}", message.document_id);
            let contents = test.fetch_email(account_id, message.document_id).await;
            find_canaries(&contents, &what, "raw message", &mut scan.violations);
            let parsed = MessageParser::new()
                .parse(&contents)
                .unwrap_or_else(|| panic!("{what} does not parse"));
            find_canaries(
                parsed.subject().unwrap_or_default().as_bytes(),
                &what,
                "subject",
                &mut scan.violations,
            );
            for part in parsed.parts.iter() {
                find_canaries(part.contents(), &what, "decoded part", &mut scan.violations);
            }
        }
    }
    scan
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access leak regression test...");
    let name = "key2@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let cal = "/dav/cal/key2%40example.com/leak/";
    let copy = "/dav/cal/key2%40example.com/leak-copy/";

    // Everything the spec lists, written through CalDAV: collection name,
    // description, colour, custom timezone; event tree, display name, dead
    // property; a VTODO; an email alarm.
    client
        .request(
            "MKCALENDAR",
            cal,
            mkcalendar_body(&[
                ("D:displayname", "leakcal-canary"),
                ("A:calendar-description", "leakdesc-canary"),
                ("C:calendar-color", "leakcolor-canary"),
            ]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let tz = crate::webdav::TEST_VTIMEZONE_1
        .replace("Eastern Standard Time (US Canada)", "leaktz-canary")
        .replace(
            "TZOFFSETTO:-0500\n",
            "TZOFFSETTO:-0500\nCOMMENT:leaktzcomment-canary\n",
        )
        .replace('\n', "\r\n");
    client
        .proppatch(cal, [("A:calendar-timezone", tz.as_str())], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // Every slot really holds its canary, so a clean scan means sealed, not
    // absent.
    let props = client
        .propfind(
            cal,
            [
                "D:displayname",
                "A:calendar-description",
                "C:calendar-color",
                "A:calendar-timezone",
            ],
        )
        .await;
    let props = props.properties(cal);
    props.get("D:displayname").with_values(["leakcal-canary"]);
    props
        .get("A:calendar-description")
        .with_values(["leakdesc-canary"]);
    props
        .get("calendar-color")
        .with_values(["leakcolor-canary", "[xmlns]:http://calendarserver.org/ns/"]);
    let tz_back = props.get("A:calendar-timezone").value().to_string();
    assert!(
        tz_back.contains("leaktz-canary") && tz_back.contains("leaktzcomment-canary"),
        "{tz_back}"
    );
    client
        .request_with_headers("PUT", EVENT_PATH, [CONTENT_TYPE], EVENT)
        .await
        .with_status(StatusCode::CREATED);
    client
        .proppatch(
            EVENT_PATH,
            [
                ("D:displayname", "leakevtname-canary"),
                ("C:leak-dead", "leakdead-canary"),
            ],
            [],
            [],
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let event_props = client
        .propfind(EVENT_PATH, ["D:displayname", "C:leak-dead"])
        .await;
    let event_props = event_props.properties(EVENT_PATH);
    event_props
        .get("D:displayname")
        .with_values(["leakevtname-canary"]);
    event_props
        .get("leak-dead")
        .with_values(["leakdead-canary", "[xmlns]:http://calendarserver.org/ns/"]);
    let todo = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VTODO\r\n",
        "UID:leak-todo\r\nDTSTAMP:20240101T000000Z\r\nDUE:20240105T000000Z\r\n",
        "SUMMARY:todo-canary\r\nDESCRIPTION:todo-description-canary\r\n",
        "END:VTODO\r\nEND:VCALENDAR\r\n"
    );
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/key2%40example.com/leak/t.ics",
            [CONTENT_TYPE],
            todo,
        )
        .await
        .with_status(StatusCode::CREATED);
    let alarm = format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\n",
            "UID:leak-alarm\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:{}\r\n",
            "DTEND;TZID=America/New_York:21250221T180000\r\n",
            "SUMMARY:alarm-event-canary\r\nLOCATION:alarm-location-canary\r\n",
            "BEGIN:VALARM\r\nTRIGGER:-P2S\r\nACTION:EMAIL\r\n",
            "ATTENDEE;CN=alarm-attendee-canary:mailto:key2@example.com\r\n",
            "SUMMARY:alarm-summary-canary\r\nDESCRIPTION:alarm-description-canary\r\n",
            "END:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        ),
        DateTime::from_timestamp(store::write::now() as i64 + 3)
            .to_rfc3339()
            .replace(['-', ':'], "")
    );
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/key2%40example.com/leak/alarm.ics",
            [CONTENT_TYPE],
            alarm,
        )
        .await
        .with_status(StatusCode::CREATED);
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
    // Let the alarm fire and its email be delivered.
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    test.wait_for_tasks().await;

    // No carrier ever reaches a client.
    for path in [cal, copy] {
        let body = client
            .request_with_headers(
                "PROPFIND",
                path,
                [("depth", "1")],
                "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\"><D:allprop/></D:propfind>",
            )
            .await
            .with_status(StatusCode::MULTI_STATUS)
            .body
            .unwrap();
        assert!(
            !body.contains("X-ZA-") && !body.contains(COLLECTION_MARKER),
            "{body}"
        );
    }
    for (path, canary) in [
        (EVENT_PATH, "summary-canary"),
        (
            "/dav/cal/key2%40example.com/leak/t.ics",
            "todo-description-canary",
        ),
        (
            "/dav/cal/key2%40example.com/leak/alarm.ics",
            "alarm-description-canary",
        ),
        (
            "/dav/cal/key2%40example.com/leak-copy/e.ics",
            "xprop-canary",
        ),
    ] {
        let body = client
            .request("GET", path, "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap();
        assert!(
            body.contains(canary) && !body.contains("X-ZA-") && !body.contains(COLLECTION_MARKER),
            "{body}"
        );
    }

    let result = scan(test, id).await;
    println!(
        "Leak scan: {} records, {} archives, {} blobs, {} events, {} calendars, {} emails",
        result.records,
        result.archives,
        result.blobs,
        result.events,
        result.calendars,
        result.emails
    );
    assert!(
        result.violations.is_empty(),
        "leaks found:\n{}",
        result.violations.join("\n")
    );
    // Not blind: e.ics, t.ics and alarm.ics (an in-account collection COPY
    // adds a name to each event document instead of duplicating it), both
    // collections, and the alarm email.
    assert!(result.events >= 3, "{result:?}");
    assert!(result.calendars >= 2, "{result:?}");
    assert!(result.emails >= 1, "the alarm email was not delivered");

    // Negative control: an unsealed event planted directly must be caught,
    // both by the canary search and by the structural check.
    {
        let resources = test
            .server
            .fetch_dav_resources(id, id, SyncCollection::Calendar)
            .await
            .unwrap();
        let calendar_id = resources.by_path("leak").unwrap().document_id();
        let ical = match calcard::Parser::new(&EVENT.replace("summary-canary", "negative-canary"))
            .entry()
        {
            calcard::Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        };
        let mut next = None;
        let event = CalendarEvent {
            names: vec![common::DavName {
                name: "planted.ics".into(),
                parent_id: calendar_id,
            }],
            data: CalendarEventData::new(
                ical,
                calcard::common::timezone::Tz::Floating,
                100,
                &mut next,
            ),
            size: 1,
            ..Default::default()
        };
        let account_info = test.server.account_info(id).await.unwrap();
        let document_id = test
            .server
            .store()
            .assign_document_ids(id, Collection::CalendarEvent, 1)
            .await
            .unwrap();
        let mut batch = store::write::BatchBuilder::new();
        event
            .insert(
                account_info.account_tenant_ids(),
                id,
                document_id,
                None,
                &mut batch,
            )
            .unwrap();
        test.server.commit_batch(batch).await.unwrap();
        // A direct store write does not wake the task manager the way the
        // DAV handlers do; without this the index task waits for its poll.
        test.server.notify_task_queue();
        test.wait_for_tasks().await;
        let planted = scan(test, id).await;
        let report = planted.violations.join("\n");
        assert!(
            planted
                .violations
                .iter()
                .any(|v| v.contains("negative-canary")),
            "the scanner must catch the planted canary:\n{report}"
        );
        assert!(
            planted
                .violations
                .iter()
                .any(|v| v.ends_with(&format!("missing {KEY_PROP}"))),
            "the scanner must catch the unsealed event structure:\n{report}"
        );
        assert!(
            planted
                .violations
                .iter()
                .any(|v| v.contains("sealed-class property Summary")),
            "the scanner must catch the plaintext SUMMARY:\n{report}"
        );
        println!(
            "Negative control caught {} violations:\n{report}",
            planted.violations.len()
        );
        test.server
            .invalidate_local_caches(&[common::ipc::CacheInvalidation::DavResources(id)])
            .await;
        client
            .request("DELETE", "/dav/cal/key2%40example.com/leak/planted.ics", "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }

    test.wait_for_tasks().await;
    for path in [copy, cal] {
        client
            .request("DELETE", path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    test.destroy_all_mailboxes(test.account(name)).await;
}
