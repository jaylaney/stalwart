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
    za::{PlantedItem, plant_inbox_item, rewrite_inbox_item, za_unlock},
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
    write::{AlignedBytes, Archive, now},
};
use types::collection::Collection;

pub const KEY_INBOX: &str = "/dav/itip/key1%40example.com/inbox/";
const PLAIN_INBOX: &str = "/dav/itip/plain%40example.com/inbox/";
const PROPFIND_DATA: &str = "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"><D:prop><D:getetag/><A:calendar-data/></D:prop></D:propfind>";

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
) -> Option<Archive<AlignedBytes>> {
    test.server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
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
    // Review Focus 1: a multiget naming it answers 404 for that href.
    let body = client
        .multiget_calendar(KEY_INBOX, &[pending_href.as_str()])
        .await
        .response
        .body
        .unwrap();
    assert!(
        body.contains(&pending_href)
            && body.contains("HTTP/1.1 404")
            && !body.contains("inbox-pending-canary"),
        "{body}"
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
    let body = plain_client
        .request("GET", &plain_href, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert_eq!(parse(&body), parse(&plain_ical));

    // Cleanup: upstream's DELETE removes an item whatever its flags.
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
    let t1 = response.sync_token().to_string();

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

/// Spec 6.6: upstream's inbox expiry deletes by creation time and needs no
/// key, so it also removes pending items older than the hold period.
pub async fn test_expiry(test: &mut TestServer) {
    println!("Running zero-access inbox expiry test...");
    let admin = test.account("admin@example.com").clone();
    let key1 = test.account("key1@example.com").clone();
    let id = key1.id().document_id();
    let sender = test.account("key2@example.com").id().document_id();
    let client = DummyWebDavClient::new(id, key1.name(), STRONG, key1.name());
    let pending = || PlantedItem::Pending {
        pair: None,
        reply_token: None,
    };
    // Older than the default 30-day `expungeSchedulingInboxAfter`.
    let old = plant_inbox_item(
        test,
        id,
        &invitation("za-inbox-old", "inbox-old-canary"),
        pending(),
        sender,
        Some(now() as i64 - 31 * 86_400),
    )
    .await;
    let fresh = plant_inbox_item(
        test,
        id,
        &invitation("za-inbox-fresh", "inbox-fresh-canary"),
        pending(),
        sender,
        None,
    )
    .await;

    // The account purge, as the scheduler runs it: no session of key1.
    admin
        .registry_create_object(Task::AccountMaintenance(TaskAccountMaintenance {
            account_id: key1.id(),
            maintenance_type: TaskAccountMaintenanceType::Purge,
            status: TaskStatus::now(),
        }))
        .await;
    test.wait_for_tasks().await;
    assert!(
        stored_item(test, id, old).await.is_none(),
        "an expired pending item survived the purge"
    );
    assert!(
        stored_item(test, id, fresh).await.is_some(),
        "the purge removed a fresh item"
    );

    client
        .request("DELETE", &item_href(KEY_INBOX, fresh), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    test.wait_for_tasks().await;
}
