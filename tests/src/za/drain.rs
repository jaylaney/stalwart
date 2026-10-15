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
        CONTENT_TYPE, KEY1_CAL, NO_REPLY, Party, START_A, START_B, attendee_statuses, invite,
        stored_pairs, stored_sequence, uid_copies,
    },
};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        PlantedItem, copy_href, envelope_wrap, inbox_ids, inbox_items, item_method, item_partstat,
        new_inbox_items, pending_ids, plant_inbox_item, trigger_drain, za_uid_leases, za_unlock,
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

    // Cleanup.
    for (party, uid) in [
        (&key2, uid),
        (&key2, uid2),
        (&key2, uid3),
        (&key3, uid3),
        (&key2, uid4),
    ] {
        let href = copy_href(test, party.id, uid).await;
        party
            .client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    for name in ["drain-r1", "drain-r2", "drain-r3", "drain-r4"] {
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
