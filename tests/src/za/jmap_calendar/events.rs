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
            CALENDAR_JMAP_USING, delete_event_records, inbox_ids, plant_event, stored_event,
            wait_for_queue, write_stored_event, za_bearer_jmap, za_grant_token, za_revoke_grant,
        },
    },
    za::{
        dav_seal::resource_id,
        fanout::{CONTENT_TYPE, NO_REPLY},
        gating::method_error,
        imip::deliver,
    },
};
use calcard::icalendar::ICalendarValue;
use common::{auth::oauth::scope::OAuthScopes, za::ZA_CALENDAR_KEYLESS};
use futures::FutureExt;
use groupware::calendar::seal::tree::{KEY_PROP, is_carrier};
use hyper::StatusCode;
use jmap_proto::types::state::{JMAPIntermediateState, State, ZaCalendarOrigin};
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
    changes_as_upstream(test, &pair).await;
    identity_get_and_changes(&pair).await;
    parse_sealed_upload(&pair).await;
    parse_message_part(test, &pair).await;
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
