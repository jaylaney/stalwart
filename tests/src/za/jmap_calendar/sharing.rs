/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 spec 6.8 (D15, D29) and 9.2: a grantee's calendar changes, the
//! sync boundary and the explicit origin of a paged sync, through the actual
//! JMAP changes handlers and CalDAV's sync-collection REPORT.

use super::{
    Pair, finish, key_account, make_calendar, mark_boundary_pending, owner_boundary, pair,
    put_event, session_token, share,
};
use crate::{
    utils::{
        account::Account,
        jmap::{JmapResponse, JmapUtils},
        server::TestServer,
        webdav::DummyWebDavClient,
        za::{CALENDAR_JMAP_USING, za_bearer_jmap, za_grant_token, za_post, za_revoke_grant},
    },
    za::{
        STRONG,
        fanout::NO_REPLY,
        gating::method_error,
        mail::{Traced, message_with},
        user_permissions,
    },
};
use common::{auth::oauth::scope::OAuthScopes, za::ZA_CALENDAR_KEYLESS};
use email::mailbox::INBOX_ID;
use futures::FutureExt;
use groupware::calendar::{
    pipeline::context::za_foreign_calendar_contexts, sharing::resolve::ZaSyncBoundary,
};
use hyper::StatusCode;
use jmap_proto::types::state::{JMAPIntermediateState, State, ZaCalendarOrigin};
use serde_json::json;
use std::panic::AssertUnwindSafe;
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
    paged_sync_retention_collision(test, &fixture).await;
    paged_sync_origin_survives_range_changes(test, &fixture).await;
    let ordinary = ordinary_accounts(test).await;
    paged_sync_legacy_and_ordinary(test, &fixture, &ordinary).await;
    calendar_state_slot_errors(test, &fixture, &ordinary).await;
    let admin = test.account("admin@example.com");
    admin.destroy_account(ordinary.plain).await;
    admin.destroy_account(ordinary.from).await;
    changes_store_errors_are_payload_free(test, &fixture).await;

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
