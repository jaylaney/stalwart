/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 spec 6.4 (D12), 6.8 (D15, D29) and 9.2: `CalendarEvent/query` and
//! `/queryChanges` of a key account. Text is matched in process memory over the
//! unsealed views, everything else as upstream does; `/queryChanges` keeps the
//! explicit origin of a paged sync.

use super::{
    Pair, event_ics, finish, make_calendar, mark_boundary_pending, owner_boundary, pair, put_event,
    share,
};
use crate::{
    utils::{
        account::Account,
        jmap::JmapResponse,
        server::TestServer,
        webdav::DummyWebDavClient,
        za::{
            CALENDAR_JMAP_USING, delete_event_records, search_keys, stored_event,
            wait_for_index_queue, za_bearer_jmap, za_grant_token, za_revoke_grant,
        },
    },
    za::{
        STRONG,
        dav_seal::resource_id,
        fanout::{CONTENT_TYPE, NO_REPLY},
        gating::method_error,
        mail::Traced,
        uid_lease::put,
        user_permissions,
    },
};
use common::{auth::oauth::scope::OAuthScopes, za::ZA_CALENDAR_KEYLESS};
use futures::FutureExt;
use groupware::cache::GroupwareCache;
use hyper::StatusCode;
use jmap::api::metadata::ZA_METADATA_REFUSED;
use jmap_proto::types::state::{JMAPIntermediateState, State, ZaCalendarOrigin};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    panic::AssertUnwindSafe,
    str::FromStr,
    time::{Duration, Instant},
};
use store::{
    ValueKey,
    write::{BatchBuilder, SearchIndex, ValueClass, serialize::RawValue},
};
use types::{
    collection::{Collection, SyncCollection},
    field::CalendarEventField,
    id::Id,
};

const PLAIN: &str = "jmapcal-query-plain@example.com";
const SAME: &str = "jmapcal-same";
const FILTERS: &str = "jmapcal-filters";
const SHARED: &str = "jmapcal-shared";
const RENAMED: &str = "jmapcal-shared-renamed";
const PLAIN_PAGES: &str = "jmapcal-plain-pages";
const BULK: &str = "jmapcal-bulk";
const PAGES: [&str; 4] = [
    "jmapcal-query-page-1",
    "jmapcal-query-page-2",
    "jmapcal-query-page-3",
    "jmapcal-query-page-4",
];
const RENAME_PAGES: [&str; 3] = [
    "jmapcal-query-page-5",
    "jmapcal-query-page-6",
    "jmapcal-query-page-7",
];
const UNAVAILABLE: &str =
    "This server is temporarily unavailable. Attempting this same operation later may succeed.";

struct Query {
    pair: Pair,
    plain: Account,
    plain_id: u32,
    plain_dav: DummyWebDavClient,
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar query tests...");
    let pair = pair(test, "query").await;
    let plain = test
        .account("admin@example.com")
        .create_user_account(
            PLAIN,
            STRONG,
            "jmapcal-query-plain",
            &[],
            user_permissions(),
        )
        .await;
    let plain_id = plain.id().document_id();
    let q = Query {
        plain_dav: DummyWebDavClient::new(plain_id, plain.name(), STRONG, plain.name()),
        plain,
        plain_id,
        pair,
    };

    two_accounts_same_answers(test, &q).await;
    filters_and_sorts(test, &q).await;
    let veiled = text_matches_the_view_not_the_tree(test, &q).await;
    let shared = grantee_privacy(test, &q).await;
    grantee_text_skips_a_sealed_class(test, &q).await;
    query_changes(test, &q, veiled, shared).await;
    query_changes_paged_origin(test, &q, shared).await;
    metadata_condition_refused(&q).await;
    query_store_errors_are_payload_free(test, &q).await;
    text_query_cost(test, &q).await;

    delete_path(&q.pair.key, &q.pair.dav, &format!("{SHARED}/")).await;
    wait_for_index_queue(test, q.plain_id, Duration::from_secs(30)).await;
    test.account("admin@example.com")
        .destroy_account(q.plain)
        .await;
    finish(test, "query").await;
}

fn home(account: &Account) -> String {
    format!("/dav/cal/{}/", account.name().replace('@', "%40"))
}

fn id(document_id: u32) -> String {
    Id::from(document_id).to_string()
}

fn vcalendar(events: &str) -> String {
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\n{events}END:VCALENDAR\r\n")
}

/// PUT of `ics` at `path` in the account's calendar home (201); returns its document id.
async fn put_ics(
    test: &TestServer,
    account: &Account,
    dav: &DummyWebDavClient,
    path: &str,
    ics: String,
) -> u32 {
    dav.request_with_headers(
        "PUT",
        &format!("{}{path}", home(account)),
        [CONTENT_TYPE],
        ics,
    )
    .await
    .with_status(StatusCode::CREATED);
    resource_id(test, account.id().document_id(), path).await
}

async fn delete_path(account: &Account, dav: &DummyWebDavClient, path: &str) {
    dav.request_with_headers(
        "DELETE",
        &format!("{}{path}", home(account)),
        [NO_REPLY],
        "",
    )
    .await
    .with_status(StatusCode::NO_CONTENT);
}

/// `CalendarEvent/query` of `owner`'s account by `caller` with `args`.
async fn query(caller: &Account, owner: &Account, mut args: Value) -> JmapResponse {
    args["accountId"] = json!(owner.id_string());
    caller.jmap_method_call("CalendarEvent/query", args).await
}

/// The ids of a query answer, in order.
fn ids(reply: &JmapResponse) -> Vec<String> {
    assert_eq!(method_error(reply), None, "{reply:?}");
    reply.response_at(0)["ids"]
        .as_array()
        .unwrap_or_else(|| panic!("no ids in {reply:?}"))
        .iter()
        .map(|id| id.as_str().unwrap().to_string())
        .collect()
}

/// `CalendarEvent/queryChanges` of `owner`'s account by `caller` from `since`.
async fn query_changes_call(
    caller: &Account,
    owner: &Account,
    since: &State,
    max: Option<usize>,
) -> JmapResponse {
    let mut args = json!({
        "accountId": owner.id_string(),
        "sinceQueryState": since.to_string(),
    });
    if let Some(max) = max {
        args["maxChanges"] = json!(max);
    }
    caller
        .jmap_method_call("CalendarEvent/queryChanges", args)
        .await
}

fn new_query_state(reply: &JmapResponse) -> State {
    assert_eq!(method_error(reply), None, "{reply:?}");
    reply.response_at(0)["newQueryState"]
        .as_str()
        .and_then(State::parse)
        .unwrap_or_else(|| panic!("no parsable newQueryState: {reply:?}"))
}

fn query_state(reply: &JmapResponse) -> State {
    assert_eq!(method_error(reply), None, "{reply:?}");
    reply.response_at(0)["queryState"]
        .as_str()
        .and_then(State::parse)
        .unwrap_or_else(|| panic!("no parsable queryState: {reply:?}"))
}

fn za_parts(state: &State) -> (&JMAPIntermediateState, &ZaCalendarOrigin) {
    match state {
        State::ZaCalendarIntermediate { range, origin } => (range, origin),
        state => panic!("expected a z continuation, got {state:?}"),
    }
}

fn assert_refused(reply: &JmapResponse, what: &str) {
    assert_eq!(
        method_error(reply),
        Some("cannotCalculateChanges"),
        "{what}: {reply:?}"
    );
}

/// Follows a `/queryChanges` sync with `maxChanges: 1` from `start` while the returned
/// state is intermediate, and returns every newQueryState in order. Each oldQueryState
/// is the submitted token.
async fn page_through(caller: &Account, owner: &Account, start: State) -> Vec<State> {
    let mut since = start;
    let mut states = Vec::new();
    for _ in 0..64 {
        let reply = query_changes_call(caller, owner, &since, Some(1)).await;
        let state = new_query_state(&reply);
        assert_eq!(
            reply.response_at(0)["oldQueryState"],
            since.to_string(),
            "{reply:?}"
        );
        states.push(state.clone());
        if !matches!(
            state,
            State::ZaCalendarIntermediate { .. } | State::Intermediate(_)
        ) {
            return states;
        }
        since = state;
    }
    panic!("the /queryChanges sync did not end");
}

/// Every page but the last is a `z` continuation of `origin`; the last is exact.
fn assert_pages(states: &[State], origin: &ZaCalendarOrigin, what: &str) {
    assert!(states.len() >= 3, "{what}: {states:?}");
    let (last, pages) = states.split_last().unwrap();
    assert!(matches!(last, State::Exact(_)), "{what}: {states:?}");
    for state in pages {
        assert_eq!(za_parts(state).1, origin, "{what}: {states:?}");
    }
}

async fn rename(q: &Query, from: &str, to: &str) {
    let destination = format!("{}{to}/", home(&q.pair.key));
    q.pair
        .dav
        .request_with_headers(
            "MOVE",
            &format!("{}{from}/", home(&q.pair.key)),
            [("destination", destination.as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
}

/// The owner's `Calendar/get`, which completes a pending boundary.
async fn owner_calendar_get(q: &Query) {
    let reply = q
        .pair
        .key
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": q.pair.key.id_string(), "ids": null }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
}

/// The starts of the events (or occurrences) `ids` name, sorted.
async fn starts(account: &Account, ids: &[String]) -> Vec<String> {
    let reply = account
        .jmap_method_call(
            "CalendarEvent/get",
            json!({
                "accountId": account.id_string(),
                "ids": ids,
                "properties": ["start"],
            }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let mut starts = reply
        .list()
        .iter()
        .map(|event| {
            event["start"]
                .as_str()
                .unwrap_or_else(|| panic!("no start: {reply:?}"))
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), ids.len(), "{reply:?}");
    starts.sort();
    starts
}

/// Task 25's three events, with an outside organizer and attendee on the meeting.
fn same_events() -> [(&'static str, String); 3] {
    [
        (
            "jmapcal-query-meeting",
            vcalendar(concat!(
                "BEGIN:VEVENT\r\nUID:jmapcal-query-meeting\r\nDTSTAMP:20240101T000000Z\r\n",
                "DTSTART:20240102T090000Z\r\nDTEND:20240102T100000Z\r\n",
                "SUMMARY:quarterly budget review\r\n",
                "DESCRIPTION:the committee is reviewing the quarterly budgets for the marketing ",
                "department this afternoon\r\n",
                "LOCATION:room alpha\r\n",
                "ORGANIZER;CN=olive organizer:mailto:jmapcal-query-olive@remote.test\r\n",
                "ATTENDEE;CN=arthur guest:mailto:jmapcal-query-arthur@remote.test\r\n",
                "END:VEVENT\r\n"
            )),
        ),
        (
            "jmapcal-query-series",
            vcalendar(concat!(
                "BEGIN:VEVENT\r\nUID:jmapcal-query-series\r\nDTSTAMP:20240101T000000Z\r\n",
                "DTSTART:20240102T090000Z\r\nDTEND:20240102T093000Z\r\nRRULE:FREQ=DAILY\r\n",
                "SUMMARY:team sync\r\n",
                "DESCRIPTION:the weekly meeting of the whole engineering team to talk about ",
                "progress\r\n",
                "END:VEVENT\r\n",
                "BEGIN:VEVENT\r\nUID:jmapcal-query-series\r\nDTSTAMP:20240101T000000Z\r\n",
                "RECURRENCE-ID:20240104T090000Z\r\n",
                "DTSTART:20240104T090000Z\r\nDTEND:20240104T093000Z\r\n",
                "SUMMARY:offsite planning\r\n",
                "END:VEVENT\r\n"
            )),
        ),
        (
            "jmapcal-query-dentist",
            vcalendar(concat!(
                "BEGIN:VEVENT\r\nUID:jmapcal-query-dentist\r\nDTSTAMP:20240101T000000Z\r\n",
                "DTSTART:20240105T140000Z\r\nDTEND:20240105T150000Z\r\n",
                "SUMMARY:dentist appointment\r\n",
                "DESCRIPTION:a routine visit to the dentist for the yearly cleaning and a check ",
                "of the teeth\r\n",
                "LOCATION:clinic beta\r\n",
                "END:VEVENT\r\n"
            )),
        ),
    ]
}

/// Spec 9.2: the same unexpanded queries give the same events on a key account and on an
/// ordinary account whose index is current; the expanded form matches each occurrence's
/// own component on both.
async fn two_accounts_same_answers(test: &TestServer, q: &Query) {
    let accounts = [(&q.pair.key, &q.pair.dav), (&q.plain, &q.plain_dav)];
    let mut uid_maps = Vec::new();
    for (account, dav) in accounts {
        make_calendar(test, account, dav, SAME).await;
        let mut uids = HashMap::new();
        for (uid, ics) in same_events() {
            let document_id = put_ics(test, account, dav, &format!("{SAME}/{uid}.ics"), ics).await;
            uids.insert(id(document_id), uid);
        }
        uid_maps.push(uids);
    }
    wait_for_index_queue(test, q.plain_id, Duration::from_secs(30)).await;

    let uids_of = |reply: &JmapResponse, uids: &HashMap<String, &'static str>| {
        let mut found = ids(reply)
            .iter()
            .map(|id| {
                *uids
                    .get(id)
                    .unwrap_or_else(|| panic!("an id the case did not write: {id} in {reply:?}"))
            })
            .collect::<Vec<_>>();
        found.sort_unstable();
        found
    };

    let conditions = [
        json!({ "title": "budget" }),
        json!({ "text": "marketing" }),
        json!({ "description": "review" }),
        json!({ "location": "alpha" }),
        json!({ "participant": "jmapcal-query-arthur@remote.test" }),
        json!({ "owner": "jmapcal-query-olive@remote.test" }),
        json!({ "title": "budget review" }),
        json!({ "title": "team offsite" }),
        json!({ "title": "\"budget review\"" }),
        json!({ "title": "\"review budget\"" }),
        json!({ "title": "\"sync offsite\"" }),
        json!({ "location": "\"room alpha\"" }),
        json!({ "location": "\"alpha room\"" }),
        json!({ "owner": "\"olive organizer\"" }),
        json!({ "owner": "\"organizer olive\"" }),
        json!({ "participant": "\"arthur guest\"" }),
        json!({ "participant": "\"guest arthur\"" }),
    ];
    for (index, condition) in conditions.iter().cloned().enumerate() {
        let negated = json!({ "operator": "NOT", "conditions": [condition.clone()] });
        for (filter, is_negated) in [(condition, false), (negated, true)] {
            let key = query(
                &q.pair.key,
                &q.pair.key,
                json!({ "filter": filter.clone() }),
            )
            .await;
            let plain = query(&q.plain, &q.plain, json!({ "filter": filter.clone() })).await;
            let (key, plain) = (uids_of(&key, &uid_maps[0]), uids_of(&plain, &uid_maps[1]));
            assert_eq!(key, plain, "condition {}: {filter}", index + 1);
            // Conditions 1, 7 and 8 match on the ordinary account: the comparison is
            // not vacuous.
            if !is_negated && [0, 6, 7].contains(&index) {
                assert!(!plain.is_empty(), "condition {}: {filter}", index + 1);
            }
        }
    }

    // Spec 9.2: text conditions combined with AND and OR (and the negation of each),
    // unexpanded. The flat ANDs are asked in both JMAP forms: an AND operator over
    // one-property conditions, and one condition with several properties.
    let flat_ands = [
        json!({ "title": "budget", "location": "alpha" }),
        json!({ "title": "budget", "description": "marketing" }),
        json!({ "title": "team", "description": "engineering" }),
        json!({ "title": "offsite", "description": "engineering" }),
        json!({
            "participant": "jmapcal-query-arthur@remote.test",
            "owner": "jmapcal-query-olive@remote.test"
        }),
    ];
    let as_operator = |condition: &Value| {
        json!({
            "operator": "AND",
            "conditions": condition
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| json!({ key.clone(): value.clone() }))
                .collect::<Vec<_>>(),
        })
    };
    let mut combined = Vec::new();
    for condition in &flat_ands {
        combined.push(condition.clone());
        combined.push(as_operator(condition));
    }
    combined.extend([
        json!({ "operator": "OR", "conditions": [{ "title": "budget" }, { "title": "dentist" }] }),
        json!({ "operator": "OR", "conditions": [
            { "uid": "jmapcal-query-meeting" }, { "description": "teeth" }
        ] }),
        json!({ "operator": "AND", "conditions": [
            { "operator": "OR", "conditions": [{ "title": "budget" }, { "title": "dentist" }] },
            { "operator": "NOT", "conditions": [{ "location": "alpha" }] }
        ] }),
        json!({ "operator": "OR", "conditions": [
            { "text": "progress" },
            { "operator": "AND", "conditions": [{ "location": "beta" }, { "title": "appointment" }] }
        ] }),
    ]);
    for filter in combined {
        let negated = json!({ "operator": "NOT", "conditions": [filter.clone()] });
        for (filter, is_negated) in [(filter, false), (negated, true)] {
            let key = query(
                &q.pair.key,
                &q.pair.key,
                json!({ "filter": filter.clone() }),
            )
            .await;
            let plain = query(&q.plain, &q.plain, json!({ "filter": filter.clone() })).await;
            let (key, plain) = (uids_of(&key, &uid_maps[0]), uids_of(&plain, &uid_maps[1]));
            assert_eq!(key, plain, "combined: {filter}");
            if !is_negated {
                assert!(!plain.is_empty(), "combined: {filter}");
            }
        }
    }

    // The expanded form, around the series.
    let expanded = |filter: Value| {
        let mut filter = filter;
        filter["after"] = json!("2024-01-01T00:00:00");
        filter["before"] = json!("2024-01-07T00:00:00");
        json!({ "filter": filter, "expandRecurrences": true })
    };
    let reply = query(
        &q.pair.key,
        &q.pair.key,
        expanded(json!({ "title": "offsite" })),
    )
    .await;
    assert_eq!(ids(&reply).len(), 1, "the overridden occurrence: {reply:?}");
    let reply = query(
        &q.pair.key,
        &q.pair.key,
        expanded(json!({ "title": "sync" })),
    )
    .await;
    assert_eq!(ids(&reply).len(), 4, "the master's occurrences: {reply:?}");
    // Spec 9.2: for the single conditions and the flat ANDs among them, the expanded
    // form matches on each occurrence's own component, on both accounts alike.
    let mut parity = vec![
        json!({ "title": "offsite" }),
        json!({ "title": "sync" }),
        json!({ "title": "team offsite" }),
    ];
    parity.extend(conditions.iter().cloned());
    parity.extend(flat_ands.iter().cloned());
    for condition in parity {
        let filter = expanded(condition.clone());
        let key = ids(&query(&q.pair.key, &q.pair.key, filter.clone()).await);
        let plain = ids(&query(&q.plain, &q.plain, filter).await);
        let key_starts = starts(&q.pair.key, &key).await;
        assert_eq!(
            key_starts,
            starts(&q.plain, &plain).await,
            "expanded {condition}"
        );
        println!(
            "two_accounts_same_answers: expanded {condition}: {} occurrences on both accounts",
            key_starts.len()
        );
    }
    let negated = json!({
        "filter": { "operator": "NOT", "conditions": [{ "title": "sync" }] },
        "expandRecurrences": true,
    });
    for account in [&q.pair.key, &q.plain] {
        let reply = query(account, account, negated.clone()).await;
        assert_eq!(method_error(&reply), Some("invalidArguments"), "{reply:?}");
    }

    delete_path(&q.pair.key, &q.pair.dav, &format!("{SAME}/")).await;
    delete_path(&q.plain, &q.plain_dav, &format!("{SAME}/")).await;
}

/// One VEVENT starting at `start` (UTC), with optional extra lines.
fn timed_ics(uid: &str, start: &str, extra: &str) -> String {
    vcalendar(&format!(
        concat!(
            "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\n",
            "DTSTART:{start}\r\nDURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:{uid}\r\n{extra}",
            "END:VEVENT\r\n"
        ),
        uid = uid,
        start = start,
        extra = extra,
    ))
}

/// Spec 6.4: without a text condition no event is opened; filters, sorts and the
/// expansion read the cache and the stored tree's visible fields.
async fn filters_and_sorts(test: &TestServer, q: &Query) {
    let (key, dav, key_id) = (&q.pair.key, &q.pair.dav, q.pair.key_id);
    let filters = make_calendar(test, key, dav, FILTERS).await;
    let pause = || tokio::time::sleep(Duration::from_millis(1100));

    // Each write in its own second, so created and updated differ between events.
    let c = put_ics(
        test,
        key,
        dav,
        "default/jmapcal-query-fs-c.ics",
        timed_ics("jmapcal-query-fs-c", "20990305T100000Z", ""),
    )
    .await;
    pause().await;
    let a = put_ics(
        test,
        key,
        dav,
        &format!("{FILTERS}/jmapcal-query-fs-a.ics"),
        timed_ics("jmapcal-query-fs-a", "20990303T100000Z", ""),
    )
    .await;
    pause().await;
    let b = put_ics(
        test,
        key,
        dav,
        &format!("{FILTERS}/jmapcal-query-fs-b.ics"),
        timed_ics("jmapcal-query-fs-b", "20990301T100000Z", ""),
    )
    .await;
    pause().await;
    let daily = put_ics(
        test,
        key,
        dav,
        &format!("{FILTERS}/jmapcal-query-fs-daily.ics"),
        timed_ics(
            "jmapcal-query-fs-daily",
            "20990310T090000Z",
            "RRULE:FREQ=DAILY;COUNT=5\r\n",
        ),
    )
    .await;
    pause().await;
    // An update of `a` moves its updated value past the others.
    dav.request_with_headers(
        "PUT",
        &format!("{}{FILTERS}/jmapcal-query-fs-a.ics", home(key)),
        [CONTENT_TYPE],
        timed_ics(
            "jmapcal-query-fs-a",
            "20990303T100000Z",
            "LOCATION:here\r\n",
        ),
    )
    .await
    .with_status(StatusCode::NO_CONTENT);
    let written = [a, b, c, daily];
    let sorted_ids = |mut list: Vec<String>| {
        list.sort();
        list
    };
    let all = sorted_ids(written.iter().map(|doc| id(*doc)).collect());

    let reply = query(key, key, json!({})).await;
    assert_eq!(sorted_ids(ids(&reply)), all, "every event");
    let reply = query(key, key, json!({ "filter": { "inCalendar": id(filters) } })).await;
    assert_eq!(
        sorted_ids(ids(&reply)),
        sorted_ids(vec![id(a), id(b), id(daily)]),
        "inCalendar"
    );
    let reply = query(
        key,
        key,
        json!({ "filter": { "uid": "jmapcal-query-fs-c" } }),
    )
    .await;
    assert_eq!(ids(&reply), [id(c)], "uid");
    let reply = query(
        key,
        key,
        json!({ "filter": { "after": "2099-03-02T00:00:00", "before": "2099-03-06T00:00:00" } }),
    )
    .await;
    assert_eq!(
        sorted_ids(ids(&reply)),
        sorted_ids(vec![id(a), id(c)]),
        "after and before"
    );

    // The order of the stored values, read from the cache.
    let cache = test
        .server
        .fetch_groupware_resources(key_id, key_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let item = |doc: u32| cache.item_by_id(doc).unwrap();
    let mut by_start = written
        .iter()
        .map(|doc| (item(*doc).event_time_range().unwrap().0, *doc))
        .collect::<Vec<_>>();
    let mut by_uid = written
        .iter()
        .map(|doc| (item(*doc).uid().unwrap().to_string(), *doc))
        .collect::<Vec<_>>();
    let mut by_created = written
        .iter()
        .map(|doc| (item(*doc).created_at().unwrap(), *doc))
        .collect::<Vec<_>>();
    let mut by_updated = written
        .iter()
        .map(|doc| (item(*doc).modified_at().unwrap(), *doc))
        .collect::<Vec<_>>();
    by_start.sort();
    by_uid.sort();
    by_created.sort();
    by_updated.sort();
    let order = |values: Vec<u32>| values.into_iter().map(id).collect::<Vec<_>>();
    let distinct = |keys: Vec<String>| {
        let mut unique = keys.clone();
        unique.dedup();
        assert_eq!(unique, keys, "the stored values are not distinct");
    };
    distinct(by_start.iter().map(|(v, _)| v.to_string()).collect());
    distinct(by_uid.iter().map(|(v, _)| v.clone()).collect());
    distinct(by_created.iter().map(|(v, _)| v.to_string()).collect());
    distinct(by_updated.iter().map(|(v, _)| v.to_string()).collect());
    assert_ne!(
        by_created.iter().map(|(_, doc)| *doc).collect::<Vec<_>>(),
        by_updated.iter().map(|(_, doc)| *doc).collect::<Vec<_>>(),
        "the update did not change the updated order"
    );
    for (property, ascending) in [
        (
            "start",
            order(by_start.iter().map(|(_, doc)| *doc).collect()),
        ),
        ("uid", order(by_uid.iter().map(|(_, doc)| *doc).collect())),
        (
            "created",
            order(by_created.iter().map(|(_, doc)| *doc).collect()),
        ),
        (
            "updated",
            order(by_updated.iter().map(|(_, doc)| *doc).collect()),
        ),
    ] {
        for is_ascending in [true, false] {
            let reply = query(
                key,
                key,
                json!({ "sort": [{ "property": property, "isAscending": is_ascending }] }),
            )
            .await;
            let mut expected = ascending.clone();
            if !is_ascending {
                expected.reverse();
            }
            assert_eq!(ids(&reply), expected, "sort {property} {is_ascending}");
        }
    }

    // The expansion of a daily series of five gives five synthetic ids.
    let reply = query(
        key,
        key,
        json!({
            "filter": { "after": "2099-03-10T00:00:00", "before": "2099-03-16T00:00:00" },
            "expandRecurrences": true,
        }),
    )
    .await;
    let occurrences = ids(&reply);
    assert_eq!(occurrences.len(), 5, "{reply:?}");
    let mut unique = occurrences.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 5, "{reply:?}");
    for occurrence in &occurrences {
        let parsed = Id::from_str(occurrence).unwrap();
        assert_ne!(occurrence, &id(daily), "{reply:?}");
        assert_eq!(parsed.document_id(), daily, "{reply:?}");
        assert_ne!(parsed.prefix_id(), 0, "{reply:?}");
    }

    delete_path(key, dav, "default/jmapcal-query-fs-c.ics").await;
    delete_path(key, dav, &format!("{FILTERS}/")).await;
}

/// Spec 6.4 (D12) and invariant 60: a text condition matches the unsealed view, not the
/// sealed tree, and nothing is indexed.
async fn text_matches_the_view_not_the_tree(test: &TestServer, q: &Query) -> u32 {
    let (key, key_id) = (&q.pair.key, q.pair.key_id);
    let veiled = put_event(
        test,
        key,
        &q.pair.dav,
        "default",
        "jmapcal-query-hidden",
        "jmapcal veiled word",
    )
    .await;
    let reply = query(key, key, json!({ "filter": { "title": "veiled" } })).await;
    assert_eq!(ids(&reply), [id(veiled)]);
    assert!(
        !stored_event(test, key_id, veiled)
            .await
            .bytes()
            .contains("veiled"),
        "the stored event holds its title in the clear"
    );
    let reply = query(key, key, json!({ "filter": { "title": "absentword" } })).await;
    assert_eq!(ids(&reply), Vec::<String>::new());
    assert!(
        search_keys(test, key_id, SearchIndex::Calendar)
            .await
            .is_empty()
    );
    veiled
}

/// Spec 6.4: privacy for a grantee is upstream's, applied to the views.
async fn grantee_privacy(test: &TestServer, q: &Query) -> u32 {
    let (key, key2, dav) = (&q.pair.key, &q.pair.key2, &q.pair.dav);
    let calendar = make_calendar(test, key, dav, SHARED).await;
    share(test, key, key2, q.pair.key2_id, SHARED, "view").await;
    let private = put_ics(
        test,
        key,
        dav,
        &format!("{SHARED}/jmapcal-query-private.ics"),
        event_ics("jmapcal-query-private", "jmapcal private word")
            .replace("END:VEVENT", "CLASS:PRIVATE\r\nEND:VEVENT"),
    )
    .await;
    let public = put_event(
        test,
        key,
        dav,
        SHARED,
        "jmapcal-query-public",
        "jmapcal public word",
    )
    .await;

    let reply = query(key2, key, json!({ "filter": { "title": "public" } })).await;
    assert_eq!(ids(&reply), [id(public)], "the grantee's public match");
    let reply = query(key2, key, json!({ "filter": { "title": "private" } })).await;
    assert_eq!(
        ids(&reply),
        Vec::<String>::new(),
        "the grantee's private match"
    );
    let reply = query(key, key, json!({ "filter": { "title": "private" } })).await;
    assert_eq!(ids(&reply), [id(private)], "the owner's private match");
    calendar
}

/// Final review FC2-1 (spec 6.3, 6.4, D12): an extension CLASS is sealed and
/// leaves the row flags clear (field policy 2), and `CalendarEvent/get` shows
/// the grantee the private view. The grantee's text query leaves the event
/// out too, by word and by prefix, as an ordinary account's flagged private
/// event matches no text leaf of a grantee; the owner's query finds it.
async fn grantee_text_skips_a_sealed_class(test: &TestServer, q: &Query) {
    const WORD: &str = "cloakedcanary";
    let (key, key2, dav) = (&q.pair.key, &q.pair.key2, &q.pair.dav);
    let path = format!("{SHARED}/jmapcal-query-classed.ics");
    let classed = put_ics(
        test,
        key,
        dav,
        &path,
        event_ics("jmapcal-query-classed", &format!("jmapcal {WORD}"))
            .replace("END:VEVENT", "CLASS:X-JMAPCAL-VEILED\r\nEND:VEVENT"),
    )
    .await;
    for filter in [json!({ "title": WORD }), json!({ "title": "cloaked*" })] {
        let reply = query(key2, key, json!({ "filter": filter.clone() })).await;
        assert_eq!(
            ids(&reply),
            Vec::<String>::new(),
            "the grantee's match of {filter}"
        );
        let reply = query(key, key, json!({ "filter": filter.clone() })).await;
        assert_eq!(ids(&reply), [id(classed)], "the owner's match of {filter}");
    }
    let reply = key2
        .jmap_method_call(
            "CalendarEvent/get",
            json!({
                "accountId": key.id_string(),
                "ids": [id(classed)],
                "properties": ["title", "privacy"],
            }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let [event] = reply.list() else {
        panic!("the grantee does not read the event: {reply:?}");
    };
    assert!(
        !event.to_string().contains(WORD),
        "the grantee's get shows the title: {event}"
    );
    delete_path(key, dav, &path).await;
}

/// Spec 6.8: `/queryChanges` runs the query function, and D15's boundary applies to it.
async fn query_changes(test: &TestServer, q: &Query, veiled: u32, shared: u32) {
    let (key, key2, key_id) = (&q.pair.key, &q.pair.key2, q.pair.key_id);
    let filter = json!({ "title": "veiled" });
    let reply = query(key, key, json!({ "filter": filter.clone() })).await;
    assert_eq!(ids(&reply), [id(veiled)]);
    let s = query_state(&reply);
    let second = put_event(
        test,
        key,
        &q.pair.dav,
        "default",
        "jmapcal-query-hidden-2",
        "jmapcal veiled again",
    )
    .await;
    let reply = key
        .jmap_method_call(
            "CalendarEvent/queryChanges",
            json!({ "accountId": key.id_string(), "filter": filter, "sinceQueryState": s.to_string() }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let added = reply.response_at(0)["added"]
        .as_array()
        .unwrap_or_else(|| panic!("no added list: {reply:?}"))
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(added, [id(second)], "{reply:?}");

    mark_boundary_pending(test, key_id, shared).await;
    assert!(owner_boundary(test, key_id, shared).await.pending);
    owner_calendar_get(q).await;
    let boundary = owner_boundary(test, key_id, shared).await;
    assert!(!boundary.pending, "{boundary:?}");
    let b = State::new_exact(boundary.rename_change_id);
    assert_refused(
        &query_changes_call(key2, key, &b, None).await,
        "the grantee at the boundary",
    );
    let reply = query_changes_call(key, key, &b, None).await;
    assert_eq!(method_error(&reply), None, "the owner: {reply:?}");

    delete_path(key, &q.pair.dav, "default/jmapcal-query-hidden.ics").await;
    delete_path(key, &q.pair.dav, "default/jmapcal-query-hidden-2.ics").await;
}

/// D29: `/queryChanges` preserves the explicit origin of a paged sync on the public path.
async fn query_changes_paged_origin(test: &TestServer, q: &Query, calendar: u32) {
    let (owner, reader, owner_id) = (&q.pair.key, &q.pair.key2, q.pair.key_id);
    let b = owner_boundary(test, owner_id, calendar).await;
    assert!(!b.pending, "{b:?}");
    put_event(test, owner, &q.pair.dav, SHARED, PAGES[0], PAGES[0]).await;
    let e = query_state(&query(reader, owner, json!({})).await);
    assert!(
        e.get_change_id() > b.rename_change_id,
        "{e:?} {:?}",
        b.rename_change_id
    );
    for uid in &PAGES[1..] {
        put_event(test, owner, &q.pair.dav, SHARED, uid, uid).await;
    }
    let initial = ZaCalendarOrigin::Initial;
    let exact = ZaCalendarOrigin::Exact(e.get_change_id());

    // Completed boundary: every intermediate page keeps the original origin.
    let pages = page_through(reader, owner, State::Initial).await;
    assert_pages(&pages, &initial, "the grantee from Initial");
    let pages = page_through(reader, owner, e.clone()).await;
    assert_pages(&pages, &exact, "the grantee from an exact state");

    // Both first pages exist before the boundary goes pending.
    let initial_first =
        new_query_state(&query_changes_call(reader, owner, &State::Initial, Some(1)).await);
    assert_eq!(za_parts(&initial_first).1, &initial);
    let exact_first = new_query_state(&query_changes_call(reader, owner, &e, Some(1)).await);
    assert_eq!(za_parts(&exact_first).1, &exact);
    mark_boundary_pending(test, owner_id, calendar).await;
    assert!(owner_boundary(test, owner_id, calendar).await.pending);
    assert_refused(
        &query_changes_call(reader, owner, &exact_first, Some(1)).await,
        "the exact-origin page while pending",
    );
    let reply = query_changes_call(reader, owner, &initial_first, Some(1)).await;
    assert_eq!(
        method_error(&reply),
        None,
        "the initial-origin page while pending: {reply:?}"
    );

    // A completed rename: the same distinction against the rename's own boundary. The
    // completion of the pending boundary moves it to `b2`; an exact sync started after
    // `b2` is served until the MOVE, and refused after it.
    owner_calendar_get(q).await;
    let completed = owner_boundary(test, owner_id, calendar).await;
    assert!(!completed.pending, "{completed:?}");
    let b2 = completed.rename_change_id;
    put_event(
        test,
        owner,
        &q.pair.dav,
        SHARED,
        RENAME_PAGES[0],
        RENAME_PAGES[0],
    )
    .await;
    let e2 = query_state(&query(reader, owner, json!({})).await);
    assert!(e2.get_change_id() > b2, "{e2:?} {b2}");
    for uid in &RENAME_PAGES[1..] {
        put_event(test, owner, &q.pair.dav, SHARED, uid, uid).await;
    }
    let exact2 = ZaCalendarOrigin::Exact(e2.get_change_id());
    let exact2_first = new_query_state(&query_changes_call(reader, owner, &e2, Some(1)).await);
    assert_eq!(za_parts(&exact2_first).1, &exact2);
    for (state, what) in [
        (&exact2_first, "the exact-origin page before the rename"),
        (&initial_first, "the initial-origin page before the rename"),
    ] {
        let reply = query_changes_call(reader, owner, state, Some(1)).await;
        assert_eq!(method_error(&reply), None, "{what}: {reply:?}");
    }
    rename(q, SHARED, RENAMED).await;
    let renamed = owner_boundary(test, owner_id, calendar).await;
    assert!(!renamed.pending, "{renamed:?}");
    assert!(renamed.rename_change_id > b2, "{renamed:?} {b2}");
    assert!(
        renamed.rename_change_id > e2.get_change_id(),
        "{renamed:?} {e2:?}"
    );
    assert_refused(
        &query_changes_call(reader, owner, &exact2_first, Some(1)).await,
        "the exact-origin page after the rename",
    );
    let reply = query_changes_call(reader, owner, &initial_first, Some(1)).await;
    assert_eq!(
        method_error(&reply),
        None,
        "the initial-origin page after the rename: {reply:?}"
    );

    // A grantee's legacy continuation has no origin: the client restarts.
    let first = new_query_state(&query_changes_call(reader, owner, &State::Initial, Some(1)).await);
    let legacy = State::Intermediate(za_parts(&first).0.clone());
    assert!(legacy.to_string().starts_with('r'));
    assert_refused(
        &query_changes_call(reader, owner, &legacy, Some(1)).await,
        "the grantee's legacy continuation",
    );
    let restart = page_through(reader, owner, State::Initial).await;
    assert_pages(&restart, &initial, "the grantee's restart");

    // A key owner's legacy chain stays legacy until it completes.
    let first = new_query_state(&query_changes_call(owner, owner, &State::Initial, Some(1)).await);
    assert_eq!(za_parts(&first).1, &initial);
    let legacy = State::Intermediate(za_parts(&first).0.clone());
    let chain = page_through(owner, owner, legacy).await;
    let (last, links) = chain.split_last().unwrap();
    assert!(matches!(last, State::Exact(_)), "{chain:?}");
    assert!(links.len() >= 2, "{chain:?}");
    for state in links {
        assert!(matches!(state, State::Intermediate(_)), "{chain:?}");
        assert!(state.to_string().starts_with('r'), "{chain:?}");
    }

    // An ordinary account keeps its states; a `z` there is invalidArguments.
    let plain = &q.plain;
    make_calendar(test, plain, &q.plain_dav, PLAIN_PAGES).await;
    put_event(
        test,
        plain,
        &q.plain_dav,
        PLAIN_PAGES,
        "jmapcal-query-plain-1",
        "1",
    )
    .await;
    let t = query_state(&query(plain, plain, json!({})).await);
    assert!(matches!(t, State::Exact(_)), "{t:?}");
    for uid in [
        "jmapcal-query-plain-2",
        "jmapcal-query-plain-3",
        "jmapcal-query-plain-4",
    ] {
        put_event(test, plain, &q.plain_dav, PLAIN_PAGES, uid, uid).await;
    }
    for start in [State::Initial, t.clone()] {
        let mut since = start;
        let mut pages = 0;
        loop {
            let reply = query_changes_call(plain, plain, &since, Some(1)).await;
            let encoded = reply.response_at(0)["newQueryState"]
                .as_str()
                .unwrap_or_else(|| panic!("no newQueryState: {reply:?}"))
                .to_string();
            let state = new_query_state(&reply);
            assert_eq!(reply.response_at(0)["oldQueryState"], since.to_string());
            match &state {
                State::Exact(_) => break,
                State::Intermediate(range) => {
                    assert!(encoded.starts_with('r'), "{encoded}");
                    assert_eq!(
                        encoded,
                        State::new_intermediate(range.from_id, range.to_id, range.items_sent)
                            .to_string()
                    );
                }
                state => panic!("an ordinary continuation {state:?}"),
            }
            pages += 1;
            assert!(pages < 64, "the ordinary sync did not end");
            since = state;
        }
        assert!(pages >= 2, "{pages}");
    }
    for origin in [
        ZaCalendarOrigin::Initial,
        ZaCalendarOrigin::Exact(t.get_change_id()),
    ] {
        let z = State::ZaCalendarIntermediate {
            range: JMAPIntermediateState {
                from_id: t.get_change_id(),
                to_id: t.get_change_id(),
                items_sent: 1,
            },
            origin,
        };
        let reply = query_changes_call(plain, plain, &z, Some(1)).await;
        assert_eq!(method_error(&reply), Some("invalidArguments"), "{reply:?}");
    }
    delete_path(plain, &q.plain_dav, &format!("{PLAIN_PAGES}/")).await;

    // A malformed `z` (every field missing) is invalidArguments.
    let malformed = "z";
    assert_eq!(State::parse(&malformed), None, "{malformed}");
    let reply = reader
        .jmap_method_call(
            "CalendarEvent/queryChanges",
            json!({ "accountId": owner.id_string(), "sinceQueryState": malformed }),
        )
        .await;
    assert_eq!(method_error(&reply), Some("invalidArguments"), "{reply:?}");

    // Upstream's refusal of an expanded query, and the existing access and keyless
    // errors: the origin grants no access.
    let reply = reader
        .jmap_method_call(
            "CalendarEvent/queryChanges",
            json!({
                "accountId": owner.id_string(),
                "sinceQueryState": initial_first.to_string(),
                "expandRecurrences": true,
            }),
        )
        .await;
    assert_refused(&reply, "an expanded /queryChanges");
    for state in [&initial_first, &exact_first] {
        let reply = query_changes_call(plain, owner, state, Some(1)).await;
        assert_eq!(method_error(&reply), Some("accountNotFound"), "{reply:?}");
    }
    let (grant, keyless) = za_grant_token(&test.server, owner.name(), OAuthScopes::ALL).await;
    let (status, reply) = za_bearer_jmap(
        &keyless,
        CALENDAR_JMAP_USING,
        json!([[
            "CalendarEvent/queryChanges",
            { "accountId": owner.id_string(), "sinceQueryState": initial_first.to_string() },
            "c0"
        ]]),
    )
    .await;
    za_revoke_grant(&test.server, grant).await;
    assert_eq!(status, 200, "{reply:?}");
    assert_eq!(
        reply.0["methodResponses"][0],
        json!(["error", { "type": "forbidden", "description": ZA_CALENDAR_KEYLESS }, "c0"])
    );

    // Restore the calendar's name and a completed boundary; the shared calendar keeps
    // no event past this case.
    rename(q, RENAMED, SHARED).await;
    owner_calendar_get(q).await;
    assert!(!owner_boundary(test, owner_id, calendar).await.pending);
    for uid in PAGES
        .iter()
        .chain(&RENAME_PAGES)
        .chain(&["jmapcal-query-private", "jmapcal-query-public"])
    {
        delete_path(owner, &q.pair.dav, &format!("{SHARED}/{uid}.ics")).await;
    }
}

/// Spec 6.4: a `metadata` condition is refused, as object metadata is.
async fn metadata_condition_refused(q: &Query) {
    let key = &q.pair.key;
    let reply = key
        .jmap_request(
            &[
                "urn:ietf:params:jmap:core",
                "urn:ietf:params:jmap:calendars",
                "urn:ietf:params:jmap:metadata",
            ],
            json!([["CalendarEvent/query", {
                "accountId": key.id_string(),
                "filter": { "metadataExists": "example.com/note" }
            }, "0"]]),
        )
        .await;
    assert_eq!(method_error(&reply), Some("unsupportedFilter"), "{reply:?}");
    assert_eq!(
        reply.response_at(0)["description"],
        ZA_METADATA_REFUSED,
        "{reply:?}"
    );
}

/// Spec 8 (ruling R21-2): a Store error of the keyed query path is rebuilt payload-free.
async fn query_store_errors_are_payload_free(test: &TestServer, q: &Query) {
    const UID: &str = "jmapcal-query-corrupted";
    const CANARY: &[u8] = b"jmapcal synthetic query content canary\0";
    const DETAILS: &str = "zero-access: calendar read failed";
    let (key, key_id) = (&q.pair.key, q.pair.key_id);
    let event = put_event(
        test,
        key,
        &q.pair.dav,
        "default",
        UID,
        "jmapcal corrupted word",
    )
    .await;
    let args = json!({ "filter": { "title": "corrupted" } });
    let warm = query(key, key, args.clone()).await;
    assert_eq!(ids(&warm), [id(event)]);
    let since = query_state(&warm);
    let (grant, keyless) = za_grant_token(&test.server, key.name(), OAuthScopes::ALL).await;
    test.wait_for_tasks().await;

    let value_key = || ValueKey {
        account_id: key_id,
        collection: Collection::CalendarEvent.into(),
        document_id: event,
        class: ValueClass::Property(CalendarEventField::Content.into()),
    };
    let original = test
        .server
        .store()
        .get_value::<RawValue>(value_key())
        .await
        .unwrap()
        .unwrap()
        .0;
    let write = |bytes: Vec<u8>| async move {
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(key_id)
            .with_collection(Collection::CalendarEvent)
            .with_document(event)
            .set(
                ValueClass::Property(CalendarEventField::Content.into()),
                bytes,
            );
        test.server.store().write_batch(&mut batch).await.unwrap();
    };

    let traced = Traced::start(trc::EventType::variants());
    // The NUL is not an archive marker. No production data is part of the canary.
    write(CANARY.to_vec()).await;
    let failed = AssertUnwindSafe(async {
        let keyed = query(key, key, args.clone()).await;
        let keyless = za_bearer_jmap(
            &keyless,
            CALENDAR_JMAP_USING,
            json!([["CalendarEvent/query", {
                "accountId": key.id_string(), "filter": { "title": "corrupted" }
            }, "c0"]]),
        )
        .await;
        // `/queryChanges` runs the same query function: with `calculateTotal` it runs
        // the query, which opens the planted content.
        let changes = key
            .jmap_method_call(
                "CalendarEvent/queryChanges",
                json!({
                    "accountId": key.id_string(),
                    "filter": { "title": "corrupted" },
                    "sinceQueryState": since.to_string(),
                    "calculateTotal": true,
                }),
            )
            .await;
        (keyed, keyless, changes)
    })
    .catch_unwind()
    .await;
    write(original.clone()).await;
    let restored = test
        .server
        .store()
        .get_value::<RawValue>(value_key())
        .await
        .unwrap()
        .unwrap()
        .0;
    let events = traced.finish().await;
    za_revoke_grant(&test.server, grant).await;
    assert!(original == restored, "original content bytes not restored");
    let read_back = query(key, key, args).await;
    assert_eq!(ids(&read_back), [id(event)]);
    delete_path(key, &q.pair.dav, &format!("default/{UID}.ics")).await;

    let (keyed, (status, keyless), changes) = failed.expect("completed query requests");
    for (method, reply) in [
        ("CalendarEvent/query", &keyed),
        ("CalendarEvent/queryChanges", &changes),
    ] {
        assert_eq!(
            method_error(reply),
            Some("serverUnavailable"),
            "{method}: {reply:?}"
        );
        assert_eq!(reply.response_at(0)["description"], UNAVAILABLE, "{method}");
    }
    assert_eq!(status, 200);
    assert_eq!(
        keyless.0["methodResponses"][0],
        json!(["error", { "type": "forbidden", "description": ZA_CALENDAR_KEYLESS }, "c0"])
    );

    fn holds(value: &trc::Value, canary: &[u8]) -> bool {
        let contains = |bytes: &[u8]| bytes.windows(canary.len()).any(|window| window == canary);
        match value {
            trc::Value::Bytes(bytes) => contains(bytes),
            trc::Value::String(text) => contains(text.as_bytes()),
            trc::Value::Event(event) => event.keys().iter().any(|(_, value)| holds(value, canary)),
            trc::Value::Array(values) => values.iter().any(|value| holds(value, canary)),
            _ => false,
        }
    }
    let rebuilt = events
        .iter()
        .filter(|event| {
            matches!(event.typ, trc::EventType::Store(_))
                && event.details == DETAILS
                && event.account_id == Some(u64::from(key_id))
        })
        .count();
    assert!(
        rebuilt >= 2,
        "no rebuilt Store error was traced for both methods ({rebuilt})"
    );
    let leaked = events
        .iter()
        .filter(|event| event.keys.iter().any(|(_, value)| holds(value, CANARY)))
        .count();
    assert_eq!(leaked, 0, "events holding the synthetic canary");
}

/// Review focus 5: one unexpanded text query without a time range over 300 events, and
/// the same query within one day.
async fn text_query_cost(test: &TestServer, q: &Query) {
    const NEEDLE: usize = 157;
    let (key, key_id) = (&q.pair.key, q.pair.key_id);
    make_calendar(test, key, &q.pair.dav, BULK).await;
    let mut paths = Vec::with_capacity(300);
    for n in 0..300usize {
        let uid = format!("jmapcal-query-bulk-{n}");
        let summary = if n == NEEDLE {
            format!("jmapcal bulk {n} needleword")
        } else {
            format!("jmapcal bulk {n}")
        };
        let ics = vcalendar(&format!(
            concat!(
                "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\n",
                "DTSTART:209904{day:02}T100000Z\r\nDURATION:PT1H\r\nSEQUENCE:0\r\n",
                "SUMMARY:{summary}\r\nEND:VEVENT\r\n"
            ),
            uid = uid,
            day = n % 30 + 1,
            summary = summary,
        ));
        let path = format!("{BULK}/{uid}.ics");
        let status = put(key.name(), STRONG, format!("{}{path}", home(key)), ics).await;
        assert_eq!(status, 201, "{path}");
        paths.push(path);
    }
    let cache = test
        .server
        .fetch_groupware_resources(key_id, key_id, SyncCollection::Calendar)
        .await
        .unwrap();
    let documents = paths
        .iter()
        .map(|path| {
            cache
                .by_path(path)
                .unwrap_or_else(|| panic!("{path} not found"))
                .document_id()
        })
        .collect::<Vec<_>>();
    // By construction the account holds these 300 events and nothing else.
    assert_eq!(cache.document_ids(false).count(), 300);
    let needle = id(documents[NEEDLE]);

    let started = Instant::now();
    let reply = query(key, key, json!({ "filter": { "title": "needleword" } })).await;
    let elapsed = started.elapsed();
    assert_eq!(ids(&reply), [needle.clone()]);
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    println!(
        "text_query_cost: 300 events opened, one text query without a time range, {} ms",
        elapsed.as_millis()
    );

    let day = NEEDLE % 30 + 1;
    let started = Instant::now();
    let reply = query(
        key,
        key,
        json!({ "filter": {
            "title": "needleword",
            "after": format!("2099-04-{day:02}T00:00:00"),
            "before": format!("2099-04-{:02}T00:00:00", day + 1),
        } }),
    )
    .await;
    let elapsed = started.elapsed();
    assert_eq!(ids(&reply), [needle]);
    println!(
        "text_query_cost: 10 events opened, the same query within one day, {} ms",
        elapsed.as_millis()
    );

    for document_id in documents {
        delete_event_records(test, key_id, document_id).await;
    }
    delete_path(key, &q.pair.dav, &format!("{BULK}/")).await;
    wait_for_index_queue(test, key_id, Duration::from_secs(60)).await;
}
