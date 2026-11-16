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
            CALENDAR_JMAP_USING, HookParked, assert_no_event_preferences, delete_event_records,
            inbox_ids, itip_tasks_for, plant_event, stored_event, wait_for_queue,
            write_stored_event, za_bearer_jmap, za_grant_token, za_revoke_grant, za_uid_leases,
        },
    },
    za::{
        dav_seal::resource_id,
        drain,
        fanout::{CONTENT_TYPE, NO_REPLY, uid_copies},
        gating::method_error,
        imip::deliver,
    },
};
use calcard::icalendar::ICalendarValue;
use common::{
    auth::oauth::scope::OAuthScopes,
    za::{ZA_CALENDAR_KEYLESS, test_hooks},
};
use futures::FutureExt;
use groupware::{
    cache::GroupwareCache,
    calendar::seal::tree::{KEY_PROP, is_carrier},
};
use hyper::StatusCode;
use jmap_proto::types::state::{JMAPIntermediateState, State, ZaCalendarOrigin};
use serde_json::{Value, json};
use std::{panic::AssertUnwindSafe, str::FromStr, time::Duration};
use store::{
    ValueKey,
    write::{BatchBuilder, ValueClass, serialize::RawValue},
};
use trc::{
    Collector, EventType, Key, StoreEvent,
    ipc::subscriber::{Interests, SubscriberBuilder},
};
use types::{
    collection::{Collection, SyncCollection},
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
    changes_as_upstream(test, &pair).await;
    identity_get_and_changes(&pair).await;
    parse_sealed_upload(&pair).await;
    parse_message_part(test, &pair).await;
    // Release 6 spec 6.5: CalendarEvent/set creates (Task 30).
    let events = Events::new(test, &pair).await;
    create_reads_back_over_caldav(&events).await;
    create_in_two_calendars(&events).await;
    create_property_rulings(&events).await;
    create_use_default_alerts(&events).await;
    create_uid_rule(&events).await;
    create_two_objects_one_fails(&events).await;
    create_stores_no_preferences_and_no_index_document(&events).await;
    tasks(&events).await;
    create_holds_the_lease(&events).await;
    events.finish().await;
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

/// Release 6 spec 6.8: the change log of a key account is served as upstream
/// serves it. Every write goes through CalDAV; every read is an HTTP JMAP call.
async fn changes_as_upstream(test: &TestServer, pair: &Pair) {
    const A: &str = "jmapcal-events-changes-a";
    const B: &str = "jmapcal-events-changes-b";
    const SLUG: &str = "jmapcal-events-changes";

    async fn changes(pair: &Pair, method: &str, since: &State) -> (Vec<Value>, State) {
        let reply = pair
            .key
            .jmap_method_call(
                method,
                json!({ "accountId": pair.key.id_string(), "sinceState": since.to_string() }),
            )
            .await;
        assert_eq!(method_error(&reply), None, "{method} {since}: {reply:?}");
        let response = reply.response_at(0);
        assert_eq!(response["oldState"], since.to_string(), "{reply:?}");
        assert_eq!(response["hasMoreChanges"], false, "{reply:?}");
        let state = response["newState"]
            .as_str()
            .and_then(State::parse)
            .unwrap_or_else(|| panic!("{method}: no newState in {reply:?}"));
        assert!(matches!(state, State::Exact(_)), "{method}: {state:?}");
        let ids = ["created", "updated", "destroyed"]
            .map(|key| response[key].clone())
            .to_vec();
        (ids, state)
    }
    let ids = |ids: &[u32]| {
        json!(
            ids.iter()
                .map(|id| Id::from(*id).to_string())
                .collect::<Vec<_>>()
        )
    };

    // The initial sync is served and ends at an exact state.
    let (_, start) = changes(pair, "CalendarEvent/changes", &State::Initial).await;
    let (_, calendars) = changes(pair, "Calendar/changes", &State::Initial).await;

    let a = put_event(test, &pair.key, &pair.dav, "default", A, "first title").await;
    let (changed, after_a) = changes(pair, "CalendarEvent/changes", &start).await;
    assert_eq!(changed, [ids(&[a]), json!([]), json!([])]);

    let b = put_event(test, &pair.key, &pair.dav, "default", B, "second title").await;
    let (changed, after_b) = changes(pair, "CalendarEvent/changes", &after_a).await;
    assert_eq!(changed, [ids(&[b]), json!([]), json!([])]);

    pair.dav
        .request_with_headers(
            "PUT",
            &event_path(&format!("default/{A}.ics")),
            [CONTENT_TYPE],
            event_ics(A, "changed title"),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let (changed, after_update) = changes(pair, "CalendarEvent/changes", &after_b).await;
    assert_eq!(changed, [json!([]), ids(&[a]), json!([])]);

    delete_event(pair, &format!("default/{B}.ics")).await;
    let (changed, after_delete) = changes(pair, "CalendarEvent/changes", &after_update).await;
    assert_eq!(changed, [json!([]), json!([]), ids(&[b])]);

    // Upstream's folding over the whole range: b's insert and delete cancel out, and
    // a's update is folded into its insert.
    let (changed, end) = changes(pair, "CalendarEvent/changes", &start).await;
    assert_eq!(changed, [ids(&[a]), json!([]), json!([])]);
    assert_eq!(end, after_delete);
    let (changed, same) = changes(pair, "CalendarEvent/changes", &after_delete).await;
    assert_eq!(changed, [json!([]), json!([]), json!([])]);
    assert_eq!(same, after_delete);
    let states = [&start, &after_a, &after_b, &after_update, &after_delete]
        .map(|state| state.get_change_id())
        .to_vec();
    assert!(
        states.windows(2).all(|pair| pair[0] < pair[1]),
        "{states:?}"
    );

    let calendar = make_calendar(test, &pair.key, &pair.dav, SLUG).await;
    let (changed, _) = changes(pair, "Calendar/changes", &calendars).await;
    assert_eq!(changed, [ids(&[calendar]), json!([]), json!([])]);

    pair.dav
        .request_with_headers("DELETE", &event_path(&format!("{SLUG}/")), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    delete_event(pair, &format!("default/{A}.ics")).await;
}

/// Release 6 spec 6.9 and 6.8 (D29): the identities are served as upstream
/// serves them, a grantee gets its own, and a `z` state is not a calendar
/// continuation here.
async fn identity_get_and_changes(pair: &Pair) {
    let owner = pair.key.id_string();
    let identity = |name: &str| {
        json!({
            "id": "a",
            "name": name,
            "calendarAddress": format!("mailto:{name}@example.com"),
            "isDefault": true,
        })
    };

    let reply = pair
        .key
        .jmap_method_call(
            "ParticipantIdentity/get",
            json!({ "accountId": owner, "ids": null }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(reply.list(), [identity("jmapcal-events")], "{reply:?}");
    let state = reply.response_at(0)["state"].as_str().unwrap().to_string();

    let reply = pair
        .key
        .jmap_method_call(
            "ParticipantIdentity/changes",
            json!({ "accountId": owner, "sinceState": state }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let response = reply.response_at(0);
    for key in ["created", "updated", "destroyed"] {
        assert_eq!(response[key], json!([]), "{key}: {reply:?}");
    }

    // A `z` state is never a calendar continuation here.
    for origin in [ZaCalendarOrigin::Initial, ZaCalendarOrigin::Exact(41)] {
        let z = State::ZaCalendarIntermediate {
            range: JMAPIntermediateState {
                from_id: 42,
                to_id: 50,
                items_sent: 3,
            },
            origin,
        }
        .to_string();
        let reply = pair
            .key
            .jmap_method_call(
                "ParticipantIdentity/changes",
                json!({ "accountId": owner, "sinceState": z }),
            )
            .await;
        assert_eq!(method_error(&reply), Some("invalidArguments"), "{reply:?}");
    }

    // A grantee asking for the owner's account gets its own identities, as
    // upstream answers.
    let reply = pair
        .key2
        .jmap_method_call(
            "ParticipantIdentity/get",
            json!({ "accountId": owner, "ids": null }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        reply.list()[0]["calendarAddress"],
        "mailto:jmapcal-events-2@example.com",
        "{reply:?}"
    );
}

/// The one-event ICS of both parse cases, with no `METHOD`.
fn parse_ics() -> String {
    event_ics("jmapcal-events-parse", "jmapcal parse canary")
}

/// Release 6 spec 6.6: the keyed owner parses a sealed upload, which the
/// handler reads through the unsealing reader.
async fn parse_sealed_upload(pair: &Pair) {
    let blob = pair
        .key
        .jmap_client()
        .await
        .upload(None, parse_ics().into_bytes(), None)
        .await
        .unwrap()
        .take_blob_id();
    let reply = pair
        .key
        .jmap_method_call(
            "CalendarEvent/parse",
            json!({ "accountId": pair.key.id_string(), "blobIds": [blob] }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        reply.response_at(0)["parsed"][&blob][0]["title"],
        "jmapcal parse canary",
        "{reply:?}"
    );
}

/// Release 6 spec 6.6: the keyed owner parses a part of a delivered message.
/// The ICS has no `METHOD`, so delivery files no invitation.
async fn parse_message_part(test: &TestServer, pair: &Pair) {
    const SENDER: &str = "dmarc-jmapcal-events-org@remote.test";

    let account = pair.key.id_string();
    let message = format!(
        concat!(
            "From: <{sender}>\r\nTo: <{to}>\r\nSubject: jmapcal parse message\r\n",
            "Message-ID: <jmapcal-events-parse@remote.test>\r\n",
            "Date: Mon, 02 Nov 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"jmapcal-parse\"\r\n\r\n",
            "--jmapcal-parse\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n",
            "A calendar file.\r\n",
            "--jmapcal-parse\r\nContent-Type: text/calendar; name=\"event.ics\"\r\n",
            "Content-Disposition: attachment\r\n\r\n{ics}\r\n--jmapcal-parse--\r\n"
        ),
        sender = SENDER,
        to = pair.key.name(),
        ics = parse_ics(),
    );
    deliver(SENDER, pair.key.name(), &message).await;
    wait_for_queue(test).await;

    let reply = pair
        .key
        .jmap_method_call(
            "Email/get",
            json!({
                "accountId": account,
                "ids": null,
                "properties": ["id", "subject", "attachments"],
            }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let email = reply
        .list()
        .iter()
        .find(|email| email["subject"] == "jmapcal parse message")
        .unwrap_or_else(|| panic!("the delivered message is missing: {reply:?}"))
        .clone();
    let blob = email["attachments"][0]["blobId"]
        .as_str()
        .unwrap_or_else(|| panic!("the message has no attachment: {email}"))
        .to_string();

    let reply = pair
        .key
        .jmap_method_call(
            "CalendarEvent/parse",
            json!({ "accountId": account, "blobIds": [blob] }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        reply.response_at(0)["parsed"][&blob][0]["title"],
        "jmapcal parse canary",
        "{reply:?}"
    );
    assert!(
        inbox_ids(test, pair.key_id).await.is_empty(),
        "without a METHOD the message is no invitation"
    );

    let reply = pair
        .key
        .jmap_method_call(
            "Email/set",
            json!({ "accountId": account, "destroy": [email["id"]] }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        reply.response_at(0)["destroyed"],
        json!([email["id"]]),
        "{reply:?}"
    );
}

/// Release 6 spec 6.5 (Task 30): the default calendar and a second one made
/// with `Calendar/set`, shared by the create cases.
struct Events<'x> {
    test: &'x TestServer,
    pair: &'x Pair,
    cal: String,
    second: String,
}

const MODIFIED: &str = "Another process modified this calendar event, please try again.";
const DRAFTS: &str = "Drafts are not available for zero-access accounts.";
const UNAVAILABLE: &str = "This property is not available for zero-access accounts.";
const ATTACHMENTS: &str = "Event attachments are not available for zero-access accounts.";

fn none() -> Vec<(&'static str, &'static str)> {
    Vec::new()
}

/// One timed event in `calendar_id`.
fn event_in(uid: &str, calendar_id: &str) -> Value {
    json!({
        "@type": "Event",
        "uid": uid,
        "title": uid,
        "start": "2031-03-04T10:00:00",
        "duration": "PT1H",
        "timeZone": "Etc/UTC",
        "calendarIds": { calendar_id: true },
    })
}

/// A display alert, as JMAP writes one.
fn display_alert(offset: &str) -> Value {
    json!({
        "@type": "Alert",
        "action": "display",
        "trigger": { "@type": "OffsetTrigger", "relativeTo": "start", "offset": offset },
    })
}

impl<'x> Events<'x> {
    async fn new(test: &'x TestServer, pair: &'x Pair) -> Self {
        let reply = pair
            .key
            .jmap_method_call(
                "Calendar/get",
                json!({ "accountId": pair.key.id_string(), "ids": null }),
            )
            .await;
        let cal = reply
            .list()
            .iter()
            .find(|entry| entry["isDefault"] == true)
            .and_then(|entry| entry["id"].as_str())
            .unwrap_or_else(|| panic!("no default calendar: {reply:?}"))
            .to_string();
        let reply = pair
            .key
            .jmap_method_call(
                "Calendar/set",
                json!({
                    "accountId": pair.key.id_string(),
                    "create": { "c": { "name": "jmapcal-events-second" } },
                }),
            )
            .await;
        let second = reply
            .pointer("/methodResponses/0/1/created/c/id")
            .and_then(|id| id.as_str())
            .unwrap_or_else(|| panic!("Calendar/set created nothing: {reply:?}"))
            .to_string();
        Events {
            test,
            pair,
            cal,
            second,
        }
    }

    fn event(&self, uid: &str) -> Value {
        event_in(uid, &self.cal)
    }

    async fn create(&self, objects: Vec<Value>) -> JmapResponse {
        self.pair
            .key
            .jmap_create("CalendarEvent", objects, none())
            .await
    }

    /// The created event `i<index>`'s document id.
    fn created_document(reply: &JmapResponse, index: u32) -> u32 {
        reply.created_id(index).document_id()
    }

    async fn get(&self, document_id: u32) -> Value {
        let id = Id::from(document_id).to_string();
        let reply = get_events(self.pair, false, json!([id])).await;
        let [event] = reply.list() else {
            panic!("Expected event {id}: {reply:?}");
        };
        event.clone()
    }

    /// Every href of the event, one per calendar it is named in.
    async fn hrefs(&self, document_id: u32) -> Vec<String> {
        self.test
            .server
            .fetch_groupware_resources(self.pair.key_id, self.pair.key_id, SyncCollection::Calendar)
            .await
            .unwrap()
            .format_resource_paths_by_id(document_id)
            .filter(|href| !href.ends_with('/'))
            .collect()
    }

    /// The href of a calendar of the account.
    async fn calendar_href(&self, calendar_id: &str) -> String {
        let document_id = Id::from_str(calendar_id).unwrap().document_id();
        self.test
            .server
            .fetch_groupware_resources(self.pair.key_id, self.pair.key_id, SyncCollection::Calendar)
            .await
            .unwrap()
            .format_resource_paths_by_id(document_id)
            .find(|href| href.ends_with('/'))
            .unwrap_or_else(|| panic!("no href for calendar {calendar_id}"))
    }

    /// The CalDAV body of the event, from its first href.
    async fn body(&self, document_id: u32) -> String {
        let hrefs = self.hrefs(document_id).await;
        drain::body(&self.pair.dav, &hrefs[0]).await
    }

    /// The JMAP destroy arrives in Task 33: events are deleted over CalDAV,
    /// each of their names in turn.
    async fn delete(&self, document_id: u32) {
        for href in self.hrefs(document_id).await {
            self.pair
                .dav
                .request_with_headers("DELETE", &href, [NO_REPLY], "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }

    async fn finish(self) {
        let href = self.calendar_href(&self.second).await;
        self.pair
            .dav
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
        assert_eq!(za_uid_leases(self.test).await, 0);
    }
}

fn set_error(value: &Value) -> (&str, Option<&str>, Value) {
    (
        value["type"].as_str().unwrap_or_default(),
        value["description"].as_str(),
        value["properties"].clone(),
    )
}

/// Spec 6.5 and D1: a JMAP create is sealed and read back over CalDAV.
async fn create_reads_back_over_caldav(events: &Events<'_>) {
    const UID: &str = "jmapcal-events-create";
    let mut object = events.event(UID);
    object["title"] = json!("Create canary");
    object["description"] = json!("Body canary");
    let reply = events.create(vec![object]).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    // The client omitted the flag: upstream's server-set reports it false.
    assert_eq!(reply.created(0)["useDefaultAlerts"], false, "{reply:?}");
    let document_id = Events::created_document(&reply, 0);

    let body = events.body(document_id).await;
    for line in [
        "UID:jmapcal-events-create",
        "SUMMARY:Create canary",
        "DESCRIPTION:Body canary",
        "DTSTART",
    ] {
        assert!(body.contains(line), "{line}: {body}");
    }
    let tree = stored_event(events.test, events.pair.key_id, document_id)
        .await
        .tree()
        .to_string();
    assert!(
        !tree.contains("Create canary") && !tree.contains("Body canary"),
        "the stored tree holds a sealed value in the clear"
    );

    let event = events.get(document_id).await;
    assert_eq!(event["title"], "Create canary", "{event}");
    assert_eq!(event["isDraft"], false, "{event}");
    assert_eq!(event["useDefaultAlerts"], false, "{event}");
    events.delete(document_id).await;
}

/// Spec 5.2: an event created in two calendars is named in both.
async fn create_in_two_calendars(events: &Events<'_>) {
    const UID: &str = "jmapcal-events-two";
    let mut object = events.event(UID);
    object["calendarIds"] = json!({ (events.cal.as_str()): true, (events.second.as_str()): true });
    let reply = events.create(vec![object]).await;
    let document_id = Events::created_document(&reply, 0);

    let event = events.get(document_id).await;
    assert_eq!(
        event["calendarIds"],
        json!({ (events.cal.as_str()): true, (events.second.as_str()): true }),
        "{event}"
    );
    for calendar in [&events.cal, &events.second] {
        let href = events.calendar_href(calendar).await;
        let listing = events
            .pair
            .dav
            .propfind_with_headers(&href, ["D:getetag"], [("depth", "1")])
            .await;
        let items = listing
            .hrefs
            .keys()
            .filter(|item| !item.ends_with('/'))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 1, "{href}: {items:?}");
        let body = drain::body(&events.pair.dav, &items[0]).await;
        assert!(body.contains("UID:jmapcal-events-two"), "{body}");
    }
    events.delete(document_id).await;
    assert_eq!(uid_copies(events.test, events.pair.key_id, UID).await, 0);
}

/// Spec 6.5: the property rulings on a create.
async fn create_property_rulings(events: &Events<'_>) {
    let (test, key_id) = (events.test, events.pair.key_id);

    let mut object = events.event("jmapcal-events-draft");
    object["isDraft"] = json!(true);
    let reply = events.create(vec![object]).await;
    assert_eq!(
        set_error(reply.not_created(0)),
        ("invalidProperties", Some(DRAFTS), json!(["isDraft"])),
        "{reply:?}"
    );
    assert_eq!(uid_copies(test, key_id, "jmapcal-events-draft").await, 0);

    for property in ["mayInviteSelf", "mayInviteOthers", "hideAttendees"] {
        let uid = format!("jmapcal-events-{property}");
        let mut object = events.event(&uid);
        object[property] = json!(true);
        let reply = events.create(vec![object]).await;
        assert_eq!(
            set_error(reply.not_created(0)),
            ("invalidProperties", Some(UNAVAILABLE), json!([property])),
            "{reply:?}"
        );
        assert_eq!(uid_copies(test, key_id, &uid).await, 0, "{property}");
    }

    for property in [
        "isDraft",
        "mayInviteSelf",
        "mayInviteOthers",
        "hideAttendees",
    ] {
        let mut object = events.event(&format!("jmapcal-events-false-{property}"));
        object[property] = json!(false);
        let reply = events.create(vec![object]).await;
        let document_id = Events::created_document(&reply, 0);
        let event = events.get(document_id).await;
        assert_eq!(event[property], false, "{property}: {event}");
        events.delete(document_id).await;
    }

    let blob = events
        .pair
        .key
        .jmap_client()
        .await
        .upload(None, b"x".to_vec(), None)
        .await
        .unwrap()
        .take_blob_id();
    let mut object = events.event("jmapcal-events-link");
    object["links"] = json!({ "l1": { "@type": "Link", "blobId": blob, "rel": "enclosure" } });
    let reply = events.create(vec![object]).await;
    assert_eq!(
        set_error(reply.not_created(0)),
        ("invalidProperties", Some(ATTACHMENTS), json!(["links"])),
        "{reply:?}"
    );
    assert_eq!(uid_copies(test, key_id, "jmapcal-events-link").await, 0);

    let mut object = events.event("jmapcal-events-metadata");
    object["metadata"] = json!({ "example.com": { "note": "metadata-canary" } });
    let reply = events
        .pair
        .key
        .jmap_request(
            &[
                "urn:ietf:params:jmap:core",
                "urn:ietf:params:jmap:calendars",
                "urn:ietf:params:jmap:metadata",
            ],
            json!([["CalendarEvent/set", {
                "accountId": events.pair.key.id_string(),
                "create": { "i0": object },
            }, "0"]]),
        )
        .await;
    assert_eq!(
        set_error(reply.not_created(0)),
        (
            "forbidden",
            Some("Object metadata is not available for zero-access accounts."),
            Value::Null
        ),
        "{reply:?}"
    );
    assert_eq!(uid_copies(test, key_id, "jmapcal-events-metadata").await, 0);
}

async fn set_default_alerts(events: &Events<'_>, with_time: Value, without_time: Value) {
    let reply = events
        .pair
        .key
        .jmap_method_call(
            "Calendar/set",
            json!({
                "accountId": events.pair.key.id_string(),
                "update": { (events.cal.as_str()): {
                    "defaultAlertsWithTime": with_time,
                    "defaultAlertsWithoutTime": without_time,
                } },
            }),
        )
        .await;
    assert!(
        reply
            .pointer(&format!("/methodResponses/0/1/updated/{}", events.cal))
            .is_some(),
        "{reply:?}"
    );
}

fn alert_ids(event: &Value) -> Vec<String> {
    event["alerts"]
        .as_object()
        .map(|alerts| alerts.keys().cloned().collect())
        .unwrap_or_default()
}

/// D11: default alerts are written into the event by value.
async fn create_use_default_alerts(events: &Events<'_>) {
    set_default_alerts(
        events,
        json!({ "t1": display_alert("-PT15M") }),
        json!({ "d1": display_alert("PT9H") }),
    )
    .await;

    let mut object = events.event("jmapcal-events-alerts");
    object["title"] = json!("Alert canary");
    object["useDefaultAlerts"] = json!(true);
    let reply = events.create(vec![object]).await;
    let created = reply.created(0);
    // Upstream's answer: the client sent the flag, and no alert is inserted.
    assert!(
        created.get("alerts").is_none() && created.get("useDefaultAlerts").is_none(),
        "{reply:?}"
    );
    let timed = Events::created_document(&reply, 0);
    let event = events.get(timed).await;
    assert_eq!(event["useDefaultAlerts"], false, "{event}");
    assert_eq!(alert_ids(&event), ["t1"], "{event}");
    assert_eq!(
        event["alerts"]["t1"]["trigger"]["offset"], "-PT15M",
        "{event}"
    );
    let body = events.body(timed).await;
    assert_eq!(body.matches("BEGIN:VALARM").count(), 1, "{body}");
    assert!(
        body.contains("TRIGGER:-PT15M") && body.contains("DESCRIPTION:Alert canary"),
        "{body}"
    );

    let mut object = events.event("jmapcal-events-alerts-day");
    object["start"] = json!("2031-03-04T00:00:00");
    object["duration"] = json!("P1D");
    object["showWithoutTime"] = json!(true);
    object["useDefaultAlerts"] = json!(true);
    let reply = events.create(vec![object]).await;
    let all_day = Events::created_document(&reply, 0);
    let event = events.get(all_day).await;
    assert_eq!(alert_ids(&event), ["d1"], "{event}");
    let body = events.body(all_day).await;
    assert!(body.contains("TRIGGER:PT9H"), "{body}");

    let mut object = events.event("jmapcal-events-alerts-own");
    object["alerts"] = json!({ "own": display_alert("-PT1H") });
    object["useDefaultAlerts"] = json!(true);
    let reply = events.create(vec![object]).await;
    let own = Events::created_document(&reply, 0);
    let event = events.get(own).await;
    assert_eq!(alert_ids(&event), ["t1"], "{event}");
    let body = events.body(own).await;
    assert_eq!(body.matches("BEGIN:VALARM").count(), 1, "{body}");
    assert!(
        body.contains("TRIGGER:-PT15M") && !body.contains("TRIGGER:-PT1H"),
        "{body}"
    );

    // The calendar's defaults change; the event already written keeps its own.
    set_default_alerts(
        events,
        json!({ "e1": {
            "@type": "Alert",
            "action": "email",
            "trigger": { "@type": "OffsetTrigger", "relativeTo": "start", "offset": "-PT30M" },
        } }),
        json!({ "d1": display_alert("PT9H") }),
    )
    .await;
    let event = events.get(timed).await;
    assert_eq!(alert_ids(&event), ["t1"], "{event}");
    assert_eq!(
        event["alerts"]["t1"]["trigger"]["offset"], "-PT15M",
        "{event}"
    );

    let mut object = events.event("jmapcal-events-alerts-mail");
    object["useDefaultAlerts"] = json!(true);
    let reply = events.create(vec![object]).await;
    let mail = Events::created_document(&reply, 0);
    let body = events.body(mail).await;
    assert!(
        body.contains("ATTENDEE:mailto:jmapcal-events@example.com"),
        "{body}"
    );

    for document_id in [timed, all_day, own, mail] {
        events.delete(document_id).await;
    }
    set_default_alerts(events, Value::Null, Value::Null).await;
}

/// Spec 3.3: a UID is unique within one calendar, checked under the lease.
async fn create_uid_rule(events: &Events<'_>) {
    const UID: &str = "jmapcal-events-uid";
    let reply = events.create(vec![events.event(UID)]).await;
    let first = Events::created_document(&reply, 0);

    let reply = events.create(vec![event_in(UID, &events.second)]).await;
    let second = Events::created_document(&reply, 0);

    let reply = events.create(vec![events.event(UID)]).await;
    assert_eq!(
        set_error(reply.not_created(0)),
        (
            "invalidProperties",
            Some("An event with UID jmapcal-events-uid already exists."),
            json!(["uid"])
        ),
        "{reply:?}"
    );
    assert_eq!(za_uid_leases(events.test).await, 0);
    assert_eq!(uid_copies(events.test, events.pair.key_id, UID).await, 2);
    events.delete(first).await;
    events.delete(second).await;
}

/// D3: each object is its own commit; a failed object does not undo another.
async fn create_two_objects_one_fails(events: &Events<'_>) {
    let mut bad = events.event("jmapcal-events-bad");
    bad["isDraft"] = json!(true);
    let reply = events
        .create(vec![events.event("jmapcal-events-ok"), bad])
        .await;
    let ok = Events::created_document(&reply, 0);
    assert_eq!(
        reply.not_created(1)["type"],
        "invalidProperties",
        "{reply:?}"
    );
    let new_state = reply.new_state().to_string();
    let read = get_events(events.pair, false, json!([Id::from(ok).to_string()])).await;
    assert_eq!(read.list().len(), 1, "{read:?}");
    assert_eq!(read.response_at(0)["state"], new_state, "{read:?}");

    let mut bad = events.event("jmapcal-events-bad-2");
    bad["isDraft"] = json!(true);
    let reply = events
        .create(vec![bad, events.event("jmapcal-events-ok-2")])
        .await;
    assert_eq!(
        reply.not_created(0)["type"],
        "invalidProperties",
        "{reply:?}"
    );
    let ok_2 = Events::created_document(&reply, 1);

    let mut bad = events.event("jmapcal-events-bad-3");
    bad["isDraft"] = json!(true);
    let reply = events.create(vec![bad]).await;
    assert_eq!(
        reply.not_created(0)["type"],
        "invalidProperties",
        "{reply:?}"
    );
    let response = reply.response_at(0);
    assert_eq!(response["newState"], response["oldState"], "{reply:?}");

    events.delete(ok).await;
    events.delete(ok_2).await;
}

/// D10 and spec 6.4: no per-user data and no search document.
async fn create_stores_no_preferences_and_no_index_document(events: &Events<'_>) {
    let (test, key_id) = (events.test, events.pair.key_id);
    let mut object = events.event("jmapcal-events-own-data");
    object["alerts"] = json!({ "a1": display_alert("-PT5M") });
    object["keywords"] = json!({ "kw": true });
    object["color"] = json!("#aabbcc");
    let reply = events.create(vec![object]).await;
    let document_id = Events::created_document(&reply, 0);
    assert_no_event_preferences(test, key_id, document_id).await;

    // The event's own alarm task is due in 2031.
    test.wait_for_tasks_skip_not_due().await;
    assert!(
        !test
            .server
            .za_has_calendar_search_documents(key_id)
            .await
            .unwrap()
    );
    assert_eq!(itip_tasks_for(test, key_id).await, 0);
    events.delete(document_id).await;
}

/// Upstream's tasks are created as VTODOs.
async fn tasks(events: &Events<'_>) {
    let reply = events
        .create(vec![json!({
            "@type": "Task",
            "uid": "jmapcal-events-task",
            "title": "Task canary",
            "due": "2031-03-06T10:00:00",
            "calendarIds": { (events.cal.as_str()): true },
        })])
        .await;
    let document_id = Events::created_document(&reply, 0);
    let event = events.get(document_id).await;
    assert_eq!(event["@type"], "Task", "{event}");
    let body = events.body(document_id).await;
    assert!(body.contains("BEGIN:VTODO"), "{body}");
    events.delete(document_id).await;
}

/// Invitations spec 5.1: a create holds the UID lease until its commit.
async fn create_holds_the_lease(events: &Events<'_>) {
    const UID: &str = "jmapcal-events-lease";
    let account = events.pair.key.clone();
    let object = events.event(UID);
    let parked = HookParked::start(
        events.pair.key_id,
        test_hooks::set_commit_pause,
        async move { account.jmap_create("CalendarEvent", [object], none()).await },
    )
    .await;
    let reply = events.create(vec![events.event(UID)]).await;
    let first = parked.finish().await;

    assert_eq!(
        set_error(reply.not_created(0)),
        ("forbidden", Some(MODIFIED), Value::Null),
        "{reply:?}"
    );
    let document_id = Events::created_document(&first, 0);
    assert_eq!(uid_copies(events.test, events.pair.key_id, UID).await, 1);
    assert_eq!(za_uid_leases(events.test).await, 0);
    events.delete(document_id).await;
}
