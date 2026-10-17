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
        CONTENT_TYPE, KEY1_CAL, MKCALENDAR, NO_REPLY, Party, START_A, START_B, attendee_statuses,
        invite, pair_token, stored_pairs, stored_sequence, uid_copies,
    },
    uid_lease::put,
};
use crate::utils::{
    http::HttpRequest,
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        HookParked, PlantedItem, copy_href, envelope_wrap, inbox_ids, inbox_items, item_method,
        item_partstat, new_inbox_items, open_pending_item_as, pending_ids, plant_inbox_item,
        rewrite_inbox_item, trigger_drain, wait_for_delivery, za_app_password, za_post,
        za_uid_leases, za_unlock,
    },
};
use ::registry::{
    schema::{
        enums::StorageQuota,
        prelude::{ObjectType, Property},
        structs::CalendarScheduling,
    },
    types::EnumImpl,
};
use calcard::icalendar::{ICalendarMethod, ICalendarValue};
use common::{
    auth::credential::AppPassword,
    za::test_hooks::{self, DRAIN_SKIP_ALL},
};
use groupware::calendar::{
    CalendarEvent, ChangedBy, EVENT_NOTIFICATION_IS_CHANGE, EVENT_NOTIFICATION_ZA_PENDING,
};
use hyper::{Method, StatusCode};
use serde_json::json;
use std::time::Duration;

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

    // 6. A request for another account's data drains the caller's own inbox
    // (spec 6.1; the comment at the drain's call in
    // `crates/dav/src/request.rs`): key2's PROPFIND of key1's calendar
    // home, whatever it answers, drains key2's pending item first.
    let uid6 = "za-drain-6";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-6.ics"),
            [CONTENT_TYPE],
            invite(uid6, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    let status = HttpRequest::with_credentials(8899, key2.name, STRONG)
        .send_full(
            Method::from_bytes(b"PROPFIND").unwrap(),
            "/dav/cal/key1%40example.com/",
            None,
            None,
        )
        .await
        .status
        .as_u16();
    assert!(
        status < 500,
        "key2's request for key1's data answered {status}"
    );
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "a request for another account's data did not drain the caller"
    );
    assert_eq!(uid_copies(test, key2.id, uid6).await, 1);

    // Cleanup: the copies with `Schedule-Reply: F` (no declined REPLY), the
    // organizer's events the same way, every item written here.
    for uid in [uid, uid2, uid3, uid6] {
        let href = copy_href(test, key2.id, uid).await;
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for name in ["drain-2", "drain-3", "drain-6"] {
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

/// PUTs `href` back with `address`'s PARTSTAT set to `partstat`, keeping
/// the rest of the body as the client read it. Returns the new body.
pub async fn set_partstat(
    client: &DummyWebDavClient,
    href: &str,
    address: &str,
    partstat: &str,
) -> String {
    let current = body(client, href).await;
    // Unfold first (RFC 5545 3.1): a long ATTENDEE line may be folded.
    let unfolded = current.replace("\r\n ", "");
    let mut out = String::new();
    for line in unfolded.split("\r\n") {
        if line.starts_with("ATTENDEE") && line.ends_with(&format!("mailto:{address}")) {
            let line = match line.find("PARTSTAT=") {
                Some(at) => {
                    let end = line[at..].find([';', ':']).map_or(line.len(), |e| at + e);
                    format!("{}PARTSTAT={partstat}{}", &line[..at], &line[end..])
                }
                None => line.replacen("ATTENDEE;", &format!("ATTENDEE;PARTSTAT={partstat};"), 1),
            };
            out.push_str(&line);
        } else {
            out.push_str(line);
        }
        out.push_str("\r\n");
    }
    client
        .request_with_headers("PUT", href, [CONTENT_TYPE], out.clone())
        .await
        .with_status(StatusCode::NO_CONTENT);
    out
}

/// The PARTSTAT of `address` in the event at `href`, as its owner reads it.
pub async fn partstat_of(client: &DummyWebDavClient, href: &str, address: &str) -> Option<String> {
    let text = body(client, href).await;
    match calcard::Parser::new(&text).entry() {
        calcard::Entry::ICalendar(ical) => item_partstat(&ical, address),
        other => panic!("{other:?}"),
    }
}

pub async fn test_reply(test: &mut TestServer) {
    println!("Running zero-access drain reply tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;
    let start3 = inbox_ids(test, key3.id).await;

    // 1. A reply with a matching token applies (spec 4.4). Both sides first
    // rewrite without a message (spec 3.3, "Every rewrite carries the
    // pairs"): key1 adds an alarm to the organizer event, key2 adds one to
    // its copy; then key2 accepts, and key1's drain applies the reply.
    let uid = "za-drain-r1";
    let organizer = format!("{KEY1_CAL}drain-r1.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy = copy_href(test, key2.id, uid).await;
    let alarm = "BEGIN:VALARM\r\nTRIGGER:-PT5M\r\nACTION:DISPLAY\r\nDESCRIPTION:r1-alarm\r\nEND:VALARM\r\nEND:VEVENT";
    let organizer_body = body(&key1.client, &organizer)
        .await
        .replace("END:VEVENT", alarm);
    key1.client
        .request_with_headers("PUT", &organizer, [CONTENT_TYPE], organizer_body)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let copy_body = body(&key2.client, &copy).await.replace("END:VEVENT", alarm);
    key2.client
        .request_with_headers("PUT", &copy, [CONTENT_TYPE], copy_body)
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        new_inbox_items(test, key2.id, &start2).await.len(),
        1,
        "an alarm-only organizer PUT sent key2 another item"
    );
    set_partstat(&key2.client, &copy, "key2@example.com", "ACCEPTED").await;
    let replies = pending_ids(test, key1.id).await;
    assert_eq!(replies.len(), 1, "key1's inbox holds one pending reply");
    assert_eq!(
        partstat_of(&key1.client, &organizer, "key2@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "the reply did not apply"
    );
    assert!(pending_ids(test, key1.id).await.is_empty());
    let item = stored_item(test, key1.id, replies[0]).await;
    assert_ne!(item.flags & EVENT_NOTIFICATION_IS_CHANGE, 0);
    assert_eq!(item_method(&item.event), Some(ICalendarMethod::Reply));
    assert!(
        !item.event.to_string().contains("X-ZA-RSVP-TOKEN"),
        "the drained reply still carries its token"
    );
    let organizer_after = body(&key1.client, &organizer).await;
    assert!(organizer_after.contains("r1-alarm"), "{organizer_after}");
    // key1's pairs are intact after the merge.
    assert_eq!(
        stored_pairs(test, &keys1, key1.id, "default/drain-r1.ics")
            .await
            .map(|pairs| pairs.len()),
        Some(1)
    );

    // 2. A forged token is dropped (spec 3.3, "Verification"; 6.5): the
    // item is deleted and key1's copy is unchanged.
    let forged = plant_inbox_item(
        test,
        key1.id,
        &reply_tree(uid, "key1@example.com", "key2@example.com", "DECLINED", 0),
        PlantedItem::Pending {
            pair: None,
            reply_token: Some([0x42; 16]),
        },
        key2.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    assert!(
        !inbox_ids(test, key1.id).await.contains(&forged),
        "a forged reply was kept"
    );
    assert_eq!(
        partstat_of(&key1.client, &organizer, "key2@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "a forged reply changed the organizer's copy"
    );
    assert_eq!(uid_copies(test, key1.id, uid).await, 1);

    // 3. A stale reply is history, not applied (spec 6.4, REPLY). key2
    // declines at SEQUENCE 0; with key1's drain skipped, key1 reschedules
    // (SEQUENCE 1); the next key1 request drains the stale decline and
    // keeps it as history; key2's fresh reply at SEQUENCE 1 then applies.
    let uid2 = "za-drain-r2";
    let organizer2 = format!("{KEY1_CAL}drain-r2.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer2,
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy2 = copy_href(test, key2.id, uid2).await;
    set_partstat(&key2.client, &copy2, "key2@example.com", "DECLINED").await;
    let stale = pending_ids(test, key1.id).await;
    assert_eq!(stale.len(), 1);
    test_hooks::set_drain_skip(key1.id);
    key1.client
        .request_with_headers(
            "PUT",
            &organizer2,
            [CONTENT_TYPE],
            invite(uid2, START_B, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(stored_sequence(&key1.client, &organizer2).await, 1);
    assert_eq!(
        pending_ids(test, key1.id).await,
        stale,
        "the skip let the drain run"
    );
    test_hooks::clear_drain_skip(key1.id);
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    assert!(pending_ids(test, key1.id).await.is_empty());
    let item = stored_item(test, key1.id, stale[0]).await;
    assert_eq!(
        item.flags & EVENT_NOTIFICATION_IS_CHANGE,
        0,
        "a stale reply is not a change"
    );
    assert!(
        item.event_id.is_some(),
        "history names the event it was for"
    );
    assert_ne!(
        partstat_of(&key1.client, &organizer2, "key2@example.com")
            .await
            .as_deref(),
        Some("DECLINED"),
        "the stale decline applied"
    );
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(stored_sequence(&key2.client, &copy2).await, 1);
    set_partstat(&key2.client, &copy2, "key2@example.com", "ACCEPTED").await;
    assert_eq!(
        partstat_of(&key1.client, &organizer2, "key2@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "the fresh reply did not apply"
    );

    // 4. Delegation between three key accounts (spec 3.3, "Delegates";
    // 6.4). key2's REPLY carries key3's entry too, and key3's later REPLY
    // carries key2's: the drain picks the replier by `changed_by` and the
    // token, so neither direction depends on attendee order. key2
    // delegates to key3: the organizer's drain merges the REPLY
    // and invites key3 itself; key3's inbox holds exactly one REQUEST, from
    // key1; key3's reply applies to key1's copy; key2's inbox receives
    // nothing from key3 and key2's copy is unchanged by key3's reply.
    let uid3 = "za-drain-r3";
    let organizer3 = format!("{KEY1_CAL}drain-r3.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer3,
            [CONTENT_TYPE],
            invite(uid3, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy3 = copy_href(test, key2.id, uid3).await;
    let delegated = body(&key2.client, &copy3)
        .await
        .replace("\r\n ", "")
        .replace(
            "PARTSTAT=NEEDS-ACTION",
            "PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\"",
        )
        .replace(
            "END:VEVENT",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key2@example.com\":mailto:key3@example.com\r\nEND:VEVENT",
        );
    key2.client
        .request_with_headers("PUT", &copy3, [CONTENT_TYPE], delegated)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let key2_items_before = inbox_ids(test, key2.id).await;
    assert_eq!(
        new_inbox_items(test, key3.id, &start3).await.len(),
        0,
        "key2's own delegate invitation reached key3 (plan 9 forbids it)"
    );
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    let key3_items = new_inbox_items(test, key3.id, &start3).await;
    assert_eq!(key3_items.len(), 1, "key3 holds exactly one invitation");
    assert_eq!(key3_items[0].1.changed_by, ChangedBy::PrincipalId(key1.id));
    assert_eq!(
        item_method(&key3_items[0].1.event),
        Some(ICalendarMethod::Request)
    );
    assert_eq!(
        attendee_statuses(&key1.client, &organizer3, "key3@example.com").await,
        ["1.2"],
        "key1's copy marks key3 delivered"
    );
    let pairs = stored_pairs(test, &keys1, key1.id, "default/drain-r3.ics")
        .await
        .expect("pairs");
    assert_eq!(
        pairs.len(),
        2,
        "key1 holds a pair for key2 and one for key3"
    );
    assert!(pairs.get("key3@example.com").is_some());
    assert_eq!(trigger_drain(key3.name, STRONG).await, 207);
    let copy3_key3 = copy_href(test, key3.id, uid3).await;
    set_partstat(&key3.client, &copy3_key3, "key3@example.com", "ACCEPTED").await;
    assert_eq!(
        partstat_of(&key1.client, &organizer3, "key3@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "the delegate's reply did not apply to the organizer"
    );
    assert_eq!(
        inbox_ids(test, key2.id).await,
        key2_items_before,
        "key2 received an item from key3"
    );
    assert_eq!(
        partstat_of(&key2.client, &copy3, "key3@example.com")
            .await
            .as_deref(),
        Some("NEEDS-ACTION"),
        "key2's copy learnt key3's status from somewhere other than the organizer"
    );

    // 5. A stale occurrence reply with no stored override (spec 6.4,
    // REPLY). key2 declines one occurrence of a recurring invitation by
    // adding an EXDATE to its copy, which upstream sends as a REPLY with
    // RECURRENCE-ID at the master's SEQUENCE 0 (`attendee.rs`, the EXDATE
    // arm; from code reading). With key1's drain skipped, key1 reschedules
    // the master (SEQUENCE 1). key1's drain judges the occurrence against
    // the master: the decline is history and no override is added.
    let uid4 = "za-drain-r4";
    let organizer4 = format!("{KEY1_CAL}drain-r4.ics");
    let recurring = |start: &str| {
        invite(uid4, start, &["key2@example.com"]).replace(
            "DURATION:PT1H\r\n",
            "DURATION:PT1H\r\nRRULE:FREQ=DAILY;COUNT=5\r\n",
        )
    };
    key1.client
        .request_with_headers("PUT", &organizer4, [CONTENT_TYPE], recurring(START_A))
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy4 = copy_href(test, key2.id, uid4).await;
    let declined_occurrence = body(&key2.client, &copy4)
        .await
        .replace("END:VEVENT", "EXDATE:20990302T100000Z\r\nEND:VEVENT");
    key2.client
        .request_with_headers("PUT", &copy4, [CONTENT_TYPE], declined_occurrence)
        .await
        .with_status(StatusCode::NO_CONTENT);
    let occurrence_reply = pending_ids(test, key1.id).await;
    assert_eq!(
        occurrence_reply.len(),
        1,
        "the EXDATE built no occurrence reply"
    );
    test_hooks::set_drain_skip(key1.id);
    key1.client
        .request_with_headers("PUT", &organizer4, [CONTENT_TYPE], recurring(START_B))
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(stored_sequence(&key1.client, &organizer4).await, 1);
    test_hooks::clear_drain_skip(key1.id);
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    assert!(pending_ids(test, key1.id).await.is_empty());
    let item = stored_item(test, key1.id, occurrence_reply[0]).await;
    assert_eq!(
        item.flags & EVENT_NOTIFICATION_IS_CHANGE,
        0,
        "the stale occurrence reply applied"
    );
    let organizer4_body = body(&key1.client, &organizer4).await;
    assert_eq!(
        organizer4_body.matches("BEGIN:VEVENT").count(),
        1,
        "the stale decline added an override: {organizer4_body}"
    );
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);

    // 6. A removed attendee's reply is history, not a forgery (spec 3.3,
    // "Maintenance"; 6.5). key1 invites key2, key2 accepts; key1 removes
    // key2 (a CANCEL to key2; key1's copy drops key2's pair); key2's copy
    // turns cancelled and keeps its pair; key2 then declines from the
    // cancelled copy, which upstream's attendee builder sends as a REPLY.
    // The sender holds no pair in key1's bundle: key1's drain keeps the
    // item as history (not deleted), and neither copy changes.
    let uid5 = "za-drain-r5";
    let organizer5 = format!("{KEY1_CAL}drain-r5.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer5,
            [CONTENT_TYPE],
            invite(uid5, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy5 = copy_href(test, key2.id, uid5).await;
    set_partstat(&key2.client, &copy5, "key2@example.com", "ACCEPTED").await;
    assert_eq!(
        partstat_of(&key1.client, &organizer5, "key2@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "the acceptance did not apply"
    );
    key1.client
        .request_with_headers(
            "PUT",
            &organizer5,
            [CONTENT_TYPE],
            invite(uid5, START_A, &[]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        pending_ids(test, key1.id).await.is_empty(),
        "the removal left key1 an item"
    );
    assert!(
        stored_pairs(test, &keys1, key1.id, "default/drain-r5.ics")
            .await
            .is_none(),
        "the organizer kept a pair for a removed attendee"
    );
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    let cancelled = body(&key2.client, &copy5).await;
    assert!(cancelled.contains("STATUS:CANCELLED"), "{cancelled}");
    assert!(
        stored_pairs(test, &keys2, key2.id, &relative(&copy5))
            .await
            .expect("pairs")
            .get("key2@example.com")
            .is_some(),
        "the cancelled copy dropped its pair"
    );
    let organizer5_before = body(&key1.client, &organizer5).await;
    assert!(
        !organizer5_before.contains("key2@example.com"),
        "{organizer5_before}"
    );
    set_partstat(&key2.client, &copy5, "key2@example.com", "DECLINED").await;
    let removed_reply = pending_ids(test, key1.id).await;
    assert_eq!(
        removed_reply.len(),
        1,
        "the decline from the cancelled copy built no reply"
    );
    let copy5_before = body(&key2.client, &copy5).await;
    let (_, event5) = raw_event(test, key1.id, "default/drain-r5.ics").await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    let item = stored_item(test, key1.id, removed_reply[0]).await;
    assert_eq!(
        item.flags & EVENT_NOTIFICATION_ZA_PENDING,
        0,
        "the removed attendee's reply is still pending"
    );
    assert_eq!(
        item.flags & EVENT_NOTIFICATION_IS_CHANGE,
        0,
        "the removed attendee's reply applied"
    );
    assert_eq!(item.event_id, Some(event5), "history names the event");
    assert_eq!(
        body(&key1.client, &organizer5).await,
        organizer5_before,
        "the removed attendee's reply changed the organizer's copy"
    );
    assert_eq!(
        body(&key2.client, &copy5).await,
        copy5_before,
        "key1's drain changed key2's copy"
    );

    // 7. A delegation to someone who already attends another instance
    // (spec 6.4, "Delegates"): key1's recurring event invites key2 to the
    // series and key2 and key3 to an override of the second occurrence.
    // key2 delegates the series (the master) to key3; key1's drain merges
    // the REPLY, which adds key3 to the master, and invites key3 to the
    // series although key3 already attended the override: attendance is
    // compared per instance.
    let uid7 = "za-drain-r7";
    let organizer7 = format!("{KEY1_CAL}drain-r7.ics");
    let recurring7 = invite(uid7, START_A, &["key2@example.com"])
        .replace(
            "DURATION:PT1H\r\n",
            "DURATION:PT1H\r\nRRULE:FREQ=DAILY;COUNT=5\r\n",
        )
        .replace(
            "END:VEVENT\r\nEND:VCALENDAR\r\n",
            &format!(
                concat!(
                    "END:VEVENT\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n",
                    "DTSTAMP:20240101T000000Z\r\nRECURRENCE-ID:{start}\r\n",
                    "DTSTART:{start}\r\nDURATION:PT1H\r\nSEQUENCE:0\r\n",
                    "SUMMARY:fo-title {uid}\r\nORGANIZER:mailto:key1@example.com\r\n",
                    "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com\r\n",
                    "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key3@example.com\r\n",
                    "END:VEVENT\r\nEND:VCALENDAR\r\n"
                ),
                uid = uid7,
                start = START_B
            ),
        );
    key1.client
        .request_with_headers("PUT", &organizer7, [CONTENT_TYPE], recurring7)
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(trigger_drain(key3.name, STRONG).await, 207);
    let copy7 = copy_href(test, key2.id, uid7).await;
    let copy7_key3 = copy_href(test, key3.id, uid7).await;
    // key2's master only: its ATTENDEE delegated, key3 added beside it.
    let delegated7 = body(&key2.client, &copy7)
        .await
        .replace("\r\n ", "")
        .split("BEGIN:VEVENT")
        .enumerate()
        .map(|(at, part)| {
            if at == 0 || part.contains("RECURRENCE-ID") {
                part.to_string()
            } else {
                part.replace(
                    "PARTSTAT=NEEDS-ACTION",
                    "PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\"",
                )
                .replace(
                    "END:VEVENT",
                    "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key2@example.com\":mailto:key3@example.com\r\nEND:VEVENT",
                )
            }
        })
        .collect::<Vec<_>>()
        .join("BEGIN:VEVENT");
    key2.client
        .request_with_headers("PUT", &copy7, [CONTENT_TYPE], delegated7)
        .await
        .with_status(StatusCode::NO_CONTENT);
    // The fan-out is synchronous; `wait_for_delivery` would wait for step
    // 1's alarms, which are not due.
    assert_eq!(
        pending_ids(test, key1.id).await.len(),
        1,
        "the master's delegation built no reply"
    );
    let before3 = inbox_ids(test, key3.id).await;
    let before2 = inbox_ids(test, key2.id).await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    let key3_items = new_inbox_items(test, key3.id, &before3).await;
    assert_eq!(
        key3_items.len(),
        1,
        "key3 has exactly one new item: the organizer's series invitation"
    );
    assert_eq!(key3_items[0].1.changed_by, ChangedBy::PrincipalId(key1.id));
    assert_eq!(
        new_inbox_items(test, key2.id, &before2).await.len(),
        0,
        "key2 received an item from the delegation"
    );
    assert!(
        pending_ids(test, key1.id).await.is_empty(),
        "the delegation reply is still pending"
    );
    assert_eq!(trigger_drain(key3.name, STRONG).await, 207);
    let copy7_key3_body = body(&key3.client, &copy7_key3).await.replace("\r\n ", "");
    let key3_attends = |component: &str, delegated: bool| {
        component.lines().any(|line| {
            let line = line.to_ascii_lowercase();
            line.starts_with("attendee")
                && line.ends_with("mailto:key3@example.com")
                && (!delegated
                    || (line.contains("delegated-from=") && line.contains("key2@example.com")))
        })
    };
    let components: Vec<&str> = copy7_key3_body.split("BEGIN:VEVENT").skip(1).collect();
    let master7 = components
        .iter()
        .find(|component| !component.contains("RECURRENCE-ID"))
        .unwrap_or_else(|| panic!("key3's copy has no master: {copy7_key3_body}"));
    assert!(
        key3_attends(master7, true),
        "key3's master does not list key3 delegated from key2: {copy7_key3_body}"
    );
    let override7 = components
        .iter()
        .find(|component| component.contains("RECURRENCE-ID"))
        .unwrap_or_else(|| panic!("key3's copy lost the override: {copy7_key3_body}"));
    assert!(
        key3_attends(override7, false),
        "key3's override no longer lists key3: {copy7_key3_body}"
    );

    // 8. A delegation in an occurrence governed by a range override (spec
    // 6.4, "Delegates"): key1's recurring event's master has key2
    // delegated to key3 (key3 attends the series as key2's delegate), and
    // a THISANDFUTURE override from the second occurrence lists key2
    // only. key2 delegates the fourth occurrence to key3 again by adding
    // an override with the master's DELEGATED-TO and DELEGATED-FROM; the
    // merge creates it, and it inherits the range override's attendance,
    // not the master's, so key3 is new there and key1's drain invites it.
    // key3's DELEGATED-FROM gains nothing against the master, so only the
    // governing-instance rule makes key3 new.
    let uid8 = "za-drain-r8";
    let organizer8 = format!("{KEY1_CAL}drain-r8.ics");
    const FOURTH: &str = "20990304T100000Z";
    let override_of = |uid: &str, recurrence_id: &str, start: &str, attendees: &[&str]| {
        let mut component = format!(
            concat!(
                "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\n",
                "{recurrence_id}\r\nDTSTART:{start}\r\nDURATION:PT1H\r\n",
                "SEQUENCE:0\r\nSUMMARY:fo-title {uid}\r\n",
                "ORGANIZER:mailto:key1@example.com\r\n"
            ),
            uid = uid,
            recurrence_id = recurrence_id,
            start = start
        );
        for attendee in attendees {
            component.push_str(attendee);
            component.push_str("\r\n");
        }
        component.push_str("END:VEVENT\r\n");
        component
    };
    let recurring8 = invite(uid8, START_A, &["key2@example.com", "key3@example.com"])
        .replace(
            "DURATION:PT1H\r\n",
            "DURATION:PT1H\r\nRRULE:FREQ=DAILY;COUNT=5\r\n",
        )
        .replace(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com",
            "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\":mailto:key2@example.com",
        )
        .replace(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key3@example.com",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key2@example.com\":mailto:key3@example.com",
        )
        .replace(
            "END:VCALENDAR\r\n",
            &format!(
                "{}END:VCALENDAR\r\n",
                override_of(
                    uid8,
                    &format!("RECURRENCE-ID;RANGE=THISANDFUTURE:{START_B}"),
                    START_B,
                    &["ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com"],
                )
            ),
        );
    key1.client
        .request_with_headers("PUT", &organizer8, [CONTENT_TYPE], recurring8)
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(trigger_drain(key3.name, STRONG).await, 207);
    let copy8 = copy_href(test, key2.id, uid8).await;
    let copy8_key3 = copy_href(test, key3.id, uid8).await;
    let delegated8 = body(&key2.client, &copy8)
        .await
        .replace("\r\n ", "")
        .replace(
            "END:VCALENDAR\r\n",
            &format!(
                "{}END:VCALENDAR\r\n",
                override_of(
                    uid8,
                    &format!("RECURRENCE-ID:{FOURTH}"),
                    FOURTH,
                    &[
                        "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\":mailto:key2@example.com",
                        "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key2@example.com\":mailto:key3@example.com",
                    ],
                )
            ),
        );
    key2.client
        .request_with_headers("PUT", &copy8, [CONTENT_TYPE], delegated8)
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        pending_ids(test, key1.id).await.len(),
        1,
        "the occurrence's delegation built no reply"
    );
    let before3 = inbox_ids(test, key3.id).await;
    let before2 = inbox_ids(test, key2.id).await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    let key3_items = new_inbox_items(test, key3.id, &before3).await;
    assert_eq!(
        key3_items.len(),
        1,
        "key3 has exactly one new item: the organizer's occurrence invitation"
    );
    assert_eq!(key3_items[0].1.changed_by, ChangedBy::PrincipalId(key1.id));
    assert_eq!(
        new_inbox_items(test, key2.id, &before2).await.len(),
        0,
        "key2 received an item from the delegation"
    );
    assert!(
        pending_ids(test, key1.id).await.is_empty(),
        "the delegation reply is still pending"
    );
    assert_eq!(trigger_drain(key3.name, STRONG).await, 207);
    let copy8_key3_body = body(&key3.client, &copy8_key3).await.replace("\r\n ", "");
    let fourth8 = copy8_key3_body
        .split("BEGIN:VEVENT")
        .skip(1)
        .find(|component| {
            component
                .lines()
                .any(|line| line.starts_with("RECURRENCE-ID") && line.ends_with(FOURTH))
        })
        .unwrap_or_else(|| panic!("key3's copy has no fourth occurrence: {copy8_key3_body}"));
    assert!(
        key3_attends(fourth8, true),
        "key3's fourth occurrence does not list key3 delegated from key2: {copy8_key3_body}"
    );

    // 9. A delegation to an existing attendee (spec 6.4, "Delegates"):
    // key1 invites key2 and key3; key3 declines and key1's drain applies
    // it, while key2's copy still shows key3 NEEDS-ACTION (a reply reaches
    // only the organizer). key2 then delegates to key3 by marking key3's
    // existing ATTENDEE as delegated from key2. key1's merge resets key3 to
    // NEEDS-ACTION, delegated from key2; key3's DELEGATED-FROM gained key2,
    // so key1's drain invites key3 although key3 already attended.
    let uid9 = "za-drain-r9";
    let organizer9 = format!("{KEY1_CAL}drain-r9.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer9,
            [CONTENT_TYPE],
            invite(uid9, START_A, &["key2@example.com", "key3@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(trigger_drain(key3.name, STRONG).await, 207);
    let copy9 = copy_href(test, key2.id, uid9).await;
    let copy9_key3 = copy_href(test, key3.id, uid9).await;
    set_partstat(&key3.client, &copy9_key3, "key3@example.com", "DECLINED").await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    assert_eq!(
        partstat_of(&key1.client, &organizer9, "key3@example.com")
            .await
            .as_deref(),
        Some("DECLINED"),
        "key3's decline did not apply"
    );
    assert_eq!(
        partstat_of(&key2.client, &copy9, "key3@example.com")
            .await
            .as_deref(),
        Some("NEEDS-ACTION"),
        "key2's copy learnt key3's decline"
    );
    let delegated9 = body(&key2.client, &copy9)
        .await
        .replace("\r\n ", "")
        .split("\r\n")
        .map(|line| {
            if line.starts_with("ATTENDEE") && line.ends_with("mailto:key2@example.com") {
                line.replace(
                    "PARTSTAT=NEEDS-ACTION",
                    "PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\"",
                )
            } else if line.starts_with("ATTENDEE") && line.ends_with("mailto:key3@example.com") {
                line.replacen(
                    "ATTENDEE;",
                    "ATTENDEE;DELEGATED-FROM=\"mailto:key2@example.com\";",
                    1,
                )
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    key2.client
        .request_with_headers("PUT", &copy9, [CONTENT_TYPE], delegated9)
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        pending_ids(test, key1.id).await.len(),
        1,
        "the delegation to an existing attendee built no reply"
    );
    let before3 = inbox_ids(test, key3.id).await;
    let before2 = inbox_ids(test, key2.id).await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    let key3_items = new_inbox_items(test, key3.id, &before3).await;
    assert_eq!(
        key3_items.len(),
        1,
        "key3 has exactly one new item: the organizer's invitation as a delegate"
    );
    assert_eq!(key3_items[0].1.changed_by, ChangedBy::PrincipalId(key1.id));
    assert_eq!(
        new_inbox_items(test, key2.id, &before2).await.len(),
        0,
        "key2 received an item from the delegation"
    );
    assert!(
        pending_ids(test, key1.id).await.is_empty(),
        "the delegation reply is still pending"
    );
    assert_eq!(
        partstat_of(&key1.client, &organizer9, "key3@example.com")
            .await
            .as_deref(),
        Some("NEEDS-ACTION"),
        "key1's copy did not reset key3"
    );
    let organizer9_body = body(&key1.client, &organizer9).await.replace("\r\n ", "");
    assert!(
        key3_attends(&organizer9_body, true),
        "key1's copy does not list key3 delegated from key2: {organizer9_body}"
    );
    assert_eq!(trigger_drain(key3.name, STRONG).await, 207);
    let copy9_key3_body = body(&key3.client, &copy9_key3).await.replace("\r\n ", "");
    assert!(
        key3_attends(&copy9_key3_body, true),
        "key3's copy does not list key3 delegated from key2: {copy9_key3_body}"
    );

    // 10. The sender of a REPLY is resolved before its token is checked
    // (spec 6.4 step 2): a REPLY naming key2 as the replier and carrying
    // key2's real token, planted as if key3 had sent it, is history for a
    // sender mismatch, not applied and not a forgery; key1's copy keeps
    // key2's status from step 1.
    let real_token = *stored_pairs(test, &keys1, key1.id, "default/drain-r1.ics")
        .await
        .expect("key1's pairs for drain-r1")
        .get("key2@example.com")
        .expect("key2's pair");
    let misattributed = plant_inbox_item(
        test,
        key1.id,
        &reply_tree(
            "za-drain-r1",
            "key1@example.com",
            "key2@example.com",
            "DECLINED",
            0,
        ),
        PlantedItem::Pending {
            pair: None,
            reply_token: Some(real_token),
        },
        key3.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    let item = stored_item(test, key1.id, misattributed).await;
    assert_eq!(
        item.flags & EVENT_NOTIFICATION_ZA_PENDING,
        0,
        "the item is still pending"
    );
    assert_eq!(
        item.flags & EVENT_NOTIFICATION_IS_CHANGE,
        0,
        "a misattributed reply was a change"
    );
    assert_eq!(item.event_id, None);
    assert_eq!(
        partstat_of(
            &key1.client,
            &format!("{KEY1_CAL}drain-r1.ics"),
            "key2@example.com"
        )
        .await
        .as_deref(),
        Some("ACCEPTED"),
        "a reply from the wrong principal applied"
    );

    // Cleanup.
    for (party, uid) in [
        (&key2, uid),
        (&key2, uid2),
        (&key2, uid3),
        (&key3, uid3),
        (&key2, uid4),
        (&key2, uid5),
        (&key2, uid7),
        (&key3, uid7),
        (&key2, uid8),
        (&key3, uid8),
        (&key2, uid9),
        (&key3, uid9),
    ] {
        let href = copy_href(test, party.id, uid).await;
        party
            .client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for name in [
        "drain-r1", "drain-r2", "drain-r3", "drain-r4", "drain-r5", "drain-r7", "drain-r8",
        "drain-r9",
    ] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    key3.delete_items_since(test, &start3).await;
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}

/// A REPLY as upstream's attendee builder shapes it: one VEVENT with the
/// ORGANIZER, the replying ATTENDEE and the UID.
pub fn reply_tree(
    uid: &str,
    organizer: &str,
    attendee: &str,
    partstat: &str,
    sequence: i64,
) -> String {
    format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nMETHOD:REPLY\r\n",
            "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nSEQUENCE:{sequence}\r\n",
            "ORGANIZER:mailto:{organizer}\r\n",
            "ATTENDEE;PARTSTAT={partstat}:mailto:{attendee}\r\n",
            "REQUEST-STATUS:2.0;Success\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        ),
        uid = uid,
        sequence = sequence,
        organizer = organizer,
        attendee = attendee,
        partstat = partstat
    )
}

pub async fn test_adoption(test: &mut TestServer) {
    println!("Running zero-access drain pair adoption tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;
    let key2_pair = |pairs: Option<groupware::calendar::seal::TokenPairs>| {
        *pairs
            .expect("pairs")
            .get("key2@example.com")
            .expect("key2's pair")
    };

    // 1. Remove, re-add, accept (spec 3.3, 6.4): the CANCEL leaves key2's
    // copy with its old pair; the re-invitation's merge adopts the new
    // token; key2's reply then applies under it.
    let uid = "za-drain-a1";
    let organizer = format!("{KEY1_CAL}drain-a1.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let token_1 = key2_pair(stored_pairs(test, &keys1, key1.id, "default/drain-a1.ics").await);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy = copy_href(test, key2.id, uid).await;
    let rel = relative(&copy);
    assert_eq!(
        key2_pair(stored_pairs(test, &keys2, key2.id, &rel).await),
        token_1
    );
    // Removed: a CANCEL with the old pair; key2's copy keeps its pair.
    key1.client
        .request_with_headers("PUT", &organizer, [CONTENT_TYPE], invite(uid, START_A, &[]))
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert!(
        stored_pairs(test, &keys1, key1.id, "default/drain-a1.ics")
            .await
            .is_none(),
        "the organizer kept a pair for a removed attendee"
    );
    let cancelled = body(&key2.client, &copy).await;
    assert!(cancelled.contains("STATUS:CANCELLED"), "{cancelled}");
    assert_eq!(
        key2_pair(stored_pairs(test, &keys2, key2.id, &rel).await),
        token_1
    );
    // Re-added: a fresh token; the merge into the cancelled copy adopts it.
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let token_2 = key2_pair(stored_pairs(test, &keys1, key1.id, "default/drain-a1.ics").await);
    assert_ne!(token_1, token_2, "a re-added attendee gets a new token");
    body(&key2.client, &copy).await;
    assert_eq!(uid_copies(test, key2.id, uid).await, 1);
    assert_eq!(
        key2_pair(stored_pairs(test, &keys2, key2.id, &rel).await),
        token_2,
        "the copy did not adopt the new token"
    );
    set_partstat(&key2.client, &copy, "key2@example.com", "ACCEPTED").await;
    assert_eq!(
        partstat_of(&key1.client, &organizer, "key2@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "the reply under the new token did not apply"
    );

    // The old token is a forgery now (spec 3.3, "Verification"): a reply
    // planted under it is deleted and the copy's status stands.
    let stale_token = plant_inbox_item(
        test,
        key1.id,
        &reply_tree(uid, "key1@example.com", "key2@example.com", "DECLINED", 0),
        PlantedItem::Pending {
            pair: None,
            reply_token: Some(token_1),
        },
        key2.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    assert!(
        !inbox_ids(test, key1.id).await.contains(&stale_token),
        "a reply under the replaced token was kept"
    );
    assert_eq!(
        partstat_of(&key1.client, &organizer, "key2@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "a reply under the replaced token applied"
    );

    // 2. Failed-create retry (spec 5.4): key1's first commit fails after
    // the fan-out wrote key2's item; the resend delivers a second item with
    // an equal SEQUENCE and a fresh token. key2 drains both in order: the
    // first imports, the second merges and adopts the new token; the reply
    // applies.
    let uid2 = "za-drain-a2";
    let organizer2 = format!("{KEY1_CAL}drain-a2.ics");
    test_hooks::set_commit_fail_once(Some(key1.id));
    key1.client
        .request_with_headers(
            "PUT",
            &organizer2,
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "the item written before the failed commit"
    );
    key1.client
        .request_with_headers(
            "PUT",
            &organizer2,
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 2);
    let stored_token = key2_pair(stored_pairs(test, &keys1, key1.id, "default/drain-a2.ics").await);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "both items drained"
    );
    assert_eq!(
        uid_copies(test, key2.id, uid2).await,
        1,
        "one copy for two items"
    );
    let copy2 = copy_href(test, key2.id, uid2).await;
    assert_eq!(
        key2_pair(stored_pairs(test, &keys2, key2.id, &relative(&copy2)).await),
        stored_token,
        "the copy holds the orphan token, not the stored one"
    );
    set_partstat(&key2.client, &copy2, "key2@example.com", "ACCEPTED").await;
    assert_eq!(
        partstat_of(&key1.client, &organizer2, "key2@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED")
    );

    // Cleanup.
    for href in [copy, copy2] {
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for name in ["drain-a1", "drain-a2"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}

pub async fn test_lease(test: &mut TestServer) {
    println!("Running zero-access drain creation lease tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;
    let start3 = inbox_ids(test, key3.id).await;
    let app = za_app_password(key2.name, STRONG, "drain-second-session").await;
    test_hooks::set_drain_retry_override(Some(0));

    // A. Drain versus drain (spec 6.3, 11). Two fan-outs for one UID
    // publish the lower document id late. The late one is key1's PUT, held
    // at the fan-out pause after its item's id was assigned; key1 holds
    // its own creation lease for the UID while held, so the second
    // fan-out comes from another organizer, key3, whose PUT of the same
    // UID contends on nothing and delivers the higher id. Session 1 drains
    // the higher id and is held, holding key2's creation lease, right
    // before its commit; the first PUT then commits the lower id. Session
    // 2 drains: both items find no event, the lease is busy, both stay
    // pending. Session 1 commits one copy (key3's invitation); its next
    // request finds the lower id, whose ORGANIZER differs, and keeps it as
    // history. Exactly one copy, both items drained.
    let uid = "za-drain-l1";
    let parked_put = HookParked::start(
        key2.id,
        test_hooks::set_fan_out_pause,
        put(
            key1.name,
            STRONG,
            format!("{KEY1_CAL}drain-l1.ics"),
            invite(uid, START_A, &["key2@example.com"]),
        ),
    )
    .await;
    key3.client
        .request_with_headers(
            "PUT",
            "/dav/cal/key3%40example.com/default/drain-l1.ics",
            [CONTENT_TYPE],
            super::reply::copy(
                uid,
                "key3@example.com",
                &["ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com"],
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    let high = pending_ids(test, key2.id).await;
    assert_eq!(high.len(), 1, "key3's item is the only committed one");
    let session_1 = HookParked::start(
        key2.id,
        test_hooks::set_drain_lease_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    assert_eq!(parked_put.finish().await, 201, "the held PUT");
    let both = pending_ids(test, key2.id).await;
    assert_eq!(both.len(), 2);
    assert!(
        both[0] < both[1] && both[1] == high[0],
        "the lower id arrived late: {both:?}"
    );
    let waited = std::time::Instant::now();
    assert_eq!(
        trigger_drain(key2.name, &app).await,
        207,
        "session 2's request"
    );
    assert!(
        waited.elapsed() >= Duration::from_secs(1),
        "session 2 did not wait on the lease ({:?})",
        waited.elapsed()
    );
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        2,
        "session 2 imported past the lease"
    );
    assert_eq!(uid_copies(test, key2.id, uid).await, 0);
    assert_eq!(session_1.finish().await, 207);
    assert_eq!(
        uid_copies(test, key2.id, uid).await,
        1,
        "session 1 committed one copy"
    );
    assert_eq!(
        pending_ids(test, key2.id).await,
        [both[0]],
        "the late item waits for a pass"
    );
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(
        uid_copies(test, key2.id, uid).await,
        1,
        "a second copy was imported"
    );
    assert_eq!(za_uid_leases(test).await, 0);
    // The copy is key3's invitation, with key3's token for key2; key1's
    // late item was kept as history (OrganizerMismatch), its event_id set.
    let keys3 = za_unlock(test, key3.id, STRONG).await;
    let key3_token = *stored_pairs(test, &keys3, key3.id, "default/drain-l1.ics")
        .await
        .expect("pairs")
        .get("key2@example.com")
        .unwrap();
    let copy = copy_href(test, key2.id, uid).await;
    assert_eq!(
        stored_pairs(test, &keys2, key2.id, &relative(&copy))
            .await
            .expect("pairs")
            .get("key2@example.com"),
        Some(&key3_token)
    );
    let late = stored_item(test, key2.id, both[0]).await;
    assert_eq!(late.flags & EVENT_NOTIFICATION_IS_CHANGE, 0);
    assert!(late.event_id.is_some());
    let copy_body = body(&key2.client, &copy).await;
    assert!(
        copy_body.contains("ORGANIZER:mailto:key3@example.com"),
        "{copy_body}"
    );

    // B. Drain versus PUT (spec 6.3, 11): session 1 is held after its first
    // lookup found nothing, before the lease; session 2 (drain skipped)
    // PUTs the same UID into key2's default calendar and wins; session 1
    // takes the lease, finds the copy and merges. One copy.
    let uid2 = "za-drain-l2";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-l2.ics"),
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let session_1 = HookParked::start(
        key2.id,
        test_hooks::set_drain_lookup_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    test_hooks::set_drain_skip(key2.id);
    let own = format!("/dav/cal/key2%40example.com/default/drain-l2-own.ics");
    assert_eq!(
        put(
            key2.name,
            &app,
            own.clone(),
            super::reply::copy(
                uid2,
                "key1@example.com",
                &["ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com"]
            ),
        )
        .await,
        201,
        "session 2's PUT of the UID"
    );
    test_hooks::clear_drain_skip(key2.id);
    assert_eq!(session_1.finish().await, 207);
    assert_eq!(
        uid_copies(test, key2.id, uid2).await,
        1,
        "the drain imported a second copy"
    );
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "the drain did not merge"
    );
    let merged = body(&key2.client, &own).await;
    assert!(
        merged.contains(&format!("fo-title {uid2}")),
        "the merge did not reach session 2's copy: {merged}"
    );
    assert_eq!(za_uid_leases(test).await, 0);

    // C. Lease expiry (spec 6.3 step 3, 11): session 1 holds the lease past
    // its 1-second duration; session 2's PUT takes the expired lease over
    // and stores the UID; session 1's fenced commit is lost, the item stays
    // pending, and the next pass merges. One copy.
    let uid3 = "za-drain-l3";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-l3.ics"),
            [CONTENT_TYPE],
            invite(uid3, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    test_hooks::set_lease_secs_override(Some(1));
    let session_1 = HookParked::start(
        key2.id,
        test_hooks::set_drain_lease_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    test_hooks::set_lease_secs_override(None);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    test_hooks::set_drain_skip(key2.id);
    let own3 = format!("/dav/cal/key2%40example.com/default/drain-l3-own.ics");
    assert_eq!(
        put(
            key2.name,
            &app,
            own3.clone(),
            super::reply::copy(
                uid3,
                "key1@example.com",
                &["ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com"]
            ),
        )
        .await,
        201,
        "the expired lease was not taken over"
    );
    test_hooks::clear_drain_skip(key2.id);
    assert_eq!(
        session_1.finish().await,
        207,
        "a lost fence failed the request"
    );
    assert_eq!(
        uid_copies(test, key2.id, uid3).await,
        1,
        "the expired holder committed"
    );
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "the lost commit drained the item"
    );
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid3).await, 1);
    assert_eq!(za_uid_leases(test).await, 0);

    // D. The item deleted under a pass (Review Focus 1): session 1 holds
    // the lease before its commit; session 2 deletes the item by href; the
    // commit loses the item's assertion, nothing is imported, the lease is
    // released, the request succeeds.
    let uid4 = "za-drain-l4";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-l4.ics"),
            [CONTENT_TYPE],
            invite(uid4, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let item = pending_ids(test, key2.id).await;
    assert_eq!(item.len(), 1);
    let session_1 = HookParked::start(
        key2.id,
        test_hooks::set_drain_lease_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    test_hooks::set_drain_skip(key2.id);
    let item_href = format!("{}{}.ics", key2.inbox(), item[0]);
    assert_eq!(
        crate::utils::http::HttpRequest::with_credentials(8899, key2.name, &app)
            .send_full(hyper::Method::DELETE, &item_href, None, None)
            .await
            .status
            .as_u16(),
        204
    );
    test_hooks::clear_drain_skip(key2.id);
    assert_eq!(session_1.finish().await, 207);
    assert_eq!(
        uid_copies(test, key2.id, uid4).await,
        0,
        "a copy without its item"
    );
    assert!(!inbox_ids(test, key2.id).await.contains(&item[0]));
    assert_eq!(
        za_uid_leases(test).await,
        0,
        "the lost commit kept its lease"
    );

    // E. Two sessions of the organizer and one delegating REPLY (spec 6.3,
    // 6.4 "Delegates"): key2 delegates to key3, so key1's drain of the
    // REPLY fans out to key3 and first takes key1's creation lease for the
    // UID. Session 1 (key1's password session) holds that lease, parked
    // inside the fan-out to key3; session 2 (key1's app-password session)
    // finds the lease busy and defers. Exactly one REQUEST reaches key3,
    // with the token key1's copy keeps; session 2's retry finds nothing to
    // do.
    let uid5 = "za-drain-l5";
    let organizer5 = format!("{KEY1_CAL}drain-l5.ics");
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let app1 = za_app_password(key1.name, STRONG, "drain test E").await;
    key1.client
        .request_with_headers(
            "PUT",
            &organizer5,
            [CONTENT_TYPE],
            invite(uid5, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy5 = copy_href(test, key2.id, uid5).await;
    let delegated = body(&key2.client, &copy5)
        .await
        .replace("\r\n ", "")
        .replace(
            "PARTSTAT=NEEDS-ACTION",
            "PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\"",
        )
        .replace(
            "END:VEVENT",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key2@example.com\":mailto:key3@example.com\r\nEND:VEVENT",
        );
    key2.client
        .request_with_headers("PUT", &copy5, [CONTENT_TYPE], delegated)
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    let reply5 = pending_ids(test, key1.id).await;
    assert_eq!(
        reply5.len(),
        1,
        "key1's inbox holds one pending delegation reply"
    );
    let before3 = inbox_ids(test, key3.id).await;
    assert_eq!(za_uid_leases(test).await, 0);
    let session_1 = HookParked::start(
        key3.id,
        test_hooks::set_fan_out_pause,
        trigger_drain(key1.name, STRONG),
    )
    .await;
    let waited = std::time::Instant::now();
    assert_eq!(
        trigger_drain(key1.name, &app1).await,
        207,
        "session 2's request"
    );
    assert!(
        waited.elapsed() >= Duration::from_secs(1),
        "session 2 did not wait on the lease ({:?})",
        waited.elapsed()
    );
    assert_eq!(
        new_inbox_items(test, key3.id, &before3).await.len(),
        0,
        "key3 received an item before session 1's fan-out wrote one"
    );
    assert_eq!(za_uid_leases(test).await, 1, "the parked holder's lease");
    assert_eq!(
        pending_ids(test, key1.id).await,
        reply5,
        "session 2 drained past the lease"
    );
    assert_eq!(session_1.finish().await, 207);
    let key3_items = new_inbox_items(test, key3.id, &before3).await;
    assert_eq!(key3_items.len(), 1, "key3 holds exactly one invitation");
    assert_ne!(key3_items[0].1.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    assert_eq!(
        za_uid_leases(test).await,
        0,
        "the delegating merge kept its lease"
    );
    assert!(
        pending_ids(test, key1.id).await.is_empty(),
        "the delegation reply is still pending"
    );
    let (_, carriers) = open_pending_item_as(test, key3.id, STRONG, key3_items[0].0).await;
    let delivered = pair_token(carriers.pair.as_ref(), "key3@example.com");
    assert_eq!(
        stored_pairs(test, &keys1, key1.id, "default/drain-l5.ics")
            .await
            .expect("pairs")
            .get("key3@example.com"),
        Some(&delivered),
        "key1's copy keeps a token other than the one key3 received"
    );
    test_hooks::set_drain_retry_override(Some(1));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(trigger_drain(key1.name, &app1).await, 207);
    assert_eq!(
        new_inbox_items(test, key3.id, &before3).await.len(),
        1,
        "session 2's retry fanned out again"
    );
    assert_eq!(za_uid_leases(test).await, 0);
    test_hooks::set_drain_retry_override(None);

    // F. The import winner commits while the loser waits for the lease
    // (spec 6.3): session 1 (key2's password session) holds the creation
    // lease, parked before its commit; session 2 (key2's app-password
    // session) enters the lease's retry loop; session 1 commits inside it.
    // Session 2 then takes the lease, re-reads the item, finds it drained
    // and skips it, writing nothing. With the default backoff in force, a
    // second invitation drains on session 2's next request, which a
    // session deferred by a lost assertion would not do.
    test_hooks::set_drain_retry_override(None);
    let uid6 = "za-drain-l6";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-l6.ics"),
            [CONTENT_TYPE],
            invite(uid6, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    assert_eq!(za_uid_leases(test).await, 0);
    let session_1 = HookParked::start(
        key2.id,
        test_hooks::set_drain_lease_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    let session_2 = tokio::spawn(trigger_drain(key2.name, &app));
    // Inside `ZaUidLease::acquire`'s retries (20 x 50 ms).
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !session_2.is_finished(),
        "session 2 did not wait on the lease"
    );
    assert_eq!(session_1.finish().await, 207);
    assert_eq!(session_2.await.unwrap(), 207, "session 2's request");
    assert_eq!(
        uid_copies(test, key2.id, uid6).await,
        1,
        "exactly one copy for the raced item"
    );
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "the raced item is still pending"
    );
    assert_eq!(za_uid_leases(test).await, 0);
    let uid7 = "za-drain-l7";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-l7.ics"),
            [CONTENT_TYPE],
            invite(uid7, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    assert_eq!(trigger_drain(key2.name, &app).await, 207);
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "session 2 was deferred after the race"
    );
    assert_eq!(uid_copies(test, key2.id, uid7).await, 1);
    let copy6 = copy_href(test, key2.id, uid6).await;
    let copy7 = copy_href(test, key2.id, uid7).await;
    assert_eq!(za_uid_leases(test).await, 0);

    // G. The delegating merge's fan-out is fenced on its lease (spec 6.3,
    // 6.4 "Delegates"): session 1 (key1's password session) holds key1's
    // creation lease, parked inside the fan-out to key3 after it chose
    // key3's token and reserved the item's id, until the 1-second lease
    // expires. Session 2 (key1's app-password session) takes the expired
    // lease over, delivers its own token to key3 and commits. Session 1's
    // item write then finds the lease taken over and delivers nothing:
    // key3 holds one invitation, with the token key1's copy keeps, and
    // key3's acceptance applies to key1's copy.
    let uid8 = "za-drain-l8";
    let organizer8 = format!("{KEY1_CAL}drain-l8.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer8,
            [CONTENT_TYPE],
            invite(uid8, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let copy8 = copy_href(test, key2.id, uid8).await;
    let delegated = body(&key2.client, &copy8)
        .await
        .replace("\r\n ", "")
        .replace(
            "PARTSTAT=NEEDS-ACTION",
            "PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:key3@example.com\"",
        )
        .replace(
            "END:VEVENT",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:key2@example.com\":mailto:key3@example.com\r\nEND:VEVENT",
        );
    key2.client
        .request_with_headers("PUT", &copy8, [CONTENT_TYPE], delegated)
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    assert_eq!(
        pending_ids(test, key1.id).await.len(),
        1,
        "key1's inbox holds one pending delegation reply"
    );
    assert_eq!(za_uid_leases(test).await, 0);
    test_hooks::set_lease_secs_override(Some(1));
    let before3 = inbox_ids(test, key3.id).await;
    let session_1 = HookParked::start(
        key3.id,
        test_hooks::set_fan_out_pause,
        trigger_drain(key1.name, STRONG),
    )
    .await;
    // Session 1 holds its 1-second lease; session 2's takes the default.
    test_hooks::set_lease_secs_override(None);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        trigger_drain(key1.name, &app1).await,
        207,
        "session 2's request"
    );
    assert_eq!(
        new_inbox_items(test, key3.id, &before3).await.len(),
        1,
        "session 2 did not deliver key3's invitation"
    );
    assert!(
        pending_ids(test, key1.id).await.is_empty(),
        "session 2 did not take the expired lease over"
    );
    assert_eq!(za_uid_leases(test).await, 0);
    // Session 1's lost fence defers its session; a short backoff lets the
    // last drain below run.
    test_hooks::set_drain_retry_override(Some(1));
    assert_eq!(
        session_1.finish().await,
        207,
        "a lost fan-out fence failed the request"
    );
    let key3_items = new_inbox_items(test, key3.id, &before3).await;
    assert_eq!(
        key3_items.len(),
        1,
        "key3 still has exactly one new item: the stale delivery was not fenced out"
    );
    assert_eq!(
        za_uid_leases(test).await,
        0,
        "the fenced-out holder wrote its lease"
    );
    let (_, carriers) = open_pending_item_as(test, key3.id, STRONG, key3_items[0].0).await;
    let delivered = pair_token(carriers.pair.as_ref(), "key3@example.com");
    assert_eq!(
        stored_pairs(test, &keys1, key1.id, "default/drain-l8.ics")
            .await
            .expect("pairs")
            .get("key3@example.com"),
        Some(&delivered),
        "key1's copy keeps a token other than the one key3 received"
    );
    // End to end: key3's acceptance carries the token key1 keeps.
    assert_eq!(trigger_drain(key3.name, STRONG).await, 207);
    let copy8_key3 = copy_href(test, key3.id, uid8).await;
    set_partstat(&key3.client, &copy8_key3, "key3@example.com", "ACCEPTED").await;
    wait_for_delivery(test).await;
    assert_eq!(trigger_drain(key1.name, &app1).await, 207);
    assert!(pending_ids(test, key1.id).await.is_empty());
    assert_eq!(
        partstat_of(&key1.client, &organizer8, "key3@example.com")
            .await
            .as_deref(),
        Some("ACCEPTED"),
        "the delegate's reply did not apply to the organizer"
    );
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(trigger_drain(key1.name, STRONG).await, 207);
    assert!(pending_ids(test, key1.id).await.is_empty());
    test_hooks::set_drain_retry_override(None);
    // key3's drain above also imported the case E invitation.
    let copy5_key3 = copy_href(test, key3.id, uid5).await;
    assert_eq!(za_uid_leases(test).await, 0);

    // Cleanup.
    test_hooks::set_drain_retry_override(None);
    for href in [copy, own, own3, copy5, copy6, copy7, copy8] {
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    // key3's drain finds nothing new by now; skipped all the same, so a
    // late item cannot leave a copy behind.
    test_hooks::set_drain_skip(key3.id);
    for href in [
        "/dav/cal/key3%40example.com/default/drain-l1.ics".to_string(),
        copy5_key3,
        copy8_key3,
    ] {
        key3.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for href in [
        format!("{KEY1_CAL}drain-l1.ics"),
        format!("{KEY1_CAL}drain-l2.ics"),
        format!("{KEY1_CAL}drain-l3.ics"),
        format!("{KEY1_CAL}drain-l4.ics"),
        organizer5,
        format!("{KEY1_CAL}drain-l6.ics"),
        format!("{KEY1_CAL}drain-l7.ics"),
        organizer8,
    ] {
        key1.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    key3.delete_items_since(test, &start3).await;
    test_hooks::clear_drain_skip(key3.id);
    let credential_id = AppPassword::parse(&app).unwrap().credential_id;
    za_post(
        "app-password/revoke",
        &json!({ "username": key2.name, "password": STRONG, "credential_id": credential_id }),
    )
    .await
    .expect(200);
    let credential_id = AppPassword::parse(&app1).unwrap().credential_id;
    za_post(
        "app-password/revoke",
        &json!({ "username": key1.name, "password": STRONG, "credential_id": credential_id }),
    )
    .await
    .expect(200);
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}

pub async fn test_cursor(test: &mut TestServer) {
    println!("Running zero-access drain cursor tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;

    // 1. An item that arrives while a pass is rewriting an earlier item is
    // processed on the next request (spec 6.2, "The cursor"): the pass is
    // held before its commit of item X; item Y arrives; the pass completes
    // X only; the next request drains Y.
    let (uid_x, uid_y) = ("za-drain-c1", "za-drain-c2");
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-c1.ics"),
            [CONTENT_TYPE],
            invite(uid_x, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let pass = HookParked::start(
        key2.id,
        test_hooks::set_drain_lease_pause,
        trigger_drain(key2.name, STRONG),
    )
    .await;
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-c2.ics"),
            [CONTENT_TYPE],
            invite(uid_y, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(pass.finish().await, 207);
    assert_eq!(uid_copies(test, key2.id, uid_x).await, 1);
    assert_eq!(
        uid_copies(test, key2.id, uid_y).await,
        0,
        "the pass took an item past its snapshot"
    );
    assert_eq!(pending_ids(test, key2.id).await.len(), 1);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(
        uid_copies(test, key2.id, uid_y).await,
        1,
        "the late item was not drained next"
    );
    assert!(pending_ids(test, key2.id).await.is_empty());

    // 2. A transient failure is retried without a new arrival, after the
    // backoff (spec 6.2, 6.5): the drain's commit fails once; the item
    // stays pending; the next request within the 30-second backoff does
    // not drain; with the backoff overridden to zero, the next request
    // drains.
    let uid_z = "za-drain-c3";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-c3.ics"),
            [CONTENT_TYPE],
            invite(uid_z, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    // The deferral is an absolute deadline taken at the failing pass, so
    // the short backoff is set before the failure; the override cannot
    // shorten a deadline already stored.
    test_hooks::set_drain_retry_override(Some(2));
    test_hooks::set_drain_fail_once(Some(key2.id));
    assert_eq!(
        trigger_drain(key2.name, STRONG).await,
        207,
        "a drain failure failed the request"
    );
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "a failed commit drained the item"
    );
    assert_eq!(uid_copies(test, key2.id, uid_z).await, 0);
    assert_eq!(
        za_uid_leases(test).await,
        0,
        "the failed commit kept its lease"
    );
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        1,
        "the drain retried inside the backoff"
    );
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(
        pending_ids(test, key2.id).await.is_empty(),
        "the retry did not drain"
    );
    assert_eq!(uid_copies(test, key2.id, uid_z).await, 1);
    test_hooks::set_drain_retry_override(None);

    // Cleanup.
    for uid in [uid_x, uid_y, uid_z] {
        let href = copy_href(test, key2.id, uid).await;
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for name in ["drain-c1", "drain-c2", "drain-c3"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}

/// A REQUEST as upstream's organizer builder shapes it, for planting.
pub fn request_tree(uid: &str, organizer: &str, attendee: &str) -> String {
    format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nMETHOD:REQUEST\r\n",
            "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:{start}\r\n",
            "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:planted {uid}\r\n",
            "ORGANIZER:mailto:{organizer}\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{attendee}\r\n",
            "END:VEVENT\r\nEND:VCALENDAR\r\n"
        ),
        uid = uid,
        start = START_A,
        organizer = organizer,
        attendee = attendee
    )
}

pub async fn test_errors(test: &mut TestServer) {
    println!("Running zero-access drain item error tests...");
    let _drain = DrainOn::new();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;

    // 1. Permanent errors (spec 6.5): a plaintext item with the pending bit
    // (no envelope) and a pending item whose envelope is corrupt are
    // deleted; nothing is imported.
    let plaintext = plant_inbox_item(
        test,
        key2.id,
        &request_tree("za-drain-e1", "key1@example.com", "key2@example.com"),
        PlantedItem::Plain {
            flags: EVENT_NOTIFICATION_ZA_PENDING,
        },
        key1.id,
        None,
    )
    .await;
    let corrupt = plant_inbox_item(
        test,
        key2.id,
        &request_tree("za-drain-e2", "key1@example.com", "key2@example.com"),
        PlantedItem::Pending {
            pair: Some(("key2@example.com", [0x11; 16])),
            reply_token: None,
        },
        key1.id,
        None,
    )
    .await;
    rewrite_inbox_item(test, key2.id, corrupt, |item| {
        // The root's last entry is `X-ZA-KEY`; its text becomes garbage.
        let envelope = item.event.components[0].entries.last_mut().unwrap();
        envelope.values = vec![ICalendarValue::Text("AAAA".to_string())];
    })
    .await;
    assert_eq!(pending_ids(test, key2.id).await.len(), 2);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let ids = inbox_ids(test, key2.id).await;
    assert!(
        !ids.contains(&plaintext) && !ids.contains(&corrupt),
        "an undecodable item was kept: {ids:?}"
    );
    assert_eq!(uid_copies(test, key2.id, "za-drain-e1").await, 0);
    assert_eq!(uid_copies(test, key2.id, "za-drain-e2").await, 0);

    // 2. Sender mismatch (spec 6.4 step 2): an item whose ORGANIZER is key1
    // but whose `changed_by` is key3 is kept as history, not applied.
    let mismatch = plant_inbox_item(
        test,
        key2.id,
        &request_tree("za-drain-e3", "key1@example.com", "key2@example.com"),
        PlantedItem::Pending {
            pair: Some(("key2@example.com", [0x22; 16])),
            reply_token: None,
        },
        key3.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let item = stored_item(test, key2.id, mismatch).await;
    assert_eq!(
        item.flags & EVENT_NOTIFICATION_ZA_PENDING,
        0,
        "history is not pending"
    );
    assert_eq!(item.event_id, None);
    assert_eq!(
        envelope_wrap(&item.event),
        Some(1),
        "history is resealed under the master key"
    );
    assert_eq!(
        uid_copies(test, key2.id, "za-drain-e3").await,
        0,
        "a mismatched sender's invitation was imported"
    );

    // 3. A REPLY for an event the organizer does not have (Review Focus 4):
    // kept as history with no event, nothing deleted.
    let orphan = plant_inbox_item(
        test,
        key2.id,
        &reply_tree(
            "za-drain-e4",
            "key2@example.com",
            "key1@example.com",
            "ACCEPTED",
            0,
        ),
        PlantedItem::Pending {
            pair: None,
            reply_token: Some([0x33; 16]),
        },
        key1.id,
        None,
    )
    .await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let item = stored_item(test, key2.id, orphan).await;
    assert_eq!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    assert_eq!(item.event_id, None);
    assert_eq!(
        uid_copies(test, key2.id, "za-drain-e4").await,
        0,
        "an orphan reply created an event"
    );

    // 4. The same UID in two calendars (Review Focus 2): key2 copies its
    // imported event into a second calendar; key1's update merges into the
    // lowest document id only.
    let uid = "za-drain-e5";
    let organizer = format!("{KEY1_CAL}drain-e5.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let first = copy_href(test, key2.id, uid).await;
    key2.client
        .request(
            "MKCALENDAR",
            "/dav/cal/key2%40example.com/drain-second/",
            MKCALENDAR,
        )
        .await
        .with_status(StatusCode::CREATED);
    // A COPY within one account links the event into the second calendar
    // (one document, two names: `copy_event` pushes a `DavName`), so the
    // second copy is a PUT of the first's body (the brief's fallback).
    let second = "/dav/cal/key2%40example.com/drain-second/e5-copy.ics";
    let first_body = body(&key2.client, &first).await;
    key2.client
        .request_with_headers("PUT", second, [CONTENT_TYPE], first_body)
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(uid_copies(test, key2.id, uid).await, 2);
    key1.client
        .request_with_headers(
            "PUT",
            &organizer,
            [CONTENT_TYPE],
            invite(uid, START_B, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let merged = body(&key2.client, &first).await;
    let untouched = body(&key2.client, second).await;
    assert!(merged.contains(&format!("DTSTART:{START_B}")), "{merged}");
    assert!(
        untouched.contains(&format!("DTSTART:{START_A}")),
        "{untouched}"
    );
    assert!(pending_ids(test, key2.id).await.is_empty());

    // 5. Two addresses that resolve to one account (plan 9 outcome): two
    // items for one UID, different pairs; the drain imports the first and
    // merges the second; the copy ends with both pairs.
    let uid2 = "za-drain-e6";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-e6.ics"),
            [CONTENT_TYPE],
            invite(uid2, START_A, &["key2@example.com", "key2+two@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        pending_ids(test, key2.id).await.len(),
        2,
        "one item per address"
    );
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    assert!(pending_ids(test, key2.id).await.is_empty());
    assert_eq!(uid_copies(test, key2.id, uid2).await, 1);
    let copy2 = copy_href(test, key2.id, uid2).await;
    let pairs = stored_pairs(test, &keys2, key2.id, &relative(&copy2))
        .await
        .expect("pairs");
    assert_eq!(
        pairs.len(),
        2,
        "the second item's pair was not adopted alongside the first"
    );
    assert!(pairs.get("key2@example.com").is_some() && pairs.get("key2+two@example.com").is_some());

    // 6. Quota (spec 6.4 step 4): an import past key2's disk quota is an
    // iTIP-level rejection, kept as history, no copy. A failed quota
    // counter read is a store error that keeps the item pending; the test
    // store cannot fail, so that path rests on the classification in
    // `is_over_quota`, the fan-out's.
    // The fan-out checks the recipient's quota too (plan 9, 7a), so the
    // invitation is delivered first and the quota lowered before key2's
    // request.
    let admin = test.account("admin@example.com").clone();
    let key2_account = test.account("key2@example.com").id();
    let uid3 = "za-drain-e7";
    key1.client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}drain-e7.ics"),
            [CONTENT_TYPE],
            invite(uid3, START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let over_quota = pending_ids(test, key2.id).await;
    assert_eq!(
        over_quota.len(),
        1,
        "the invitation was delivered under the full quota"
    );
    admin
        .registry_update_object(
            ObjectType::Account,
            key2_account,
            json!({ Property::Quotas: { StorageQuota::MaxDiskQuota.as_str(): 1 } }),
        )
        .await;
    assert_eq!(trigger_drain(key2.name, STRONG).await, 207);
    let item = stored_item(test, key2.id, over_quota[0]).await;
    assert_eq!(
        item.flags & EVENT_NOTIFICATION_ZA_PENDING,
        0,
        "an over-quota import stayed pending"
    );
    assert_eq!(item.event_id, None);
    assert_eq!(
        uid_copies(test, key2.id, uid3).await,
        0,
        "an import past the quota"
    );
    admin
        .registry_update_object(
            ObjectType::Account,
            key2_account,
            json!({ Property::Quotas: {} }),
        )
        .await;

    // Cleanup.
    for href in [first, second.to_string(), copy2] {
        key2.client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key2.client
        .request("DELETE", "/dav/cal/key2%40example.com/drain-second/", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    for name in ["drain-e5", "drain-e6", "drain-e7"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    assert_eq!(za_uid_leases(test).await, 0);
    test.wait_for_tasks_skip_not_due().await;
}
