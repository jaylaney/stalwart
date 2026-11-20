/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 spec 6.8 (D15, D29) and 9.2: a grantee's calendar changes, the
//! sync boundary and the explicit origin of a paged sync, through the actual
//! JMAP changes handlers and CalDAV's sync-collection REPORT.

use super::{
    Pair, event_ics, finish, key_account, make_calendar, mark_boundary_pending, owner_boundary,
    pair, put_event, session_token, share,
};
use crate::{
    utils::{
        account::Account,
        jmap::{JmapResponse, JmapUtils},
        server::TestServer,
        smtp_sink::sink,
        webdav::DummyWebDavClient,
        za::{
            CALENDAR_JMAP_USING, assert_no_event_preferences, copy_href, inbox_ids, item_method,
            new_inbox_items, open_pending_item_as, pending_ids, stored_event, trigger_drain,
            wait_for_queue, za_bearer_jmap, za_grant_token, za_post, za_post_from, za_revoke_grant,
            za_unlock,
        },
    },
    za::{
        STRONG,
        dav_seal::resource_id,
        drain::{DrainOn, body, partstat_of, set_partstat},
        fanout::{CONTENT_TYPE, NO_REPLY, Party, uid_copies},
        gating::method_error,
        imip,
        mail::{Traced, message_with},
        user_permissions,
    },
};
use calcard::icalendar::ICalendarMethod;
use common::{auth::oauth::scope::OAuthScopes, za::ZA_CALENDAR_KEYLESS};
use email::mailbox::INBOX_ID;
use futures::FutureExt;
use groupware::calendar::{
    pipeline::context::za_foreign_calendar_contexts,
    sharing::resolve::{ZaSyncBoundary, za_owned_calendar_context},
};
use hyper::StatusCode;
use jmap_proto::types::state::{JMAPIntermediateState, State, ZaCalendarOrigin};
use serde_json::{Value, json};
use std::{panic::AssertUnwindSafe, str::FromStr};
use store::{
    IterateParams, ValueKey,
    write::{BatchBuilder, LogCollection, ValueClass, serialize::RawValue},
};
use types::{
    collection::{Collection, SyncCollection},
    field::Field,
    id::Id,
};

const SLUG: &str = "jmapcal-shared";
const RENAMED: &str = "jmapcal-shared-renamed";
const HOME: &str = "/dav/cal/jmapcal-sharing%40example.com/";
const EVENTS: [&str; 2] = ["jmapcal-sharing-one", "jmapcal-sharing-two"];
const PLAIN: &str = "jmapcal-sharing-plain@example.com";
const PLAIN_FROM: &str = "jmapcal-sharing-plain-from@example.com";
const PLAIN_SLUG: &str = "jmapcal-plain";

struct Sharing {
    pair: Pair,
    reader: Account,
    reader_id: u32,
    reader_dav: DummyWebDavClient,
    shared: u32,
}

struct Ordinary {
    plain: Account,
    plain_dav: DummyWebDavClient,
    from: Account,
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar sharing tests...");
    let pair = pair(test, "sharing").await;
    let (reader, reader_id) = key_account(test, "sharing-reader", user_permissions()).await;
    let reader_dav = DummyWebDavClient::new(reader_id, reader.name(), STRONG, reader.name());
    let shared = make_calendar(test, &pair.key, &pair.dav, SLUG).await;
    for uid in EVENTS {
        put_event(test, &pair.key, &pair.dav, SLUG, uid, uid).await;
    }
    share(test, &pair.key, &reader, reader_id, SLUG, "view").await;
    let fixture = Sharing {
        pair,
        reader,
        reader_id,
        reader_dav,
        shared,
    };
    complete_boundary(test, &fixture).await;

    sync_boundary_matches_caldav(test, &fixture).await;
    grantee_state_above_the_boundary(test, &fixture).await;
    paged_sync_retention_collision(test, &fixture).await;
    initial_sync_not_raised_over_a_grant(test, &fixture).await;
    exact_sync_raised_over_a_container_change(test, &fixture).await;
    paged_sync_origin_survives_range_changes(test, &fixture).await;
    let ordinary = ordinary_accounts(test).await;
    paged_sync_legacy_and_ordinary(test, &fixture, &ordinary).await;
    calendar_state_slot_errors(test, &fixture, &ordinary).await;
    let admin = test.account("admin@example.com");
    admin.destroy_account(ordinary.plain).await;
    admin.destroy_account(ordinary.from).await;
    changes_store_errors_are_payload_free(test, &fixture).await;
    // Release 6 spec 6.1, 6.5, 6.7, 6.10 (Task 36): a grantee over JMAP.
    // Each case deletes what it creates and leaves the view grant.
    grantee_reads_and_writes_at_both_levels(test, &fixture).await;
    delegated_reply_carries_sent_by(test, &fixture).await;
    revoke_ends_access(test, &fixture).await;
    revoked_and_unshared_calendars_hidden(test, &fixture).await;
    non_grantee_refused(test, &fixture).await;
    grantee_default_alerts(test, &fixture).await;
    grantee_session_lists_the_sharer(&fixture).await;
    move_with_scheduling_into_shared(test, &fixture).await;
    grantee_destroy_sends_delegated_cancel(test, &fixture).await;
    grantee_held_to_effective_privacy(test, &fixture).await;
    grantee_answers_of_upstreams_checks(test, &fixture).await;

    // Each case leaves a completed boundary, the two setup events and the view grant.
    let owner = &fixture.pair.key;
    let revoked = za_post(
        "shares/revoke",
        &json!({
            "username": owner.name(), "password": STRONG,
            "calendar": SLUG, "address": fixture.reader.name()
        }),
    )
    .await
    .expect(200);
    assert_eq!(revoked["changed"], true);
    fixture
        .pair
        .dav
        .request_with_headers("DELETE", &home(SLUG), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    finish(test, "sharing").await;
}

fn home(slug: &str) -> String {
    format!("{HOME}{slug}/")
}

fn id(document_id: u32) -> String {
    Id::from(document_id).to_string()
}

fn token(change_id: u64) -> String {
    format!("urn:stalwart:davsync:{change_id:x}")
}

async fn changes(
    caller: &Account,
    owner: &Account,
    method: &str,
    since: &State,
    max: Option<usize>,
) -> JmapResponse {
    let mut args = json!({ "accountId": owner.id_string(), "sinceState": since.to_string() });
    if let Some(max) = max {
        args["maxChanges"] = json!(max);
    }
    caller.jmap_method_call(method, args).await
}

fn new_state(reply: &JmapResponse) -> State {
    assert_eq!(method_error(reply), None, "{reply:?}");
    reply.response_at(0)["newState"]
        .as_str()
        .and_then(State::parse)
        .unwrap_or_else(|| panic!("no parsable newState: {reply:?}"))
}

fn created(reply: &JmapResponse) -> Vec<String> {
    assert_eq!(method_error(reply), None, "{reply:?}");
    reply.response_at(0)["created"]
        .as_array()
        .unwrap_or_else(|| panic!("no created list: {reply:?}"))
        .iter()
        .map(|id| id.as_str().unwrap().to_string())
        .collect()
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

/// Follows a sync from `start` to its end and returns every newState in order.
async fn page_through(
    caller: &Account,
    owner: &Account,
    method: &str,
    start: State,
    max: usize,
) -> Vec<State> {
    let mut since = start;
    let mut states = Vec::new();
    for _ in 0..64 {
        let reply = changes(caller, owner, method, &since, Some(max)).await;
        let state = new_state(&reply);
        assert_eq!(
            reply.response_at(0)["oldState"],
            since.to_string(),
            "{reply:?}"
        );
        let more = reply.response_at(0)["hasMoreChanges"] == true;
        states.push(state.clone());
        if !more {
            return states;
        }
        since = state;
    }
    panic!("{method}: the sync did not end");
}

/// Every page but the last is a `z` continuation of `origin`; the last is exact.
fn assert_pages(states: &[State], origin: &ZaCalendarOrigin, what: &str) {
    let (last, pages) = states.split_last().unwrap();
    assert!(matches!(last, State::Exact(_)), "{what}: {states:?}");
    for state in pages {
        assert_eq!(za_parts(state).1, origin, "{what}: {states:?}");
    }
}

/// The status of CalDAV's sync-collection REPORT from `token`.
async fn sync_status(client: &DummyWebDavClient, path: &str, token: &str) -> StatusCode {
    let response = client
        .request(
            "REPORT",
            path,
            format!(
                "<D:sync-collection xmlns:D=\"DAV:\"><D:prop><D:getetag/></D:prop><D:sync-token>{token}</D:sync-token><D:sync-level>1</D:sync-level></D:sync-collection>"
            ),
        )
        .await;
    if response.status == StatusCode::FORBIDDEN {
        assert!(
            response
                .body
                .as_ref()
                .is_ok_and(|body| body.contains("valid-sync-token")),
            "{:?}",
            response.body
        );
    }
    response.status
}

/// The owner's keyed CalDAV request completes a pending boundary.
async fn complete_boundary(test: &TestServer, f: &Sharing) -> u64 {
    f.pair
        .dav
        .request("PROPFIND", &home(SLUG), "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    assert!(!owner_boundary(test, f.pair.key_id, f.shared).await.pending);
    let boundary = reader_boundary(test, f).await;
    assert!(!boundary.pending, "{boundary:?}");
    boundary.grant_change_id.max(boundary.rename_change_id)
}

/// The boundary of the reader's grant, as the reader's fresh read context sees it:
/// the owner's own context carries no grant change id.
async fn reader_boundary(test: &TestServer, f: &Sharing) -> ZaSyncBoundary {
    let token = session_token(test, f.reader_id).await;
    let contexts = za_foreign_calendar_contexts(
        &test.server,
        &token,
        f.pair.key_id,
        [f.shared].into_iter(),
        None,
    )
    .await
    .unwrap_or_else(|_| panic!("the reader's context of the shared calendar"));
    contexts[&f.shared].boundary.unwrap()
}

async fn rename(f: &Sharing, from: &str, to: &str) {
    let destination = home(to);
    f.pair
        .dav
        .request_with_headers(
            "MOVE",
            &home(from),
            [("destination", destination.as_str())],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
}

async fn delete_events(f: &Sharing, slug: &str, uids: &[&str]) {
    for uid in uids {
        f.pair
            .dav
            .request_with_headers(
                "DELETE",
                &format!("{}{uid}.ics", home(slug)),
                [NO_REPLY],
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}

/// The owner's Calendar change log from `from` on: each record's change id and
/// whether it is the empty marker log retention leaves.
async fn calendar_log(test: &TestServer, owner_id: u32, from: u64) -> Vec<(u64, bool)> {
    let collection = LogCollection::Sync(SyncCollection::Calendar);
    let mut records = Vec::new();
    test.server
        .store()
        .iterate(
            IterateParams::new(
                collection.log_key(owner_id, from),
                collection.log_key(owner_id, u64::MAX),
            )
            .ascending(),
            |key, value| {
                let change_id = u64::from_be_bytes(key[key.len() - 8..].try_into().unwrap());
                records.push((change_id, value.is_empty()));
                Ok(true)
            },
        )
        .await
        .unwrap();
    records
}

async fn sync_boundary_matches_caldav(test: &TestServer, f: &Sharing) {
    let (owner, reader) = (&f.pair.key, &f.reader);
    let path = home(SLUG);
    let b = complete_boundary(test, f).await;
    put_event(
        test,
        owner,
        &f.pair.dav,
        SLUG,
        "jmapcal-sharing-later",
        "later",
    )
    .await;
    let later = new_state(
        &changes(
            reader,
            owner,
            "CalendarEvent/changes",
            &State::Initial,
            None,
        )
        .await,
    )
    .get_change_id();
    assert!(later > b, "{later} {b}");

    // Completed: exact states at or below the boundary are refused, later ones pass.
    for (since, refused) in [(b - 1, true), (b, true), (later, false)] {
        for method in ["CalendarEvent/changes", "Calendar/changes"] {
            let reply = changes(reader, owner, method, &State::Exact(since), None).await;
            if refused {
                assert_refused(&reply, method);
            } else {
                assert_eq!(method_error(&reply), None, "{method}: {reply:?}");
            }
        }
        let expected = if refused {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::MULTI_STATUS
        };
        assert_eq!(
            sync_status(&f.reader_dav, &path, &token(since)).await,
            expected
        );
    }
    // The owner is never checked.
    for since in [b - 1, b] {
        for method in ["CalendarEvent/changes", "Calendar/changes"] {
            let reply = changes(owner, owner, method, &State::Exact(since), None).await;
            assert_eq!(method_error(&reply), None, "owner {method}: {reply:?}");
        }
    }
    assert_eq!(
        sync_status(&f.pair.dav, &path, &token(b)).await,
        StatusCode::MULTI_STATUS
    );
    // An initial paged sync passes; every continuation carries the Initial origin.
    let initial = ZaCalendarOrigin::Initial;
    let pages = page_through(reader, owner, "CalendarEvent/changes", State::Initial, 1).await;
    assert!(pages.len() >= 3, "{pages:?}");
    assert_pages(&pages, &initial, "completed CalendarEvent");
    let pages = page_through(reader, owner, "Calendar/changes", State::Initial, 1).await;
    assert_pages(&pages, &initial, "completed Calendar");

    // Pending: every exact state is refused, by both protocols; initial syncs pass.
    mark_boundary_pending(test, f.pair.key_id, f.shared).await;
    assert!(reader_boundary(test, f).await.pending);
    for method in ["CalendarEvent/changes", "Calendar/changes"] {
        let reply = changes(reader, owner, method, &State::Exact(later), None).await;
        assert_refused(&reply, method);
        let pages = page_through(reader, owner, method, State::Initial, 1).await;
        assert_pages(&pages, &initial, method);
    }
    assert_eq!(
        sync_status(&f.reader_dav, &path, &token(later)).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        sync_status(&f.reader_dav, &path, "").await,
        StatusCode::MULTI_STATUS
    );
    assert!(reader_boundary(test, f).await.pending);
    for since in [b, later] {
        for method in ["CalendarEvent/changes", "Calendar/changes"] {
            let reply = changes(owner, owner, method, &State::Exact(since), None).await;
            assert_eq!(method_error(&reply), None, "owner {method}: {reply:?}");
        }
    }

    // An exact page started after the boundary is refused once the boundary is
    // pending again, while the initial-origin page of the same range continues.
    let b = complete_boundary(test, f).await;
    put_event(
        test,
        owner,
        &f.pair.dav,
        SLUG,
        "jmapcal-sharing-start",
        "start",
    )
    .await;
    let start = new_state(
        &changes(
            reader,
            owner,
            "CalendarEvent/changes",
            &State::Initial,
            None,
        )
        .await,
    );
    assert!(start.get_change_id() > b);
    for uid in ["jmapcal-sharing-page-a", "jmapcal-sharing-page-b"] {
        put_event(test, owner, &f.pair.dav, SLUG, uid, uid).await;
    }
    let exact = new_state(&changes(reader, owner, "CalendarEvent/changes", &start, Some(1)).await);
    assert_eq!(
        za_parts(&exact).1,
        &ZaCalendarOrigin::Exact(start.get_change_id())
    );
    let first = new_state(
        &changes(
            reader,
            owner,
            "CalendarEvent/changes",
            &State::Initial,
            Some(1),
        )
        .await,
    );
    assert_eq!(za_parts(&first).1, &initial);
    mark_boundary_pending(test, f.pair.key_id, f.shared).await;
    assert_refused(
        &changes(reader, owner, "CalendarEvent/changes", &exact, Some(1)).await,
        "exact-origin page while pending",
    );
    assert_eq!(
        sync_status(&f.reader_dav, &path, &token(start.get_change_id())).await,
        StatusCode::FORBIDDEN
    );
    let rest = page_through(reader, owner, "CalendarEvent/changes", first, 1).await;
    assert_pages(&rest, &initial, "initial-origin page while pending");

    complete_boundary(test, f).await;
    delete_events(
        f,
        SLUG,
        &[
            "jmapcal-sharing-later",
            "jmapcal-sharing-start",
            "jmapcal-sharing-page-a",
            "jmapcal-sharing-page-b",
        ],
    )
    .await;
}

/// PR #90 review C4617611565: with no event written after a rename and its
/// completed boundary (both container changes), a grantee's event state from
/// `/changes`, `/get` and `/query` lies at or above the boundary, so the next
/// incremental call is served, and an `edit` grantee's `ifInState` matches it.
/// The owner's `/changes` state stays upstream's item state (only that state of
/// the owner is asserted).
async fn grantee_state_above_the_boundary(test: &TestServer, f: &Sharing) {
    const UID: &str = "jmapcal-sharing-quiet";
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    let method = "CalendarEvent/changes";
    share(test, owner, reader, f.reader_id, SLUG, "edit").await;
    complete_boundary(test, f).await;
    put_event(test, owner, &f.pair.dav, SLUG, UID, UID).await;
    let item = new_state(&changes(owner, owner, method, &State::Initial, None).await);
    rename(f, SLUG, RENAMED).await;
    rename(f, RENAMED, SLUG).await;
    let b = complete_boundary(test, f).await;
    assert!(b > item.get_change_id(), "{b} {item:?}");

    let s = new_state(&changes(reader, owner, method, &State::Initial, None).await);
    assert!(s.get_change_id() > b, "{s:?} {b}");
    assert_eq!(
        calendar_log(test, owner_id, s.get_change_id())
            .await
            .first(),
        Some(&(s.get_change_id(), false)),
        "the record of the grantee's state"
    );
    let reply = changes(reader, owner, method, &s, None).await;
    assert_eq!(new_state(&reply), s, "{reply:?}");
    for list in ["created", "updated", "destroyed"] {
        assert_eq!(reply.response_at(0)[list], json!([]), "{list}: {reply:?}");
    }

    let reply = reader
        .jmap_method_call(
            "CalendarEvent/get",
            json!({ "accountId": owner.id_string(), "ids": [] }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let got = State::parse(reply.state()).unwrap();
    assert_eq!(got, s, "the grantee's /get state");
    let reply = reader
        .jmap_method_call(
            "CalendarEvent/query",
            json!({ "accountId": owner.id_string() }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        reply.response_at(0)["queryState"]
            .as_str()
            .and_then(State::parse),
        Some(s.clone()),
        "the grantee's /query state"
    );
    let reply = changes(reader, owner, method, &got, None).await;
    assert_eq!(method_error(&reply), None, "from the /get state: {reply:?}");
    let reply = reader
        .jmap_method_call(
            "CalendarEvent/set",
            json!({ "accountId": owner.id_string(), "ifInState": s.to_string() }),
        )
        .await;
    assert_eq!(
        method_error(&reply),
        None,
        "the grantee's ifInState: {reply:?}"
    );

    // The owner's `/changes` state is upstream's item state, below the boundary.
    assert_eq!(
        new_state(&changes(owner, owner, method, &State::Initial, None).await),
        item
    );
    delete_events(f, SLUG, &[UID]).await;
    share(test, owner, reader, f.reader_id, SLUG, "view").await;
    complete_boundary(test, f).await;
}

/// The amendment's collision (b < m < f < g < c): after retention leaves an empty
/// marker at m, an initial and an exact(m) first page cover the same range.
async fn paged_sync_retention_collision(test: &TestServer, f: &Sharing) {
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    let b = complete_boundary(test, f).await;
    put_event(test, owner, &f.pair.dav, SLUG, "jmapcal-sharing-m", "m").await;
    let m = new_state(
        &changes(
            reader,
            owner,
            "CalendarEvent/changes",
            &State::Initial,
            None,
        )
        .await,
    )
    .get_change_id();
    assert!(b < m, "{b} {m}");
    let first = put_event(test, owner, &f.pair.dav, SLUG, "jmapcal-sharing-f", "f").await;
    let second = put_event(test, owner, &f.pair.dav, SLUG, "jmapcal-sharing-g", "g").await;
    let [(m_record, false), (f_id, false), (g_id, false)] =
        calendar_log(test, owner_id, m).await[..]
    else {
        panic!("expected the records m, f and g");
    };
    assert_eq!(m_record, m);
    assert!(m < f_id && f_id < g_id, "{m} {f_id} {g_id}");

    let marker = test
        .server
        .truncate_change_log(owner_id, SyncCollection::Calendar.into(), 2)
        .await
        .unwrap();
    assert_eq!(marker, Some(m));
    assert_eq!(
        calendar_log(test, owner_id, 0).await,
        [(m, true), (f_id, false), (g_id, false)]
    );

    let initial = changes(
        reader,
        owner,
        "CalendarEvent/changes",
        &State::Initial,
        Some(1),
    )
    .await;
    let exact = changes(
        reader,
        owner,
        "CalendarEvent/changes",
        &State::Exact(m),
        Some(1),
    )
    .await;
    for reply in [&initial, &exact] {
        assert_eq!(created(reply), [id(first)], "{reply:?}");
        assert_eq!(reply.response_at(0)["hasMoreChanges"], true, "{reply:?}");
    }
    let (initial, exact) = (new_state(&initial), new_state(&exact));
    let (initial_range, initial_origin) = za_parts(&initial);
    let (exact_range, exact_origin) = za_parts(&exact);
    let range = JMAPIntermediateState {
        from_id: f_id,
        to_id: g_id,
        items_sent: 1,
    };
    assert_eq!(initial_range, &range);
    assert_eq!(exact_range, &range);
    assert_eq!(initial_origin, &ZaCalendarOrigin::Initial);
    assert_eq!(exact_origin, &ZaCalendarOrigin::Exact(m));
    // The legacy form of both pages is the same string; the `z` forms differ.
    assert_eq!(
        State::Intermediate(range.clone()).to_string(),
        State::Intermediate(exact_range.clone()).to_string()
    );
    assert_ne!(initial.to_string(), exact.to_string());

    rename(f, SLUG, RENAMED).await;
    let boundary = reader_boundary(test, f).await;
    assert!(!boundary.pending);
    let c = boundary.rename_change_id;
    assert!(g_id < c, "{g_id} {c}");
    let served = changes(reader, owner, "CalendarEvent/changes", &initial, Some(1)).await;
    assert_eq!(created(&served), [id(second)], "{served:?}");
    // PR #90 review, fix round 2 (H1): the continuation of an initial-origin
    // sync is not raised, so its final state is the last item change `g`.
    assert_eq!(new_state(&served), State::Exact(g_id));
    assert_eq!(served.response_at(0)["oldState"], initial.to_string());
    assert_refused(
        &changes(reader, owner, "CalendarEvent/changes", &exact, Some(1)).await,
        "exact(m) continuation after the rename",
    );

    rename(f, RENAMED, SLUG).await;
    complete_boundary(test, f).await;
    delete_events(
        f,
        SLUG,
        &[
            "jmapcal-sharing-m",
            "jmapcal-sharing-f",
            "jmapcal-sharing-g",
        ],
    )
    .await;
}

/// PR #90 review, fix round 2 (H1): a grant that completes while an
/// initial-origin sync pages does not lift the sync's final state over it.
/// Calendar B is already shared with the module's second key account, so the
/// grant to the reader writes no event; B's event is older than A's. The final
/// state stays at or below the grant's boundary, the next call restarts, and a
/// fresh sync reports B's event.
async fn initial_sync_not_raised_over_a_grant(test: &TestServer, f: &Sharing) {
    const B: &str = "jmapcal-sharing-granted";
    const B1: &str = "jmapcal-sharing-b1";
    const A_EVENTS: [&str; 2] = ["jmapcal-sharing-a1", "jmapcal-sharing-a2"];
    let (owner, reader) = (&f.pair.key, &f.reader);
    let method = "CalendarEvent/changes";
    complete_boundary(test, f).await;
    let b = make_calendar(test, owner, &f.pair.dav, B).await;
    share(test, owner, &f.pair.key2, f.pair.key2_id, B, "view").await;
    let b1 = put_event(test, owner, &f.pair.dav, B, B1, B1).await;
    for uid in A_EVENTS {
        put_event(test, owner, &f.pair.dav, SLUG, uid, uid).await;
    }

    let first = new_state(&changes(reader, owner, method, &State::Initial, Some(1)).await);
    assert_eq!(za_parts(&first).1, &ZaCalendarOrigin::Initial);
    share(test, owner, reader, f.reader_id, B, "view").await;
    f.pair
        .dav
        .request("PROPFIND", &home(B), "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let token = session_token(test, f.reader_id).await;
    let contexts =
        za_foreign_calendar_contexts(&test.server, &token, f.pair.key_id, [b].into_iter(), None)
            .await
            .unwrap_or_else(|_| panic!("the reader's context of the granted calendar"));
    let boundary = contexts[&b].boundary.unwrap();
    assert!(!boundary.pending, "{boundary:?}");
    let grant = boundary.grant_change_id.max(boundary.rename_change_id);

    let pages = page_through(reader, owner, method, first, 1).await;
    let last = pages.last().unwrap().clone();
    assert!(matches!(last, State::Exact(_)), "{pages:?}");
    assert!(
        last.get_change_id() <= grant,
        "the final state {last:?} is above the grant's boundary {grant}"
    );
    assert_refused(
        &changes(reader, owner, method, &last, None).await,
        "the next call after the grant",
    );
    let fresh = changes(reader, owner, method, &State::Initial, None).await;
    assert!(
        created(&fresh).contains(&id(b1)),
        "a fresh sync omits the granted calendar's event: {fresh:?}"
    );

    for grantee in [reader.name(), f.pair.key2.name()] {
        za_post(
            "shares/revoke",
            &json!({
                "username": owner.name(), "password": STRONG,
                "calendar": B, "address": grantee
            }),
        )
        .await
        .expect(200);
    }
    delete_events(f, B, &[B1]).await;
    delete_events(f, SLUG, &A_EVENTS).await;
    f.pair
        .dav
        .request_with_headers("DELETE", &home(B), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// PR #90 review, fix round 2 (H1): the final page of an exact-origin sync is
/// raised to the log's tip past a container change that is not a boundary of
/// the reader (the owner creates a calendar it does not share), and the next
/// call from it is served and empty. The "an item change follows, so no
/// raise" branch of an exact origin is `paged_sync_origin_survives_range_changes`.
async fn exact_sync_raised_over_a_container_change(test: &TestServer, f: &Sharing) {
    const UNSHARED: &str = "jmapcal-sharing-unshared-later";
    const START: &str = "jmapcal-sharing-x0";
    const PAGED: [&str; 2] = ["jmapcal-sharing-y1", "jmapcal-sharing-y2"];
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    let method = "CalendarEvent/changes";
    let b = complete_boundary(test, f).await;
    put_event(test, owner, &f.pair.dav, SLUG, START, START).await;
    let s = new_state(&changes(reader, owner, method, &State::Initial, None).await);
    assert!(s.get_change_id() > b, "{s:?} {b}");
    let mut paged = Vec::new();
    for uid in PAGED {
        paged.push(put_event(test, owner, &f.pair.dav, SLUG, uid, uid).await);
    }
    let page1 = new_state(&changes(reader, owner, method, &s, Some(1)).await);
    assert_eq!(
        za_parts(&page1).1,
        &ZaCalendarOrigin::Exact(s.get_change_id())
    );
    make_calendar(test, owner, &f.pair.dav, UNSHARED).await;
    let tip = test
        .server
        .store()
        .get_last_change_id(owner_id, SyncCollection::Calendar.into())
        .await
        .unwrap()
        .unwrap();

    let reply = changes(reader, owner, method, &page1, Some(1)).await;
    assert_eq!(created(&reply), [id(paged[1])], "{reply:?}");
    assert_eq!(reply.response_at(0)["hasMoreChanges"], false, "{reply:?}");
    let last = new_state(&reply);
    assert_eq!(
        last,
        State::Exact(tip),
        "the final page is raised to the tip"
    );
    let reply = changes(reader, owner, method, &last, None).await;
    assert_eq!(new_state(&reply), last, "{reply:?}");
    for list in ["created", "updated", "destroyed"] {
        assert_eq!(reply.response_at(0)[list], json!([]), "{list}: {reply:?}");
    }

    f.pair
        .dav
        .request_with_headers("DELETE", &home(UNSHARED), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let mut uids = vec![START];
    uids.extend(PAGED);
    delete_events(f, SLUG, &uids).await;
}

async fn paged_sync_origin_survives_range_changes(test: &TestServer, f: &Sharing) {
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    // Calendar paging, by the owner, from an exact and from an initial start. It
    // runs before this case trims the log, which would retire the exact start.
    let calendars = "Calendar/changes";
    let t = new_state(&changes(owner, owner, calendars, &State::Initial, None).await);
    let slugs = [
        "jmapcal-sharing-k1",
        "jmapcal-sharing-k2",
        "jmapcal-sharing-k3",
        "jmapcal-sharing-k4",
    ];
    for slug in slugs {
        make_calendar(test, owner, &f.pair.dav, slug).await;
    }
    let pages = page_through(owner, owner, calendars, t.clone(), 1).await;
    assert!(pages.len() >= 4, "{pages:?}");
    assert_pages(
        &pages,
        &ZaCalendarOrigin::Exact(t.get_change_id()),
        "exact Calendar",
    );
    let ranges = pages[..pages.len() - 1]
        .iter()
        .map(|state| za_parts(state).0.clone())
        .collect::<Vec<_>>();
    assert!(
        ranges
            .windows(2)
            .all(|pair| pair[0].from_id == pair[1].from_id && pair[0].to_id == pair[1].to_id),
        "{ranges:?}"
    );
    let pages = page_through(owner, owner, calendars, State::Initial, 1).await;
    assert!(pages.len() >= 4, "{pages:?}");
    assert_pages(&pages, &ZaCalendarOrigin::Initial, "initial Calendar");
    for slug in slugs {
        f.pair
            .dav
            .request_with_headers("DELETE", &home(slug), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }

    let method = "CalendarEvent/changes";
    let b = complete_boundary(test, f).await;
    put_event(test, owner, &f.pair.dav, SLUG, "jmapcal-sharing-s", "s").await;
    let s = new_state(&changes(reader, owner, method, &State::Initial, None).await).get_change_id();
    assert!(s > b);
    let origin = ZaCalendarOrigin::Exact(s);

    // An insert and delete that fold away move the range's first ID past them.
    put_event(test, owner, &f.pair.dav, SLUG, "jmapcal-sharing-p", "p").await;
    delete_events(f, SLUG, &["jmapcal-sharing-p"]).await;
    let mut events = Vec::new();
    for uid in [
        "jmapcal-sharing-e1",
        "jmapcal-sharing-e2",
        "jmapcal-sharing-e3",
        "jmapcal-sharing-e4",
    ] {
        events.push(put_event(test, owner, &f.pair.dav, SLUG, uid, uid).await);
    }
    let after_s = calendar_log(test, owner_id, s + 1).await;
    let ids = after_s.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let [p_insert, p_delete, e1, _, _, e4] = ids[..] else {
        panic!("expected six records after s: {ids:?}");
    };
    assert!(s < p_insert);
    let range = |items_sent| JMAPIntermediateState {
        from_id: e1,
        to_id: e4,
        items_sent,
    };

    let reply = changes(reader, owner, method, &State::Exact(s), Some(1)).await;
    assert_eq!(created(&reply), [id(events[0])]);
    let page1 = new_state(&reply);
    assert_eq!(za_parts(&page1), (&range(1), &origin));

    // Writes after the range, with their own folding, change neither range nor origin.
    put_event(test, owner, &f.pair.dav, SLUG, "jmapcal-sharing-q", "q").await;
    delete_events(f, SLUG, &["jmapcal-sharing-q"]).await;
    let mut later = Vec::new();
    for uid in ["jmapcal-sharing-r1", "jmapcal-sharing-r2"] {
        later.push(put_event(test, owner, &f.pair.dav, SLUG, uid, uid).await);
    }
    let reply = changes(reader, owner, method, &page1, Some(1)).await;
    assert_eq!(created(&reply), [id(events[1])]);
    let page2 = new_state(&reply);
    assert_eq!(za_parts(&page2), (&range(2), &origin));

    // Trimming the log before the range keeps the continuation served.
    let marker = test
        .server
        .truncate_change_log(owner_id, SyncCollection::Calendar.into(), 8)
        .await
        .unwrap();
    assert_eq!(marker, Some(p_delete));
    let reply = changes(reader, owner, method, &page2, Some(1)).await;
    assert_eq!(created(&reply), [id(events[2])]);
    let page3 = new_state(&reply);
    assert_eq!(za_parts(&page3), (&range(3), &origin));

    // PR #90 review RA-1: the final page of the pinned range ends at the range's
    // last event change, since events were written after the range; the next call
    // from it reports them, and nothing is reported twice. `/queryChanges` takes
    // the same adapter and ends at the same state.
    let reply = changes(reader, owner, method, &page3, Some(1)).await;
    assert_eq!(created(&reply), [id(events[3])]);
    assert_eq!(reply.response_at(0)["hasMoreChanges"], false, "{reply:?}");
    let last = new_state(&reply);
    assert_eq!(last, State::Exact(e4), "the range-final page");
    let reply = reader
        .jmap_method_call(
            "CalendarEvent/queryChanges",
            json!({
                "accountId": owner.id_string(),
                "sinceQueryState": page3.to_string(),
                "maxChanges": 1,
            }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        reply.response_at(0)["newQueryState"]
            .as_str()
            .and_then(State::parse),
        Some(last.clone()),
        "the range-final /queryChanges state: {reply:?}"
    );
    let reply = changes(reader, owner, method, &last, None).await;
    let mut sent = created(&reply);
    sent.sort();
    let mut expected = later
        .iter()
        .map(|document_id| id(*document_id))
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(sent, expected, "the events written while paging: {reply:?}");
    let earlier = events
        .iter()
        .map(|document_id| id(*document_id))
        .collect::<Vec<_>>();
    for list in ["updated", "destroyed"] {
        let listed = reply.response_at(0)[list]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            listed.iter().all(|item| item
                .as_str()
                .is_some_and(|item| !earlier.contains(&item.to_string())
                    && !expected.contains(&item.to_string()))),
            "{list} repeats an event: {reply:?}"
        );
    }

    // Upstream's range-exhausted branch (items_sent at or above the range's total)
    // reads past the range: the new range's first ID moved past q's folded records,
    // and the origin is still the sync's original start.
    let after_e4 = calendar_log(test, owner_id, e4 + 1).await;
    let [_, _, (r1, false), (r2, false)] = after_e4[..] else {
        panic!("expected four records after e4: {after_e4:?}");
    };
    let exhausted = State::ZaCalendarIntermediate {
        range: range(1000),
        origin: origin.clone(),
    };
    let reply = changes(reader, owner, method, &exhausted, Some(1)).await;
    assert_eq!(created(&reply), [id(later[0])]);
    assert_eq!(
        za_parts(&new_state(&reply)),
        (
            &JMAPIntermediateState {
                from_id: r1,
                to_id: r2,
                items_sent: 1,
            },
            &origin
        )
    );

    // Trimming into the range keeps upstream's refusal: a retry of page 2's
    // continuation meets the marker inside its range.
    let marker = test
        .server
        .truncate_change_log(owner_id, SyncCollection::Calendar.into(), 7)
        .await
        .unwrap();
    assert_eq!(marker, Some(e1));
    let reply = changes(reader, owner, method, &page2, Some(1)).await;
    assert_refused(&reply, "continuation over a trimmed range");

    complete_boundary(test, f).await;
    delete_events(
        f,
        SLUG,
        &[
            "jmapcal-sharing-s",
            "jmapcal-sharing-e1",
            "jmapcal-sharing-e2",
            "jmapcal-sharing-e3",
            "jmapcal-sharing-e4",
            "jmapcal-sharing-r1",
            "jmapcal-sharing-r2",
        ],
    )
    .await;
}

async fn ordinary_accounts(test: &TestServer) -> Ordinary {
    let admin = test.account("admin@example.com");
    let plain = admin
        .create_user_account(
            PLAIN,
            STRONG,
            "jmapcal-sharing-plain",
            &[],
            user_permissions(),
        )
        .await;
    let from = admin
        .create_user_account(
            PLAIN_FROM,
            STRONG,
            "jmapcal-sharing-plain-from",
            &[],
            user_permissions(),
        )
        .await;
    let plain_id = plain.id().document_id();
    Ordinary {
        plain_dav: DummyWebDavClient::new(plain_id, plain.name(), STRONG, plain.name()),
        plain,
        from,
    }
}

async fn paged_sync_legacy_and_ordinary(test: &TestServer, f: &Sharing, o: &Ordinary) {
    let (owner, reader) = (&f.pair.key, &f.reader);
    let method = "CalendarEvent/changes";
    complete_boundary(test, f).await;

    // A grantee's legacy continuation has no origin: the client restarts from Initial.
    let first = new_state(&changes(reader, owner, method, &State::Initial, Some(1)).await);
    let legacy = State::Intermediate(za_parts(&first).0.clone());
    assert!(legacy.to_string().starts_with('r'));
    for method in ["CalendarEvent/changes", "Calendar/changes"] {
        assert_refused(
            &changes(reader, owner, method, &legacy, Some(1)).await,
            "grantee legacy continuation",
        );
    }
    let restart = page_through(reader, owner, method, State::Initial, 1).await;
    assert_pages(&restart, &ZaCalendarOrigin::Initial, "grantee restart");

    // A key owner's legacy chain stays legacy until it completes.
    put_event(
        test,
        owner,
        &f.pair.dav,
        "default",
        "jmapcal-sharing-l1",
        "l1",
    )
    .await;
    let t = new_state(&changes(owner, owner, method, &State::Initial, None).await);
    for uid in [
        "jmapcal-sharing-l2",
        "jmapcal-sharing-l3",
        "jmapcal-sharing-l4",
    ] {
        put_event(test, owner, &f.pair.dav, "default", uid, uid).await;
    }
    let first = new_state(&changes(owner, owner, method, &State::Initial, Some(1)).await);
    assert_eq!(za_parts(&first).1, &ZaCalendarOrigin::Initial);
    let legacy = State::Intermediate(za_parts(&first).0.clone());
    let chain = page_through(owner, owner, method, legacy, 1).await;
    let (last, pages) = chain.split_last().unwrap();
    assert!(matches!(last, State::Exact(_)), "{chain:?}");
    assert!(pages.len() >= 2, "{chain:?}");
    for state in pages {
        assert!(matches!(state, State::Intermediate(_)), "{chain:?}");
        assert!(state.to_string().starts_with('r'));
    }
    // Its next paged syncs carry an explicit origin.
    for (start, origin) in [
        (State::Initial, ZaCalendarOrigin::Initial),
        (t.clone(), ZaCalendarOrigin::Exact(t.get_change_id())),
    ] {
        let pages = page_through(owner, owner, method, start, 1).await;
        assert!(pages.len() >= 2, "{pages:?}");
        assert_pages(&pages, &origin, "owner after the legacy chain");
    }
    let home_default = "/dav/cal/jmapcal-sharing%40example.com/default/";
    for uid in [
        "jmapcal-sharing-l1",
        "jmapcal-sharing-l2",
        "jmapcal-sharing-l3",
        "jmapcal-sharing-l4",
    ] {
        f.pair
            .dav
            .request_with_headers(
                "DELETE",
                &format!("{home_default}{uid}.ics"),
                [NO_REPLY],
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }

    // An ordinary account keeps upstream's states, byte for byte.
    let plain = &o.plain;
    make_calendar(test, plain, &o.plain_dav, PLAIN_SLUG).await;
    put_event(
        test,
        plain,
        &o.plain_dav,
        PLAIN_SLUG,
        "jmapcal-plain-1",
        "1",
    )
    .await;
    let t = new_state(&changes(plain, plain, method, &State::Initial, None).await);
    assert!(matches!(t, State::Exact(_)));
    for uid in ["jmapcal-plain-2", "jmapcal-plain-3", "jmapcal-plain-4"] {
        put_event(test, plain, &o.plain_dav, PLAIN_SLUG, uid, uid).await;
    }
    for start in [State::Initial, t.clone()] {
        let mut since = start;
        let mut pages = 0;
        loop {
            let reply = changes(plain, plain, method, &since, Some(1)).await;
            let encoded = reply.response_at(0)["newState"]
                .as_str()
                .unwrap()
                .to_string();
            let state = new_state(&reply);
            assert_eq!(state.to_string(), encoded);
            if reply.response_at(0)["hasMoreChanges"] != true {
                assert!(matches!(state, State::Exact(_)), "{state:?}");
                break;
            }
            let State::Intermediate(range) = &state else {
                panic!("ordinary continuation {encoded}");
            };
            assert!(encoded.starts_with('r'));
            assert_eq!(
                encoded,
                State::new_intermediate(range.from_id, range.to_id, range.items_sent).to_string()
            );
            pages += 1;
            since = state;
        }
        assert!(pages >= 2, "{pages}");
    }
    for origin in [
        ZaCalendarOrigin::Initial,
        ZaCalendarOrigin::Exact(t.get_change_id()),
    ] {
        let state = State::ZaCalendarIntermediate {
            range: JMAPIntermediateState {
                from_id: t.get_change_id(),
                to_id: t.get_change_id(),
                items_sent: 1,
            },
            origin,
        };
        for method in ["CalendarEvent/changes", "Calendar/changes"] {
            let reply = changes(plain, plain, method, &state, Some(1)).await;
            assert_eq!(
                method_error(&reply),
                Some("invalidArguments"),
                "{method}: {reply:?}"
            );
        }
    }
    o.plain_dav
        .request_with_headers(
            "DELETE",
            &format!("/dav/cal/jmapcal-sharing-plain%40example.com/{PLAIN_SLUG}/"),
            [NO_REPLY],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn calendar_state_slot_errors(test: &TestServer, f: &Sharing, o: &Ordinary) {
    let owner = &f.pair.key;
    let range = JMAPIntermediateState {
        from_id: 42,
        to_id: 50,
        items_sent: 3,
    };
    let za_states = [ZaCalendarOrigin::Initial, ZaCalendarOrigin::Exact(41)].map(|origin| {
        State::ZaCalendarIntermediate {
            range: range.clone(),
            origin,
        }
        .to_string()
    });

    // Non-calendar state slots, on the key owner with its keys.
    for state in &za_states {
        for (method, slot) in [
            ("Email/changes", "sinceState"),
            ("Mailbox/changes", "sinceState"),
            ("Thread/changes", "sinceState"),
            ("Identity/changes", "sinceState"),
            ("AddressBook/changes", "sinceState"),
            ("ContactCard/changes", "sinceState"),
            ("ParticipantIdentity/changes", "sinceState"),
            ("CalendarEventNotification/changes", "sinceState"),
            ("Email/queryChanges", "sinceQueryState"),
            ("Mailbox/queryChanges", "sinceQueryState"),
            ("CalendarEventNotification/queryChanges", "sinceQueryState"),
        ] {
            let reply = owner
                .jmap_method_call(
                    method,
                    json!({ "accountId": owner.id_string(), slot: state }),
                )
                .await;
            assert_eq!(
                method_error(&reply),
                Some("invalidArguments"),
                "{method}: {reply:?}"
            );
        }
        // An ordinary account's calendar slots.
        for (method, slot) in [
            ("Calendar/changes", "sinceState"),
            ("CalendarEvent/changes", "sinceState"),
            ("CalendarEvent/queryChanges", "sinceQueryState"),
        ] {
            let reply = o
                .plain
                .jmap_method_call(
                    method,
                    json!({ "accountId": o.plain.id_string(), slot: state }),
                )
                .await;
            assert_eq!(
                method_error(&reply),
                Some("invalidArguments"),
                "plain {method}: {reply:?}"
            );
        }
        // Admission answers first: a caller without access and a keyless token.
        for method in ["Calendar/changes", "CalendarEvent/changes"] {
            let reply = f
                .pair
                .key2
                .jmap_method_call(
                    method,
                    json!({ "accountId": owner.id_string(), "sinceState": state }),
                )
                .await;
            assert_eq!(
                method_error(&reply),
                Some("accountNotFound"),
                "{method}: {reply:?}"
            );
        }
        let (grant, keyless) = za_grant_token(&test.server, owner.name(), OAuthScopes::ALL).await;
        let (status, reply) = za_bearer_jmap(
            &keyless,
            CALENDAR_JMAP_USING,
            json!([
                ["Calendar/changes", { "accountId": owner.id_string(), "sinceState": state }, "c0"],
                ["CalendarEvent/changes", { "accountId": owner.id_string(), "sinceState": state }, "c1"]
            ]),
        )
        .await;
        za_revoke_grant(&test.server, grant).await;
        assert_eq!(status, 200, "{reply:?}");
        for (index, call_id) in ["c0", "c1"].into_iter().enumerate() {
            assert_eq!(
                reply.0["methodResponses"][index],
                json!(["error", { "type": "forbidden", "description": ZA_CALENDAR_KEYLESS }, call_id])
            );
        }
    }

    precondition_matrix(test, o, &za_states).await;
}

/// Spec 6.8: `z` in a precondition slot is a state mismatch, as an invalid string is.
async fn precondition_matrix(_test: &TestServer, o: &Ordinary, za_states: &[String; 2]) {
    let (plain, from) = (&o.plain, &o.from);
    let malformed = format!("z{}", &za_states[1][1..].replace('a', "b"));
    assert_eq!(State::parse(&malformed), None, "{malformed}");
    let mut values = za_states.to_vec();
    values.push(malformed);
    let control = "invalid-state".to_string();

    // The target calendar, shared with the source account, and the source event.
    let target = plain
        .jmap_create(
            "Calendar",
            [json!({ "name": "jmapcal sharing target" })],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created(0)
        .id()
        .to_string();
    plain
        .jmap_update(
            "Calendar",
            [(
                &target,
                json!({ "shareWith": { from.id_string(): {
                    "mayReadItems": true, "mayWriteAll": true, "mayDelete": true
                } } }),
            )],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .updated(&target);
    let source_calendar = from
        .jmap_create(
            "Calendar",
            [json!({ "name": "jmapcal sharing source" })],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created(0)
        .id()
        .to_string();
    let event = |title: &str, calendar: &str| {
        json!({
            "@type": "Event",
            "uid": format!("{title}@jmapcal-sharing"),
            "title": title,
            "start": "2099-03-01T10:00:00",
            "duration": "PT1H",
            "calendarIds": { calendar: true },
        })
    };
    let source = from
        .jmap_create(
            "CalendarEvent",
            [event("jmapcal-sharing-source", &source_calendar)],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created(0)
        .id()
        .to_string();
    let events_of = |account: &Account| {
        let account = account.clone();
        async move {
            let reply = account
                .jmap_method_call(
                    "CalendarEvent/get",
                    json!({ "accountId": account.id_string(), "ids": null }),
                )
                .await;
            reply
                .list()
                .iter()
                .map(|event| event["id"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        }
    };
    let blob = plain
        .jmap_client()
        .await
        .upload(
            None,
            message_with("jmapcal sharing import", "body").into_bytes(),
            None,
        )
        .await
        .unwrap()
        .take_blob_id();
    let inbox = Id::from(INBOX_ID).to_string();
    let target_before = events_of(plain).await;
    let source_before = events_of(from).await;
    assert_eq!(source_before, [source.clone()]);

    for value in values.iter().chain([&control]) {
        let mismatch = |reply: &JmapResponse, what: &str| {
            assert_eq!(
                method_error(reply),
                Some("stateMismatch"),
                "{what} {value}: {reply:?}"
            );
        };
        let reply = plain
            .jmap_method_call(
                "CalendarEvent/set",
                json!({
                    "accountId": plain.id_string(), "ifInState": value,
                    "create": { "e": event("jmapcal-sharing-set", &target) }
                }),
            )
            .await;
        mismatch(&reply, "CalendarEvent/set ifInState");
        let reply = plain
            .jmap_method_call(
                "Email/import",
                json!({
                    "accountId": plain.id_string(), "ifInState": value,
                    "emails": { "i": { "blobId": blob, "mailboxIds": { &inbox: true } } }
                }),
            )
            .await;
        mismatch(&reply, "Email/import ifInState");
        for slot in ["ifInState", "ifFromInState"] {
            let reply = from
                .jmap_method_call(
                    "CalendarEvent/copy",
                    json!({
                        "fromAccountId": from.id_string(), "accountId": plain.id_string(),
                        slot: value,
                        "create": { &source: { "id": &source, "calendarIds": { &target: true } } }
                    }),
                )
                .await;
            mismatch(&reply, slot);
        }
        assert_eq!(events_of(plain).await, target_before, "{value}");
        let emails = plain
            .jmap_method_call("Email/query", json!({ "accountId": plain.id_string() }))
            .await;
        assert_eq!(emails.response_at(0)["ids"], json!([]), "{value}");

        // destroyFromIfInState is checked by the generated follow-up /set, after
        // the copy succeeded: the copy stays and the source is kept.
        let reply = from
            .jmap_method_call(
                "CalendarEvent/copy",
                json!({
                    "fromAccountId": from.id_string(), "accountId": plain.id_string(),
                    "onSuccessDestroyOriginal": true, "destroyFromIfInState": value,
                    "create": { &source: { "id": &source, "calendarIds": { &target: true } } }
                }),
            )
            .await;
        assert_eq!(method_error(&reply), None, "{value}: {reply:?}");
        let copy = reply.response_at(0)["created"][&source]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{value}: no copy in {reply:?}"))
            .to_string();
        assert_eq!(
            reply.0["methodResponses"][1][0], "error",
            "{value}: {reply:?}"
        );
        assert_eq!(
            reply.0["methodResponses"][1][1]["type"], "stateMismatch",
            "{value}: {reply:?}"
        );
        assert_eq!(events_of(from).await, source_before, "{value}");
        let mut expected = target_before.clone();
        expected.push(copy.clone());
        let mut present = events_of(plain).await;
        present.sort();
        expected.sort();
        assert_eq!(present, expected, "{value}");
        let destroyed = plain
            .jmap_destroy("CalendarEvent", [copy.as_str()], Vec::<(&str, &str)>::new())
            .await;
        assert_eq!(destroyed.response_at(0)["destroyed"], json!([copy]));
        assert_eq!(events_of(plain).await, target_before, "{value}");
    }
}

async fn changes_store_errors_are_payload_free(test: &TestServer, f: &Sharing) {
    const CANARY: &[u8] = b"jmapcal synthetic sharing calendar canary\0";
    const DETAILS: &str = "zero-access: calendar read failed";
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    let b = complete_boundary(test, f).await;
    put_event(
        test,
        owner,
        &f.pair.dav,
        SLUG,
        "jmapcal-sharing-warm",
        "warm",
    )
    .await;
    let since = new_state(
        &changes(
            reader,
            owner,
            "CalendarEvent/changes",
            &State::Initial,
            None,
        )
        .await,
    );
    assert!(since.get_change_id() > b);
    // A successful exact-origin call warms the caches.
    let methods = ["CalendarEvent/changes", "Calendar/changes"];
    for method in methods {
        let reply = changes(reader, owner, method, &since, None).await;
        assert_eq!(method_error(&reply), None, "{method}: {reply:?}");
    }
    let (grant, keyless) = za_grant_token(&test.server, owner.name(), OAuthScopes::ALL).await;
    test.wait_for_tasks().await;

    let key = || ValueKey {
        account_id: owner_id,
        collection: Collection::Calendar.into(),
        document_id: f.shared,
        class: ValueClass::Property(Field::ARCHIVE.into()),
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
            .with_account_id(owner_id)
            .with_collection(Collection::Calendar)
            .with_document(f.shared)
            .set(ValueClass::Property(Field::ARCHIVE.into()), bytes);
        test.server.store().write_batch(&mut batch).await.unwrap();
    };

    let traced = Traced::start(trc::EventType::variants());
    // The NUL is not an archive marker. No production data is part of the canary.
    write(CANARY.to_vec()).await;
    let failed = AssertUnwindSafe(async {
        let mut keyed = Vec::new();
        for method in methods {
            keyed.push(changes(reader, owner, method, &since, None).await);
        }
        let keyless = za_bearer_jmap(
            &keyless,
            CALENDAR_JMAP_USING,
            json!(methods.map(|method| json!([method, {
                "accountId": owner.id_string(), "sinceState": since.to_string()
            }, method]))),
        )
        .await;
        (keyed, keyless)
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
    let events = traced.finish().await;
    za_revoke_grant(&test.server, grant).await;
    assert!(original == restored, "original archive bytes not restored");
    for method in methods {
        let reply = changes(reader, owner, method, &since, None).await;
        assert_eq!(method_error(&reply), None, "restored {method}: {reply:?}");
    }
    delete_events(f, SLUG, &["jmapcal-sharing-warm"]).await;

    let (keyed, (status, keyless)) = failed.expect("completed changes requests");
    for (method, reply) in methods.iter().zip(&keyed) {
        assert_eq!(
            method_error(reply),
            Some("serverUnavailable"),
            "{method}: {reply:?}"
        );
        assert_eq!(
            reply.response_at(0)["description"],
            "This server is temporarily unavailable. Attempting this same operation later may succeed."
        );
    }
    assert_eq!(status, 200);
    for (index, method) in methods.into_iter().enumerate() {
        assert_eq!(
            keyless.0["methodResponses"][index],
            json!(["error", { "type": "forbidden", "description": ZA_CALENDAR_KEYLESS }, method])
        );
    }

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
                && event.account_id == Some(u64::from(owner_id))
        })
        .count();
    assert!(
        rebuilt >= 2,
        "no rebuilt Store error was traced ({rebuilt})"
    );
    let leaked = events
        .iter()
        .filter(|event| event.keys.iter().any(|(_, value)| holds(value, CANARY)))
        .count();
    assert_eq!(leaked, 0, "events holding the synthetic canary");
}

// Release 6 spec 6.1, 6.5, 6.7, 6.10 and 9.2 (Task 36): a grantee using a
// shared key-account calendar over JMAP, with its own keyed session.

const ORG: &str = "jmapcal-sharing-org@remote.test";
const READER: &str = "jmapcal-sharing-reader@example.com";
const ALERTS_SLUG: &str = "jmapcal-alerts";
const MODIFY: &str = "You are not allowed to modify this event.";
const SHARE_TEXT: &str = "Calendar sharing is managed on the account page.";

fn no_args() -> Vec<(&'static str, Value)> {
    Vec::new()
}

/// `sendSchedulingMessages: true`.
fn send() -> Vec<(&'static str, Value)> {
    vec![("sendSchedulingMessages", Value::Bool(true))]
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

fn alert(action: &str, offset: &str) -> Value {
    json!({
        "@type": "Alert",
        "action": action,
        "trigger": { "@type": "OffsetTrigger", "relativeTo": "start", "offset": offset },
    })
}

fn set_error(value: &Value) -> (&str, Option<&str>) {
    (
        value["type"].as_str().unwrap_or_default(),
        value["description"].as_str(),
    )
}

/// Whether an unfolded `TRIGGER` line of `body` has the value `offset`,
/// whatever its parameters: a JMAP alert sent with `relativeTo: "start"`
/// is written `TRIGGER;RELATED=START:<offset>`, a default alert
/// `TRIGGER:<offset>`.
fn has_trigger(body: &str, offset: &str) -> bool {
    body.replace("\r\n ", "")
        .lines()
        .map(str::trim_end)
        .any(|line| {
            (line.starts_with("TRIGGER:") || line.starts_with("TRIGGER;"))
                && line.rsplit(':').next() == Some(offset)
        })
}

fn alert_ids(event: &Value) -> Vec<String> {
    event["alerts"]
        .as_object()
        .map(|alerts| alerts.keys().cloned().collect())
        .unwrap_or_default()
}

fn assert_set(reply: &JmapResponse, outcome: &str, id: &str, what: &str) {
    assert_eq!(method_error(reply), None, "{what}: {reply:?}");
    assert!(
        reply
            .pointer(&format!("/methodResponses/0/1/{outcome}/{id}"))
            .is_some(),
        "{what}: {id} not {outcome}: {reply:?}"
    );
}

/// The one event `caller` reads with `CalendarEvent/get` in `owner`'s account.
async fn event_get(caller: &Account, owner: &Account, id: &str) -> Value {
    let reply = caller
        .jmap_method_call(
            "CalendarEvent/get",
            json!({ "accountId": owner.id_string(), "ids": [id] }),
        )
        .await;
    let [event] = reply.list() else {
        panic!("expected event {id}: {reply:?}");
    };
    event.clone()
}

/// The owner's CalDAV body of its one event with `uid`.
async fn owner_body(test: &TestServer, f: &Sharing, uid: &str) -> String {
    body(&f.pair.dav, &copy_href(test, f.pair.key_id, uid).await).await
}

/// The owner's CalDAV deletion of its one event with `uid`, without scheduling.
async fn owner_delete(test: &TestServer, f: &Sharing, uid: &str) {
    let href = copy_href(test, f.pair.key_id, uid).await;
    f.pair
        .dav
        .request_with_headers("DELETE", &href, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

async fn calendar_update(
    caller: &Account,
    owner: &Account,
    id: &str,
    patch: Value,
) -> JmapResponse {
    caller
        .jmap_method_call(
            "Calendar/set",
            json!({ "accountId": owner.id_string(), "update": { id: patch } }),
        )
        .await
}

/// The methods a key account's calendars are served by, each naming `account`.
fn served_calls(account: &str, calendar: &str, event: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("Calendar/get", json!({ "accountId": account, "ids": null })),
        ("Calendar/query", json!({ "accountId": account })),
        (
            "Calendar/changes",
            json!({ "accountId": account, "sinceState": "n" }),
        ),
        (
            "Calendar/set",
            json!({ "accountId": account, "update": { calendar: { "sortOrder": 1 } } }),
        ),
        (
            "CalendarEvent/get",
            json!({ "accountId": account, "ids": null }),
        ),
        ("CalendarEvent/query", json!({ "accountId": account })),
        (
            "CalendarEvent/changes",
            json!({ "accountId": account, "sinceState": "n" }),
        ),
        (
            "CalendarEvent/set",
            json!({ "accountId": account, "update": { event: { "title": "jmapcal-sharing refused" } } }),
        ),
    ]
}

/// Spec 6.1: a caller the method's access check does not admit gets
/// upstream's `accountNotFound` from every served method.
async fn assert_account_not_found(
    caller: &Account,
    owner: &Account,
    calendar: &str,
    event: &str,
    what: &str,
) {
    for (method, args) in served_calls(owner.id_string(), calendar, event) {
        let reply = caller.jmap_method_call(method, args).await;
        assert_eq!(
            method_error(&reply),
            Some("accountNotFound"),
            "{what}: {method}: {reply:?}"
        );
    }
}

/// Spec 6.5 (D10), 6.7: at `view` the grantee reads and every write is
/// refused; at `edit` it writes the event itself, alerts, keywords and
/// colour included, and no per-user copy is stored.
async fn grantee_reads_and_writes_at_both_levels(test: &TestServer, f: &Sharing) {
    const EDIT: &str = "jmapcal-sharing-edit";
    const RENAMED_TITLE: &str = "jmapcal-sharing-edit renamed";
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    let cal = id(f.shared);
    let seed_id = resource_id(test, owner_id, &format!("{SLUG}/{}.ics", EVENTS[0])).await;
    let seed = id(seed_id);
    let seed_href = format!("{}{}.ics", home(SLUG), EVENTS[0]);
    let seed_before = body(&f.pair.dav, &seed_href).await;

    // View.
    let reply = reader
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": owner.id_string(), "ids": null }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let [entry] = reply.list() else {
        panic!("grantee_reads_and_writes_at_both_levels: the view list: {reply:?}");
    };
    assert_eq!(entry["id"], cal.as_str(), "{entry}");
    assert_eq!(entry["myRights"]["mayReadItems"], true, "{entry}");
    assert_eq!(entry["myRights"]["mayWriteAll"], false, "{entry}");
    assert_eq!(
        event_get(reader, owner, &seed).await["title"],
        EVENTS[0],
        "grantee_reads_and_writes_at_both_levels: the seed's title"
    );

    let reply = reader
        .jmap_create_account(
            owner,
            "CalendarEvent",
            [event_in("jmapcal-sharing-view-create", &cal)],
            no_args(),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(set_error(reply.not_created(0)).0, "forbidden", "{reply:?}");
    assert_eq!(
        uid_copies(test, owner_id, "jmapcal-sharing-view-create").await,
        0
    );
    let reply = reader
        .jmap_update_account(
            owner,
            "CalendarEvent",
            [(
                seed.clone(),
                json!({ "title": "jmapcal-sharing not allowed" }),
            )],
            no_args(),
        )
        .await;
    assert_eq!(
        set_error(reply.not_updated(&seed)),
        ("forbidden", Some(MODIFY)),
        "{reply:?}"
    );
    let reply = reader
        .jmap_update_account(
            owner,
            "CalendarEvent",
            [(
                seed.clone(),
                json!({ "alerts": { "a1": alert("display", "-PT5M") } }),
            )],
            no_args(),
        )
        .await;
    assert_eq!(
        set_error(reply.not_updated(&seed)).0,
        "forbidden",
        "{reply:?}"
    );
    let reply = reader
        .jmap_destroy_account(owner, "CalendarEvent", [seed.as_str()], no_args())
        .await;
    let remove = format!("You are not allowed to remove events from calendar {cal}.");
    assert_eq!(
        set_error(reply.not_destroyed(&seed)),
        ("forbidden", Some(remove.as_str())),
        "{reply:?}"
    );
    let key2 = f.pair.key2.id_string().to_string();
    let reply = calendar_update(
        reader,
        owner,
        &cal,
        json!({ "shareWith": { &key2: { "mayReadItems": true } } }),
    )
    .await;
    assert_eq!(
        set_error(reply.not_updated(&cal)),
        ("forbidden", Some(SHARE_TEXT)),
        "{reply:?}"
    );
    assert_eq!(
        body(&f.pair.dav, &seed_href).await,
        seed_before,
        "grantee_reads_and_writes_at_both_levels: a refused write changed the seed"
    );

    // Edit.
    share(test, owner, reader, f.reader_id, SLUG, "edit").await;
    let mut object = event_in(EDIT, &cal);
    object["alerts"] = json!({ "a1": alert("display", "-PT5M") });
    object["keywords"] = json!({ "kw": true });
    object["color"] = json!("#aabbcc");
    let reply = reader
        .jmap_create_account(owner, "CalendarEvent", [object], no_args())
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let document_id = reply.created_id(0).document_id();
    let event_id = id(document_id);
    assert_no_event_preferences(test, owner_id, document_id).await;
    let owned = owner_body(test, f, EDIT).await;
    assert!(
        owned.contains("BEGIN:VALARM") && has_trigger(&owned, "-PT5M"),
        "{owned}"
    );
    let event = event_get(owner, owner, &event_id).await;
    assert_eq!(alert_ids(&event), ["a1"], "{event}");
    assert_eq!(event["alerts"]["a1"]["action"], "display", "{event}");
    assert_eq!(
        event["alerts"]["a1"]["trigger"]["offset"], "-PT5M",
        "{event}"
    );
    assert_eq!(event["keywords"], json!({ "kw": true }), "{event}");
    assert_eq!(event["color"], "#aabbcc", "{event}");

    let reply = reader
        .jmap_update_account(
            owner,
            "CalendarEvent",
            [(event_id.clone(), json!({ "title": RENAMED_TITLE }))],
            no_args(),
        )
        .await;
    assert_set(&reply, "updated", &event_id, "the grantee's title update");
    let owned = owner_body(test, f, EDIT).await;
    assert!(
        owned.contains(&format!("SUMMARY:{RENAMED_TITLE}")),
        "{owned}"
    );
    let href = copy_href(test, owner_id, EDIT).await;
    let reply = reader
        .jmap_destroy_account(owner, "CalendarEvent", [event_id.as_str()], no_args())
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        reply.destroyed().collect::<Vec<_>>(),
        [event_id.as_str()],
        "{reply:?}"
    );
    f.pair
        .dav
        .request("GET", &href, "")
        .await
        .with_status(StatusCode::NOT_FOUND);
    assert_eq!(uid_copies(test, owner_id, EDIT).await, 0);
    share(test, owner, reader, f.reader_id, SLUG, "view").await;
}

/// Spec 6.5, release 4 D12: a grantee at `edit` answers for the owner; the
/// REPLY to the outside organizer carries the grantee's SENT-BY.
async fn delegated_reply_carries_sent_by(test: &TestServer, f: &Sharing) {
    const UID: &str = "jmapcal-sharing-delegated";
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    let address = owner.name();
    share(test, owner, reader, f.reader_id, SLUG, "edit").await;

    // The owner's attendee copy is a scheduled object (a schedule tag): an
    // event the owner PUT itself would not be, and an attendee's change to
    // it would send nothing (`za_schedule_update` treats it as a create).
    // So the organizer's REQUEST is delivered by mail and imported by the
    // owner's drain, as `scheduling::invitation_by_email_found_by_uid`
    // does, and the owner then moves the copy into the shared calendar.
    let owner_start = inbox_ids(test, owner_id).await;
    assert!(pending_ids(test, owner_id).await.is_empty());
    let card = imip::add_contact(&f.pair.dav, address, ORG).await;
    sink().clear();
    // The envelope sender passes the test-mode DMARC check by its prefix.
    imip::deliver(
        &format!("dmarc-{ORG}"),
        address,
        &imip::itip_mail(
            ORG,
            address,
            "REQUEST",
            &imip::event(UID, ORG, &[address], 0, ""),
        ),
    )
    .await;
    wait_for_queue(test).await;
    {
        let _drain = DrainOn::new();
        assert_eq!(trigger_drain(address, STRONG).await, 207);
    }
    assert_eq!(uid_copies(test, owner_id, UID).await, 1);
    let event_id = Id::from(
        test.server
            .document_ids_matching(
                owner_id,
                Collection::CalendarEvent,
                types::field::CalendarEventField::Uid,
                UID.as_bytes(),
            )
            .await
            .unwrap()
            .min()
            .unwrap(),
    )
    .to_string();
    let default = default_calendar(owner).await;
    assert_ne!(default, id(f.shared));
    assert_eq!(
        event_get(owner, owner, &event_id).await["calendarIds"],
        json!({ (default.as_str()): true }),
        "delegated_reply_carries_sent_by: the imported copy's placement"
    );
    let reply = owner
        .jmap_update(
            "CalendarEvent",
            [(
                event_id.clone(),
                json!({ "calendarIds": { (id(f.shared)): true } }),
            )],
            no_args(),
        )
        .await;
    assert_set(
        &reply,
        "updated",
        &event_id,
        "the owner's move into the shared calendar",
    );
    assert_eq!(
        event_get(owner, owner, &event_id).await["calendarIds"],
        json!({ (id(f.shared)): true })
    );
    wait_for_queue(test).await;
    sink().clear();

    let own = format!("mailto:{}", owner.name());
    let event = event_get(reader, owner, &event_id).await;
    let participant = event["participants"]
        .as_object()
        .and_then(|participants| {
            participants
                .iter()
                .find(|(_, value)| {
                    value["calendarAddress"]
                        .as_str()
                        .is_some_and(|address| address.eq_ignore_ascii_case(&own))
                })
                .map(|(key, _)| key.clone())
        })
        .unwrap_or_else(|| {
            panic!("delegated_reply_carries_sent_by: no owner participant: {event}")
        });
    let path = format!("participants/{participant}/participationStatus");
    let reply = reader
        .jmap_update_account(
            owner,
            "CalendarEvent",
            [(event_id.clone(), json!({ path: "accepted" }))],
            send(),
        )
        .await;
    assert_set(&reply, "updated", &event_id, "the grantee's answer");
    wait_for_queue(test).await;
    let mail = sink().take_to(ORG);
    assert_eq!(
        mail.len(),
        1,
        "delegated_reply_carries_sent_by: mail to the organizer"
    );
    assert_eq!(mail[0].calendar_method().as_deref(), Some("REPLY"));
    let calendar = mail[0]
        .calendar_part()
        .unwrap_or_default()
        .replace("\r\n ", "");
    let sent_by = format!("sent-by=\"mailto:{READER}\"");
    assert!(
        calendar.lines().any(|line| {
            let line = line.to_ascii_lowercase();
            line.starts_with("attendee")
                && line.contains("partstat=accepted")
                && line.contains(&sent_by)
                && line.contains(&own.to_ascii_lowercase())
        }),
        "delegated_reply_carries_sent_by: the reply's ATTENDEE line: {calendar}"
    );

    // What the case left: the event, the contact, the delivered message and
    // the inbox item; the deletion sends nothing.
    owner_delete(test, f, UID).await;
    assert_eq!(uid_copies(test, owner_id, UID).await, 0);
    imip::delete_contact(&f.pair.dav, &card).await;
    let account = owner.id_string();
    let reply = owner
        .jmap_method_call(
            "Email/get",
            json!({ "accountId": account, "ids": null, "properties": ["id", "subject"] }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let mails: Vec<Value> = reply
        .list()
        .iter()
        .filter(|email| email["subject"] == "iMIP REQUEST")
        .map(|email| email["id"].clone())
        .collect();
    assert_eq!(mails.len(), 1, "the delivered message: {reply:?}");
    let reply = owner
        .jmap_method_call(
            "Email/set",
            json!({ "accountId": account, "destroy": mails.clone() }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(reply.response_at(0)["destroyed"], json!(mails), "{reply:?}");
    Party::new(test, address)
        .delete_items_since(test, &owner_start)
        .await;
    wait_for_queue(test).await;
    sink().clear();
    share(test, owner, reader, f.reader_id, SLUG, "view").await;
}

/// Spec 6.1: after the owner revokes its only grant, upstream's access check
/// answers every served method first; the event the grantee wrote stays.
async fn revoke_ends_access(test: &TestServer, f: &Sharing) {
    const UID: &str = "jmapcal-sharing-revoke";
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    let cal = id(f.shared);
    share(test, owner, reader, f.reader_id, SLUG, "edit").await;
    let reply = reader
        .jmap_create_account(owner, "CalendarEvent", [event_in(UID, &cal)], no_args())
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let event_id = reply.created_id(0).to_string();

    let revoked = za_post(
        "shares/revoke",
        &json!({
            "username": owner.name(), "password": STRONG,
            "calendar": SLUG, "grant": f.reader_id
        }),
    )
    .await
    .expect(200);
    assert_eq!(revoked["changed"], true);
    assert_account_not_found(reader, owner, &cal, &event_id, "revoke_ends_access").await;
    let session = reader.jmap_session_object().await;
    assert!(
        session.0["accounts"].get(owner.id_string()).is_none(),
        "revoke_ends_access: the grantee's session still lists the owner: {session:?}"
    );
    f.reader_dav
        .request("PROPFIND", &home(SLUG), "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    let event = event_get(owner, owner, &event_id).await;
    assert_eq!(event["title"], UID, "{event}");

    owner_delete(test, f, UID).await;
    assert_eq!(uid_copies(test, owner_id, UID).await, 0);
    share(test, owner, reader, f.reader_id, SLUG, "view").await;
}

/// The calendar ids `caller` lists in `owner`'s account.
async fn listed_calendars(caller: &Account, owner: &Account) -> Vec<String> {
    let reply = caller
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": owner.id_string(), "ids": null }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    reply
        .list()
        .iter()
        .map(|calendar| calendar["id"].as_str().unwrap().to_string())
        .collect()
}

/// Whether `caller`'s `CalendarEvent/query` of `owner`'s account with
/// `filter` returns `event`. A method error returns nothing.
async fn query_returns(caller: &Account, owner: &Account, filter: Value, event: &str) -> bool {
    let mut args = json!({ "accountId": owner.id_string() });
    if !filter.is_null() {
        args["filter"] = filter;
    }
    let reply = caller.jmap_method_call("CalendarEvent/query", args).await;
    method_error(&reply).is_none()
        && reply.response_at(0)["ids"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(event)))
}

/// Spec 6.1, 6.3 (final review FC2-20): while the grantee keeps a grant on
/// the owner's account, so that upstream's account check admits it, the
/// fork's fresh context is the only check. A calendar whose grant was
/// revoked, and one never shared, are not listed, and their events are
/// neither read nor returned by a query, with or without a filter.
async fn revoked_and_unshared_calendars_hidden(test: &TestServer, f: &Sharing) {
    const KEPT: &str = "jmapcal-two-kept";
    const REVOKED: &str = "jmapcal-two-revoked";
    const UNSHARED: &str = "jmapcal-unshared";
    let (owner, reader) = (&f.pair.key, &f.reader);
    let mut calendars = Vec::new();
    let mut events = Vec::new();
    for slug in [KEPT, REVOKED, UNSHARED] {
        let calendar = make_calendar(test, owner, &f.pair.dav, slug).await;
        let uid = format!("{slug}-event");
        let document_id = put_event(test, owner, &f.pair.dav, slug, &uid, &uid).await;
        calendars.push(id(calendar));
        events.push(id(document_id));
    }
    for slug in [KEPT, REVOKED] {
        share(test, owner, reader, f.reader_id, slug, "view").await;
    }
    let revoked = za_post(
        "shares/revoke",
        &json!({
            "username": owner.name(), "password": STRONG,
            "calendar": REVOKED, "grant": f.reader_id
        }),
    )
    .await
    .expect(200);
    assert_eq!(revoked["changed"], true);

    let listed = listed_calendars(reader, owner).await;
    assert!(
        listed.contains(&calendars[0]),
        "the kept calendar: {listed:?}"
    );
    assert!(
        listed.contains(&id(f.shared)),
        "the first grant: {listed:?}"
    );
    assert!(
        !listed.contains(&calendars[1]),
        "the revoked calendar is listed: {listed:?}"
    );
    assert!(
        !listed.contains(&calendars[2]),
        "the unshared calendar is listed: {listed:?}"
    );
    let event = event_get(reader, owner, &events[0]).await;
    assert_eq!(event["uid"], format!("{KEPT}-event"), "{event}");
    assert!(query_returns(reader, owner, Value::Null, &events[0]).await);

    for (calendar, event, what) in [
        (&calendars[1], &events[1], "revoked"),
        (&calendars[2], &events[2], "unshared"),
    ] {
        let reply = reader
            .jmap_method_call(
                "CalendarEvent/get",
                json!({ "accountId": owner.id_string(), "ids": [event] }),
            )
            .await;
        assert_eq!(method_error(&reply), None, "{what}: {reply:?}");
        assert!(
            reply.list().is_empty(),
            "{what}: the event was read: {reply:?}"
        );
        assert_eq!(
            reply.not_found().collect::<Vec<_>>(),
            [event.as_str()],
            "{what}: {reply:?}"
        );
        assert!(
            !query_returns(reader, owner, json!({ "inCalendar": calendar }), event).await,
            "{what}: a query naming the calendar returned its event"
        );
        assert!(
            !query_returns(reader, owner, Value::Null, event).await,
            "{what}: a query without a filter returned the event"
        );
    }

    let revoked = za_post(
        "shares/revoke",
        &json!({
            "username": owner.name(), "password": STRONG,
            "calendar": KEPT, "grant": f.reader_id
        }),
    )
    .await
    .expect(200);
    assert_eq!(revoked["changed"], true);
    for slug in [KEPT, REVOKED, UNSHARED] {
        f.pair
            .dav
            .request_with_headers("DELETE", &home(slug), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
}

/// Spec 6.1: an account never granted gets `accountNotFound` from every
/// served method; the fork's `forbidden` comes only after the access check.
async fn non_grantee_refused(test: &TestServer, f: &Sharing) {
    let seed = id(resource_id(test, f.pair.key_id, &format!("{SLUG}/{}.ics", EVENTS[0])).await);
    assert_account_not_found(
        &f.pair.key2,
        &f.pair.key,
        &id(f.shared),
        &seed,
        "non_grantee_refused",
    )
    .await;
}

/// Review focus 3 (D11): a grantee's `useDefaultAlerts: true` writes its own
/// entry's defaults by value, starting as the copy of the owner's it took at
/// the grant; an email alert names the grantee.
async fn grantee_default_alerts(test: &TestServer, f: &Sharing) {
    const FIRST: &str = "jmapcal-sharing-alerts-1";
    const SECOND: &str = "jmapcal-sharing-alerts-2";
    const OWN: &str = "jmapcal-sharing-alerts-own";
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    let calendar = id(make_calendar(test, owner, &f.pair.dav, ALERTS_SLUG).await);
    let reply = calendar_update(
        owner,
        owner,
        &calendar,
        json!({ "defaultAlertsWithTime": { "o1": alert("display", "-PT10M") } }),
    )
    .await;
    assert_set(&reply, "updated", &calendar, "the owner's defaults");
    share(test, owner, reader, f.reader_id, ALERTS_SLUG, "edit").await;

    // The grantee set none: the owner's defaults, as granted.
    let mut object = event_in(FIRST, &calendar);
    object["useDefaultAlerts"] = json!(true);
    let reply = reader
        .jmap_create_account(owner, "CalendarEvent", [object], no_args())
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    reply.created_id(0);
    let first_body = owner_body(test, f, FIRST).await;
    assert_eq!(
        first_body.matches("BEGIN:VALARM").count(),
        1,
        "{first_body}"
    );
    assert!(has_trigger(&first_body, "-PT10M"), "{first_body}");

    // The grantee's own defaults.
    let reply = calendar_update(
        reader,
        owner,
        &calendar,
        json!({ "defaultAlertsWithTime": { "g1": alert("email", "-PT45M") } }),
    )
    .await;
    assert_set(&reply, "updated", &calendar, "the grantee's defaults");
    let mut object = event_in(SECOND, &calendar);
    object["useDefaultAlerts"] = json!(true);
    let reply = reader
        .jmap_create_account(owner, "CalendarEvent", [object], no_args())
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let second = reply.created_id(0).document_id();
    let second_body = owner_body(test, f, SECOND).await;
    assert!(
        has_trigger(&second_body, "-PT45M") && !has_trigger(&second_body, "-PT10M"),
        "grantee_default_alerts: the second event's alarms: {second_body}"
    );
    assert!(
        second_body.contains(&format!("ATTENDEE:mailto:{READER}")),
        "{second_body}"
    );
    let event = event_get(owner, owner, &id(second)).await;
    assert_eq!(event["useDefaultAlerts"], false, "{event}");
    assert_eq!(
        event["alerts"]["g1"]["trigger"]["offset"], "-PT45M",
        "{event}"
    );
    assert_no_event_preferences(test, owner_id, second).await;

    // The owner's own create still takes the owner's defaults.
    let mut object = event_in(OWN, &calendar);
    object["useDefaultAlerts"] = json!(true);
    let reply = owner
        .jmap_create("CalendarEvent", [object], no_args())
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let own = reply.created_id(0).to_string();
    assert_eq!(
        alert_ids(&event_get(owner, owner, &own).await),
        ["o1"],
        "grantee_default_alerts: the owner's own create"
    );
    assert_eq!(
        owner_body(test, f, FIRST).await,
        first_body,
        "grantee_default_alerts: the first event changed"
    );

    // Cleanup before any wait: no email alarm task stays queued.
    for uid in [FIRST, SECOND, OWN] {
        owner_delete(test, f, uid).await;
    }
    let revoked = za_post(
        "shares/revoke",
        &json!({
            "username": owner.name(), "password": STRONG,
            "calendar": ALERTS_SLUG, "grant": f.reader_id
        }),
    )
    .await
    .expect(200);
    assert_eq!(revoked["changed"], true);
    f.pair
        .dav
        .request_with_headers("DELETE", &home(ALERTS_SLUG), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Spec 6.10 (D13): a grantee's session lists the sharer's account with the
/// calendar capability and without the metadata capability.
async fn grantee_session_lists_the_sharer(f: &Sharing) {
    let owner = f.pair.key.id_string();
    let session = f.reader.jmap_session_object().await;
    let account = session.0["accounts"]
        .get(owner)
        .unwrap_or_else(|| panic!("the grantee's session lacks the owner: {session:?}"));
    assert_eq!(account["isPersonal"], false, "{account}");
    let capabilities = &account["accountCapabilities"];
    assert!(
        capabilities.get("urn:ietf:params:jmap:calendars").is_some(),
        "{account}"
    );
    assert!(
        capabilities.get("urn:ietf:params:jmap:metadata").is_none(),
        "{account}"
    );
    let session = f.pair.key2.jmap_session_object().await;
    assert!(
        session.0["accounts"].get(owner).is_none(),
        "a never-granted account's session lists the owner: {session:?}"
    );
}

/// The owner's default calendar's JMAP id.
async fn default_calendar(account: &Account) -> String {
    let reply = account
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": account.id_string(), "ids": null }),
        )
        .await;
    reply
        .list()
        .iter()
        .find(|entry| entry["isDefault"] == true)
        .and_then(|entry| entry["id"].as_str())
        .unwrap_or_else(|| panic!("no default calendar for {}: {reply:?}", account.name()))
        .to_string()
}

/// The new inbox items of `account_id` since `before` that name `uid`, each
/// opened with the account's keys: (document id, pending, method).
async fn new_items_for(
    test: &TestServer,
    account_id: u32,
    before: &[u32],
    uid: &str,
) -> Vec<(u32, bool, Option<ICalendarMethod>)> {
    let pending = pending_ids(test, account_id).await;
    let uid_line = format!("UID:{uid}");
    let mut items = Vec::new();
    for (document_id, _) in new_inbox_items(test, account_id, before).await {
        let is_pending = pending.contains(&document_id);
        if !is_pending {
            continue;
        }
        let (ical, _) = open_pending_item_as(test, account_id, STRONG, document_id).await;
        if ical.to_string().contains(&uid_line) {
            items.push((document_id, is_pending, item_method(&ical)));
        }
    }
    items
}

/// The token the owner's stored bundle of `document_id` holds for
/// `address`, read in the domain of `calendar_id`'s context.
async fn stored_pair(
    test: &TestServer,
    owner_id: u32,
    document_id: u32,
    calendar_id: u32,
    address: &str,
) -> Vec<u8> {
    let keys = za_unlock(test, owner_id, STRONG).await;
    let context = za_owned_calendar_context(&test.server, &keys, calendar_id)
        .await
        .unwrap_or_else(|_| panic!("the owner's context of calendar {calendar_id}"));
    let pairs = stored_event(test, owner_id, document_id)
        .await
        .pairs_in(&context.domain)
        .unwrap_or_else(|err| panic!("the stored pairs do not open: {err:?}"))
        .unwrap_or_else(|| panic!("event {document_id} holds no pairs"));
    pairs
        .get(address)
        .unwrap_or_else(|| panic!("event {document_id} holds no pair for {address}"))
        .to_vec()
}

/// Review addition: moves between the owner's default calendar and the
/// shared one keep the stored token pair, with and without scheduling. One
/// update moves the event into the shared calendar and sends scheduling
/// messages; two quiet updates (no flag) move it back to the default
/// calendar and into the shared one again, sending nothing. The pair the
/// owner's bundle holds for the attendee is the same token read in the
/// default calendar's domain before the first move and in each target
/// calendar's domain after every move (a lost pair would be re-minted by a
/// scheduled move's fan-out, and lost for good by a quiet one). The
/// attendee's reply, applied only if its token matches the stored pair, then
/// reaches the moved event.
async fn move_with_scheduling_into_shared(test: &TestServer, f: &Sharing) {
    const UID: &str = "jmapcal-sharing-move-sched";
    const TITLE: &str = "Moved with scheduling";
    let (owner, key2, reader) = (&f.pair.key, &f.pair.key2, &f.reader);
    let (owner_id, key2_id) = (f.pair.key_id, f.pair.key2_id);
    let owner_start = inbox_ids(test, owner_id).await;
    let key2_start = inbox_ids(test, key2_id).await;

    let default = default_calendar(owner).await;
    let shared = id(f.shared);
    assert_ne!(
        default, shared,
        "move_with_scheduling_into_shared: the default calendar is the shared one"
    );
    let mut object = event_in(UID, &default);
    object["participants"] = json!({
        "p0": {
            "@type": "Participant",
            "calendarAddress": format!("mailto:{}", owner.name()),
            "roles": { "owner": true, "chair": true },
            "participationStatus": "accepted",
        },
        "p1": {
            "@type": "Participant",
            "calendarAddress": format!("mailto:{}", key2.name()),
            "kind": "individual",
            "participationStatus": "needs-action",
            "expectReply": true,
        },
    });
    let reply = owner.jmap_create("CalendarEvent", [object], send()).await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let document_id = reply.created_id(0).document_id();
    let event_id = id(document_id);
    let invited = new_items_for(test, key2_id, &key2_start, UID).await;
    assert_eq!(
        invited.len(),
        1,
        "move_with_scheduling_into_shared: key2's invitation: {invited:?}"
    );
    assert_eq!(
        event_get(owner, owner, &event_id).await["calendarIds"],
        json!({ (default.as_str()): true }),
        "move_with_scheduling_into_shared: the placement before the move"
    );
    let default_id = Id::from_str(&default).unwrap().document_id();
    let before_token = stored_pair(test, owner_id, document_id, default_id, key2.name()).await;

    let before_move = inbox_ids(test, key2_id).await;
    let reply = owner
        .jmap_update(
            "CalendarEvent",
            [(
                event_id.clone(),
                json!({ "calendarIds": { (shared.as_str()): true }, "title": TITLE }),
            )],
            send(),
        )
        .await;
    assert_set(&reply, "updated", &event_id, "the move with scheduling");
    assert_eq!(
        event_get(owner, owner, &event_id).await["calendarIds"],
        json!({ (shared.as_str()): true }),
        "move_with_scheduling_into_shared: the placement after the move"
    );
    let after_token = stored_pair(test, owner_id, document_id, f.shared, key2.name()).await;
    assert!(
        before_token == after_token,
        "move_with_scheduling_into_shared: the stored pair changed across the move"
    );
    let moved = new_items_for(test, key2_id, &before_move, UID).await;
    assert_eq!(
        new_inbox_items(test, key2_id, &before_move).await.len(),
        1,
        "move_with_scheduling_into_shared: key2's new items after the move"
    );
    let [(request, true, Some(ICalendarMethod::Request))] = moved[..] else {
        panic!("move_with_scheduling_into_shared: the move's item: {moved:?}");
    };
    assert_eq!(event_get(reader, owner, &event_id).await["title"], TITLE);
    assert_eq!(event_get(owner, owner, &event_id).await["title"], TITLE);

    // Moves without scheduling: back to the default calendar, then into the
    // shared one again. Nothing is sent, so no fan-out re-mints a token; the
    // stored pair must be carried across each domain change as it is.
    for (target, target_id, what) in [
        (
            default.as_str(),
            default_id,
            "the quiet move to the default calendar",
        ),
        (
            shared.as_str(),
            f.shared,
            "the quiet move back into the shared calendar",
        ),
    ] {
        let before = inbox_ids(test, key2_id).await;
        let reply = owner
            .jmap_update(
                "CalendarEvent",
                [(event_id.clone(), json!({ "calendarIds": { target: true } }))],
                no_args(),
            )
            .await;
        assert_set(&reply, "updated", &event_id, what);
        assert_eq!(
            event_get(owner, owner, &event_id).await["calendarIds"],
            json!({ target: true }),
            "move_with_scheduling_into_shared: the placement after {what}"
        );
        assert!(
            new_inbox_items(test, key2_id, &before).await.is_empty(),
            "move_with_scheduling_into_shared: {what} sent key2 an item"
        );
        let token = stored_pair(test, owner_id, document_id, target_id, key2.name()).await;
        assert!(
            token == before_token,
            "move_with_scheduling_into_shared: the stored pair changed across {what}"
        );
    }

    // Only the move's REQUEST stays in key2's inbox, and nothing in the
    // owner's, before the drain is on.
    let key2_party = Party::new(test, key2.name());
    let owner_party = Party::new(test, owner.name());
    key2_party
        .delete_items(
            test,
            inbox_ids(test, key2_id)
                .await
                .into_iter()
                .filter(|item| *item != request),
        )
        .await;
    owner_party
        .delete_items(test, inbox_ids(test, owner_id).await)
        .await;
    assert_eq!(pending_ids(test, key2_id).await, [request]);
    assert!(pending_ids(test, owner_id).await.is_empty());
    let owner_href = copy_href(test, owner_id, UID).await;
    {
        let _drain = DrainOn::new();
        assert_eq!(trigger_drain(key2.name(), STRONG).await, 207);
        assert_eq!(uid_copies(test, key2_id, UID).await, 1);
        let copy = copy_href(test, key2_id, UID).await;
        set_partstat(&f.pair.dav2, &copy, key2.name(), "ACCEPTED").await;
        assert_eq!(trigger_drain(owner.name(), STRONG).await, 207);
        assert_eq!(
            partstat_of(&f.pair.dav, &owner_href, key2.name())
                .await
                .as_deref(),
            Some("ACCEPTED"),
            "move_with_scheduling_into_shared: the reply was not applied after the move"
        );
    }

    // Cleanup, without scheduling: neither destroy carries the flag.
    let copy = Id::from(
        test.server
            .document_ids_matching(
                key2_id,
                Collection::CalendarEvent,
                types::field::CalendarEventField::Uid,
                UID.as_bytes(),
            )
            .await
            .unwrap()
            .min()
            .unwrap(),
    )
    .to_string();
    let reply = key2
        .jmap_destroy("CalendarEvent", [copy.as_str()], no_args())
        .await;
    assert_set_destroyed(&reply, &copy);
    let reply = owner
        .jmap_destroy("CalendarEvent", [event_id.as_str()], no_args())
        .await;
    assert_set_destroyed(&reply, &event_id);
    assert_eq!(uid_copies(test, owner_id, UID).await, 0);
    assert_eq!(uid_copies(test, key2_id, UID).await, 0);
    key2_party.delete_items_since(test, &key2_start).await;
    owner_party.delete_items_since(test, &owner_start).await;
    assert!(new_inbox_items(test, key2_id, &key2_start).await.is_empty());
    assert!(
        new_inbox_items(test, owner_id, &owner_start)
            .await
            .is_empty()
    );
}

fn assert_set_destroyed(reply: &JmapResponse, id: &str) {
    assert_eq!(method_error(reply), None, "{reply:?}");
    assert_eq!(reply.destroyed().collect::<Vec<_>>(), [id], "{reply:?}");
}

/// The unfolded lines of a pending item that carry a `SENT-BY` parameter,
/// opened with the recipient's keys.
async fn sent_by_lines(test: &TestServer, account_id: u32, document_id: u32) -> Vec<String> {
    let (ical, _) = open_pending_item_as(test, account_id, STRONG, document_id).await;
    ical.to_string()
        .replace("\r\n ", "")
        .lines()
        .map(str::trim_end)
        .filter(|line| line.to_ascii_lowercase().contains("sent-by="))
        .map(str::to_string)
        .collect()
}

/// Release 4's delegated scheduling over JMAP: a grantee at `edit` that
/// destroys a scheduled event of the shared calendar with
/// `sendSchedulingMessages: true` sends the attendee a CANCEL naming the
/// grantee as `SENT-BY`, as the grantee's CalDAV DELETE of its twin does.
async fn grantee_destroy_sends_delegated_cancel(test: &TestServer, f: &Sharing) {
    const JM: &str = "jmapcal-sharing-cancel-jm";
    const DAV: &str = "jmapcal-sharing-cancel-dav";
    let (owner, key2, reader) = (&f.pair.key, &f.pair.key2, &f.reader);
    let (owner_id, key2_id) = (f.pair.key_id, f.pair.key2_id);
    let shared = id(f.shared);
    let key2_start = inbox_ids(test, key2_id).await;
    share(test, owner, reader, f.reader_id, SLUG, "edit").await;

    let mut ids = Vec::new();
    for uid in [JM, DAV] {
        let before = inbox_ids(test, key2_id).await;
        let mut object = event_in(uid, &shared);
        object["participants"] = json!({
            "p0": {
                "@type": "Participant",
                "calendarAddress": format!("mailto:{}", owner.name()),
                "roles": { "owner": true, "chair": true },
                "participationStatus": "accepted",
            },
            "p1": {
                "@type": "Participant",
                "calendarAddress": format!("mailto:{}", key2.name()),
                "kind": "individual",
                "participationStatus": "needs-action",
                "expectReply": true,
            },
        });
        let reply = owner.jmap_create("CalendarEvent", [object], send()).await;
        assert_eq!(method_error(&reply), None, "{reply:?}");
        ids.push(reply.created_id(0).to_string());
        let invited = new_items_for(test, key2_id, &before, uid).await;
        assert!(
            matches!(invited[..], [(_, true, Some(ICalendarMethod::Request))]),
            "grantee_destroy_sends_delegated_cancel: {uid}'s invitation: {invited:?}"
        );
    }

    // The JMAP destroy by the grantee, with the flag.
    let before = inbox_ids(test, key2_id).await;
    let reply = reader
        .jmap_destroy_account(owner, "CalendarEvent", [ids[0].as_str()], send())
        .await;
    assert_set_destroyed(&reply, &ids[0]);
    assert_eq!(uid_copies(test, owner_id, JM).await, 0);
    let items = new_items_for(test, key2_id, &before, JM).await;
    let [(jm_item, true, Some(ICalendarMethod::Cancel))] = items[..] else {
        panic!("grantee_destroy_sends_delegated_cancel: the JMAP destroy's item: {items:?}");
    };

    // The CalDAV twin: the grantee's DELETE, without `Schedule-Reply: F`.
    let href = copy_href(test, owner_id, DAV).await;
    let before = inbox_ids(test, key2_id).await;
    f.reader_dav
        .request("DELETE", &href, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(uid_copies(test, owner_id, DAV).await, 0);
    let items = new_items_for(test, key2_id, &before, DAV).await;
    let [(dav_item, true, Some(ICalendarMethod::Cancel))] = items[..] else {
        panic!("grantee_destroy_sends_delegated_cancel: the CalDAV DELETE's item: {items:?}");
    };

    // Both name the grantee as SENT-BY, on the same property.
    let sent_by = format!("sent-by=\"mailto:{READER}\"");
    let dav_lines = sent_by_lines(test, key2_id, dav_item).await;
    let jm_lines = sent_by_lines(test, key2_id, jm_item).await;
    assert!(
        matches!(&dav_lines[..], [line] if line.to_ascii_lowercase().contains(&sent_by)),
        "grantee_destroy_sends_delegated_cancel: the CalDAV CANCEL's SENT-BY: {dav_lines:?}"
    );
    assert_eq!(
        jm_lines, dav_lines,
        "grantee_destroy_sends_delegated_cancel: the JMAP CANCEL differs from its CalDAV twin"
    );

    Party::new(test, key2.name())
        .delete_items_since(test, &key2_start)
        .await;
    assert!(new_inbox_items(test, key2_id, &key2_start).await.is_empty());
    for uid in [JM, DAV] {
        assert_eq!(uid_copies(test, key2_id, uid).await, 0);
    }
    share(test, owner, reader, f.reader_id, SLUG, "view").await;
}

/// PR #90 review C4617611549: an `edit` grantee's destroy and update are held to
/// the event's effective privacy, the higher of the row flags' and the unsealed
/// tree's, as `CalendarEvent/get` and the text query read it. An extension
/// `CLASS` is sealed and leaves the flags public; the update that would set
/// `privacy: "public"` (and drop the sealed value) is refused too. A public
/// event of the same calendar is still destroyed by the grantee.
async fn grantee_held_to_effective_privacy(test: &TestServer, f: &Sharing) {
    const VEILED: &str = "jmapcal-sharing-veiled";
    const OPEN: &str = "jmapcal-sharing-open";
    const TITLE: &str = "jmapcal sharing veiled title";
    const PRIVATE: &str = "Private events can only be modified by the calendar owner.";
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    share(test, owner, reader, f.reader_id, SLUG, "edit").await;
    let href = format!("{}{VEILED}.ics", home(SLUG));
    let veiled_ics =
        event_ics(VEILED, TITLE).replace("END:VEVENT", "CLASS:X-JMAPCAL-VEILED\r\nEND:VEVENT");
    // PR #90 review RA-14: first as a series, for the synthetic id of its first
    // occurrence, then as a single event, so that id resolves to the base event.
    f.pair
        .dav
        .request_with_headers(
            "PUT",
            &href,
            [CONTENT_TYPE],
            veiled_ics.replace("END:VEVENT", "RRULE:FREQ=DAILY;COUNT=2\r\nEND:VEVENT"),
        )
        .await
        .with_status(StatusCode::CREATED);
    let veiled = id(resource_id(test, owner_id, &format!("{SLUG}/{VEILED}.ics")).await);
    let reply = owner
        .jmap_method_call(
            "CalendarEvent/query",
            json!({
                "accountId": owner.id_string(),
                "filter": { "after": "2099-02-28T00:00:00", "before": "2099-03-05T00:00:00" },
                "expandRecurrences": true,
            }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    let ids = reply.ids().map(str::to_string).collect::<Vec<_>>();
    let reply = owner
        .jmap_method_call(
            "CalendarEvent/get",
            json!({ "accountId": owner.id_string(), "ids": ids, "properties": ["id", "uid", "start"] }),
        )
        .await;
    let first = reply
        .list()
        .iter()
        .find(|event| event["uid"] == VEILED && event["start"] == "2099-03-01T10:00:00")
        .and_then(|event| event["id"].as_str())
        .unwrap_or_else(|| panic!("no first occurrence: {reply:?}"))
        .to_string();
    assert_ne!(first, veiled, "the first occurrence's id is synthetic");
    f.pair
        .dav
        .request_with_headers("PUT", &href, [CONTENT_TYPE], veiled_ics)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let before = owner_body(test, f, VEILED).await;
    let open = id(put_event(test, owner, &f.pair.dav, SLUG, OPEN, OPEN).await);

    let reply = reader
        .jmap_destroy_account(owner, "CalendarEvent", [veiled.as_str()], no_args())
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        set_error(reply.not_destroyed(&veiled)),
        ("forbidden", Some(PRIVATE)),
        "the grantee's destroy: {reply:?}"
    );
    let reply = reader
        .jmap_method_call(
            "CalendarEvent/set",
            json!({
                "accountId": owner.id_string(),
                "update": { &veiled: { "privacy": "public", "title": "x" } },
            }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        set_error(reply.not_updated(&veiled)),
        ("forbidden", Some(PRIVATE)),
        "the grantee's update: {reply:?}"
    );
    // RA-14: an occurrence destroy that resolves to the base event, and a
    // change of the event's calendars.
    let reply = reader
        .jmap_destroy_account(owner, "CalendarEvent", [first.as_str()], no_args())
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        set_error(reply.not_destroyed(&first)),
        ("forbidden", Some(PRIVATE)),
        "the grantee's occurrence destroy: {reply:?}"
    );
    let default = id(resource_id(test, owner_id, "default").await);
    let reply = reader
        .jmap_method_call(
            "CalendarEvent/set",
            json!({
                "accountId": owner.id_string(),
                "update": { &veiled: { format!("calendarIds/{default}"): true } },
            }),
        )
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        set_error(reply.not_updated(&veiled)),
        ("forbidden", Some(PRIVATE)),
        "the grantee's calendarIds change: {reply:?}"
    );
    assert_eq!(uid_copies(test, owner_id, VEILED).await, 1);
    let body = owner_body(test, f, VEILED).await;
    assert!(
        body.contains("CLASS:X-JMAPCAL-VEILED") && body.contains(TITLE),
        "grantee_held_to_effective_privacy: the owner's event changed"
    );
    assert!(
        body == before,
        "grantee_held_to_effective_privacy: the owner's event changed"
    );

    let reply = reader
        .jmap_destroy_account(owner, "CalendarEvent", [open.as_str()], no_args())
        .await;
    assert_set_destroyed(&reply, &open);
    assert_eq!(uid_copies(test, owner_id, OPEN).await, 0);

    // The owner's destroy of the same synthetic id removes the event: the id
    // resolves to the base event.
    let reply = owner
        .jmap_destroy_account(owner, "CalendarEvent", [first.as_str()], no_args())
        .await;
    assert_set_destroyed(&reply, &first);
    assert_eq!(uid_copies(test, owner_id, VEILED).await, 0);
    share(test, owner, reader, f.reader_id, SLUG, "view").await;
}

/// Upstream's checks an `edit` grantee's `CalendarEvent/set` meets, answered by
/// the key writer with upstream's texts: a privacy other than public, set on a
/// create or an update; an event the owner classified private, which the
/// grantee may neither change nor destroy; a calendar shared at `view`, which
/// the grantee may not add an event to; and, over
/// CalDAV, a UID the owner holds in a confidential event, refused without
/// naming it. Nothing of a refused write is stored.
async fn grantee_answers_of_upstreams_checks(test: &TestServer, f: &Sharing) {
    const VIEWED: &str = "jmapcal-sharing-viewed";
    const CLASSIFIED: &str = "jmapcal-sharing-classified";
    const CONFIDENTIAL: &str = "jmapcal-sharing-confidential";
    const PUBLIC: &str = "jmapcal-sharing-public";
    const MINE: &str = "jmapcal-sharing-mine";
    const PRIVATE: &str = "Private events can only be modified by the calendar owner.";
    const OWNER_ONLY: &str = "Only the calendar owner can set a privacy other than public.";
    let (owner, reader) = (&f.pair.key, &f.reader);
    let owner_id = f.pair.key_id;
    share(test, owner, reader, f.reader_id, SLUG, "edit").await;
    let viewed = make_calendar(test, owner, &f.pair.dav, VIEWED).await;
    share(test, owner, reader, f.reader_id, VIEWED, "view").await;
    // The owner's reads complete both calendars' sync boundaries.
    complete_boundary(test, f).await;
    f.pair
        .dav
        .request("PROPFIND", &home(VIEWED), "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let (shared, viewed) = (id(f.shared), id(viewed));
    let event = |uid: &str, calendars: &[&str]| {
        json!({
            "@type": "Event",
            "uid": uid,
            "title": uid,
            "start": "2031-03-04T10:00:00",
            "duration": "PT1H",
            "timeZone": "Etc/UTC",
            "calendarIds": Value::Object(
                calendars.iter().map(|id| (id.to_string(), json!(true))).collect()
            ),
        })
    };
    let classified = |uid: &str, class: &str| {
        event_ics(uid, uid).replace("END:VEVENT", &format!("CLASS:{class}\r\nEND:VEVENT"))
    };

    // A create that sets a privacy other than public.
    let mut object = event(MINE, &[&shared]);
    object["privacy"] = json!("private");
    let reply = reader
        .jmap_create_account(owner, "CalendarEvent", [object], no_args())
        .await;
    assert_eq!(method_error(&reply), None, "{reply:?}");
    assert_eq!(
        set_error(reply.not_created(0)),
        ("invalidProperties", Some(OWNER_ONLY)),
        "{reply:?}"
    );
    assert_eq!(uid_copies(test, owner_id, MINE).await, 0);

    // An event the owner classified private.
    f.pair
        .dav
        .request_with_headers(
            "PUT",
            &format!("{}{CLASSIFIED}.ics", home(SLUG)),
            [CONTENT_TYPE],
            classified(CLASSIFIED, "PRIVATE"),
        )
        .await
        .with_status(StatusCode::CREATED);
    let private = id(resource_id(test, owner_id, &format!("{SLUG}/{CLASSIFIED}.ics")).await);
    let before = owner_body(test, f, CLASSIFIED).await;
    let reply = reader
        .jmap_update_account(
            owner,
            "CalendarEvent",
            [(private.clone(), json!({ "title": "x" }))],
            no_args(),
        )
        .await;
    assert_eq!(
        set_error(reply.not_updated(&private)),
        ("forbidden", Some(PRIVATE)),
        "the grantee's update: {reply:?}"
    );
    let reply = reader
        .jmap_destroy_account(owner, "CalendarEvent", [private.as_str()], no_args())
        .await;
    assert_eq!(
        set_error(reply.not_destroyed(&private)),
        ("forbidden", Some(PRIVATE)),
        "the grantee's destroy: {reply:?}"
    );
    assert_eq!(owner_body(test, f, CLASSIFIED).await, before);

    // A public event: no privacy change, and no calendar shared at `view`.
    let reply = owner
        .jmap_create("CalendarEvent", [event(PUBLIC, &[&shared])], no_args())
        .await;
    let public = reply.created_id(0).to_string();
    let reply = reader
        .jmap_update_account(
            owner,
            "CalendarEvent",
            [(public.clone(), json!({ "privacy": "private" }))],
            no_args(),
        )
        .await;
    assert_eq!(
        set_error(reply.not_updated(&public)),
        ("invalidProperties", Some(OWNER_ONLY)),
        "{reply:?}"
    );
    let reply = reader
        .jmap_update_account(
            owner,
            "CalendarEvent",
            [(
                public.clone(),
                json!({ format!("calendarIds/{viewed}"): true }),
            )],
            no_args(),
        )
        .await;
    let added = format!("You are not allowed to add calendar events to calendar {viewed}.");
    assert_eq!(
        set_error(reply.not_updated(&public)),
        ("forbidden", Some(added.as_str())),
        "{reply:?}"
    );
    let document_id = Id::from_str(&public).unwrap().document_id();
    let names = stored_event(test, owner_id, document_id)
        .await
        .za_event()
        .meta
        .names
        .len();
    assert_eq!(names, 1, "a refused update changed the event's calendars");

    // Over CalDAV, a UID the owner holds in a confidential event.
    f.pair
        .dav
        .request_with_headers(
            "PUT",
            &format!("{}{CONFIDENTIAL}.ics", home(SLUG)),
            [CONTENT_TYPE],
            classified(CONFIDENTIAL, "CONFIDENTIAL"),
        )
        .await
        .with_status(StatusCode::CREATED);
    f.reader_dav
        .request_with_headers(
            "PUT",
            &format!("{}jmapcal-sharing-copy.ics", home(SLUG)),
            [CONTENT_TYPE],
            event_ics(CONFIDENTIAL, "jmapcal sharing copy"),
        )
        .await
        .with_status(StatusCode::FORBIDDEN);
    assert_eq!(uid_copies(test, owner_id, CONFIDENTIAL).await, 1);

    // The account page's share endpoints refuse a wrong password before
    // any change: the view grant stays.
    for (path, body) in [
        (
            "shares/grant",
            json!({ "calendar": VIEWED, "address": reader.name(), "level": "edit" }),
        ),
        (
            "shares/revoke",
            json!({ "calendar": VIEWED, "grant": f.reader_id }),
        ),
    ] {
        let mut body = body;
        body["username"] = json!(owner.name());
        body["password"] = json!("not the password at all");
        assert_eq!(
            za_post_from("10.0.60.12", path, &body).await.status,
            401,
            "{path}"
        );
    }
    let listed = za_post(
        "shares",
        &json!({ "username": owner.name(), "password": STRONG }),
    )
    .await
    .expect(200);
    assert!(
        listed.to_string().contains(VIEWED) && listed.to_string().contains("\"view\""),
        "the view grant changed: {listed}"
    );

    let reply = owner
        .jmap_destroy("CalendarEvent", [public.as_str()], no_args())
        .await;
    assert_set_destroyed(&reply, &public);
    owner_delete(test, f, CLASSIFIED).await;
    owner_delete(test, f, CONFIDENTIAL).await;
    let revoked = za_post(
        "shares/revoke",
        &json!({
            "username": owner.name(), "password": STRONG,
            "calendar": VIEWED, "grant": f.reader_id
        }),
    )
    .await
    .expect(200);
    assert_eq!(revoked["changed"], true);
    f.pair
        .dav
        .request_with_headers("DELETE", &home(VIEWED), [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    share(test, owner, reader, f.reader_id, SLUG, "view").await;
}
