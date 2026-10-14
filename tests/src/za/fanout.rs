/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 5: a key account's own request delivers sealed pending
//! items to internal key recipients instead of queueing mail. Plan 9 has no
//! drain, so the tests read items at store level and open them with the
//! recipient's private key (`open_pending_item_as`).

use super::{STRONG, dav_seal::raw_event};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        envelope_wrap, inbox_ids, item_attendees, item_method, item_sequence, mail_count,
        new_inbox_items, open_pending_item_as, schedule_statuses, wait_for_delivery, za_unlock,
    },
};
use calcard::icalendar::{ICalendarMethod, ICalendarProperty};
use groupware::calendar::{
    ChangedBy, EVENT_NOTIFICATION_ZA_PENDING,
    seal::{TOKEN_LEN, TokenPairs, event_token_pairs},
};
use hyper::StatusCode;
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
    for name in ["fo-mixed", "fo-dup", "fo-own", "fo-fresh", "fo-gone"] {
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
