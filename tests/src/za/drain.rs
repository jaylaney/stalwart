/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6: the drain. A key account's own DAV request opens its
//! pending inbox items, applies them through upstream's merge and reseals
//! them; the flows of spec 4 and the cases of spec 11.

use super::{
    STRONG,
    dav_seal::raw_event,
    fanout::{
        CONTENT_TYPE, KEY1_CAL, NO_REPLY, Party, START_A, START_B, invite, stored_pairs,
        stored_sequence, uid_copies,
    },
};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        copy_href, envelope_wrap, inbox_ids, inbox_items, item_method, new_inbox_items,
        pending_ids, trigger_drain, za_uid_leases, za_unlock,
    },
};
use ::registry::schema::{prelude::Property, structs::CalendarScheduling};
use calcard::icalendar::ICalendarMethod;
use common::za::test_hooks::{self, DRAIN_SKIP_ALL};
use groupware::calendar::{
    CalendarEvent, ChangedBy, EVENT_NOTIFICATION_IS_CHANGE, EVENT_NOTIFICATION_ZA_PENDING,
};
use hyper::StatusCode;

/// The suite skips every account's drain (`za_tests` sets `DRAIN_SKIP_ALL`
/// after setup, so the plan 8 and 9 modules keep reading pending items);
/// a drain module turns it on for its duration.
pub struct DrainOn;

impl DrainOn {
    pub fn new() -> Self {
        test_hooks::clear_drain_skip(DRAIN_SKIP_ALL);
        DrainOn
    }
}

impl Drop for DrainOn {
    fn drop(&mut self) {
        test_hooks::set_drain_skip(DRAIN_SKIP_ALL);
    }
}

/// A GET as the owner's client reads it: 200, unsealed, no carrier.
pub async fn body(client: &DummyWebDavClient, href: &str) -> String {
    let body = client
        .request("GET", href, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(!body.contains("X-ZA-"), "{body}");
    body
}

/// `default/<name>` of a copy's href, for `raw_event` and `stored_pairs`.
pub fn relative(href: &str) -> String {
    format!("default/{}", href.rsplit('/').next().unwrap())
}

/// The stored item `document_id` of `account_id`.
pub async fn stored_item(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
) -> groupware::calendar::CalendarEventNotification {
    inbox_items(test, account_id)
        .await
        .into_iter()
        .find(|(id, _)| *id == document_id)
        .map(|(_, item)| item)
        .unwrap_or_else(|| panic!("item {document_id} is gone"))
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access drain tests...");
    let _drain = DrainOn::new();
    let admin = test.account("admin@example.com").clone();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;
    assert_eq!(za_uid_leases(test).await, 0);

    // 1. The session's first request drains (spec 6.2, trigger 1). key1
    // invites key2; the item waits, pending, until key2's first DAV
    // request, a PROPFIND of key2's principal: any DAV request drains, not
    // only a calendar one (Review Focus 5).
    let uid = "za-drain-1";
    let organizer = format!("{KEY1_CAL}drain-1.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let items = new_inbox_items(test, key2.id, &start2).await;
    assert_eq!(items.len(), 1);
    let item_id = items[0].0;
    assert_eq!(pending_ids(test, key2.id).await, [item_id]);
    assert_eq!(
        uid_copies(test, key2.id, uid).await,
        0,
        "a copy before the drain"
    );
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207); // (from code reading)
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "the item is still pending"
    );
    assert_eq!(uid_copies(test, key2.id, uid).await, 1, "one imported copy");
    let item = stored_item(test, key2.id, item_id).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    assert_eq!(
        item.flags & EVENT_NOTIFICATION_IS_CHANGE,
        0,
        "an import is not a change"
    );
    assert_eq!(item.changed_by, ChangedBy::PrincipalId(key1.id));
    assert_eq!(
        envelope_wrap(&item.event),
        Some(1),
        "resealed under the master key"
    );
    assert!(item.event_id.is_some(), "the item names its event");
    assert!(
        !item.event.to_string().contains("fo-title"),
        "the drained item is sealed"
    );
    // The copy as key2's client reads it: the invitation, on the default
    // calendar, with a schedule tag, no METHOD.
    let href = copy_href(test, key2.id, uid).await;
    assert!(href.contains("/default/"), "{href}");
    let copy = body(&key2.client, &href).await;
    assert!(
        copy.contains(&format!("fo-title {uid}"))
            && copy.contains("ORGANIZER:mailto:key1@example.com")
            && copy.contains("mailto:key2@example.com")
            && !copy.contains("METHOD:"),
        "{copy}"
    );
    let rel = relative(&href);
    let (archive, _) = raw_event(test, key2.id, &rel).await;
    assert!(
        archive
            .unarchive::<CalendarEvent>()
            .unwrap()
            .schedule_tag
            .is_some()
    );
    // Spec 3.3, "Attendee side": the copy's bundle holds key2's pair, the
    // token key1 handed out, and nothing else.
    let key2_token = *stored_pairs(test, &keys1, key1.id, "default/drain-1.ics")
        .await
        .expect("key1's pairs")
        .get("key2@example.com")
        .expect("key2's pair");
    let copy_pairs = stored_pairs(test, &keys2, key2.id, &rel)
        .await
        .expect("the copy's pairs");
    assert_eq!(copy_pairs.len(), 1);
    assert_eq!(copy_pairs.get("key2@example.com"), Some(&key2_token));
    // The inbox as clients see it (spec 6.6): listed and readable, METHOD
    // intact.
    let listing = key2
        .client
        .request_with_headers("PROPFIND", &key2.inbox(), [("depth", "1")], "")
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let item_href = format!("{}{item_id}.ics", key2.inbox());
    assert!(
        listing.hrefs().contains(&item_href.as_str()),
        "{:?}",
        listing.hrefs()
    );
    let shown = body(&key2.client, &item_href).await;
    assert!(
        shown.contains("METHOD:REQUEST") && shown.contains(&format!("fo-title {uid}")),
        "{shown}"
    );
    assert_eq!(za_uid_leases(test).await, 0, "the import left its lease");

    // 2. Mid-session arrival (trigger 2): key2 is connected; the next
    // invitation is drained by key2's next request, a GET of the first copy.
    let uid2 = "za-drain-2";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-2.ics"),
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    body(&key2.client, &href).await;
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid2).await, 1);

    // 3. Update (spec 4.2; Review Focus 3): key2 adds an alarm to its copy
    // (a PUT that builds no message), then key1 moves the event. The merge
    // keeps key2's alarm and takes the new time and SEQUENCE; the drained
    // item is a change.
    let with_alarm = copy.replace(
        "END:VEVENT",
        "BEGIN:VALARM\r\nTRIGGER:-PT10M\r\nACTION:DISPLAY\r\nDESCRIPTION:drain-alarm\r\nEND:VALARM\r\nEND:VEVENT",
    );
    key2.client
        .request_with_headers("PUT", &href, [CONTENT_TYPE], with_alarm)
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        new_inbox_items(test, key1.id, &start1).await.len(),
        0,
        "an alarm-only PUT of the copy built a reply"
    );
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_B, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let update_item = pending_ids(test, key2.id).await;
    assert_eq!(update_item.len(), 1, "the update reached key2");
    let merged = body(&key2.client, &href).await;
    assert!(
        merged.contains(&format!("DTSTART:{START_B}")) && merged.contains("drain-alarm"),
        "{merged}"
    );
    assert_eq!(stored_sequence(&key2.client, &href).await, 1);
    assert_eq!(uid_copies(test, key2.id, uid).await, 1);
    let item = stored_item(test, key2.id, update_item[0]).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    assert_ne!(
        item.flags & EVENT_NOTIFICATION_IS_CHANGE,
        0,
        "a merge is a change"
    );

    // 4. Cancel (spec 4.3): key1's DELETE sends a CANCEL; key2's copy
    // becomes STATUS:CANCELLED and stays on the calendar.
    key1.client
        .request("DELETE", &organizer, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let cancel_item = pending_ids(test, key2.id).await;
    assert_eq!(cancel_item.len(), 1);
    let cancelled = body(&key2.client, &href).await;
    assert!(cancelled.contains("STATUS:CANCELLED"), "{cancelled}");
    assert_eq!(uid_copies(test, key2.id, uid).await, 1);
    let item = stored_item(test, key2.id, cancel_item[0]).await;
    assert_eq!(item_method(&item.event), Some(ICalendarMethod::Cancel));
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);

    // 5. A local sender always adds (spec 6.4): with auto-add off, the
    // invitation still lands on key2's default calendar.
    admin
        .registry_update_setting(
            CalendarScheduling {
                auto_add_invitations: false,
                ..Default::default()
            },
            &[Property::AutoAddInvitations],
        )
        .await;
    admin.reload_settings().await;
    let uid3 = "za-drain-3";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-3.ics"),
            [CONTENT_TYPE],
            invite(uid3, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(
        uid_copies(test, key2.id, uid3).await,
        1,
        "auto-add off kept the copy out"
    );
    admin
        .registry_update_setting(
            CalendarScheduling::default(),
            &[Property::AutoAddInvitations],
        )
        .await;
    admin.reload_settings().await;

    // Cleanup: the copies with `Schedule-Reply: F` (no declined REPLY), the
    // organizer's events the same way, every item written here.
    for uid in [uid, uid2, uid3] {
        let href = copy_href(test, key2.id, uid).await;
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for name in ["drain-2", "drain-3"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key2.delete_items_since(test, &start2).await;
    key1.delete_items_since(test, &start1).await;
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}
