/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Spec section 9: rule-expansion traces carry the event's UID, never its
//! iCalendar text, for key and plain accounts.

use super::STRONG;
use crate::utils::{server::TestServer, webdav::DummyWebDavClient};
use groupware::{cache::GroupwareCache, calendar::CalendarEvent};
use hyper::StatusCode;
use std::time::Duration;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, BatchBuilder},
};
use trc::{
    CalendarEvent as TraceCalendarEvent, Collector, EventType, Key,
    ipc::subscriber::{Interests, SubscriberBuilder},
};
use types::collection::{Collection, SyncCollection};

const SUBSCRIBER_ID: &str = "za-expansion-test";
const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
const SUMMARY_CANARY: &str = "expansion-canary-summary-5b1e";
const DESCRIPTION_CANARY: &str = "expansion-canary-description-8c3f";

/// An RRULE that ends before it starts: calcard reports an expansion error.
fn bad_rrule(uid: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20240102T090000Z\r\nRRULE:FREQ=DAILY;UNTIL=20200101T000000Z\r\nSUMMARY:{SUMMARY_CANARY}\r\nDESCRIPTION:{DESCRIPTION_CANARY}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

/// A valid event; its stored time ranges are broken afterwards.
fn valid(uid: &str) -> String {
    format!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSUMMARY:{SUMMARY_CANARY}\r\nDESCRIPTION:{DESCRIPTION_CANARY}\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    )
}

const TIME_RANGE_QUERY: &str = "<?xml version=\"1.0\" encoding=\"utf-8\" ?><C:calendar-query xmlns:D=\"DAV:\" xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/></D:prop><C:filter><C:comp-filter name=\"VCALENDAR\"><C:comp-filter name=\"VEVENT\"><C:time-range start=\"20990101T000000Z\" end=\"20990103T000000Z\"/></C:comp-filter></C:comp-filter></C:filter></C:calendar-query>";

/// Every string anywhere in an event's values, flattened (as in tracing.rs).
fn strings(value: &trc::Value, out: &mut Vec<String>) {
    match value {
        trc::Value::String(s) => out.push(s.to_string()),
        trc::Value::Array(items) => items.iter().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// Empties the first stored time range's instances, keeping everything else
/// (including the seal): `expand` then returns `None` at query time.
async fn break_time_ranges(test: &TestServer, account_id: u32, document_id: u32) {
    let stored = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEvent,
            document_id,
        ))
        .await
        .unwrap()
        .unwrap();
    let mut event = stored.deserialize::<CalendarEvent>().unwrap();
    let mut ranges = event.data.time_ranges.into_vec();
    assert!(!ranges.is_empty());
    ranges[0].instances = Box::default();
    event.data.time_ranges = ranges.into_boxed_slice();
    let account_info = test.server.account_info(account_id).await.unwrap();
    let mut batch = BatchBuilder::new();
    event
        .update(
            account_info.account_tenant_ids(),
            stored.to_unarchived::<CalendarEvent>().unwrap(),
            account_id,
            document_id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_task_queue();
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access rule-expansion trace tests...");
    let key1 = test.account("key1@example.com").clone();
    let key_id = key1.id().document_id();
    let key_client = DummyWebDavClient::new(key_id, "key1@example.com", STRONG, "key1@example.com");
    let plain = test.account("plain@example.com").clone();
    let plain_id = plain.id().document_id();
    let plain_client = DummyWebDavClient::new(plain_id, plain.name(), plain.secret(), plain.name());
    let key_cal = "/dav/cal/key1%40example.com/default/";
    let plain_cal = "/dav/cal/plain%40example.com/expansion/";
    plain_client
        .request(
            "MKCALENDAR",
            plain_cal,
            "<?xml version=\"1.0\" encoding=\"utf-8\" ?><A:mkcalendar xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"/>",
        )
        .await
        .with_status(StatusCode::CREATED);

    let event_type = EventType::Calendar(TraceCalendarEvent::RuleExpansionError);
    let mut interests = Interests::default();
    interests.set(event_type);
    let (_tx, mut rx) = SubscriberBuilder::new(SUBSCRIBER_ID.into())
        .set_interests([event_type])
        .with_lossy(false)
        .register();
    Collector::union_interests(interests);
    Collector::reload();

    let clients = [
        ("key", key_id, &key_client, key_cal, "default"),
        ("plain", plain_id, &plain_client, plain_cal, "expansion"),
    ];
    for (who, _, client, cal, _) in &clients {
        client
            .request_with_headers(
                "PUT",
                &format!("{cal}bad-rrule-{who}.ics"),
                [CONTENT_TYPE],
                bad_rrule(&format!("za-rrule-{who}")),
            )
            .await
            .with_status(StatusCode::CREATED);
    }
    // (UID, account id, document id) of each broken event
    let mut broken = Vec::new();
    for (who, id, client, cal, name) in &clients {
        client
            .request_with_headers(
                "PUT",
                &format!("{cal}broken-{who}.ics"),
                [CONTENT_TYPE],
                valid(&format!("za-chrono-{who}")),
            )
            .await
            .with_status(StatusCode::CREATED);
        test.wait_for_tasks().await;
        let document_id = test
            .server
            .fetch_dav_resources(*id, *id, SyncCollection::Calendar)
            .await
            .unwrap()
            .by_path(&format!("{name}/broken-{who}.ics"))
            .unwrap()
            .document_id();
        broken.push((format!("za-chrono-{who}"), *id, document_id));
        break_time_ranges(test, *id, document_id).await;
        client
            .request_with_headers("REPORT", cal, [("depth", "1")], TIME_RANGE_QUERY)
            .await
            .with_status(StatusCode::MULTI_STATUS);
    }

    tokio::time::sleep(Duration::from_millis(500)).await;
    // (details, reason strings, every string of the event, account id, document id)
    let mut captured: Vec<(String, Vec<String>, String, Option<u64>, Option<u64>)> = Vec::new();
    while let Ok(batch) = rx.try_recv() {
        for event in batch {
            let mut details = String::new();
            let mut reason = Vec::new();
            let mut all = Vec::new();
            let mut account_id = None;
            let mut document_id = None;
            for (key, value) in event.keys.iter() {
                strings(value, &mut all);
                match key {
                    Key::Details => {
                        let mut d = Vec::new();
                        strings(value, &mut d);
                        details = d.join("");
                    }
                    Key::Reason => strings(value, &mut reason),
                    Key::AccountId => {
                        if let trc::Value::UInt(v) = value {
                            account_id = Some(*v);
                        }
                    }
                    Key::DocumentId => {
                        if let trc::Value::UInt(v) = value {
                            document_id = Some(*v);
                        }
                    }
                    _ => {}
                }
            }
            captured.push((details, reason, all.join("\n"), account_id, document_id));
        }
    }
    Collector::remove_subscriber(SUBSCRIBER_ID.into());
    Collector::reload();

    let dump = format!("{captured:?}");
    for uid in [
        "za-rrule-key",
        "za-rrule-plain",
        "za-chrono-key",
        "za-chrono-plain",
    ] {
        assert!(
            captured.iter().any(|(details, ..)| details == uid),
            "no rule-expansion trace with UID {uid}: {dump}"
        );
    }
    for (details, reason, _, account_id, document_id) in &captured {
        if details.starts_with("za-chrono-") {
            assert!(
                reason.iter().any(|s| s == "chrono error"),
                "unexpected chrono reason: {dump}"
            );
            let (_, expected_account, expected_document) = broken
                .iter()
                .find(|(uid, ..)| uid == details)
                .expect("broken event");
            assert_eq!(
                *account_id,
                Some(*expected_account as u64),
                "chrono account id: {dump}"
            );
            assert_eq!(
                *document_id,
                Some(*expected_document as u64),
                "chrono document id: {dump}"
            );
        } else if details == "za-rrule-key" {
            assert_eq!(reason, &["RRule error".to_string()], "key reason: {dump}");
        } else if details == "za-rrule-plain" {
            assert!(
                reason.iter().any(|s| s.contains("Until date")),
                "plain reason must stay upstream's text: {dump}"
            );
        }
    }
    for (_, _, all, ..) in &captured {
        for forbidden in [
            SUMMARY_CANARY,
            DESCRIPTION_CANARY,
            "BEGIN:VCALENDAR",
            "SUMMARY:",
            "X-ZA-",
        ] {
            assert!(
                !all.contains(forbidden),
                "iCalendar text ({forbidden}) in rule-expansion trace: {dump}"
            );
        }
    }

    test.wait_for_tasks().await;
    for (who, _, client, cal, _) in &clients[..1] {
        for name in ["bad-rrule", "broken"] {
            client
                .request("DELETE", &format!("{cal}{name}-{who}.ics"), "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }
    plain_client
        .request("DELETE", plain_cal, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}
