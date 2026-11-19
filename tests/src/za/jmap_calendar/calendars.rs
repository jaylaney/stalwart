/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{Pair, finish, make_calendar, pair, put_event, share};
use crate::{
    utils::{
        account::Account,
        http::HttpRequest,
        jmap::JmapResponse,
        server::TestServer,
        smtp_sink::sink,
        webdav::DummyWebDavClient,
        za::{
            HookParked, delete_inbox_item, inbox_ids, item_method, open_pending_item_as,
            wait_for_index_queue, wait_for_queue, za_post,
        },
    },
    za::{
        STRONG,
        dav_seal::{raw_calendar, resource_id},
        fanout::{NO_REPLY, invite, uid_copies},
        gating::method_error,
        mail::Traced,
        uid_lease::put,
    },
};
use calcard::icalendar::ICalendarMethod;
use common::za::{
    imip::ZA_IMIP_DROPPED,
    test_hooks::{
        ForceCommitSplit, set_calendar_delete_fence_pause, set_delete_commit_fail_once,
        set_fan_out_pause, set_second_chunk_fail_once,
    },
};
use groupware::calendar::{CALENDAR_SUBSCRIBED, Calendar, CalendarPreferences, Timezone};
use hyper::{Method, StatusCode};
use jmap_proto::types::state::State;
use serde_json::{Value, json};
use std::time::Duration;
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes, BatchBuilder, ValueClass},
};
use trc::{CalendarEvent, EventType, Key, StoreEvent};
use types::{
    collection::{Collection, SyncCollection},
    field::PrincipalField,
    id::Id,
};

const SLUG: &str = "jmapcal-work";
const PATH: &str = "/dav/cal/jmapcal-calendars%40example.com/jmapcal-work/";
const HOME: &str = "/dav/cal/jmapcal-calendars%40example.com/";
const OWNER_NAME: &str = "jmapcal owner name";
const READER_NAME: &str = "jmapcal reader name";
const OUTSIDE: &str = "jmapcal-calendars-out@remote.test";
const SHARE_TEXT: &str = "Calendar sharing is managed on the account page.";
const INCOMPLETE: &str = "The server could not complete this change, try again.";
const NOT_ALLOWED: &str = "You are not allowed to modify this calendar.";
const MODIFIED: &str = "Another process modified this calendar, please try again.";
/// The fixed reason of `Calendar/set`'s store-failure trace (`calendar/set/za.rs`).
const FAILURE_REASON: &str = "zero-access: calendar write failed";
/// The delete-commit hook's own details: an inert canary that the original
/// error is never traced.
const HOOK_DETAILS: &str = "zero-access test hook: delete commit failure";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar tests...");
    let pair = pair(test, "calendars").await;
    get_shows_own_entry(test, &pair).await;
    pair.dav
        .request_with_headers("DELETE", PATH, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let created = create_seals_name_description_colour(test, &pair).await;
    update_own_entry(test, &pair, &created).await;
    grantee_updates_only_its_entry(test, &pair).await;
    update_without_entry_refused(test, &pair).await;
    colour_before_and_after_sharing(test, &pair).await;
    destroy_with_events_sends_cancellations(test, &pair).await;
    destroy_is_one_transaction(test, &pair).await;
    destroy_fenced_against_a_concurrent_create(test, &pair).await;
    destroy_fence_lost_to_a_crossing_write(test, &pair).await;
    destroy_non_empty_without_flag_refused(test, &pair).await;
    grantee_destroy_refused(test, &pair).await;
    on_success_set_is_default(test, &pair).await;
    share_with_refused(test, &pair).await;
    per_object_commits(&pair).await;
    store_failure_stops_the_call(test, &pair).await;
    finish(test, "calendars").await;
}

async fn set_display_name(dav: &DummyWebDavClient, name: &str) {
    let patch = format!(
        "<D:propertyupdate xmlns:D=\"DAV:\"><D:set><D:prop><D:displayname>{name}</D:displayname></D:prop></D:set></D:propertyupdate>"
    );
    let reply = dav
        .request("PROPPATCH", PATH, patch)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let body = reply.body.unwrap();
    assert!(body.contains("200 OK"), "{body}");
}

async fn get_shows_own_entry(test: &TestServer, pair: &Pair) {
    let shared_id = make_calendar(test, &pair.key, &pair.dav, SLUG).await;
    set_display_name(&pair.dav, OWNER_NAME).await;
    share(test, &pair.key, &pair.key2, pair.key2_id, SLUG, "edit").await;
    set_display_name(&pair.dav2, READER_NAME).await;

    let shared = Id::from(shared_id).to_string();
    let account_id = pair.key.id_string();
    let reply = pair
        .key
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account_id, "ids": [shared] }),
        )
        .await;
    assert_eq!(reply.list()[0]["name"], OWNER_NAME, "{reply:?}");

    let reply = pair
        .key2
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account_id, "ids": null }),
        )
        .await;
    let [calendar] = reply.list() else {
        panic!("Expected exactly the shared calendar: {reply:?}");
    };
    assert_eq!(calendar["id"], shared);
    assert_eq!(calendar["name"], READER_NAME);
    assert_eq!(calendar["myRights"]["mayReadItems"], true);

    let default = Id::from(resource_id(test, pair.key_id, "default").await).to_string();
    let reply = pair
        .key2
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account_id, "ids": [default] }),
        )
        .await;
    assert!(reply.list().is_empty(), "{reply:?}");
    assert_eq!(
        reply.response_at(0)["notFound"],
        json!([default]),
        "{reply:?}"
    );

    let raw = raw_calendar(test, pair.key_id, SLUG).await;
    let raw = String::from_utf8_lossy(raw.as_bytes());
    assert!(!raw.contains(OWNER_NAME) && !raw.contains(READER_NAME));

    let revoked = za_post(
        "shares/revoke",
        &json!({
            "username": pair.key.name(), "password": STRONG,
            "calendar": SLUG, "address": pair.key2.name()
        }),
    )
    .await
    .expect(200);
    assert_eq!(revoked["changed"], true);
    let reply = pair
        .key2
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account_id, "ids": null }),
        )
        .await;
    assert_eq!(method_error(&reply), Some("accountNotFound"), "{reply:?}");
}

/// The stored calendar, deserialised. Never printed: it holds sealed bytes.
async fn stored(test: &TestServer, owner_id: u32, document_id: u32) -> Calendar {
    test.server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            owner_id,
            Collection::Calendar,
            document_id,
        ))
        .await
        .unwrap()
        .expect("calendar archive")
        .deserialize::<Calendar>()
        .unwrap()
}

/// The stored default calendar id of the account.
async fn stored_default(test: &TestServer, account_id: u32) -> Option<u32> {
    test.server
        .store()
        .get_value::<u32>(ValueKey {
            account_id,
            collection: Collection::Principal.into(),
            document_id: 0,
            class: ValueClass::Property(PrincipalField::DefaultCalendarId.into()),
        })
        .await
        .unwrap()
}

async fn calendar_set(caller: &Account, owner: &Account, mut args: Value) -> JmapResponse {
    args["accountId"] = json!(owner.id_string());
    caller.jmap_method_call("Calendar/set", args).await
}

async fn calendar_get(caller: &Account, owner: &Account) -> JmapResponse {
    let reply = caller
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": owner.id_string(), "ids": null }),
        )
        .await;
    assert_eq!(reply.error_type_at(0), None, "{reply:?}");
    reply
}

/// The `Calendar/get` entry of `id`, as `caller` reads it in `owner`'s account.
async fn calendar_entry(caller: &Account, owner: &Account, id: &str) -> Value {
    let reply = caller
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": owner.id_string(), "ids": [id] }),
        )
        .await;
    let [entry] = reply.list() else {
        panic!("Expected calendar {id}: {reply:?}");
    };
    entry.clone()
}

/// The JMAP id of the `Calendar/get` entry whose name is `name`.
async fn calendar_named(caller: &Account, owner: &Account, name: &str) -> String {
    let reply = calendar_get(caller, owner).await;
    reply
        .list()
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("no calendar named {name}: {reply:?}"))["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn listed(reply: &JmapResponse, id: &str) -> bool {
    reply.list().iter().any(|entry| entry["id"] == id)
}

fn set_error(value: &Value) -> (&str, Option<&str>) {
    (
        value["type"].as_str().unwrap_or_default(),
        value["description"].as_str(),
    )
}

async fn destroy(pair: &Pair, id: &str) {
    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({ "destroy": [id], "onDestroyRemoveEvents": true }),
    )
    .await;
    assert_eq!(reply.destroyed().collect::<Vec<_>>(), [id], "{reply:?}");
}

async fn dav_delete(pair: &Pair, slug: &str) {
    pair.dav
        .request_with_headers("DELETE", &format!("{HOME}{slug}/"), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn regrant(pair: &Pair, slug: &str, level: &str) {
    za_post(
        "shares/grant",
        &json!({
            "username": pair.key.name(), "password": STRONG,
            "calendar": slug, "address": pair.key2.name(), "level": level,
        }),
    )
    .await
    .expect(200);
}

/// Spec 6.7: a create seals the name, description and colour under the
/// owner's keys; the visible values stay in the clear.
async fn create_seals_name_description_colour(test: &TestServer, pair: &Pair) -> String {
    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({ "create": { "i0": {
            "name": "Name canary",
            "description": "Description canary",
            "color": "#ff8800",
            "sortOrder": 7,
            "timeZone": "Europe/Vienna",
            "isVisible": false,
        } } }),
    )
    .await;
    let id = reply.created_id(0).to_string();
    let entry = calendar_entry(&pair.key, &pair.key, &id).await;
    assert_eq!(entry["name"], "Name canary", "{entry}");
    assert_eq!(entry["description"], "Description canary", "{entry}");
    assert_eq!(entry["color"], "#ff8800", "{entry}");
    assert_eq!(entry["sortOrder"], 7, "{entry}");
    assert_eq!(entry["timeZone"], "Europe/Vienna", "{entry}");
    assert_eq!(entry["isVisible"], false, "{entry}");

    let calendar = stored(test, pair.key_id, document_id(&id)).await;
    let [entry] = calendar.preferences.as_slice() else {
        panic!(
            "create_seals_name_description_colour: {} stored entries",
            calendar.preferences.len()
        );
    };
    assert_eq!(entry.account_id, pair.key_id);
    assert!(
        entry.name.starts_with("$za$"),
        "the stored name is not sealed"
    );
    assert!(entry.description.is_none(), "a stored description");
    assert!(entry.color.is_none(), "a stored colour");
    assert_eq!(entry.sort_order, 7);
    assert!(calendar.acls.is_empty(), "stored grants");
    let debug = format!("{calendar:?}");
    for canary in ["Name canary", "Description canary", "#ff8800"] {
        assert!(!debug.contains(canary), "{canary} is stored in the clear");
    }

    let path = format!("{HOME}{}/", calendar.name);
    let props = pair.dav.propfind(&path, ["D:displayname"]).await;
    props
        .properties(&path)
        .get("D:displayname")
        .with_values(["Name canary"]);
    id
}

/// Spec 6.7: an update edits the caller's own entry and reseals it.
async fn update_own_entry(test: &TestServer, pair: &Pair, id: &str) {
    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({ "update": { id: {
            "name": "Renamed canary",
            "description": null,
            "color": "#112233",
            "sortOrder": 9,
        } } }),
    )
    .await;
    reply.updated(id);
    let before = reply.response_at(0)["oldState"]
        .as_str()
        .unwrap()
        .to_string();
    let new_state = reply.new_state().to_string();

    let get = calendar_get(&pair.key, &pair.key).await;
    let entry = get
        .list()
        .iter()
        .find(|entry| entry["id"] == id)
        .expect("updated calendar");
    assert_eq!(entry["name"], "Renamed canary", "{entry}");
    assert_eq!(entry["description"], Value::Null, "{entry}");
    assert_eq!(entry["color"], "#112233", "{entry}");
    assert_eq!(entry["sortOrder"], 9, "{entry}");
    assert_ne!(new_state, before);
    assert_eq!(new_state, get.state());

    let calendar = stored(test, pair.key_id, document_id(id)).await;
    let [entry] = calendar.preferences.as_slice() else {
        panic!(
            "update_own_entry: {} stored entries",
            calendar.preferences.len()
        );
    };
    assert!(
        entry.name.starts_with("$za$"),
        "the stored name is not sealed"
    );
    assert_eq!(entry.sort_order, 9);
    let debug = format!("{calendar:?}");
    for canary in ["Renamed canary", "#112233"] {
        assert!(!debug.contains(canary), "{canary} is stored in the clear");
    }
    destroy(pair, id).await;
}

/// Spec 6.7: a grantee changes only its own entry, and only at `edit`.
async fn grantee_updates_only_its_entry(test: &TestServer, pair: &Pair) {
    const OWN: &str = "jmapcal-own-entry";
    let document_id = make_calendar(test, &pair.key, &pair.dav, OWN).await;
    share(test, &pair.key, &pair.key2, pair.key2_id, OWN, "edit").await;
    let id = Id::from(document_id).to_string();
    let owner_before = stored(test, pair.key_id, document_id)
        .await
        .personal_preferences(pair.key_id)
        .cloned()
        .expect("owner entry");

    let update = json!({ "update": { &id: { "name": "Reader name canary", "sortOrder": 3 } } });
    let reply = calendar_set(&pair.key2, &pair.key, update.clone()).await;
    reply.updated(&id);

    let calendar = stored(test, pair.key_id, document_id).await;
    assert!(
        calendar.personal_preferences(pair.key_id) == Some(&owner_before),
        "grantee_updates_only_its_entry: the owner's entry changed"
    );
    assert_eq!(calendar.preferences.len(), 2, "a third entry");
    assert!(
        calendar
            .preferences
            .iter()
            .all(|entry| entry.name.starts_with("$za$")),
        "an entry is not sealed"
    );
    assert_eq!(
        calendar_entry(&pair.key2, &pair.key, &id).await["name"],
        "Reader name canary"
    );
    assert_eq!(calendar_entry(&pair.key, &pair.key, &id).await["name"], OWN);

    regrant(pair, OWN, "view").await;
    let reply = calendar_set(&pair.key2, &pair.key, update).await;
    assert_eq!(
        set_error(reply.not_updated(&id)).0,
        "forbidden",
        "{reply:?}"
    );
    dav_delete(pair, OWN).await;
}

/// Plants a calendar shaped like upstream's `create_default_calendar`
/// (`tests/src/za/setup.rs`), then empties its stored preferences, which
/// `Calendar::insert` would refill.
async fn insert_calendar_without_entry(test: &TestServer, account_id: u32) -> u32 {
    let account_info = test.server.account_info(account_id).await.unwrap();
    let mut batch = BatchBuilder::new();
    let slot = batch.reserve_document_id(account_id, Collection::Calendar);
    Calendar {
        name: "jmapcal-no-entry".to_string(),
        preferences: vec![CalendarPreferences {
            account_id,
            name: "jmapcal-no-entry".to_string(),
            flags: CALENDAR_SUBSCRIBED,
            time_zone: Timezone::Default,
            ..Default::default()
        }],
        ..Default::default()
    }
    .insert(account_info.changed_by(), account_id, slot, &mut batch)
    .unwrap();
    let document_id = test.server.commit_batch(batch).await.unwrap().slot(slot);

    let archive = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            account_id,
            Collection::Calendar,
            document_id,
        ))
        .await
        .unwrap()
        .expect("calendar archive");
    let mut calendar = archive.deserialize::<Calendar>().unwrap();
    calendar.preferences.clear();
    let mut batch = BatchBuilder::new();
    calendar
        .update(
            account_info.changed_by(),
            archive.to_unarchived::<Calendar>().unwrap(),
            account_id,
            document_id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    document_id
}

async fn remove_calendar(test: &TestServer, account_id: u32, document_id: u32) {
    use groupware::DestroyArchive;
    let account_info = test.server.account_info(account_id).await.unwrap();
    let archive = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            account_id,
            Collection::Calendar,
            document_id,
        ))
        .await
        .unwrap()
        .expect("calendar archive");
    let mut batch = BatchBuilder::new();
    DestroyArchive(archive.to_unarchived::<Calendar>().unwrap())
        .delete(
            &test.server,
            account_info.changed_by(),
            account_id,
            document_id,
            None,
            &mut batch,
        )
        .await
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
}

/// Spec 6.7: the handler never adds a preference entry.
async fn update_without_entry_refused(test: &TestServer, pair: &Pair) {
    let document_id = insert_calendar_without_entry(test, pair.key_id).await;
    let id = Id::from(document_id).to_string();
    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({ "update": { &id: { "name": "x" } } }),
    )
    .await;
    assert_eq!(
        set_error(reply.not_updated(&id)),
        ("forbidden", Some(NOT_ALLOWED)),
        "{reply:?}"
    );
    assert!(
        stored(test, pair.key_id, document_id)
            .await
            .preferences
            .is_empty(),
        "update_without_entry_refused: an entry was added"
    );
    remove_calendar(test, pair.key_id, document_id).await;
}

async fn set_colour(caller: &Account, owner: &Account, id: &str, colour: &str) {
    calendar_set(
        caller,
        owner,
        json!({ "update": { id: { "color": colour } } }),
    )
    .await
    .updated(id);
}

/// D14: a shared calendar's colour, before and after the grant.
async fn colour_before_and_after_sharing(test: &TestServer, pair: &Pair) {
    const COLOUR: &str = "jmapcal-colour";
    let document_id = make_calendar(test, &pair.key, &pair.dav, COLOUR).await;
    let id = Id::from(document_id).to_string();
    set_colour(&pair.key, &pair.key, &id, "#00aa55").await;
    share(test, &pair.key, &pair.key2, pair.key2_id, COLOUR, "edit").await;
    assert_eq!(calendar_named(&pair.key2, &pair.key, COLOUR).await, id);

    // 1. and 2.
    assert_eq!(
        calendar_entry(&pair.key, &pair.key, &id).await["color"],
        "#00aa55"
    );
    let entry = calendar_entry(&pair.key2, &pair.key, &id).await;
    assert_eq!(entry["color"], "#00aa55", "{entry}");
    assert_eq!(entry["name"], COLOUR, "{entry}");

    // 3.
    set_colour(&pair.key, &pair.key, &id, "#112233").await;
    assert_eq!(
        calendar_entry(&pair.key, &pair.key, &id).await["color"],
        "#112233"
    );

    // 4.
    set_colour(&pair.key2, &pair.key, &id, "#abcdef").await;
    assert_eq!(
        calendar_entry(&pair.key2, &pair.key, &id).await["color"],
        "#abcdef"
    );
    assert_eq!(
        calendar_entry(&pair.key, &pair.key, &id).await["color"],
        "#112233"
    );

    // 5.
    let calendar = stored(test, pair.key_id, document_id).await;
    assert!(
        calendar
            .preferences
            .iter()
            .all(|entry| entry.color.is_none() && entry.name.starts_with("$za$")),
        "colour_before_and_after_sharing: an entry holds a clear colour or name"
    );
    dav_delete(pair, COLOUR).await;
}

fn organized_invite(pair: &Pair, uid: &str) -> String {
    invite(uid, "20990301T100000Z", &[pair.key2.name(), OUTSIDE])
        .replace("key1@example.com", pair.key.name())
}

async fn put_invite(pair: &Pair, slug: &str, uid: &str) {
    assert_eq!(
        put(
            pair.key.name(),
            STRONG,
            format!("{HOME}{slug}/{uid}.ics"),
            organized_invite(pair, uid),
        )
        .await,
        StatusCode::CREATED.as_u16(),
        "{uid}"
    );
}

async fn delete_new_items(test: &TestServer, pair: &Pair, start: &[u32]) {
    for id in inbox_ids(test, pair.key2_id).await {
        if !start.contains(&id) {
            delete_inbox_item(test, &pair.dav2, pair.key2_id, id).await;
        }
    }
}

/// D16: destroying a calendar sends cancellations, as the CalDAV
/// collection DELETE does.
async fn destroy_with_events_sends_cancellations(test: &TestServer, pair: &Pair) {
    const JMAP: &str = "jmapcal-destroy-jmap";
    const DAV: &str = "jmapcal-destroy-dav";
    const UID_J: &str = "jmapcal-cal-destroy-j";
    const UID_D: &str = "jmapcal-cal-destroy-d";
    let start = inbox_ids(test, pair.key2_id).await;
    let jmap_id = Id::from(make_calendar(test, &pair.key, &pair.dav, JMAP).await).to_string();
    make_calendar(test, &pair.key, &pair.dav, DAV).await;
    put_invite(pair, JMAP, UID_J).await;
    put_invite(pair, DAV, UID_D).await;
    wait_for_queue(test).await;
    sink().clear();
    let before = inbox_ids(test, pair.key2_id).await;

    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({ "destroy": [&jmap_id], "onDestroyRemoveEvents": true }),
    )
    .await;
    assert_eq!(
        reply.destroyed().collect::<Vec<_>>(),
        [jmap_id.as_str()],
        "{reply:?}"
    );
    let response = HttpRequest::with_credentials(8899, pair.key.name(), STRONG)
        .send_full(Method::DELETE, &format!("{HOME}{DAV}/"), None, None)
        .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    wait_for_queue(test).await;

    let mut cancelled = Vec::new();
    for id in inbox_ids(test, pair.key2_id).await {
        if before.contains(&id) {
            continue;
        }
        let (ical, _) = open_pending_item_as(test, pair.key2_id, STRONG, id).await;
        assert_eq!(item_method(&ical), Some(ICalendarMethod::Cancel));
        cancelled.push(ical.uids().next().expect("item UID").to_string());
    }
    cancelled.sort();
    assert_eq!(cancelled, [UID_D, UID_J], "key2's new items");

    let mail = sink().take_to(OUTSIDE);
    assert_eq!(mail.len(), 2, "outside attendee's cancellation mail");
    for uid in [UID_J, UID_D] {
        let uid_line = format!("UID:{uid}");
        let matching = mail
            .iter()
            .filter(|message| {
                message
                    .calendar_part()
                    .is_some_and(|part| part.lines().any(|line| line == uid_line))
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 1, "{uid}: cancellation mail");
        assert_eq!(matching[0].calendar_method().as_deref(), Some("CANCEL"));
    }
    assert_eq!(uid_copies(test, pair.key_id, UID_J).await, 0);
    delete_new_items(test, pair, &start).await;
}

/// The owner's Calendar log records after `tip`.
async fn changes_after(test: &TestServer, owner: u32, tip: u64) -> Vec<u64> {
    let records = test
        .server
        .store()
        .za_changes_records_range(owner, SyncCollection::Calendar, tip, u64::MAX)
        .await
        .unwrap();
    assert!(!records.truncated, "{records:?}");
    records
        .records
        .into_iter()
        .map(|(change_id, _)| change_id)
        .collect()
}

async fn calendar_tip(test: &TestServer, owner: u32) -> u64 {
    test.server
        .store()
        .get_last_change_id(owner, SyncCollection::Calendar.into())
        .await
        .unwrap()
        .unwrap()
}

async fn calendar_stored(test: &TestServer, owner: u32, document_id: u32) -> bool {
    test.server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            owner,
            Collection::Calendar,
            document_id,
        ))
        .await
        .unwrap()
        .is_some()
}

/// PR #90 review C4617611521: a calendar destroy is one physical transaction.
/// A forced split whose second chunk fails no longer leaves the first chunk's
/// events deleted (the release 5 residual of a chunked collection DELETE); a
/// failed commit leaves the calendar and every event, and mails nothing.
async fn destroy_is_one_transaction(test: &TestServer, pair: &Pair) {
    const SLUG: &str = "jmapcal-destroy-whole";
    const UIDS: [&str; 3] = [
        "jmapcal-cal-whole-1",
        "jmapcal-cal-whole-2",
        "jmapcal-cal-whole-3",
    ];
    let owner = pair.key_id;
    let start = inbox_ids(test, pair.key2_id).await;
    let calendar = make_calendar(test, &pair.key, &pair.dav, SLUG).await;
    let calendar_id = Id::from(calendar).to_string();
    put_invite(pair, SLUG, UIDS[0]).await;
    put_invite(pair, SLUG, UIDS[1]).await;
    put_event(test, &pair.key, &pair.dav, SLUG, UIDS[2], "whole").await;
    let invited = [
        resource_id(test, owner, &format!("{SLUG}/{}.ics", UIDS[0])).await,
        resource_id(test, owner, &format!("{SLUG}/{}.ics", UIDS[1])).await,
    ];
    wait_for_queue(test).await;
    // No queued index write of the account meets the forced split below.
    wait_for_index_queue(test, owner, Duration::from_secs(30)).await;

    // A forced commit failure: nothing is deleted and nothing is mailed; each
    // invitation's mail is traced dropped.
    sink().clear();
    let traced = Traced::start(&[EventType::Calendar(CalendarEvent::ItipMessageError)]);
    set_delete_commit_fail_once(Some(owner));
    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({ "destroy": [&calendar_id], "onDestroyRemoveEvents": true }),
    )
    .await;
    set_delete_commit_fail_once(None);
    assert_eq!(
        set_error(reply.not_destroyed(&calendar_id)),
        ("forbidden", Some(INCOMPLETE)),
        "{reply:?}"
    );
    assert!(calendar_stored(test, owner, calendar).await);
    for uid in UIDS {
        assert_eq!(uid_copies(test, owner, uid).await, 1, "{uid}");
    }
    wait_for_queue(test).await;
    assert_eq!(sink().count_to(OUTSIDE), 0, "the failed destroy mailed");
    let mut dropped = traced
        .finish()
        .await
        .into_iter()
        .filter(|event| event.details == ZA_IMIP_DROPPED && event.account_id == Some(owner as u64))
        .map(|event| {
            event
                .keys
                .iter()
                .find_map(|(key, value)| match (key, value) {
                    (Key::DocumentId, trc::Value::UInt(id)) => Some(*id as u32),
                    _ => None,
                })
                .expect("a document id")
        })
        .collect::<Vec<_>>();
    dropped.sort_unstable();
    assert_eq!(dropped, invited, "one dropped trace per invitation");
    // The fan-out ran before the failed commit: key2's stranded items go.
    delete_new_items(test, pair, &start).await;

    // A forced split whose second chunk would fail: the destroy commits whole.
    sink().clear();
    let before = inbox_ids(test, pair.key2_id).await;
    let tip = calendar_tip(test, owner).await;
    let reply = {
        let _split = ForceCommitSplit::new(owner);
        set_second_chunk_fail_once(Some(owner));
        let reply = calendar_set(
            &pair.key,
            &pair.key,
            json!({ "destroy": [&calendar_id], "onDestroyRemoveEvents": true }),
        )
        .await;
        set_second_chunk_fail_once(None);
        reply
    };
    assert_eq!(
        reply.destroyed().collect::<Vec<_>>(),
        [calendar_id.as_str()],
        "{reply:?}"
    );
    for uid in UIDS {
        assert_eq!(uid_copies(test, owner, uid).await, 0, "{uid}");
    }
    assert!(!calendar_stored(test, owner, calendar).await);
    let records = changes_after(test, owner, tip).await;
    assert_eq!(records.len(), 1, "one log record: {records:?}");
    assert_eq!(
        reply.new_state().to_string(),
        State::Exact(records[0]).to_string()
    );
    wait_for_queue(test).await;
    let mail = sink().take_to(OUTSIDE);
    assert_eq!(mail.len(), 2, "outside attendee's cancellation mail");
    for message in &mail {
        assert_eq!(message.calendar_method().as_deref(), Some("CANCEL"));
    }
    let mut cancelled = Vec::new();
    for id in inbox_ids(test, pair.key2_id).await {
        if before.contains(&id) {
            continue;
        }
        let (ical, _) = open_pending_item_as(test, pair.key2_id, STRONG, id).await;
        assert_eq!(item_method(&ical), Some(ICalendarMethod::Cancel));
        cancelled.push(ical.uids().next().expect("item UID").to_string());
    }
    cancelled.sort();
    assert_eq!(cancelled, [UIDS[0], UIDS[1]], "key2's new items");
    delete_new_items(test, pair, &start).await;
}

/// PR #90 review C4617611506: a destroy parked in its fan-out while a CalDAV
/// PUT adds an event to the calendar is refused with upstream's "modified"
/// text; the calendar and both events stay, and the outside attendee's
/// cancellation is not mailed.
async fn destroy_fenced_against_a_concurrent_create(test: &TestServer, pair: &Pair) {
    const SLUG: &str = "jmapcal-destroy-fence";
    const INVITED: &str = "jmapcal-cal-fence-invited";
    const LATE: &str = "jmapcal-cal-fence-late";
    let owner = pair.key_id;
    let start = inbox_ids(test, pair.key2_id).await;
    let calendar = make_calendar(test, &pair.key, &pair.dav, SLUG).await;
    let calendar_id = Id::from(calendar).to_string();
    put_invite(pair, SLUG, INVITED).await;
    wait_for_queue(test).await;
    sink().clear();

    let parked = {
        let (key, calendar_id) = (pair.key.clone(), calendar_id.clone());
        HookParked::start(pair.key2_id, set_fan_out_pause, async move {
            calendar_set(
                &key,
                &key,
                json!({ "destroy": [&calendar_id], "onDestroyRemoveEvents": true }),
            )
            .await
        })
        .await
    };
    put_event(test, &pair.key, &pair.dav, SLUG, LATE, "late").await;
    let reply = parked.finish().await;
    assert_eq!(
        set_error(reply.not_destroyed(&calendar_id)),
        ("forbidden", Some(MODIFIED)),
        "{reply:?}"
    );
    assert!(calendar_stored(test, owner, calendar).await);
    assert_eq!(uid_copies(test, owner, INVITED).await, 1);
    assert_eq!(uid_copies(test, owner, LATE).await, 1);
    wait_for_queue(test).await;
    assert_eq!(sink().count_to(OUTSIDE), 0, "the refused destroy mailed");
    // The fan-out ran before the refusal: key2's stranded item goes.
    delete_new_items(test, pair, &start).await;

    destroy(pair, &calendar_id).await;
    assert_eq!(uid_copies(test, owner, INVITED).await, 0);
    assert_eq!(uid_copies(test, owner, LATE).await, 0);
    wait_for_queue(test).await;
    sink().clear();
    delete_new_items(test, pair, &start).await;
}

/// PR #90 review RA-3: both child-list comparisons pass and the change-id
/// fence alone refuses. The destroy is parked after its second comparison,
/// immediately before its commit, while an event is written to another
/// calendar of the account (the child list does not change); the commit is
/// assigned a later change id than the fence expects.
async fn destroy_fence_lost_to_a_crossing_write(test: &TestServer, pair: &Pair) {
    const SLUG: &str = "jmapcal-destroy-crossed";
    const KEPT: &str = "jmapcal-cal-crossed";
    const CROSSING: &str = "jmapcal-cal-crossing";
    const CROSSING_DAV: &str = "jmapcal-cal-crossing-dav";
    let owner = pair.key_id;
    let calendar = make_calendar(test, &pair.key, &pair.dav, SLUG).await;
    let calendar_id = Id::from(calendar).to_string();
    put_event(test, &pair.key, &pair.dav, SLUG, KEPT, "kept").await;

    let parked = {
        let (key, calendar_id) = (pair.key.clone(), calendar_id.clone());
        HookParked::start(owner, set_calendar_delete_fence_pause, async move {
            calendar_set(
                &key,
                &key,
                json!({ "destroy": [&calendar_id], "onDestroyRemoveEvents": true }),
            )
            .await
        })
        .await
    };
    put_event(test, &pair.key, &pair.dav, "default", CROSSING, "crossing").await;
    let reply = parked.finish().await;
    assert_eq!(
        set_error(reply.not_destroyed(&calendar_id)),
        ("forbidden", Some(MODIFIED)),
        "{reply:?}"
    );
    assert!(calendar_stored(test, owner, calendar).await);
    assert_eq!(uid_copies(test, owner, KEPT).await, 1);

    // The same over CalDAV: the collection DELETE answers 503 (`Busy`; a lost
    // assertion would answer the same, so no answer tells the two apart).
    let parked = {
        let name = pair.key.name().to_string();
        HookParked::start(owner, set_calendar_delete_fence_pause, async move {
            HttpRequest::with_credentials(8899, &name, STRONG)
                .send_full(Method::DELETE, &format!("{HOME}{SLUG}/"), None, None)
                .await
                .status
        })
        .await
    };
    put_event(
        test,
        &pair.key,
        &pair.dav,
        "default",
        CROSSING_DAV,
        "crossing",
    )
    .await;
    assert_eq!(parked.finish().await, StatusCode::SERVICE_UNAVAILABLE);
    assert!(calendar_stored(test, owner, calendar).await);
    assert_eq!(uid_copies(test, owner, KEPT).await, 1);

    for uid in [CROSSING, CROSSING_DAV] {
        pair.dav
            .request_with_headers(
                "DELETE",
                &format!("{HOME}default/{uid}.ics"),
                [NO_REPLY],
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    destroy(pair, &calendar_id).await;
    assert_eq!(uid_copies(test, owner, KEPT).await, 0);
}

/// Upstream's refusal of a non-empty calendar stays, and sends nothing.
async fn destroy_non_empty_without_flag_refused(test: &TestServer, pair: &Pair) {
    const KEPT: &str = "jmapcal-destroy-kept";
    const UID: &str = "jmapcal-cal-destroy-k";
    let start = inbox_ids(test, pair.key2_id).await;
    let id = Id::from(make_calendar(test, &pair.key, &pair.dav, KEPT).await).to_string();
    put_invite(pair, KEPT, UID).await;
    wait_for_queue(test).await;
    sink().clear();
    let before = inbox_ids(test, pair.key2_id).await;

    let reply = calendar_set(&pair.key, &pair.key, json!({ "destroy": [&id] })).await;
    assert_eq!(
        set_error(reply.not_destroyed(&id)).0,
        "calendarHasEvent",
        "{reply:?}"
    );
    assert_eq!(uid_copies(test, pair.key_id, UID).await, 1);
    wait_for_queue(test).await;
    assert_eq!(inbox_ids(test, pair.key2_id).await, before, "key2's items");
    assert!(sink().messages().is_empty(), "mail after a refused destroy");
    dav_delete(pair, KEPT).await;
    delete_new_items(test, pair, &start).await;
}

/// A grantee cannot destroy a shared calendar, as on CalDAV.
async fn grantee_destroy_refused(test: &TestServer, pair: &Pair) {
    const SHARED: &str = "jmapcal-grantee-destroy";
    make_calendar(test, &pair.key, &pair.dav, SHARED).await;
    share(test, &pair.key, &pair.key2, pair.key2_id, SHARED, "edit").await;
    let id = calendar_named(&pair.key, &pair.key, SHARED).await;
    let reply = calendar_set(
        &pair.key2,
        &pair.key,
        json!({ "destroy": [&id], "onDestroyRemoveEvents": true }),
    )
    .await;
    assert_eq!(
        set_error(reply.not_destroyed(&id)).0,
        "forbidden",
        "{reply:?}"
    );
    assert!(listed(&calendar_get(&pair.key, &pair.key).await, &id));
    assert!(listed(&calendar_get(&pair.key2, &pair.key).await, &id));
    dav_delete(pair, SHARED).await;
}

async fn the_default(pair: &Pair) -> String {
    let reply = calendar_get(&pair.key, &pair.key).await;
    let defaults = reply
        .list()
        .iter()
        .filter(|entry| entry["isDefault"] == true)
        .map(|entry| entry["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let [id] = defaults.as_slice() else {
        panic!("Expected exactly one default: {reply:?}");
    };
    id.clone()
}

/// Spec 6.7: `onSuccessSetIsDefault` writes the default calendar id.
async fn on_success_set_is_default(test: &TestServer, pair: &Pair) {
    let former = the_default(pair).await;

    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({
            "create": { "i0": { "name": "jmapcal-default" } },
            "onSuccessSetIsDefault": "#i0",
        }),
    )
    .await;
    assert_eq!(reply.created(0)["isDefault"], true, "{reply:?}");
    let created = reply.created_id(0).to_string();
    assert_eq!(the_default(pair).await, created);
    assert_eq!(
        stored_default(test, pair.key_id).await,
        Some(document_id(&created))
    );

    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({
            "create": { "i0": { "color": "#000000" } },
            "onSuccessSetIsDefault": &former,
        }),
    )
    .await;
    reply.not_created(0);
    assert_eq!(the_default(pair).await, created);
    assert_eq!(
        stored_default(test, pair.key_id).await,
        Some(document_id(&created))
    );

    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({ "onSuccessSetIsDefault": &former }),
    )
    .await;
    assert_eq!(reply.error_type_at(0), None, "{reply:?}");
    assert_eq!(the_default(pair).await, former);
    assert_eq!(
        stored_default(test, pair.key_id).await,
        Some(document_id(&former))
    );
    destroy(pair, &created).await;
}

/// Spec 6.11: `shareWith` is refused, as a property or a patch pointer.
async fn share_with_refused(test: &TestServer, pair: &Pair) {
    const TARGET: &str = "jmapcal-share";
    let key2 = pair.key2.id_string().to_string();
    let grant = json!({ &key2: { "mayReadItems": true } });
    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({ "create": { "i0": { "name": TARGET, "shareWith": grant } } }),
    )
    .await;
    assert_eq!(
        set_error(reply.not_created(0)),
        ("forbidden", Some(SHARE_TEXT)),
        "{reply:?}"
    );

    let document_id = make_calendar(test, &pair.key, &pair.dav, TARGET).await;
    let id = Id::from(document_id).to_string();
    for update in [
        json!({ "shareWith": grant }),
        json!({ format!("shareWith/{key2}"): { "mayReadItems": true } }),
    ] {
        let reply = calendar_set(&pair.key, &pair.key, json!({ "update": { &id: update } })).await;
        assert_eq!(
            set_error(reply.not_updated(&id)),
            ("forbidden", Some(SHARE_TEXT)),
            "{reply:?}"
        );
    }
    assert!(
        stored(test, pair.key_id, document_id).await.acls.is_empty(),
        "share_with_refused: stored grants"
    );
    dav_delete(pair, TARGET).await;
}

/// D3: each object is its own commit.
async fn per_object_commits(pair: &Pair) {
    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({ "create": {
            "i0": { "name": "jmapcal-first" },
            "i1": { "color": "#000000" },
        } }),
    )
    .await;
    let first = reply.created_id(0).to_string();
    assert_eq!(
        set_error(reply.not_created(1)).0,
        "invalidProperties",
        "{reply:?}"
    );
    let new_state = reply.new_state().to_string();
    let get = calendar_get(&pair.key, &pair.key).await;
    assert!(listed(&get, &first));
    assert_eq!(new_state, get.state());

    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({ "create": { "i0": { "color": "#000000" } } }),
    )
    .await;
    reply.not_created(0);
    assert_eq!(
        reply.new_state(),
        reply.response_at(0)["oldState"].as_str().unwrap()
    );

    let missing = Id::from(1_000_000u32).to_string();
    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({
            "create": { "i0": { "name": "jmapcal-second" } },
            "update": { &missing: { "name": "x" } },
            "destroy": [&first],
        }),
    )
    .await;
    let second = reply.created_id(0).to_string();
    reply.not_updated(&missing);
    assert_eq!(
        reply.destroyed().collect::<Vec<_>>(),
        [first.as_str()],
        "{reply:?}"
    );
    let get = calendar_get(&pair.key, &pair.key).await;
    assert!(listed(&get, &second) && !listed(&get, &first), "{get:?}");
    destroy(pair, &second).await;
}

/// 5.3 (D28): a store error on one object answers it and every later one.
async fn store_failure_stops_the_call(test: &TestServer, pair: &Pair) {
    let x = Id::from(make_calendar(test, &pair.key, &pair.dav, "jmapcal-fail-x").await).to_string();
    let y = Id::from(make_calendar(test, &pair.key, &pair.dav, "jmapcal-fail-y").await).to_string();
    let traced = Traced::start(trc::EventType::variants());
    set_delete_commit_fail_once(Some(pair.key_id));
    let reply = calendar_set(
        &pair.key,
        &pair.key,
        json!({
            "create": { "i0": { "name": "jmapcal-fail-new" } },
            "destroy": [&x, &y],
        }),
    )
    .await;
    set_delete_commit_fail_once(None);
    let events = traced.finish().await;

    // Spec 8, D28 (I §7.5): the store failure is traced as a fresh event of
    // its kind with the ids and the fixed reason, never as the original error.
    let x_id = u64::from(document_id(&x));
    assert!(
        events.iter().any(|event| {
            event.typ == EventType::Store(StoreEvent::UnexpectedError)
                && event.account_id == Some(u64::from(pair.key_id))
                && event.details == FAILURE_REASON
                && event.keys.iter().any(|(key, value)| {
                    *key == Key::DocumentId && matches!(value, trc::Value::UInt(id) if *id == x_id)
                })
        }),
        "store_failure_stops_the_call: no store-failure trace of the destroyed calendar"
    );
    assert!(
        !events.iter().any(|event| event.text.contains(HOOK_DETAILS)),
        "store_failure_stops_the_call: the original error was traced"
    );

    assert_eq!(method_error(&reply), None, "{reply:?}");
    let created = reply.created_id(0).to_string();
    for id in [&x, &y] {
        assert_eq!(
            set_error(reply.not_destroyed(id)),
            ("forbidden", Some(INCOMPLETE)),
            "{reply:?}"
        );
    }
    let new_state = reply.new_state().to_string();
    let get = calendar_get(&pair.key, &pair.key).await;
    assert!(listed(&get, &created) && listed(&get, &x) && listed(&get, &y));
    assert_eq!(new_state, get.state());
    for id in [&created, &x, &y] {
        destroy(pair, id).await;
    }
}

fn document_id(id: &str) -> u32 {
    id.parse::<Id>()
        .unwrap_or_else(|_| panic!("invalid id {id}"))
        .document_id()
}
