/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 5: a key account's own request delivers sealed pending
//! items to internal key recipients instead of queueing mail. Plan 9 has no
//! drain, so the tests read items at store level and open them with the
//! recipient's private key (`open_pending_item_as`).

use super::{STRONG, dav_seal::raw_event, user_permissions};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        envelope_wrap, hold_task_consumer, inbox_ids, item_attendees, item_method, item_sequence,
        itip_tasks_for, mail_count, new_inbox_items, open_pending_item_as, schedule_statuses,
        wait_for_delivery, za_uid_leases, za_unlock,
    },
};
use ::registry::{
    schema::{
        enums::{Permission, StorageQuota},
        prelude::{ObjectType, Property},
        structs::{CalendarScheduling, Permissions, PermissionsList},
    },
    types::{EnumImpl, map::Map},
};
use calcard::{
    Entry, Parser,
    icalendar::{ICalendarMethod, ICalendarProperty},
};
use common::za::test_hooks;
use groupware::calendar::{
    ChangedBy, EVENT_NOTIFICATION_ZA_PENDING,
    seal::{TOKEN_LEN, TokenPairs, event_token_pairs},
};
use hyper::StatusCode;
use serde_json::json;
use types::{collection::Collection, field::CalendarEventField};
use vault::session::SessionKeys;

pub const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
/// RFC 6638 `Schedule-Reply: F`: a cleanup DELETE that builds no CANCEL.
pub const NO_REPLY: (&str, &str) = ("schedule-reply", "F");
pub const KEY1_CAL: &str = "/dav/cal/key1%40example.com/default/";
pub const START_A: &str = "20990301T100000Z";
pub const START_B: &str = "20990302T100000Z";
pub const MKCALENDAR: &str = "<?xml version=\"1.0\" encoding=\"utf-8\" ?><A:mkcalendar xmlns:D=\"DAV:\" xmlns:A=\"urn:ietf:params:xml:ns:caldav\"/>";

/// A key account of the suite (password `STRONG`) and its CalDAV client.
pub struct Party {
    pub id: u32,
    pub name: &'static str,
    pub client: DummyWebDavClient,
}

impl Party {
    pub fn new(test: &TestServer, name: &'static str) -> Self {
        let id = test.account(name).id().document_id();
        Party {
            id,
            name,
            client: DummyWebDavClient::new(id, name, STRONG, name),
        }
    }

    pub fn inbox(&self) -> String {
        format!("/dav/itip/{}/inbox/", self.name.replace('@', "%40"))
    }

    /// Deletes inbox items by href: upstream's DELETE, unchanged for pending
    /// items (plan 8).
    pub async fn delete_items(&self, ids: impl IntoIterator<Item = u32>) {
        for id in ids {
            self.client
                .request("DELETE", &format!("{}{id}.ics", self.inbox()), "")
                .await
                .with_status(StatusCode::NO_CONTENT);
        }
    }

    /// Deletes every inbox item written since `before`.
    pub async fn delete_items_since(&self, test: &TestServer, before: &[u32]) {
        let ids: Vec<u32> = inbox_ids(test, self.id)
            .await
            .into_iter()
            .filter(|id| !before.contains(id))
            .collect();
        self.delete_items(ids).await;
    }
}

/// An invitation organized by key1. Each of `attendees` becomes an
/// ATTENDEE that expects a reply.
pub fn invite(uid: &str, start: &str, attendees: &[&str]) -> String {
    let mut ical = format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
            "UID:{uid}\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:{start}\r\nDURATION:PT1H\r\n",
            "SEQUENCE:0\r\nSUMMARY:fo-title {uid}\r\n",
            "ORGANIZER:mailto:key1@example.com\r\n"
        ),
        uid = uid,
        start = start
    );
    for attendee in attendees {
        ical.push_str(&format!(
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{attendee}\r\n"
        ));
    }
    ical.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    ical
}

/// The token pairs of an account's stored event at `path` (`default/x.ics`).
pub async fn stored_pairs(
    test: &TestServer,
    keys: &SessionKeys,
    account_id: u32,
    path: &str,
) -> Option<TokenPairs> {
    let (archive, _) = raw_event(test, account_id, path).await;
    event_token_pairs(&archive, keys, account_id).unwrap()
}

/// The SCHEDULE-STATUS values of `address`'s ATTENDEE properties in the
/// sender's copy, as its client reads it.
pub async fn attendee_statuses(
    client: &DummyWebDavClient,
    path: &str,
    address: &str,
) -> Vec<String> {
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    assert!(!body.contains("X-ZA-"), "{body}");
    schedule_statuses(&body, ICalendarProperty::Attendee, address)
}

/// The SEQUENCE of the event at `path`, as its owner's client reads it.
pub async fn stored_sequence(client: &DummyWebDavClient, path: &str) -> i64 {
    let body = client
        .request("GET", path, "")
        .await
        .with_status(StatusCode::OK)
        .body
        .unwrap();
    match Parser::new(&body).entry() {
        Entry::ICalendar(ical) => item_sequence(&ical).expect("a stored SEQUENCE"),
        other => panic!("not an iCalendar body: {other:?}"),
    }
}

/// The token an opened item's pair carries for `address`.
pub fn pair_token(pair: Option<&TokenPairs>, address: &str) -> [u8; TOKEN_LEN] {
    let pair = pair.expect("the item carries a pair");
    assert_eq!(pair.len(), 1, "an item carries one pair");
    *pair.get(address).expect("the pair names the recipient")
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access fan-out tests...");
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let key5 = Party::new(test, "key5@example.com");
    let plain_id = test.account("plain@example.com").id().document_id();
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let start1 = inbox_ids(test, key1.id).await;
    let start2 = inbox_ids(test, key2.id).await;
    let start3 = inbox_ids(test, key3.id).await;
    let start5 = inbox_ids(test, key5.id).await;

    // 1. Invite with a mixed attendee list (spec 5.2, 5.3): key2 is
    // internal; plain is a non-key account; outsider is external.
    let plain_items = inbox_ids(test, plain_id).await;
    let plain_mail = mail_count(test, plain_id).await;
    let mixed = format!("{KEY1_CAL}fo-mixed.ics");
    let response = key1
        .client
        .request_with_headers(
            "PUT",
            &mixed,
            [CONTENT_TYPE],
            invite(
                "za-fo-mixed",
                START_A,
                &[
                    "key2@example.com",
                    "plain@example.com",
                    "outsider@remote.test",
                ],
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert!(
        response.headers.get("schedule-tag").is_some(),
        "the schedule tag is set as upstream sets it: {:?}",
        response.headers
    );
    assert_eq!(
        new_inbox_items(test, plain_id, &plain_items).await.len(),
        0,
        "plain's inbox received an item"
    );
    let items = new_inbox_items(test, key2.id, &start2).await;
    assert_eq!(items.len(), 1, "key2's inbox holds exactly one new item");
    let (item_id, item) = &items[0];
    assert_ne!(
        item.flags & EVENT_NOTIFICATION_ZA_PENDING,
        0,
        "the item is pending"
    );
    assert_eq!(item.event_id, None);
    assert_eq!(item.changed_by, ChangedBy::PrincipalId(key1.id));
    assert_eq!(
        envelope_wrap(&item.event),
        Some(2),
        "sealed to key2's public key"
    );
    assert!(
        !item.event.to_string().contains("fo-title"),
        "the stored item is sealed"
    );
    let (tree, carriers) = open_pending_item_as(test, key2.id, STRONG, *item_id).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Request));
    assert_eq!(item_sequence(&tree), Some(0));
    assert!(tree.uids().any(|uid| uid == "za-fo-mixed"));
    assert_eq!(
        item_attendees(&tree),
        [
            "key2@example.com",
            "outsider@remote.test",
            "plain@example.com"
        ]
    );
    assert!(
        !tree.to_string().contains("X-ZA-"),
        "no carrier left in the opened tree"
    );
    assert!(carriers.reply_token.is_none());
    let key2_token = pair_token(carriers.pair.as_ref(), "key2@example.com");
    let pairs = stored_pairs(test, &keys1, key1.id, "default/fo-mixed.ics")
        .await
        .expect("key1's bundle holds pairs");
    assert_eq!(pairs.len(), 3, "one pair per scheduled attendee");
    assert_eq!(pairs.get("key2@example.com"), Some(&key2_token));
    assert_eq!(
        attendee_statuses(&key1.client, &mixed, "key2@example.com").await,
        ["1.2"]
    );
    assert_eq!(
        attendee_statuses(&key1.client, &mixed, "plain@example.com").await,
        ["5.3"]
    );
    assert_eq!(
        attendee_statuses(&key1.client, &mixed, "outsider@remote.test").await,
        ["5.3"]
    );
    // No mail (invariant 12). The task queue is read with the consumer held
    // in `test_task_queue` (Task 5).
    wait_for_delivery(test).await;
    assert_eq!(
        mail_count(test, plain_id).await,
        plain_mail,
        "plain received mail"
    );
    assert_eq!(
        inbox_ids(test, key1.id).await,
        start1,
        "nothing in the sender's inbox"
    );

    // Review Focus 1: the same attendee twice, in two cases.
    let before2 = inbox_ids(test, key2.id).await;
    let dup = format!("{KEY1_CAL}fo-dup.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &dup,
            [CONTENT_TYPE],
            invite(
                "za-fo-dup",
                START_A,
                &["KEY2@Example.com", "key2@example.com"],
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "one item for an attendee listed twice");
    let (_, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    let dup_token = pair_token(carriers.pair.as_ref(), "key2@example.com");
    let pairs = stored_pairs(test, &keys1, key1.id, "default/fo-dup.ics")
        .await
        .unwrap();
    assert_eq!(pairs.len(), 1, "one pair for an attendee listed twice");
    assert_eq!(pairs.get("key2@example.com"), Some(&dup_token));
    assert_eq!(
        attendee_statuses(&key1.client, &dup, "key2@example.com").await,
        ["1.2", "1.2"]
    );

    // Review Focus 3: a sub-address of the sender's own account. Not
    // attempted; nothing reaches key1's own inbox.
    let own = format!("{KEY1_CAL}fo-own.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &own,
            [CONTENT_TYPE],
            invite("za-fo-own", START_A, &["key1+fo@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        inbox_ids(test, key1.id).await,
        start1,
        "an item reached the sender"
    );
    assert_eq!(
        attendee_statuses(&key1.client, &own, "key1+fo@example.com").await,
        ["5.3"]
    );

    // Review Focus 4: a recipient that never made a DAV request. The inbox
    // is virtual (`SCHEDULE_INBOX_ID`); nothing has to exist first.
    let fresh = format!("{KEY1_CAL}fo-fresh.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &fresh,
            [CONTENT_TYPE],
            invite("za-fo-fresh", START_A, &["key5@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let items = new_inbox_items(test, key5.id, &start5).await;
    assert_eq!(items.len(), 1, "key5's inbox holds the item");
    let (tree, carriers) = open_pending_item_as(test, key5.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Request));
    pair_token(carriers.pair.as_ref(), "key5@example.com");
    assert_eq!(
        attendee_statuses(&key1.client, &fresh, "key5@example.com").await,
        ["1.2"]
    );

    // 2. Updates fan out (spec 5.5). key1 moves the event: a second REQUEST
    // with SEQUENCE 1 and the same token.
    let before2 = inbox_ids(test, key2.id).await;
    key1.client
        .request_with_headers(
            "PUT",
            &mixed,
            [CONTENT_TYPE],
            invite(
                "za-fo-mixed",
                START_B,
                &[
                    "key2@example.com",
                    "plain@example.com",
                    "outsider@remote.test",
                ],
            ),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "the update reached key2 once");
    let (tree, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Request));
    assert_eq!(item_sequence(&tree), Some(1));
    assert_eq!(
        pair_token(carriers.pair.as_ref(), "key2@example.com"),
        key2_token,
        "the token is stable across updates"
    );

    // key1 removes key2 and adds key3: key2 gets a CANCEL with its old pair,
    // key3 a REQUEST with a fresh one.
    let before2 = inbox_ids(test, key2.id).await;
    let before3 = inbox_ids(test, key3.id).await;
    key1.client
        .request_with_headers(
            "PUT",
            &mixed,
            [CONTENT_TYPE],
            invite(
                "za-fo-mixed",
                START_B,
                &[
                    "key3@example.com",
                    "plain@example.com",
                    "outsider@remote.test",
                ],
            ),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "the removed attendee gets one CANCEL");
    let (tree, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Cancel));
    assert_eq!(
        pair_token(carriers.pair.as_ref(), "key2@example.com"),
        key2_token,
        "the CANCEL carries the removed attendee's old pair (spec 3.3)"
    );
    let items = new_inbox_items(test, key3.id, &before3).await;
    assert_eq!(items.len(), 1, "the added attendee gets one REQUEST");
    let (tree, carriers) = open_pending_item_as(test, key3.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Request));
    let key3_token = pair_token(carriers.pair.as_ref(), "key3@example.com");
    assert_ne!(key3_token, key2_token);

    // 10. Pairs maintenance: the bundle keeps the current attendees' pairs.
    let pairs = stored_pairs(test, &keys1, key1.id, "default/fo-mixed.ics")
        .await
        .unwrap();
    assert_eq!(pairs.len(), 3, "key3, plain and outsider");
    assert!(
        pairs.get("key2@example.com").is_none(),
        "the removed pair is gone"
    );
    assert_eq!(pairs.get("key3@example.com"), Some(&key3_token));
    // A re-added attendee gets a different token.
    let before2 = inbox_ids(test, key2.id).await;
    key1.client
        .request_with_headers(
            "PUT",
            &mixed,
            [CONTENT_TYPE],
            invite(
                "za-fo-mixed",
                START_B,
                &[
                    "key2@example.com",
                    "key3@example.com",
                    "plain@example.com",
                    "outsider@remote.test",
                ],
            ),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "the re-added attendee gets one REQUEST");
    let (_, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    let key2_new_token = pair_token(carriers.pair.as_ref(), "key2@example.com");
    assert_ne!(
        key2_new_token, key2_token,
        "a re-added attendee gets a new token"
    );
    let pairs = stored_pairs(test, &keys1, key1.id, "default/fo-mixed.ics")
        .await
        .unwrap();
    assert_eq!(pairs.len(), 4);
    assert_eq!(pairs.get("key2@example.com"), Some(&key2_new_token));
    assert_eq!(
        pairs.get("key3@example.com"),
        Some(&key3_token),
        "a kept attendee keeps its token"
    );

    // 2 (continued). key1 DELETEs the event: each internal attendee gets a
    // CANCEL built from the unsealed stored tree, with its pair (spec 5.5).
    // Each CANCEL carries the stored SEQUENCE plus one; plain (5.3) gets no
    // mail.
    let stored_sequence = stored_sequence(&key1.client, &mixed).await;
    let plain_mail = mail_count(test, plain_id).await;
    let before2 = inbox_ids(test, key2.id).await;
    let before3 = inbox_ids(test, key3.id).await;
    key1.client
        .request("DELETE", &mixed, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key3.id, &before3).await;
    assert_eq!(items.len(), 1, "key3 gets one CANCEL");
    let (tree, carriers) = open_pending_item_as(test, key3.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Cancel));
    assert_eq!(item_sequence(&tree), Some(stored_sequence + 1));
    assert_eq!(
        pair_token(carriers.pair.as_ref(), "key3@example.com"),
        key3_token
    );
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "key2 gets one CANCEL");
    let (tree, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Cancel));
    assert_eq!(item_sequence(&tree), Some(stored_sequence + 1));
    assert_eq!(
        pair_token(carriers.pair.as_ref(), "key2@example.com"),
        key2_new_token,
        "key2's CANCEL carries its renewed pair"
    );
    wait_for_delivery(test).await;
    assert_eq!(
        mail_count(test, plain_id).await,
        plain_mail,
        "the DELETE mailed plain"
    );

    // A collection DELETE cancels each scheduled event in it.
    let collection = "/dav/cal/key1%40example.com/fo-collection/";
    key1.client
        .request("MKCALENDAR", collection, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    for uid in ["za-fo-coll-1", "za-fo-coll-2"] {
        key1.client
            .request_with_headers(
                "PUT",
                &format!("{collection}{uid}.ics"),
                [CONTENT_TYPE],
                invite(uid, START_A, &["key2@example.com"]),
            )
            .await
            .with_status(StatusCode::CREATED);
    }
    let mut coll_tokens = Vec::new();
    for uid in ["za-fo-coll-1", "za-fo-coll-2"] {
        let token = *stored_pairs(test, &keys1, key1.id, &format!("fo-collection/{uid}.ics"))
            .await
            .unwrap()
            .get("key2@example.com")
            .unwrap();
        coll_tokens.push((uid, token));
    }
    let before2 = inbox_ids(test, key2.id).await;
    key1.client
        .request("DELETE", collection, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 2, "one CANCEL per scheduled event");
    for (item_id, item) in &items {
        assert_eq!(item_method(&item.event), Some(ICalendarMethod::Cancel));
        let (tree, carriers) = open_pending_item_as(test, key2.id, STRONG, *item_id).await;
        let (_, token) = coll_tokens
            .iter()
            .find(|(uid, _)| tree.uids().any(|item_uid| item_uid == *uid))
            .expect("the CANCEL names one of the collection's events");
        assert_eq!(
            pair_token(carriers.pair.as_ref(), "key2@example.com"),
            *token,
            "a collection CANCEL carries its event's pair"
        );
    }

    // Review Focus 2: an update that removes every ATTENDEE sends only
    // CANCELs, with the old pairs, and leaves no pair in the bundle.
    let gone = format!("{KEY1_CAL}fo-gone.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &gone,
            [CONTENT_TYPE],
            invite("za-fo-gone", START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let gone_token = *stored_pairs(test, &keys1, key1.id, "default/fo-gone.ics")
        .await
        .unwrap()
        .get("key2@example.com")
        .unwrap();
    let before2 = inbox_ids(test, key2.id).await;
    key1.client
        .request_with_headers(
            "PUT",
            &gone,
            [CONTENT_TYPE],
            invite("za-fo-gone", START_A, &[]),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "only a CANCEL");
    let (tree, carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Cancel));
    assert_eq!(
        pair_token(carriers.pair.as_ref(), "key2@example.com"),
        gone_token
    );
    assert!(
        stored_pairs(test, &keys1, key1.id, "default/fo-gone.ics")
            .await
            .is_none(),
        "no attendee, no pair"
    );

    // Cleanup: the events without CANCELs, then every new item.
    for name in ["fo-dup", "fo-own", "fo-fresh", "fo-gone"] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.delete_items_since(test, &start1).await;
    key2.delete_items_since(test, &start2).await;
    key3.delete_items_since(test, &start3).await;
    key5.delete_items_since(test, &start5).await;
    wait_for_delivery(test).await;
    // Counting plain's mail created its default mailboxes; plain outlives
    // the suite.
    test.destroy_all_mailboxes(test.account("plain@example.com"))
        .await;
}

/// How many stored events of `account_id` carry `uid`.
pub async fn uid_copies(test: &TestServer, account_id: u32, uid: &str) -> u64 {
    test.server
        .document_ids_matching(
            account_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .unwrap()
        .len()
}

/// Spec 5.3 step 5 and 5.4: a recipient's failure is that recipient's
/// `5.1`, never a failed PUT; a key account without
/// `CalendarSchedulingReceive` is not attempted (5.2); over the recipient
/// limit the PUT fails before any delivery; a failed sender commit after the
/// fan-out leaves its items.
pub async fn test_failures(test: &mut TestServer) {
    println!("Running zero-access fan-out failure tests...");
    let admin = test.account("admin@example.com").clone();
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let key3 = Party::new(test, "key3@example.com");
    let key2_account = test.account("key2@example.com").id();
    let keys1 = za_unlock(test, key1.id, STRONG).await;
    let start2 = inbox_ids(test, key2.id).await;
    let start3 = inbox_ids(test, key3.id).await;

    // 7a. Quota: key2's quota cannot hold the item. key2 gets 5.1 and no
    // item; key3 is unaffected.
    admin
        .registry_update_object(
            ObjectType::Account,
            key2_account,
            json!({ Property::Quotas: { StorageQuota::MaxDiskQuota.as_str(): 1 } }),
        )
        .await;
    let quota = format!("{KEY1_CAL}fo-quota.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &quota,
            [CONTENT_TYPE],
            invite(
                "za-fo-quota",
                START_A,
                &["key2@example.com", "key3@example.com"],
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        inbox_ids(test, key2.id).await,
        start2,
        "an item reached key2 past its quota"
    );
    assert_eq!(new_inbox_items(test, key3.id, &start3).await.len(), 1);
    assert_eq!(
        attendee_statuses(&key1.client, &quota, "key2@example.com").await,
        ["5.1"]
    );
    assert_eq!(
        attendee_statuses(&key1.client, &quota, "key3@example.com").await,
        ["1.2"]
    );
    admin
        .registry_update_object(
            ObjectType::Account,
            key2_account,
            json!({ Property::Quotas: {} }),
        )
        .await;

    // 7b. Inbound size: a message at or over `itip_inbound_max_ical_size`.
    // 100 bytes is the setting's schema minimum, well under any invitation.
    admin
        .registry_update_setting(
            CalendarScheduling {
                itip_max_size: 100,
                ..Default::default()
            },
            &[Property::ItipMaxSize],
        )
        .await;
    admin.reload_settings().await;
    let size = format!("{KEY1_CAL}fo-size.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &size,
            [CONTENT_TYPE],
            invite("za-fo-size", START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        inbox_ids(test, key2.id).await,
        start2,
        "an oversized item reached key2"
    );
    assert_eq!(
        attendee_statuses(&key1.client, &size, "key2@example.com").await,
        ["5.1"]
    );
    admin
        .registry_update_setting(CalendarScheduling::default(), &[Property::ItipMaxSize])
        .await;
    admin.reload_settings().await;

    // 7d. A key account without `CalendarSchedulingReceive` (spec 5.2): not
    // attempted. The permission is revoked through the registry as
    // `disabled.rs` disables `Authenticate` (`Property::Permissions`,
    // `Permissions::Merge`); the edit invalidates key3's access token
    // (`permissions_changed` -> `CacheInvalidation::AccessToken`,
    // `crates/common/src/cache/invalidate.rs:63-71`), so the fan-out's next
    // `access_token(key3)` reads it.
    fn key3_permissions(disabled: Vec<Permission>) -> serde_json::Value {
        // As the suite created key3: `user_permissions()` enabled.
        json!({ Property::Permissions: Permissions::Merge(PermissionsList {
            disabled_permissions: Map::new(disabled),
            enabled_permissions: Map::new(user_permissions()),
        }) })
    }
    let key3_account = test.account("key3@example.com").id();
    admin
        .registry_update_object(
            ObjectType::Account,
            key3_account,
            key3_permissions(vec![Permission::CalendarSchedulingReceive]),
        )
        .await;
    let before2 = inbox_ids(test, key2.id).await;
    let before3 = inbox_ids(test, key3.id).await;
    let receive = format!("{KEY1_CAL}fo-receive.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &receive,
            [CONTENT_TYPE],
            invite(
                "za-fo-receive",
                START_A,
                &["key2@example.com", "key3@example.com"],
            ),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        inbox_ids(test, key3.id).await,
        before3,
        "an item reached key3 without CalendarSchedulingReceive"
    );
    assert_eq!(new_inbox_items(test, key2.id, &before2).await.len(), 1);
    assert_eq!(
        attendee_statuses(&key1.client, &receive, "key3@example.com").await,
        ["5.3"]
    );
    assert_eq!(
        attendee_statuses(&key1.client, &receive, "key2@example.com").await,
        ["1.2"]
    );
    // Restore key3's permissions, and check that the restore took.
    admin
        .registry_update_object(ObjectType::Account, key3_account, key3_permissions(vec![]))
        .await;
    let restored = format!("{KEY1_CAL}fo-restored.ics");
    key1.client
        .request_with_headers(
            "PUT",
            &restored,
            [CONTENT_TYPE],
            invite("za-fo-restored", START_A, &["key3@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        new_inbox_items(test, key3.id, &before3).await.len(),
        1,
        "key3 receives again after the restore"
    );
    assert_eq!(
        attendee_statuses(&key1.client, &restored, "key3@example.com").await,
        ["1.2"]
    );
    // The items now, so 7c and 8 keep their baselines; the events with the
    // module's cleanup.
    key2.delete_items_since(test, &before2).await;
    key3.delete_items_since(test, &before3).await;

    // 7c. Max recipients (spec 5.1): the PUT fails with the precondition
    // before any delivery, and nothing is stored.
    admin
        .registry_update_setting(
            CalendarScheduling {
                max_recipients: 2,
                ..Default::default()
            },
            &[Property::MaxRecipients],
        )
        .await;
    admin.reload_settings().await;
    let before3 = inbox_ids(test, key3.id).await;
    let response = key1
        .client
        .request_with_headers(
            "PUT",
            &format!("{KEY1_CAL}fo-many.ics"),
            [CONTENT_TYPE],
            invite(
                "za-fo-many",
                START_A,
                &[
                    "key2@example.com",
                    "key3@example.com",
                    "outsider@remote.test",
                ],
            ),
        )
        .await
        .with_status(StatusCode::PRECONDITION_FAILED);
    assert!(
        response
            .expect_body()
            .contains("max-attendees-per-instance"),
        "{}",
        response.expect_body()
    );
    assert_eq!(inbox_ids(test, key2.id).await, start2);
    assert_eq!(inbox_ids(test, key3.id).await, before3);
    assert_eq!(
        uid_copies(test, key1.id, "za-fo-many").await,
        0,
        "nothing was stored"
    );
    admin
        .registry_update_setting(CalendarScheduling::default(), &[Property::MaxRecipients])
        .await;
    admin.reload_settings().await;

    // 8. The sender's commit fails after the fan-out wrote key2's item: 500.
    // The client resends and gets 201; key2 holds two items with an equal
    // SEQUENCE and different tokens, and key1's bundle holds the second.
    let commit = format!("{KEY1_CAL}fo-commit.ics");
    let body = invite("za-fo-commit", START_A, &["key2@example.com"]);
    test_hooks::set_commit_fail_once(Some(key1.id));
    key1.client
        .request_with_headers("PUT", &commit, [CONTENT_TYPE], body.clone())
        .await
        .with_status(StatusCode::INTERNAL_SERVER_ERROR);
    test_hooks::set_commit_fail_once(None);
    assert_eq!(
        new_inbox_items(test, key2.id, &start2).await.len(),
        1,
        "the item was written before the failed commit"
    );
    assert_eq!(
        uid_copies(test, key1.id, "za-fo-commit").await,
        0,
        "no event was stored"
    );
    key1.client
        .request_with_headers("PUT", &commit, [CONTENT_TYPE], body)
        .await
        .with_status(StatusCode::CREATED);
    let items = new_inbox_items(test, key2.id, &start2).await;
    assert_eq!(items.len(), 2);
    let (first, first_carriers) = open_pending_item_as(test, key2.id, STRONG, items[0].0).await;
    let (second, second_carriers) = open_pending_item_as(test, key2.id, STRONG, items[1].0).await;
    assert_eq!(
        item_sequence(&first),
        item_sequence(&second),
        "an equal SEQUENCE"
    );
    let first_token = pair_token(first_carriers.pair.as_ref(), "key2@example.com");
    let second_token = pair_token(second_carriers.pair.as_ref(), "key2@example.com");
    assert_ne!(
        first_token, second_token,
        "the retry hands out a fresh token"
    );
    assert_eq!(
        stored_pairs(test, &keys1, key1.id, "default/fo-commit.ics")
            .await
            .unwrap()
            .get("key2@example.com"),
        Some(&second_token)
    );

    // Cleanup.
    for name in [
        "fo-quota",
        "fo-size",
        "fo-receive",
        "fo-restored",
        "fo-commit",
    ] {
        key1.client
            .request_with_headers("DELETE", &format!("{KEY1_CAL}{name}.ics"), [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key2.delete_items_since(test, &start2).await;
    key3.delete_items_since(test, &start3).await;
}

const PLAIN_CAL: &str = "/dav/cal/plain%40example.com/fo-queue/";

/// A plain organizer's invitation to key2 (spec 5.2, "Plain organizers").
const PLAIN_INVITE: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\n",
    "UID:za-fo-plain\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990305T100000Z\r\n",
    "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:fo-plain-title\r\n",
    "ORGANIZER:mailto:plain@example.com\r\n",
    "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);

/// Spec 11, task queue: with the consumer held, a key organizer's PUT and
/// DELETE queue no `CalendarItipMessage` task (they fan out instead); a
/// plain organizer's PUT does (control), and its invitation reaches key2 as
/// mail only (test 6).
pub async fn test_task_queue(test: &mut TestServer) {
    println!("Running zero-access task queue tests...");
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let plain = test.account("plain@example.com").clone();
    let plain_id = plain.id().document_id();
    let plain_client = DummyWebDavClient::new(plain_id, plain.name(), plain.secret(), plain.name());
    let start2 = inbox_ids(test, key2.id).await;

    let queue = format!("{KEY1_CAL}fo-queue.ics");
    let held = hold_task_consumer(test).await;
    key1.client
        .request_with_headers(
            "PUT",
            &queue,
            [CONTENT_TYPE],
            invite("za-fo-queue", START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    key1.client
        .request("DELETE", &queue, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        itip_tasks_for(test, key1.id).await,
        0,
        "a CalendarItipMessage task was queued for the key organizer"
    );
    drop(held);
    let methods: Vec<_> = new_inbox_items(test, key2.id, &start2)
        .await
        .iter()
        .map(|(_, item)| item_method(&item.event))
        .collect();
    assert_eq!(
        methods,
        [
            Some(ICalendarMethod::Request),
            Some(ICalendarMethod::Cancel)
        ],
        "the PUT and the DELETE fanned out"
    );

    // Control and test 6: a plain organizer inviting key2 takes upstream's
    // path, a task and then mail.
    plain_client
        .request("MKCALENDAR", PLAIN_CAL, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    let key2_mail = mail_count(test, key2.id).await;
    let before2 = inbox_ids(test, key2.id).await;
    let held = hold_task_consumer(test).await;
    plain_client
        .request_with_headers(
            "PUT",
            &format!("{PLAIN_CAL}fo-plain.ics"),
            [CONTENT_TYPE],
            PLAIN_INVITE,
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        itip_tasks_for(test, plain_id).await,
        1,
        "the plain organizer's PUT queued its task (control)"
    );
    drop(held);
    wait_for_delivery(test).await;
    assert_eq!(
        mail_count(test, key2.id).await,
        key2_mail + 1,
        "the mail reached key2"
    );
    assert_eq!(
        inbox_ids(test, key2.id).await,
        before2,
        "nothing in key2's inbox"
    );
    assert_eq!(
        uid_copies(test, key2.id, "za-fo-plain").await,
        0,
        "nothing in key2's calendar"
    );

    plain_client
        .request_with_headers(
            "DELETE",
            &format!("{PLAIN_CAL}fo-plain.ics"),
            [NO_REPLY],
            "",
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    plain_client
        .request("DELETE", PLAIN_CAL, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    key2.delete_items_since(test, &start2).await;
    wait_for_delivery(test).await;
}

const SHARED_PLAIN: &str = "/dav/cal/plain%40example.com/default/fo-shared.ics";

/// Final review F1: the sending state follows the calendar owner's key
/// status, not the authenticated account's. key1 (a key account) writes an
/// invitation into plain's shared calendar: upstream's path, a task and then
/// mail, for the PUT and for the DELETE; no item and no lease.
pub async fn test_plain_owner(test: &mut TestServer) {
    println!("Running zero-access plain-owner scheduling tests...");
    let key1 = Party::new(test, "key1@example.com");
    let key2 = Party::new(test, "key2@example.com");
    let plain = test.account("plain@example.com").clone();
    let plain_id = plain.id().document_id();
    let plain_client = DummyWebDavClient::new(plain_id, plain.name(), plain.secret(), plain.name());
    let start2 = inbox_ids(test, key2.id).await;
    let key2_mail = mail_count(test, key2.id).await;

    // As release 1's `dav_gate` does: plain grants key1 write access to its
    // `default` calendar (created here: earlier modules removed plain's).
    plain_client
        .request(
            "MKCALENDAR",
            "/dav/cal/plain%40example.com/default/",
            MKCALENDAR,
        )
        .await
        .with_status(StatusCode::CREATED);
    plain_client
        .acl(
            "/dav/cal/plain@example.com/default/",
            "/dav/pal/key1%40example.com/",
            ["read", "write"],
        )
        .await
        .with_status(StatusCode::OK);

    let held = hold_task_consumer(test).await;
    key1.client
        .request_with_headers(
            "PUT",
            SHARED_PLAIN,
            [CONTENT_TYPE],
            invite("za-fo-shared", START_A, &["key2@example.com"]),
        )
        .await
        .with_status(StatusCode::CREATED);
    assert_eq!(
        itip_tasks_for(test, plain_id).await,
        1,
        "a key user's invitation into a plain calendar was dropped"
    );
    assert_eq!(
        inbox_ids(test, key2.id).await,
        start2,
        "a plain calendar's invitation reached key2's inbox"
    );
    assert_eq!(
        za_uid_leases(test).await,
        0,
        "a plain calendar's create took a lease"
    );
    drop(held);
    wait_for_delivery(test).await;
    assert_eq!(
        mail_count(test, key2.id).await,
        key2_mail + 1,
        "the invitation reached key2 as mail"
    );

    // The DELETE (no `Schedule-Reply: F`) follows the owner too: upstream's
    // CANCEL task.
    let held = hold_task_consumer(test).await;
    key1.client
        .request("DELETE", SHARED_PLAIN, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    assert_eq!(
        itip_tasks_for(test, plain_id).await,
        1,
        "the DELETE in a plain calendar queued no CANCEL"
    );
    assert_eq!(
        inbox_ids(test, key2.id).await,
        start2,
        "a plain calendar's CANCEL reached key2's inbox"
    );
    drop(held);
    wait_for_delivery(test).await;
    assert_eq!(
        mail_count(test, key2.id).await,
        key2_mail + 2,
        "the CANCEL reached key2 as mail"
    );

    // Cleanup: revoke the grant; `plain` outlives the suite, so drop the
    // calendar.
    plain_client
        .acl(
            "/dav/cal/plain@example.com/default/",
            "/dav/pal/key1%40example.com/",
            [],
        )
        .await
        .with_status(StatusCode::OK);
    plain_client
        .request("DELETE", "/dav/cal/plain@example.com/default", "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}
