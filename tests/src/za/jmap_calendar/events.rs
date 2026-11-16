/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    Pair, event_ics, finish, make_calendar, mark_boundary_pending, owner_boundary, pair, put_event,
    share,
};
use crate::{
    utils::{
        jmap::JmapResponse,
        server::TestServer,
        za::{
            CALENDAR_JMAP_USING, delete_event_records, plant_event, stored_event,
            write_stored_event, za_bearer_jmap, za_grant_token, za_revoke_grant,
        },
    },
    za::{
        dav_seal::resource_id,
        fanout::{CONTENT_TYPE, NO_REPLY},
        gating::method_error,
    },
};
use calcard::icalendar::ICalendarValue;
use common::{auth::oauth::scope::OAuthScopes, za::ZA_CALENDAR_KEYLESS};
use futures::FutureExt;
use groupware::calendar::seal::tree::{KEY_PROP, is_carrier};
use hyper::StatusCode;
use serde_json::{Value, json};
use std::{panic::AssertUnwindSafe, time::Duration};
use store::{
    ValueKey,
    write::{BatchBuilder, ValueClass, serialize::RawValue},
};
use trc::{
    Collector, EventType, Key, StoreEvent,
    ipc::subscriber::{Interests, SubscriberBuilder},
};
use types::{
    collection::Collection,
    field::{CalendarEventField, Field},
    id::Id,
};

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar event tests...");
    let pair = pair(test, "events").await;
    no_access_gets_upstreams_answer(&pair).await;
    let shared = make_calendar(test, &pair.key, &pair.dav, "jmapcal-shared").await;
    share(
        test,
        &pair.key,
        &pair.key2,
        pair.key2_id,
        "jmapcal-shared",
        "view",
    )
    .await;
    get_serves_the_caldav_tree(test, &pair).await;
    get_keyless_is_refused(test, &pair).await;
    get_legacy_event(test, &pair).await;
    get_unreadable_event_is_not_found(test, &pair).await;
    get_completes_pending_boundary(test, &pair, shared).await;
    read_store_errors_are_payload_free(test, &pair).await;
    pair.dav
        .request_with_headers("DELETE", &event_path("jmapcal-shared/"), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    finish(test, "events").await;
}

fn event_path(path: &str) -> String {
    format!("/dav/cal/jmapcal-events%40example.com/{path}")
}

async fn delete_event(pair: &Pair, path: &str) {
    pair.dav
        .request_with_headers("DELETE", &event_path(path), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn get_events(pair: &Pair, grantee: bool, ids: Value) -> JmapResponse {
    let caller = if grantee { &pair.key2 } else { &pair.key };
    caller
        .jmap_method_call(
            "CalendarEvent/get",
            json!({ "accountId": pair.key.id_string(), "ids": ids }),
        )
        .await
}

async fn no_access_gets_upstreams_answer(pair: &Pair) {
    for method in ["CalendarEvent/get", "Calendar/get"] {
        let reply = pair
            .key2
            .jmap_method_call(
                method,
                json!({ "accountId": pair.key.id_string(), "ids": null }),
            )
            .await;
        assert_eq!(method_error(&reply), Some("accountNotFound"), "{reply:?}");
    }
}

async fn get_serves_the_caldav_tree(test: &TestServer, pair: &Pair) {
    const UID: &str = "jmapcal-events-get";
    const PATH: &str = "default/jmapcal-events-get.ics";

    let ical = event_ics(UID, "jmapcal title canary").replace(
        "END:VEVENT",
        "DESCRIPTION:jmapcal description canary\r\nLOCATION:jmapcal location canary\r\nEND:VEVENT",
    );
    pair.dav
        .request_with_headers("PUT", &event_path(PATH), [CONTENT_TYPE], ical)
        .await
        .with_status(StatusCode::CREATED);
    let document_id = resource_id(test, pair.key_id, PATH).await;
    let id = Id::from(document_id).to_string();
    let reply = get_events(pair, false, json!([id])).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let [event] = reply.list() else {
        panic!("Expected one event: {reply:?}");
    };
    assert_eq!(event["uid"], UID);
    assert_eq!(event["title"], "jmapcal title canary");
    assert_eq!(event["description"], "jmapcal description canary");
    assert_eq!(event["duration"], "PT1H");
    assert!(
        event["start"]
            .as_str()
            .is_some_and(|start| start.starts_with("2099-03-01T10:00:00")),
        "{event}"
    );
    assert!(
        event["locations"]
            .to_string()
            .contains("jmapcal location canary"),
        "{event}"
    );
    for property in [
        "isDraft",
        "mayInviteSelf",
        "mayInviteOthers",
        "hideAttendees",
        "useDefaultAlerts",
    ] {
        assert_eq!(event[property], false, "{property}: {event}");
    }
    assert!(
        !stored_event(test, pair.key_id, document_id)
            .await
            .bytes()
            .contains("canary")
    );

    let reply = get_events(pair, false, Value::Null).await;
    assert!(
        reply.list().iter().any(|event| event["uid"] == UID),
        "{reply:?}"
    );
    delete_event(pair, PATH).await;
}

async fn get_keyless_is_refused(test: &TestServer, pair: &Pair) {
    let (grant, token) = za_grant_token(&test.server, pair.key.name(), OAuthScopes::ALL).await;
    let account = pair.key.id_string();
    let (status, reply) = za_bearer_jmap(
        &token,
        CALENDAR_JMAP_USING,
        json!([
            ["CalendarEvent/get", { "accountId": account, "ids": null }, "c0"],
            ["Calendar/get", { "accountId": account, "ids": null }, "c1"],
            ["Calendar/query", { "accountId": account }, "c2"]
        ]),
    )
    .await;
    za_revoke_grant(&test.server, grant).await;
    assert_eq!(status, 200, "{reply:?}");
    assert_eq!(
        ZA_CALENDAR_KEYLESS,
        "Calendars in this account are encrypted at rest and are not available to this session."
    );
    for (index, call_id) in ["c0", "c1", "c2"].into_iter().enumerate() {
        assert_eq!(
            reply.0["methodResponses"][index],
            json!(["error", { "type": "forbidden", "description": ZA_CALENDAR_KEYLESS }, call_id])
        );
    }
}

async fn get_legacy_event(test: &TestServer, pair: &Pair) {
    let id = plant_event(
        test,
        pair.key_id,
        "default",
        "jmapcal-legacy.ics",
        &event_ics("jmapcal-events-legacy", "jmapcal legacy title"),
        None,
    )
    .await;
    let reply = get_events(pair, false, json!([Id::from(id).to_string()])).await;
    assert_eq!(
        reply.list()[0]["title"],
        "jmapcal legacy title",
        "{reply:?}"
    );
    delete_event(pair, "default/jmapcal-legacy.ics").await;
}

async fn get_unreadable_event_is_not_found(test: &TestServer, pair: &Pair) {
    let mut ids = Vec::new();
    for uid in ["jmapcal-events-tampered", "jmapcal-events-intact"] {
        ids.push(put_event(test, &pair.key, &pair.dav, "default", uid, uid).await);
    }
    let stored = stored_event(test, pair.key_id, ids[0]).await;
    let mut event = stored.za_event();
    let carrier = event.content.data.event.components[0]
        .entries
        .iter_mut()
        .find(|entry| is_carrier(entry, KEY_PROP))
        .expect("sealed event has a key carrier");
    let ICalendarValue::Text(value) = &mut carrier.values[0] else {
        panic!("expected a text key carrier");
    };
    value.replace_range(..1, if value.starts_with('A') { "B" } else { "A" });
    write_stored_event(test, pair.key_id, ids[0], &stored, event).await;

    let ids_str = ids
        .iter()
        .map(|&id| Id::from(id).to_string())
        .collect::<Vec<_>>();
    let reply = get_events(pair, false, json!(ids_str)).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(reply.response_at(0)["notFound"], json!([ids_str[0]]));
    let [intact] = reply.list() else {
        panic!("Expected one intact event: {reply:?}");
    };
    assert_eq!(intact["id"], ids_str[1]);
    assert_eq!(intact["uid"], "jmapcal-events-intact");
    delete_event_records(test, pair.key_id, ids[0]).await;
    delete_event(pair, "default/jmapcal-events-intact.ics").await;
}

async fn get_completes_pending_boundary(test: &TestServer, pair: &Pair, shared: u32) {
    let id = put_event(
        test,
        &pair.key,
        &pair.dav,
        "jmapcal-shared",
        "jmapcal-events-boundary",
        "jmapcal shared title",
    )
    .await;
    let ids = json!([Id::from(id).to_string()]);
    mark_boundary_pending(test, pair.key_id, shared).await;
    assert!(owner_boundary(test, pair.key_id, shared).await.pending);

    let reply = get_events(pair, true, ids.clone()).await;
    assert_eq!(
        reply.list()[0]["title"],
        "jmapcal shared title",
        "{reply:?}"
    );
    assert!(owner_boundary(test, pair.key_id, shared).await.pending);

    let reply = get_events(pair, false, ids).await;
    assert_eq!(
        reply.list()[0]["title"],
        "jmapcal shared title",
        "{reply:?}"
    );
    assert!(!owner_boundary(test, pair.key_id, shared).await.pending);

    delete_event(pair, "jmapcal-shared/jmapcal-events-boundary.ics").await;
}

async fn read_store_errors_are_payload_free(test: &TestServer, pair: &Pair) {
    const UID: &str = "jmapcal-events-bad-archive";
    const SUBSCRIBER: &str = "jmapcal-read-store-error";
    const STORE_ERROR: EventType = EventType::Store(StoreEvent::DataCorruption);

    let event_id = put_event(test, &pair.key, &pair.dav, "default", UID, UID).await;
    let calendar_id = resource_id(test, pair.key_id, "default").await;
    let (grant, token) = za_grant_token(&test.server, pair.key.name(), OAuthScopes::ALL).await;
    test.wait_for_tasks().await;

    let mut previous_interests = Interests::default();
    for &event in EventType::variants() {
        if Collector::has_interest(event) {
            previous_interests.set(event);
        }
    }
    let (_tx, mut rx, _) = SubscriberBuilder::new(SUBSCRIBER.into())
        .set_interests([STORE_ERROR])
        .with_lossy(false)
        .register();
    let mut interest = Interests::default();
    interest.set(STORE_ERROR);
    Collector::union_interests(interest);
    Collector::reload();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !Collector::get_subscribers()
            .iter()
            .any(|id| id == SUBSCRIBER)
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("register calendar archive trace subscriber");

    let mut outcomes = Vec::new();
    for (method, collection, field, document_id, canary) in [
        (
            "CalendarEvent/get",
            Collection::CalendarEvent,
            CalendarEventField::Content.into(),
            event_id,
            b"synthetic event archive bytes\0".as_slice(),
        ),
        (
            "Calendar/get",
            Collection::Calendar,
            Field::ARCHIVE.into(),
            calendar_id,
            b"synthetic calendar archive bytes\0".as_slice(),
        ),
    ] {
        let id = Id::from(document_id).to_string();
        let account = pair.key.id_string();
        let args = json!({ "accountId": account, "ids": [id] });
        let warm = pair.key.jmap_method_call(method, args.clone()).await;
        assert_eq!(warm.list()[0]["id"], id);

        let key = || ValueKey {
            account_id: pair.key_id,
            collection: collection.into(),
            document_id,
            class: ValueClass::Property(field),
        };
        let original = test
            .server
            .store()
            .get_value::<RawValue>(key())
            .await
            .unwrap()
            .unwrap()
            .0;
        let write = |bytes: Vec<u8>| async move {
            let mut batch = BatchBuilder::new();
            batch
                .with_account_id(pair.key_id)
                .with_collection(collection)
                .with_document(document_id)
                .set(ValueClass::Property(field), bytes);
            test.server.store().write_batch(&mut batch).await.unwrap();
        };

        // The NUL is not an archive marker. No production data is part of the canary.
        write(canary.to_vec()).await;
        let failed = AssertUnwindSafe(async {
            let keyless =
                za_bearer_jmap(&token, CALENDAR_JMAP_USING, json!([[method, args, "c0"]])).await;
            let keyed = pair.key.jmap_method_call(method, args.clone()).await;
            (keyless, keyed)
        })
        .catch_unwind()
        .await;
        write(original.clone()).await;
        let restored = test
            .server
            .store()
            .get_value::<RawValue>(key())
            .await
            .unwrap()
            .unwrap()
            .0;
        assert!(
            original == restored,
            "{method}: original property bytes not restored"
        );
        let read_back = pair.key.jmap_method_call(method, args).await;
        assert_eq!(read_back.list()[0]["id"], id);

        let trace = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(batch) = rx.recv().await {
                if let Some(event) = batch.into_iter().find(|event| {
                    event.inner.typ == STORE_ERROR
                        && event.value_as_uint(Key::AccountId) == Some(u64::from(pair.key_id))
                        && event.keys.iter().any(|(key, value)| {
                            *key == Key::CausedBy && value.as_str() == Some(method)
                        })
                }) {
                    return Some(event);
                }
            }
            None
        })
        .await;
        outcomes.push((method, canary, failed, trace));
    }

    Collector::remove_subscriber(SUBSCRIBER.into());
    Collector::set_interests(previous_interests);
    Collector::reload();
    za_revoke_grant(&test.server, grant).await;
    delete_event(pair, "default/jmapcal-events-bad-archive.ics").await;

    for (method, canary, failed, trace) in outcomes {
        let ((status, keyless), keyed) = failed.expect("completed getter requests");
        assert_eq!(status, 200);
        assert_eq!(method_error(&keyless), Some("forbidden"));
        assert_eq!(keyless.response_at(0)["description"], ZA_CALENDAR_KEYLESS);
        assert_eq!(method_error(&keyed), Some("serverUnavailable"));
        assert_eq!(
            keyed.response_at(0)["description"],
            "This server is temporarily unavailable. Attempting this same operation later may succeed."
        );
        let trace = trace
            .expect("timed out waiting for calendar Store error")
            .expect("calendar Store subscriber closed");
        let value = trace.value(Key::Value);
        let canary_observed =
            matches!(value, Some(trc::Value::Bytes(bytes)) if bytes.as_slice() == canary);
        assert!(
            value.is_none(),
            "{method}: raw value reached the method-error trace (synthetic canary observed: {canary_observed})"
        );
        assert_eq!(
            trace.value_as_str(Key::Details),
            Some("zero-access: calendar read failed")
        );
        assert!(trace.keys.iter().all(|(key, _)| matches!(
            key,
            Key::AccountId | Key::DocumentId | Key::Details | Key::SpanId | Key::CausedBy
        )));
    }
}
