/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6.6: the scheduling inbox as a key account's clients
//! see it. Items are planted straight into the store (`plant_inbox_item`):
//! the fan-out (plan 9) and the drain (plan 10) do not exist yet.

use super::STRONG;
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{PlantedItem, destroy_inbox_item, plant_inbox_item, rewrite_inbox_item, za_unlock},
};
use ::registry::schema::{
    enums::TaskAccountMaintenanceType,
    structs::{Task, TaskAccountMaintenance, TaskStatus},
};
use calcard::{Entry, Parser};
use dav_proto::Depth;
use groupware::calendar::{EVENT_NOTIFICATION_ZA_PENDING, seal::seal_drained_item};
use hyper::StatusCode;
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes, now},
};
use types::collection::Collection;

pub const KEY_INBOX: &str = "/dav/itip/key1%40example.com/inbox/";
const PLAIN_INBOX: &str = "/dav/itip/plain%40example.com/inbox/";
const PROPFIND_DATA: &str = "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/><A:calendar-data/></D:prop></D:propfind>";
const PROPNAME_DATA: &str =
    "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\"><D:propname/></D:propfind>";

/// An invitation from key2 to key1, as the fan-out will deliver it.
pub fn invitation(uid: &str, canary: &str) -> String {
    format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nMETHOD:REQUEST\r\n",
            "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\n",
            "DTSTART:20990101T100000Z\r\nDTEND:20990101T110000Z\r\nSEQUENCE:0\r\n",
            "SUMMARY:{canary}\r\nORGANIZER:mailto:key2@example.com\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key1@example.com\r\n",
            "END:VEVENT\r\nEND:VCALENDAR\r\n"
        ),
        uid = uid,
        canary = canary
    )
}

/// The href the resource cache gives an inbox item (`<document id>.ics`).
pub fn item_href(inbox: &str, document_id: u32) -> String {
    format!("{inbox}{document_id}.ics")
}

pub fn parse(text: &str) -> calcard::icalendar::ICalendar {
    match Parser::new(text).entry() {
        Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    }
}

pub async fn stored_item(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
) -> Option<Archive<ArchiveBytes>> {
    test.server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEventNotification,
            document_id,
        ))
        .await
        .unwrap()
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access inbox read path tests...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let sender = test.account("key2@example.com").id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);

    let pending_ical = invitation("za-inbox-pending", "inbox-pending-canary");
    let drained_ical = invitation("za-inbox-drained", "inbox-drained-canary");
    let legacy_ical = invitation("za-inbox-legacy", "inbox-legacy-canary");
    let pending = plant_inbox_item(
        test,
        id,
        &pending_ical,
        PlantedItem::Pending {
            pair: Some(("key1@example.com", [0x5A; 16])),
            reply_token: None,
        },
        sender,
        None,
    )
    .await;
    let drained = plant_inbox_item(
        test,
        id,
        &drained_ical,
        PlantedItem::Drained { password: STRONG },
        sender,
        None,
    )
    .await;
    // Review Focus 4: a plaintext item (no key envelope) passes through.
    let legacy = plant_inbox_item(
        test,
        id,
        &legacy_ical,
        PlantedItem::Plain { flags: 0 },
        sender,
        None,
    )
    .await;
    let pending_href = item_href(KEY_INBOX, pending);
    let drained_href = item_href(KEY_INBOX, drained);
    let legacy_href = item_href(KEY_INBOX, legacy);

    // Listing: the pending item is absent; the others show plaintext.
    let response = client
        .request_with_headers("PROPFIND", KEY_INBOX, [("depth", "1")], PROPFIND_DATA)
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let hrefs: Vec<String> = response.hrefs().into_iter().map(str::to_string).collect();
    assert!(
        !hrefs.contains(&pending_href),
        "pending item listed: {hrefs:?}"
    );
    assert!(
        hrefs.contains(&drained_href) && hrefs.contains(&legacy_href),
        "{hrefs:?}"
    );
    let body = response.body.unwrap();
    assert!(
        body.contains("inbox-drained-canary")
            && body.contains("inbox-legacy-canary")
            && !body.contains("inbox-pending-canary")
            && !body.contains("X-ZA-")
            && !body.contains("HTTP/1.1 500"),
        "{body}"
    );

    // GET, and Review Focus 2: HEAD and PROPFIND depth 0, all 404.
    client
        .request("GET", &pending_href, "")
        .await
        .with_status(StatusCode::NOT_FOUND);
    client
        .request("HEAD", &pending_href, "")
        .await
        .with_status(StatusCode::NOT_FOUND);
    client
        .request_with_headers("PROPFIND", &pending_href, [("depth", "0")], PROPFIND_DATA)
        .await
        .with_status(StatusCode::NOT_FOUND);
    // Upgrade ruling 176 (B-M4): DELETE too, and the item stays.
    client
        .request("DELETE", &pending_href, "")
        .await
        .with_status(StatusCode::NOT_FOUND);
    assert!(
        stored_item(test, id, pending).await.is_some(),
        "the refused DELETE removed the pending item"
    );
    // PR #34 review: the `propname` form hides it too, listed (depth 1) or
    // named (depth 0); a visible item still answers.
    let hrefs: Vec<String> = client
        .request_with_headers("PROPFIND", KEY_INBOX, [("depth", "1")], PROPNAME_DATA)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .hrefs()
        .into_iter()
        .map(str::to_string)
        .collect();
    assert!(
        !hrefs.contains(&pending_href),
        "pending item listed by propname: {hrefs:?}"
    );
    assert!(
        hrefs.contains(&drained_href) && hrefs.contains(&legacy_href),
        "{hrefs:?}"
    );
    client
        .request_with_headers("PROPFIND", &pending_href, [("depth", "0")], PROPNAME_DATA)
        .await
        .with_status(StatusCode::NOT_FOUND);
    let hrefs: Vec<String> = client
        .request_with_headers("PROPFIND", &drained_href, [("depth", "0")], PROPNAME_DATA)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .hrefs()
        .into_iter()
        .map(str::to_string)
        .collect();
    assert!(hrefs.contains(&drained_href), "{hrefs:?}");
    // RFC 4791 7.9: a calendar-multiget may ask for `propname`. 0.16 bound
    // every multiget to the calendar collection, so each inbox href was 404;
    // 1.0 routes a scheduling multiget to the inbox's items, so the visible
    // item answers with its names and the pending one is still not served.
    let body = client
        .request(
            "REPORT",
            KEY_INBOX,
            format!(
                "<?xml version=\"1.0\"?><A:calendar-multiget xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><D:propname/><D:href>{pending_href}</D:href><D:href>{drained_href}</D:href></A:calendar-multiget>"
            ),
        )
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    let entry = |href: &str| {
        body.split("</D:response>")
            .find(|entry| entry.contains(&format!(">{href}<")))
            .unwrap_or_default()
    };
    assert!(
        !entry(&pending_href).contains("200 OK") && entry(&drained_href).contains("200 OK"),
        "{body}"
    );
    // Review Focus 1: a multiget naming it answers exactly as for an href
    // that names no item: 404 for both, at 0.16 and at 1.0 (the pending
    // item's 404 since `a5cddf448`, ruling 136).
    let body = client
        .multiget_calendar(KEY_INBOX, &[pending_href.as_str()])
        .await
        .response
        .body
        .unwrap();
    let missing_href = format!("{KEY_INBOX}4294967.ics");
    let missing = client
        .multiget_calendar(KEY_INBOX, &[missing_href.as_str()])
        .await
        .response
        .body
        .unwrap();
    assert!(!body.contains("inbox-pending-canary"), "{body}");
    assert_eq!(
        body.replace(&pending_href, "HREF"),
        missing.replace(&missing_href, "HREF"),
        "a pending item is answered unlike a missing one"
    );
    // An initial sync-collection does not report it either.
    let body = client
        .sync_collection(KEY_INBOX, "", Depth::One, None, ["D:getetag"])
        .await
        .body
        .unwrap();
    assert!(
        !body.contains(&pending_href) && body.contains(&drained_href),
        "{body}"
    );

    // Release 1 spec 10: a session without key1's keys (a master-user login,
    // as in `dav_gate`) is refused (403) before the pending bit is read.
    let admin = test.account("admin@example.com").clone();
    let master = Box::leak(format!("{name}%{}", admin.name()).into_boxed_str());
    let master_client = DummyWebDavClient::new(id, master, admin.secret(), name);
    master_client
        .request("GET", &pending_href, "")
        .await
        .with_status(StatusCode::FORBIDDEN);
    master_client
        .request_with_headers("PROPFIND", KEY_INBOX, [("depth", "1")], PROPFIND_DATA)
        .await
        .with_status(StatusCode::FORBIDDEN);

    // GET of the drained and the legacy item: their plaintext trees.
    for (href, ical) in [(&drained_href, &drained_ical), (&legacy_href, &legacy_ical)] {
        let body = client
            .request("GET", href, "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap();
        assert!(!body.contains("X-ZA-"), "{body}");
        assert_eq!(parse(&body), parse(ical), "{href}");
    }
    let stored = stored_item(test, id, drained).await.unwrap();
    assert!(!String::from_utf8_lossy(stored.as_bytes()).contains("inbox-drained-canary"));

    // Review Focus 3: a `pk`-wrapped item whose pending bit was cleared is an
    // unseal failure: GET 500, one 500 entry in the listing, nothing changed.
    rewrite_inbox_item(test, id, pending, |item| {
        item.flags &= !EVENT_NOTIFICATION_ZA_PENDING
    })
    .await;
    let before = stored_item(test, id, pending).await.unwrap();
    client
        .request("GET", &pending_href, "")
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    let body = client
        .request_with_headers("PROPFIND", KEY_INBOX, [("depth", "1")], PROPFIND_DATA)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .body
        .unwrap();
    assert!(
        body.contains(&pending_href)
            && body.contains("HTTP/1.1 500")
            && body.contains("inbox-drained-canary")
            && !body.contains("inbox-pending-canary"),
        "{body}"
    );
    let after = stored_item(test, id, pending)
        .await
        .expect("nothing deleted");
    assert_eq!(after.as_bytes(), before.as_bytes(), "nothing rewritten");

    // Control: a plain account's inbox is upstream's; the bit means nothing.
    let plain = test.account("plain@example.com").clone();
    let plain_id = plain.id().document_id();
    let plain_client = DummyWebDavClient::new(plain_id, plain.name(), plain.secret(), plain.name());
    let plain_ical = invitation("za-inbox-plain", "inbox-plain-canary");
    let plain_item = plant_inbox_item(
        test,
        plain_id,
        &plain_ical,
        PlantedItem::Plain {
            flags: EVENT_NOTIFICATION_ZA_PENDING,
        },
        sender,
        None,
    )
    .await;
    let plain_href = item_href(PLAIN_INBOX, plain_item);
    let hrefs: Vec<String> = plain_client
        .request_with_headers("PROPFIND", PLAIN_INBOX, [("depth", "1")], PROPFIND_DATA)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .hrefs()
        .into_iter()
        .map(str::to_string)
        .collect();
    assert!(hrefs.contains(&plain_href), "{hrefs:?}");
    let hrefs: Vec<String> = plain_client
        .request_with_headers("PROPFIND", PLAIN_INBOX, [("depth", "1")], PROPNAME_DATA)
        .await
        .with_status(StatusCode::MULTI_STATUS)
        .hrefs()
        .into_iter()
        .map(str::to_string)
        .collect();
    assert!(hrefs.contains(&plain_href), "{hrefs:?}");
    let body = plain_client
        .request("GET", &plain_href, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert_eq!(parse(&body), parse(&plain_ical));

    // Cleanup: upstream's DELETE, for the three items as they now stand
    // (the pending item's bit was cleared above).
    for href in [&pending_href, &drained_href, &legacy_href] {
        client
            .request("DELETE", href, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    plain_client
        .request("DELETE", &plain_href, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
}

/// Spec 13: a pending item drained between two syncs. The resource cache
/// knows no flags, so the item is skipped where it is loaded; once rewritten
/// in its drained form (same document id, bit cleared) a sync reports it.
pub async fn test_sync(test: &mut TestServer) {
    println!("Running zero-access inbox sync-collection test...");
    let name = "key1@example.com";
    let id = test.account(name).id().document_id();
    let sender = test.account("key2@example.com").id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let keys = za_unlock(test, id, STRONG).await;

    let t0 = client
        .sync_collection(KEY_INBOX, "", Depth::One, None, ["D:getetag"])
        .await
        .sync_token()
        .to_string();
    let ical = invitation("za-inbox-sync", "inbox-sync-canary");
    let item = plant_inbox_item(
        test,
        id,
        &ical,
        PlantedItem::Pending {
            pair: Some(("key1@example.com", [0x33; 16])),
            reply_token: None,
        },
        sender,
        None,
    )
    .await;
    let href = item_href(KEY_INBOX, item);
    let reported = |hrefs: Vec<&str>| hrefs.iter().filter(|h| **h == href).count();

    let response = client
        .sync_collection(KEY_INBOX, &t0, Depth::One, None, ["D:getetag"])
        .await;
    assert_eq!(reported(response.hrefs()), 0, "a pending item was reported");
    assert!(response.hrefs().is_empty(), "{:?}", response.hrefs());
    let t1 = response.sync_token().to_string();
    // The token advances past a skipped insert.
    assert_ne!(t0, t1);

    rewrite_inbox_item(test, id, item, |stored| {
        stored.event = parse(&ical);
        seal_drained_item(&mut stored.event, &keys, id).unwrap();
        stored.flags &= !EVENT_NOTIFICATION_ZA_PENDING;
    })
    .await;

    for (token, taken) in [(&t1, "while it was pending"), (&t0, "before it arrived")] {
        let response = client
            .sync_collection(KEY_INBOX, token, Depth::One, None, ["D:getetag"])
            .await;
        assert_eq!(
            reported(response.hrefs()),
            1,
            "drained item from a token taken {taken}: {:?}",
            response.hrefs()
        );
        assert_eq!(response.hrefs().len(), 1, "{:?}", response.hrefs());
        let body = response.body.as_deref().unwrap_or_default();
        assert!(body.contains("HTTP/1.1 200"), "{body}");
    }
    client
        .request("GET", &href, "")
        .await
        .with_status(StatusCode::OK);
    client
        .request("DELETE", &href, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
}

/// Upgrade ruling 174 (R-b): upstream's inbox expiry deletes by creation
/// time and needs no key, but a pending item waits for the drain however
/// old it is (invitations spec 6.6): an invitee who connects no client
/// within `expungeSchedulingInboxAfter` keeps the invitation. With the hold
/// period set to one second, the account purge removes key1's drained item
/// and the ordinary account's item, and keeps key1's pending one.
pub async fn test_expiry(test: &mut TestServer) {
    use ::registry::schema::{prelude::Property, structs::DataRetention};
    use ::registry::types::duration::Duration;
    println!("Running zero-access inbox expiry test...");
    let admin = test.account("admin@example.com").clone();
    let key1 = test.account("key1@example.com").clone();
    let plain = test.account("plain@example.com").clone();
    let id = key1.id().document_id();
    let plain_id = plain.id().document_id();
    let sender = test.account("key2@example.com").id().document_id();
    let prior = admin
        .registry_get::<DataRetention>(types::id::Id::singleton())
        .await
        .expunge_scheduling_inbox_after;
    let hold = |after: Option<Duration>| {
        let admin = admin.clone();
        async move {
            admin
                .registry_update_setting(
                    DataRetention {
                        expunge_scheduling_inbox_after: after,
                        ..Default::default()
                    },
                    &[Property::ExpungeSchedulingInboxAfter],
                )
                .await;
            admin.reload_settings().await;
        }
    };
    hold(Some(Duration::from_millis(1000))).await;
    // Every item is ten seconds old: past the one-second hold period.
    let created = Some(now() as i64 - 10);
    let pending = plant_inbox_item(
        test,
        id,
        &invitation("za-inbox-old", "inbox-old-canary"),
        PlantedItem::Pending {
            pair: None,
            reply_token: None,
        },
        sender,
        created,
    )
    .await;
    let drained = plant_inbox_item(
        test,
        id,
        &invitation("za-inbox-old-drained", "inbox-old-drained-canary"),
        PlantedItem::Drained { password: STRONG },
        sender,
        created,
    )
    .await;
    let ordinary = plant_inbox_item(
        test,
        plain_id,
        &invitation("za-inbox-old-plain", "inbox-old-plain-canary"),
        PlantedItem::Plain { flags: 0 },
        sender,
        created,
    )
    .await;

    // The account purges, as the scheduler runs them: no session of key1.
    // Ruling 189: key1's `AutoExpunge` trace counts the one item expunged.
    let traced = super::mail::Traced::start(&[trc::EventType::Store(trc::StoreEvent::AutoExpunge)]);
    for account_id in [key1.id(), plain.id()] {
        admin
            .registry_create_object(Task::AccountMaintenance(TaskAccountMaintenance {
                account_id,
                maintenance_type: TaskAccountMaintenanceType::Purge,
                status: TaskStatus::now(),
            }))
            .await;
    }
    test.wait_for_tasks().await;
    hold(prior).await;
    let totals = traced
        .finish()
        .await
        .into_iter()
        .filter(|event| event.account_id == Some(id as u64))
        .map(|event| {
            event
                .keys
                .iter()
                .find(|(key, _)| *key == trc::Key::Total)
                .and_then(|(_, value)| value.to_uint())
        })
        .collect::<Vec<_>>();
    assert_eq!(totals, vec![Some(1)], "key1's AutoExpunge total");
    assert!(
        stored_item(test, id, pending).await.is_some(),
        "the purge removed a pending item"
    );
    assert!(
        stored_item(test, id, drained).await.is_none(),
        "an expired drained item survived the purge"
    );
    assert!(
        stored_item(test, plain_id, ordinary).await.is_none(),
        "an ordinary account's expired item survived the purge"
    );

    destroy_inbox_item(test, id, pending).await;
    test.wait_for_tasks().await;
}
