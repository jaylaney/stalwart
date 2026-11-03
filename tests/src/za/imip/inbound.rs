/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 5, inbound (iMIP spec 4): a key account's incoming iMIP mail
//! becomes a sealed, mail-borne pending item at delivery, sealed to the
//! served public key with the sender in `X-ZA-SENDER`, beside the sealed
//! mail. The delivery cases run with the drain skipped (the suite's
//! `DRAIN_SKIP_ALL`), so every item is read at store level; the drain cases
//! after them (spec 4.3) each hold `DrainOn` while they drain.

use super::{
    ATT, DELEGATE, NODMARC, ORG, STRANGER, add_contact, delete_contact, deliver, event,
    is_request_for, itip_mail, with_method,
};
use crate::{
    utils::{
        account::Account,
        server::TestServer,
        smtp::SmtpConnection,
        smtp_sink::sink,
        webdav::DummyWebDavClient,
        za::{
            InboxItem, copy_href, envelope_wrap, inbox_ids, inbox_items, item_method,
            item_partstat, mail_borne_items, mail_count, new_inbox_items, open_pending_item_as,
            pending_ids, schedule_statuses, trigger_drain, wait_for_delivery, wait_for_queue,
            za_unlock,
        },
    },
    za::{
        STRONG,
        drain::{DrainOn, body, partstat_of, relative, set_partstat},
        fanout::{CONTENT_TYPE, MKCALENDAR, NO_REPLY, Party, stored_pairs, uid_copies},
        pinning::with_partstat,
        uid_lease::put,
        user_permissions,
    },
};
use ::registry::schema::{
    enums::MtaInboundThrottleKey, prelude::ObjectType, structs::MtaInboundThrottle,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use calcard::icalendar::{ICalendar, ICalendarComponentType, ICalendarMethod, ICalendarProperty};
use common::auth::BuildAccessToken;
use email::{
    mailbox::INBOX_ID,
    message::{
        delivery::SenderAuthentication,
        ingest::{EmailIngest, IngestEmail, IngestSource},
    },
};
use groupware::calendar::{
    ChangedBy, EVENT_NOTIFICATION_PRIVATE, EVENT_NOTIFICATION_ZA_PENDING,
    seal::{TOKEN_LEN, TokenPairs},
};
use hyper::StatusCode;
use mail_parser::MessageParser;
use serde_json::json;
use types::id::Id;

/// The key attendee of this module's cases.
pub const KEY: &str = "imip-key@example.com";
/// A second key account: its address as a mail sender is refused.
pub const KEY2: &str = "imip-key2@example.com";
/// A plain local principal, as a mail sender.
pub const PLAIN: &str = "imip-plain@example.com";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access iMIP inbound tests...");
    let admin = test.account("admin@example.com").clone();
    for (name, description) in [(KEY, "iMIP key"), (KEY2, "iMIP key two")] {
        let account = admin
            .create_key_user_account(name, STRONG, description, &[], user_permissions())
            .await;
        test.insert_account(account);
    }
    let plain = admin
        .create_user_account(PLAIN, STRONG, "iMIP plain", &[], user_permissions())
        .await;
    // The module mails imip-key from remote.test more often than upstream's
    // default inbound throttle on a sender domain and recipient allows (25 an
    // hour); `cleanup` restores it.
    let throttles = rcpt_throttles(&admin).await;
    set_throttles(&admin, &throttles, false).await;
    let key = Party::new(test, KEY);
    let start = inbox_ids(test, key.id).await;
    // A new UID from ORG needs ORG in the address book (auto-add is off).
    let org_card = add_contact(&key.client, KEY, ORG).await;

    delivered_request(test, &key).await;
    refused_deliveries(test, &key).await;
    auto_add_by_contact_and_principal(test, &key).await;
    known_uid_by_pending_item(test, &key).await;
    unknown_uid_other_methods(test, &key).await;
    rf_1_provider_shaped_invitation(test, &key).await;

    // The drain cases (spec 4.3). The items of `imip-in-1` and `imip-in-d3`
    // are still pending here; every other delivery case deleted its own.
    // The first drain drains every pending item, so the case that claims a
    // single drain for both of its items runs first.
    let key2 = Party::new(test, KEY2);
    let start2 = inbox_ids(test, key2.id).await;
    logged_out_request_then_cancel(test, &key).await;
    drained_request_and_merges(test, &key).await;
    replies(test, &key).await;
    contact_deleted_before_drain(test, &key).await;
    refresh_answer(test, &key).await;
    outside_delegation(test, &key, &key2).await;
    pairs_untouched_by_mail(test, &key, &key2).await;
    delegate_reply_into_delegators_copy(test, &key, &key2).await;
    plain_local_organizer(test, &key, &plain).await;
    rf_2_sender_case(test, &key).await;
    rf_4_recurring_instance(test, &key, &plain).await;

    cleanup(
        test,
        &admin,
        &key,
        [(&key, start.as_slice()), (&key2, start2.as_slice())],
        vec![org_card],
        plain,
    )
    .await;
    set_throttles(&admin, &throttles, true).await;
}

/// The enabled inbound throttles keyed by recipient (upstream's default
/// "Sender address to recipient throttle" among them).
async fn rcpt_throttles(admin: &Account) -> Vec<Id> {
    admin
        .registry_get_all::<MtaInboundThrottle>()
        .await
        .into_iter()
        .filter(|(_, throttle)| {
            throttle.enable
                && throttle
                    .key
                    .iter()
                    .any(|key| *key == MtaInboundThrottleKey::Rcpt)
        })
        .map(|(id, _)| id)
        .collect()
}

/// Enables or disables the throttles `ids`, then reloads the settings.
async fn set_throttles(admin: &Account, ids: &[Id], enable: bool) {
    for id in ids {
        admin
            .registry_update_object(
                ObjectType::MtaInboundThrottle,
                *id,
                json!({ "enable": enable }),
            )
            .await;
    }
    admin.reload_settings().await;
}

/// Spec 4.1, 4.2, 8.2: an outside REQUEST gives one pending item beside the sealed mail.
async fn delivered_request(test: &TestServer, key: &Party) {
    let (before, mails) = (
        inbox_ids(test, key.id).await,
        mail_count(test, key.id).await,
    );
    deliver(
        "dmarc-org@remote.test",
        key.name,
        &itip_mail(
            ORG,
            key.name,
            "REQUEST",
            &event("imip-in-1", ORG, &[key.name], 0, ""),
        ),
    )
    .await;
    wait_for_queue(test).await;
    let items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(items.len(), 1);
    let (id, item) = &items[0];
    assert_eq!(item.changed_by, ChangedBy::CalendarAddress(String::new()));
    assert_eq!(
        item.flags & (EVENT_NOTIFICATION_ZA_PENDING | EVENT_NOTIFICATION_PRIVATE),
        EVENT_NOTIFICATION_ZA_PENDING | EVENT_NOTIFICATION_PRIVATE
    );
    assert!(item.calendar_ids.is_empty() && item.event_id.is_none());
    assert_eq!(
        envelope_wrap(&item.event),
        Some(2),
        "sealed to the served key"
    );
    assert!(
        !item.event.to_string().contains("imip-org"),
        "the sender only in the carrier"
    );
    let (tree, carriers) = open_pending_item_as(test, key.id, STRONG, *id).await;
    assert_eq!(carriers.sender.as_deref(), Some(ORG));
    assert!(carriers.pair.is_none() && carriers.reply_token.is_none());
    assert_eq!(item_method(&tree), Some(ICalendarMethod::Request));
    assert_eq!(
        mail_count(test, key.id).await,
        mails + 1,
        "the mail is stored whatever happens"
    );
}

/// One delivery `refused_deliveries` expects no item for.
struct Refused {
    label: &'static str,
    envelope_from: String,
    message: String,
    /// Ingested directly as spam (`is_spam: true`), not over LMTP.
    junk: bool,
    to: &'static str,
    account_id: u32,
}

impl Refused {
    async fn deliver(&self, test: &TestServer) {
        if self.junk {
            ingest_junk(
                test,
                self.account_id,
                self.to,
                &self.envelope_from,
                &self.message,
            )
            .await;
        } else {
            deliver(&self.envelope_from, self.to, &self.message).await;
        }
    }
}

/// The cases of `refused_deliveries`, in order. Each would otherwise be
/// taken: ORG is in the address book, and imip-plain is a local principal.
fn refused_cases(test: &TestServer, key: &Party) -> Vec<Refused> {
    let to = key.name;
    let case = |label, envelope_from: &str, message: String, junk| Refused {
        label,
        envelope_from: envelope_from.to_string(),
        message,
        junk,
        to,
        account_id: key.id,
    };
    vec![
        // Spec 4.1's gate: no DMARC pass and no authenticated submission.
        case(
            "no DMARC",
            NODMARC,
            itip_mail(
                ORG,
                to,
                "REQUEST",
                &event("imip-in-nodmarc", ORG, &[to], 0, ""),
            ),
            false,
        ),
        // Spec 4.1's gate: junk. The sender is imip-plain, not ORG: the
        // spam filter's `trustContacts` would turn junk from a ContactCard's
        // address into ham (`card_is_ham`).
        case(
            "junk",
            "dmarc-plain@remote.test",
            itip_mail(
                PLAIN,
                to,
                "REQUEST",
                &event("imip-in-junk", PLAIN, &[to], 0, ""),
            ),
            true,
        ),
        // Step 1: a part of `itip_inbound_max_ical_size` bytes.
        case(
            "oversized",
            "dmarc-org@remote.test",
            oversized_request(to, test.server.core.groupware.itip_inbound_max_ical_size),
            false,
        ),
        // Step 3: a method that does not continue, for a UID the account
        // knows (`delivered_request`'s pending item), so step 5 would let
        // it through.
        case(
            "PUBLISH",
            "dmarc-org@remote.test",
            itip_mail(ORG, to, "PUBLISH", &event("imip-in-1", ORG, &[to], 0, "")),
            false,
        ),
        // Step 4: a local key account's own mail.
        case(
            "a key account's mail",
            "dmarc-key2@remote.test",
            itip_mail(
                KEY2,
                to,
                "REQUEST",
                &event("imip-in-key2", KEY2, &[to], 0, ""),
            ),
            false,
        ),
        // Step 5: an unknown sender's new UID with auto-add off.
        case(
            "an unknown sender's new UID",
            "dmarc-stranger@remote.test",
            itip_mail(
                STRANGER,
                to,
                "REQUEST",
                &event("imip-in-stranger", STRANGER, &[to], 0, ""),
            ),
            false,
        ),
    ]
}

/// Spec 4.1 steps 1-5, 8.2: no item for any of these; each mail is still stored.
async fn refused_deliveries(test: &TestServer, key: &Party) {
    for case in refused_cases(test, key) {
        let (before, mails) = (
            inbox_ids(test, key.id).await,
            mail_count(test, key.id).await,
        );
        case.deliver(test).await;
        wait_for_queue(test).await;
        assert_eq!(
            inbox_ids(test, key.id).await,
            before,
            "{}: an item was written",
            case.label
        );
        assert_eq!(
            mail_count(test, key.id).await,
            mails + 1,
            "{}: the mail was not stored",
            case.label
        );
    }
}

/// Spec 4.1 step 5, D2: the stranger's new UID gets an item once it is a
/// ContactCard, and a local principal's (imip-plain over DMARC) gets one.
async fn auto_add_by_contact_and_principal(test: &TestServer, key: &Party) {
    let card = add_contact(&key.client, key.name, STRANGER).await;
    assert_one_mail_borne_item(
        test,
        key,
        "dmarc-stranger@remote.test",
        STRANGER,
        "imip-in-card",
    )
    .await;
    delete_contact(&key.client, &card).await;
    assert_one_mail_borne_item(
        test,
        key,
        "dmarc-plain@remote.test",
        PLAIN,
        "imip-in-principal",
    )
    .await;
}

/// Delivers `sender`'s REQUEST for the new `uid`, asserts exactly one new
/// mail-borne pending item whose carrier names `sender`, and deletes it.
async fn assert_one_mail_borne_item(
    test: &TestServer,
    key: &Party,
    envelope_from: &str,
    sender: &str,
    uid: &str,
) {
    let before = inbox_ids(test, key.id).await;
    deliver(
        envelope_from,
        key.name,
        &itip_mail(
            sender,
            key.name,
            "REQUEST",
            &event(uid, sender, &[key.name], 0, ""),
        ),
    )
    .await;
    wait_for_queue(test).await;
    let items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(items.len(), 1, "{sender}: one new item");
    let (id, item) = &items[0];
    assert_eq!(
        item.changed_by,
        ChangedBy::CalendarAddress(String::new()),
        "{sender}: a mail-borne item"
    );
    assert_ne!(item.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    let (_, carriers) = open_pending_item_as(test, key.id, STRONG, *id).await;
    assert_eq!(carriers.sender.as_deref(), Some(sender));
    key.delete_items(test, [*id]).await;
}

/// Spec 4.1 step 5, D3: a CANCEL whose UID only a pending item names gets an item.
async fn known_uid_by_pending_item(test: &TestServer, key: &Party) {
    let uid = "imip-in-d3";
    for (method, ics) in [
        ("REQUEST", event(uid, ORG, &[key.name], 0, "")),
        (
            "CANCEL",
            event(uid, ORG, &[key.name], 1, "STATUS:CANCELLED"),
        ),
    ] {
        deliver(
            "dmarc-org@remote.test",
            key.name,
            &itip_mail(ORG, key.name, method, &ics),
        )
        .await;
        wait_for_queue(test).await;
    }
    let methods: Vec<_> = mail_borne_items(test, key.id)
        .await
        .iter()
        .filter(|(_, item)| item.event.uids().any(|item_uid| item_uid == uid))
        .map(|(_, item)| item_method(&item.event))
        .collect();
    assert_eq!(
        methods,
        [
            Some(ICalendarMethod::Request),
            Some(ICalendarMethod::Cancel)
        ]
    );
}

/// Spec 4.1 step 5: any other method for an unknown UID writes nothing.
async fn unknown_uid_other_methods(test: &TestServer, key: &Party) {
    let reply = event("imip-in-u2", key.name, &[ATT], 0, "")
        .replace("PARTSTAT=NEEDS-ACTION;RSVP=TRUE", "PARTSTAT=ACCEPTED");
    for (envelope_from, sender, method, ics) in [
        (
            "dmarc-org@remote.test",
            ORG,
            "CANCEL",
            event("imip-in-u1", ORG, &[key.name], 1, "STATUS:CANCELLED"),
        ),
        ("dmarc-att@remote.test", ATT, "REPLY", reply),
    ] {
        let (before, mails) = (
            inbox_ids(test, key.id).await,
            mail_count(test, key.id).await,
        );
        deliver(
            envelope_from,
            key.name,
            &itip_mail(sender, key.name, method, &ics),
        )
        .await;
        wait_for_queue(test).await;
        assert_eq!(
            inbox_ids(test, key.id).await,
            before,
            "{method}: an item was written"
        );
        assert_eq!(mail_count(test, key.id).await, mails + 1, "{method}");
    }
}

/// Review Focus 1.
async fn rf_1_provider_shaped_invitation(test: &TestServer, key: &Party) {
    let uid = "imip-in-rf1";
    let summary = "Quarterly review \u{2013} r\u{e9}union";
    let ics = with_method(
        &event(uid, ORG, &[key.name], 0, "").replace(
            &format!("SUMMARY:imip {uid}"),
            &format!("SUMMARY:{summary}"),
        ),
        "REQUEST",
    );
    let message = provider_mail(key.name, &ics);
    // The calendar part as a mail reader decodes it.
    let parsed = MessageParser::new().parse(message.as_bytes()).unwrap();
    let decoded = parsed
        .root()
        .parts()
        .find(|part| part.is_content_type("text", "calendar"))
        .and_then(|part| part.text())
        .expect("the calendar part")
        .into_owned();
    let decoded_summary = summary_of(&ICalendar::parse(&decoded).unwrap());
    assert_eq!(decoded_summary.as_deref(), Some(summary));

    let before = inbox_ids(test, key.id).await;
    deliver("dmarc-org@remote.test", key.name, &message).await;
    wait_for_queue(test).await;
    let items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(
        items.len(),
        1,
        "one item for the calendar part, none for the attachment"
    );
    let (tree, carriers) = open_pending_item_as(test, key.id, STRONG, items[0].0).await;
    assert_eq!(carriers.sender.as_deref(), Some(ORG));
    assert_eq!(summary_of(&tree), decoded_summary);
    key.delete_items(test, [items[0].0]).await;
}

/// The UIDs of the drain cases' events.
const DRAIN_UIDS: &[&str] = &[
    "imip-in-1",
    "imip-in-d3",
    "imip-in-r",
    "imip-in-d2",
    "imip-in-dl",
    "imip-in-p",
    "imip-in-sd",
    "imip-in-pl",
    "imip-in-rf2",
    "imip-in-rf4",
];

/// The account's events with any of `DRAIN_UIDS`.
async fn uid_copies_of_module(test: &TestServer, account_id: u32) -> u64 {
    let mut copies = 0;
    for uid in DRAIN_UIDS {
        copies += uid_copies(test, account_id, uid).await;
    }
    copies
}

/// Delivers `from`'s iTIP `method` message holding `ics` to `to`, with MAIL
/// FROM `dmarc-<from>` (the test-mode DMARC pass), and waits for the queue.
async fn mail_in(test: &TestServer, from: &str, to: &str, method: &str, ics: &str) {
    deliver(
        &format!("dmarc-{from}"),
        to,
        &itip_mail(from, to, method, ics),
    )
    .await;
    wait_for_queue(test).await;
}

/// Drains `party`'s inbox (any DAV request of a key account drains first).
async fn drain(party: &Party) {
    assert_eq!(trigger_drain(party.name, STRONG).await, 207);
}

/// `party`'s own calendar href for an event it PUTs as `<uid>.ics`.
fn own_href(party: &Party, uid: &str) -> String {
    format!(
        "/dav/cal/{}/default/{uid}.ics",
        party.name.replace('@', "%40")
    )
}

/// A DELETE that builds no CANCEL or REPLY (`Schedule-Reply: F`).
async fn delete_quietly(client: &DummyWebDavClient, href: &str) {
    client
        .request_with_headers("DELETE", href, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// An iTIP VEVENT for `uid` organized by `organizer`, whose ATTENDEE lines
/// are `attendees` (address, PARTSTAT), for a REPLY.
fn reply_ics(uid: &str, organizer: &str, sequence: u32, attendees: &[(&str, &str)]) -> String {
    let lines: String = attendees
        .iter()
        .map(|(address, partstat)| format!("ATTENDEE;PARTSTAT={partstat}:mailto:{address}\r\n"))
        .collect();
    event(uid, organizer, &[], sequence, &lines)
}

/// One instance of `uid`'s series, organized by ORG: RECURRENCE-ID
/// `recurrence_id`, starting at `start` for an hour; `extra` as in `event`.
fn instance_ics(
    uid: &str,
    attendees: &[&str],
    sequence: u32,
    recurrence_id: &str,
    start: &str,
    extra: &str,
) -> String {
    let end = format!(
        "{}T{:02}{}",
        &start[..8],
        start[9..11].parse::<u32>().unwrap() + 1,
        &start[11..]
    );
    event(
        uid,
        ORG,
        attendees,
        sequence,
        &format!("RECURRENCE-ID:{recurrence_id}\r\n{extra}"),
    )
    .replace("DTSTART:20990110T090000Z", &format!("DTSTART:{start}"))
    .replace("DTEND:20990110T100000Z", &format!("DTEND:{end}"))
}

/// The account's inbox items whose (visible) UID is `uid`, by ascending
/// document id.
async fn items_for(test: &TestServer, account_id: u32, uid: &str) -> Vec<(u32, InboxItem)> {
    inbox_items(test, account_id)
        .await
        .into_iter()
        .filter(|(_, item)| item.event.uids().any(|item_uid| item_uid == uid))
        .collect()
}

/// Asserts that `items` are `count` drained mail-borne items (D4: the row
/// keeps `CalendarAddress("")` once drained).
fn assert_drained_mail_borne(items: &[(u32, InboxItem)], count: usize, label: &str) {
    assert_eq!(items.len(), count, "{label}: items");
    for (id, item) in items {
        assert_eq!(
            item.flags & EVENT_NOTIFICATION_ZA_PENDING,
            0,
            "{label}: item {id} is still pending"
        );
        assert_eq!(
            item.changed_by,
            ChangedBy::CalendarAddress(String::new()),
            "{label}: item {id}"
        );
    }
}

/// `address`'s PARTSTAT in an event body.
fn partstat(body: &str, address: &str) -> Option<String> {
    item_partstat(&ICalendar::parse(body).unwrap(), address)
}

/// The ETag of `href` as `client` reads it.
async fn etag(client: &DummyWebDavClient, href: &str) -> String {
    client
        .request("GET", href, "")
        .await
        .with_status(StatusCode::OK)
        .etag()
        .to_string()
}

/// Spec 4.3, D7: the Task 4 REQUEST imports; update, ADD and CANCEL merge;
/// the account's own DISPLAY alarm survives.
async fn drained_request_and_merges(test: &TestServer, key: &Party) {
    let _drain = DrainOn::new();
    let uid = "imip-in-1";
    // `logged_out_request_then_cancel`'s drain drained this item already;
    // this one finds nothing pending.
    drain(key).await;
    let href = copy_href(test, key.id, uid).await;
    assert_drained_mail_borne(&items_for(test, key.id, uid).await, 1, "the REQUEST");

    // The client adds a DISPLAY alarm to its copy.
    let alarm = "BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT15M\r\nDESCRIPTION:imip-alarm\r\nEND:VALARM\r\nEND:VEVENT";
    let with_alarm = body(&key.client, &href)
        .await
        .replacen("END:VEVENT", alarm, 1);
    key.client
        .request_with_headers("PUT", &href, [CONTENT_TYPE], with_alarm)
        .await
        .with_status(StatusCode::NO_CONTENT);

    // A REQUEST update: SEQUENCE 1, a new SUMMARY, a weekly series.
    let update = event(uid, ORG, &[key.name], 1, "RRULE:FREQ=WEEKLY;COUNT=4")
        .replace(&format!("SUMMARY:imip {uid}"), "SUMMARY:imip-in-1 updated");
    mail_in(test, ORG, key.name, "REQUEST", &update).await;
    drain(key).await;
    let got = body(&key.client, &href).await;
    assert!(
        got.contains("SUMMARY:imip-in-1 updated")
            && got.contains("RRULE:FREQ=WEEKLY")
            && got.contains("DESCRIPTION:imip-alarm"),
        "the update did not merge, or the alarm did not survive: {got}"
    );

    // RFC 5546 3.2.4: an ADD of one new instance to the series.
    let add = instance_ics(
        uid,
        &[key.name],
        1,
        "20990117T090000Z",
        "20990117T140000Z",
        "",
    )
    .replace(&format!("SUMMARY:imip {uid}"), "SUMMARY:imip-in-1 added");
    mail_in(test, ORG, key.name, "ADD", &add).await;
    drain(key).await;
    let got = body(&key.client, &href).await;
    assert!(
        got.contains("SUMMARY:imip-in-1 added")
            && got.contains("RECURRENCE-ID:20990117T090000Z")
            && got.contains("DESCRIPTION:imip-alarm"),
        "the ADD did not merge: {got}"
    );

    // A CANCEL of the event.
    mail_in(
        test,
        ORG,
        key.name,
        "CANCEL",
        &event(uid, ORG, &[key.name], 2, "STATUS:CANCELLED"),
    )
    .await;
    drain(key).await;
    let got = body(&key.client, &href).await;
    assert!(got.contains("STATUS:CANCELLED"), "{got}");
    assert_drained_mail_borne(
        &items_for(test, key.id, uid).await,
        4,
        "the REQUEST, update, ADD and CANCEL",
    );
    assert!(pending_ids(test, key.id).await.is_empty());
    delete_quietly(&key.client, &href).await;
}

/// Spec 4.3, 3.2 item 9, 4.6: REPLYs into the key organizer's event.
async fn replies(test: &TestServer, key: &Party) {
    let _drain = DrainOn::new();
    let uid = "imip-in-r";
    let href = own_href(key, uid);
    assert_eq!(
        put(
            key.name,
            STRONG,
            href.clone(),
            event(uid, key.name, &[ATT, DELEGATE], 0, "")
        )
        .await,
        201
    );

    // ATT accepts; its REPLY also names DELEGATE as DECLINED, which
    // upstream's merge ignores: only the sender's line changes.
    mail_in(
        test,
        ATT,
        key.name,
        "REPLY",
        &reply_ics(
            uid,
            key.name,
            0,
            &[(ATT, "ACCEPTED"), (DELEGATE, "DECLINED")],
        ),
    )
    .await;
    drain(key).await;
    let got = body(&key.client, &href).await;
    assert_eq!(partstat(&got, ATT).as_deref(), Some("ACCEPTED"), "{got}");
    assert_eq!(
        schedule_statuses(&got, ICalendarProperty::Attendee, ATT),
        ["2.0"],
        "{got}"
    );
    assert_eq!(
        partstat(&got, DELEGATE).as_deref(),
        Some("NEEDS-ACTION"),
        "{got}"
    );

    // 3.2 item 9 (D6 withdrawn): after the organizer's SEQUENCE 2, ATT's
    // REPLY at SEQUENCE 0 still applies.
    let bumped = got.replace("SEQUENCE:0", "SEQUENCE:2");
    assert!(bumped.contains("SEQUENCE:2"), "{got}");
    assert_eq!(put(key.name, STRONG, href.clone(), bumped).await, 204);
    mail_in(
        test,
        ATT,
        key.name,
        "REPLY",
        &reply_ics(uid, key.name, 0, &[(ATT, "TENTATIVE")]),
    )
    .await;
    drain(key).await;
    assert_eq!(
        partstat_of(&key.client, &href, ATT).await.as_deref(),
        Some("TENTATIVE")
    );

    // 4.6: a non-participant's REPLY takes a pending slot, then the drain
    // rejects and deletes it; the event is unchanged.
    let tag = etag(&key.client, &href).await;
    let before = inbox_ids(test, key.id).await;
    mail_in(
        test,
        STRANGER,
        key.name,
        "REPLY",
        &reply_ics(uid, key.name, 2, &[(STRANGER, "ACCEPTED")]),
    )
    .await;
    assert_eq!(pending_ids(test, key.id).await.len(), 1);
    let items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(items.len(), 1);
    drain(key).await;
    assert!(
        !inbox_ids(test, key.id).await.contains(&items[0].0),
        "the non-participant's REPLY was not deleted"
    );
    assert_eq!(etag(&key.client, &href).await, tag, "the event changed");
    // `refresh_answer` uses this event, then deletes it.
}

/// D3: REQUEST then CANCEL delivered while logged out (Task 4's items) end cancelled.
async fn logged_out_request_then_cancel(test: &TestServer, key: &Party) {
    let uid = "imip-in-d3";
    let items = items_for(test, key.id, uid).await;
    assert_eq!(items.len(), 2);
    assert!(
        items
            .iter()
            .all(|(_, item)| item.flags & EVENT_NOTIFICATION_ZA_PENDING != 0),
        "both items are pending before the drain"
    );
    let _drain = DrainOn::new();
    drain(key).await;
    let href = copy_href(test, key.id, uid).await;
    let got = body(&key.client, &href).await;
    assert!(got.contains("STATUS:CANCELLED"), "{got}");
    assert_drained_mail_borne(
        &items_for(test, key.id, uid).await,
        2,
        "the REQUEST and CANCEL",
    );
    delete_quietly(&key.client, &href).await;
}

/// D2: a ContactCard deleted before the drain deletes the item.
async fn contact_deleted_before_drain(test: &TestServer, key: &Party) {
    let uid = "imip-in-d2";
    // Without the drain: the card, the delivery, the card's deletion (a
    // DAV request of key drains first while `DrainOn` is held).
    let card = add_contact(&key.client, key.name, STRANGER).await;
    let before = inbox_ids(test, key.id).await;
    mail_in(
        test,
        STRANGER,
        key.name,
        "REQUEST",
        &event(uid, STRANGER, &[key.name], 0, ""),
    )
    .await;
    let items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(items.len(), 1, "the card let the item in");
    delete_contact(&key.client, &card).await;

    let _drain = DrainOn::new();
    drain(key).await;
    assert_eq!(
        uid_copies(test, key.id, uid).await,
        0,
        "a copy was imported"
    );
    assert!(
        !inbox_ids(test, key.id).await.contains(&items[0].0),
        "the item outlived the drain's AutoAddDisabled"
    );
}

/// D14: a REFRESH is answered by mail, after the item's deletion.
async fn refresh_answer(test: &TestServer, key: &Party) {
    let _drain = DrainOn::new();
    let uid = "imip-in-r";
    let href = own_href(key, uid);
    sink().clear();
    let before = inbox_ids(test, key.id).await;
    mail_in(
        test,
        ATT,
        key.name,
        "REFRESH",
        &event(uid, key.name, &[ATT], 0, ""),
    )
    .await;
    let items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(items.len(), 1);
    drain(key).await;
    wait_for_queue(test).await;
    let answers = sink().take_to(ATT);
    assert_eq!(
        answers.iter().filter(|m| is_request_for(m, uid)).count(),
        1,
        "one REQUEST answers the REFRESH"
    );
    assert_eq!(answers.len(), 1, "nothing else reached ATT");
    assert!(
        !inbox_ids(test, key.id).await.contains(&items[0].0),
        "the REFRESH item outlived its answer"
    );
    delete_quietly(&key.client, &href).await;
}

/// D15, 4.3 point 4: ATT's REPLY delegating to imip-key2 and DELEGATE.
async fn outside_delegation(test: &TestServer, key: &Party, key2: &Party) {
    let _drain = DrainOn::new();
    let uid = "imip-in-dl";
    let href = own_href(key, uid);
    assert_eq!(
        put(
            key.name,
            STRONG,
            href.clone(),
            event(uid, key.name, &[ATT], 0, "")
        )
        .await,
        201
    );
    sink().clear();
    let before2 = inbox_ids(test, key2.id).await;
    let delegated = format!(
        concat!(
            "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:{key2}\",\"mailto:{delegate}\":mailto:{att}\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:{att}\":mailto:{key2}\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:{att}\":mailto:{delegate}\r\n"
        ),
        key2 = KEY2,
        delegate = DELEGATE,
        att = ATT
    );
    mail_in(
        test,
        ATT,
        key.name,
        "REPLY",
        &event(uid, key.name, &[], 0, &delegated),
    )
    .await;
    drain(key).await;
    wait_for_queue(test).await;
    // The internal delegate gets the organizer's fan-out item; the outside
    // one is not attempted (5.3) and gets no mail.
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "imip-key2's fan-out item");
    assert_eq!(items[0].1.changed_by, ChangedBy::PrincipalId(key.id));
    assert_ne!(items[0].1.flags & EVENT_NOTIFICATION_ZA_PENDING, 0);
    let got = body(&key.client, &href).await;
    assert_eq!(
        schedule_statuses(&got, ICalendarProperty::Attendee, DELEGATE),
        ["5.3"],
        "{got}"
    );
    assert_eq!(
        sink().count_to(DELEGATE),
        0,
        "the outside delegate was mailed"
    );

    // imip-key2 drains and accepts; its REPLY is authenticated by the pair
    // the delegating fan-out added.
    drain(key2).await;
    let copy = copy_href(test, key2.id, uid).await;
    set_partstat(&key2.client, &copy, KEY2, "ACCEPTED").await;
    drain(key).await;
    assert_eq!(
        partstat_of(&key.client, &href, KEY2).await.as_deref(),
        Some("ACCEPTED")
    );
    delete_quietly(&key2.client, &copy).await;
    delete_quietly(&key.client, &href).await;
}

/// The pair count and each of `addresses`' tokens, to compare pairs.
fn pair_view(
    pairs: Option<&TokenPairs>,
    addresses: &[&str],
) -> (usize, Vec<Option<[u8; TOKEN_LEN]>>) {
    (
        pairs.map_or(0, TokenPairs::len),
        addresses
            .iter()
            .map(|address| pairs.and_then(|pairs| pairs.get(address)).copied())
            .collect(),
    )
}

/// 4.3 point 4, invariant 45, spec question 6: pairs are never touched by mail.
async fn pairs_untouched_by_mail(test: &TestServer, key: &Party, key2: &Party) {
    let _drain = DrainOn::new();
    let uid = "imip-in-p";
    let href = own_href(key, uid);
    let path = format!("default/{uid}.ics");
    let keys = za_unlock(test, key.id, STRONG).await;
    let keys2 = za_unlock(test, key2.id, STRONG).await;
    assert_eq!(
        put(
            key.name,
            STRONG,
            href.clone(),
            event(uid, key.name, &[KEY2, ATT], 0, "")
        )
        .await,
        201
    );
    let organizer_pairs = pair_view(
        stored_pairs(test, &keys, key.id, &path).await.as_ref(),
        &[KEY2, ATT],
    );
    // Release 2 mints a pair per scheduled attendee, ATT's included.
    assert!(
        organizer_pairs.0 == 2 && organizer_pairs.1.iter().all(Option::is_some),
        "the organizer holds imip-key2's and ATT's pairs"
    );

    // ATT's mail-borne REPLY merges; the organizer's pairs stay as stored.
    mail_in(
        test,
        ATT,
        key.name,
        "REPLY",
        &reply_ics(uid, key.name, 0, &[(ATT, "ACCEPTED")]),
    )
    .await;
    drain(key).await;
    assert_eq!(
        partstat_of(&key.client, &href, ATT).await.as_deref(),
        Some("ACCEPTED"),
        "the REPLY merged"
    );
    assert_eq!(
        pair_view(
            stored_pairs(test, &keys, key.id, &path).await.as_ref(),
            &[KEY2, ATT]
        ),
        organizer_pairs,
        "a mail-borne REPLY changed the organizer's pairs"
    );

    // imip-key2's copy holds its pair; an outside REQUEST for the UID from
    // ORG is rejected (OrganizerMismatch), its item deleted, the pair kept.
    drain(key2).await;
    let copy = copy_href(test, key2.id, uid).await;
    let copy_path = relative(&copy);
    let copy_pairs = pair_view(
        stored_pairs(test, &keys2, key2.id, &copy_path)
            .await
            .as_ref(),
        &[KEY2],
    );
    assert!(
        copy_pairs.0 == 1 && copy_pairs.1[0].is_some(),
        "imip-key2's copy holds its pair"
    );
    let before2 = inbox_ids(test, key2.id).await;
    mail_in(test, ORG, KEY2, "REQUEST", &event(uid, ORG, &[KEY2], 1, "")).await;
    let items = new_inbox_items(test, key2.id, &before2).await;
    assert_eq!(items.len(), 1, "the known UID let the item in");
    drain(key2).await;
    assert!(
        !inbox_ids(test, key2.id).await.contains(&items[0].0),
        "the rejected REQUEST's item was not deleted"
    );
    assert_eq!(
        pair_view(
            stored_pairs(test, &keys2, key2.id, &copy_path)
                .await
                .as_ref(),
            &[KEY2]
        ),
        copy_pairs,
        "a mail-borne REQUEST changed the copy's pairs"
    );
    delete_quietly(&key2.client, &copy).await;
    delete_quietly(&key.client, &href).await;
}

/// 4.3: an outside delegate's REPLY into a key delegator's copy merges and fans out nothing.
async fn delegate_reply_into_delegators_copy(test: &TestServer, key: &Party, key2: &Party) {
    let _drain = DrainOn::new();
    let uid = "imip-in-sd";
    mail_in(
        test,
        ORG,
        key.name,
        "REQUEST",
        &event(uid, ORG, &[key.name], 0, ""),
    )
    .await;
    drain(key).await;
    let copy = copy_href(test, key.id, uid).await;

    // key delegates to DELEGATE.
    let delegated = body(&key.client, &copy)
        .await
        .replace("\r\n ", "")
        .split("\r\n")
        .map(|line| {
            if line.starts_with("ATTENDEE") && line.ends_with(&format!("mailto:{}", key.name)) {
                format!(
                    "{}\r\nATTENDEE;DELEGATED-FROM=\"mailto:{}\";PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{DELEGATE}",
                    with_partstat(line, &format!("DELEGATED;DELEGATED-TO=\"mailto:{DELEGATE}\"")),
                    key.name
                )
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\r\n");
    assert!(delegated.contains("DELEGATED-TO"), "{delegated}");
    assert_eq!(put(key.name, STRONG, copy.clone(), delegated).await, 204);

    // DELEGATE's REPLY delegates on to imip-key2. It lists its delegator
    // too (RFC 5546 4.2.5), as it must: upstream's snapshot refuses a
    // message that names none of the recipient's addresses.
    let before2 = inbox_ids(test, key2.id).await;
    let reply = format!(
        concat!(
            "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:{delegate}\":mailto:{key}\r\n",
            "ATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:{key2}\";DELEGATED-FROM=\"mailto:{key}\":mailto:{delegate}\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE;DELEGATED-FROM=\"mailto:{delegate}\":mailto:{key2}\r\n"
        ),
        key2 = KEY2,
        key = key.name,
        delegate = DELEGATE
    );
    let before = inbox_ids(test, key.id).await;
    mail_in(
        test,
        DELEGATE,
        key.name,
        "REPLY",
        &event(uid, ORG, &[], 0, &reply),
    )
    .await;
    let items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(items.len(), 1, "the delegate's REPLY item");
    drain(key).await;
    // Upstream's guarded arm applied the merge: the item is kept, drained.
    assert_drained_mail_borne(
        &new_inbox_items(test, key.id, &before).await,
        1,
        "the delegate's REPLY",
    );
    let got = body(&key.client, &copy).await;
    assert_eq!(
        partstat(&got, DELEGATE).as_deref(),
        Some("DELEGATED"),
        "the delegate's REPLY did not merge into the delegator's copy: {got}"
    );
    assert!(
        got.replace("\r\n ", "").contains(&format!("mailto:{KEY2}")),
        "upstream's merge adds the delegate's delegate: {got}"
    );
    assert_eq!(
        inbox_ids(test, key2.id).await,
        before2,
        "an attendee's copy fanned out"
    );
    delete_quietly(&key.client, &copy).await;
}

/// Spec 4.5: a plain local organizer's invitation. (The plain local attendee's REPLY
/// needs the outbound path: Task 7's `plain_attendee_round_trip`.)
async fn plain_local_organizer(test: &TestServer, key: &Party, plain: &Account) {
    let plain_client = plain_client(plain);
    let calendar = "/dav/cal/imip-plain%40example.com/imip/";
    plain_client
        .request("MKCALENDAR", calendar, MKCALENDAR)
        .await
        .with_status(StatusCode::CREATED);
    let uid = "imip-in-pl";
    let href = format!("{calendar}{uid}.ics");
    let before = inbox_ids(test, key.id).await;
    plain_client
        .request_with_headers(
            "PUT",
            &href,
            [CONTENT_TYPE],
            event(uid, PLAIN, &[key.name], 0, ""),
        )
        .await
        .with_status(StatusCode::CREATED);
    // Upstream's task mails it; local delivery makes the item.
    wait_for_delivery(test).await;
    let items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].1.changed_by,
        ChangedBy::CalendarAddress(String::new())
    );
    let (_, carriers) = open_pending_item_as(test, key.id, STRONG, items[0].0).await;
    assert_eq!(carriers.sender.as_deref(), Some(PLAIN));

    let _drain = DrainOn::new();
    drain(key).await;
    let copy = copy_href(test, key.id, uid).await;
    // imip-plain's update reaches the copy after the next drain.
    plain_client
        .request_with_headers(
            "PUT",
            &href,
            [CONTENT_TYPE],
            event(uid, PLAIN, &[key.name], 1, "")
                .replace(&format!("SUMMARY:imip {uid}"), "SUMMARY:imip-in-pl updated"),
        )
        .await
        .with_status(StatusCode::NO_CONTENT);
    wait_for_delivery(test).await;
    drain(key).await;
    let got = body(&key.client, &copy).await;
    assert!(got.contains("SUMMARY:imip-in-pl updated"), "{got}");
    delete_quietly(&key.client, &copy).await;
    delete_quietly(&plain_client, &href).await;
    plain_client
        .request("DELETE", calendar, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
}

/// Review Focus 2.
async fn rf_2_sender_case(test: &TestServer, key: &Party) {
    let uid = "imip-in-rf2";
    let ics = event(uid, ORG, &[key.name], 0, "").replace(
        &format!("ORGANIZER:mailto:{ORG}"),
        &format!("ORGANIZER;CN=Org:mailto:{ORG}"),
    );
    let before = inbox_ids(test, key.id).await;
    deliver(
        "dmarc-org@remote.test",
        key.name,
        &itip_mail("\"Org\" <IMIP-ORG@Remote.Test>", key.name, "REQUEST", &ics),
    )
    .await;
    wait_for_queue(test).await;
    let items = new_inbox_items(test, key.id, &before).await;
    assert_eq!(items.len(), 1);
    let (_, carriers) = open_pending_item_as(test, key.id, STRONG, items[0].0).await;
    assert_eq!(carriers.sender.as_deref(), Some("imip-org@remote.test"));

    let _drain = DrainOn::new();
    drain(key).await;
    let copy = copy_href(test, key.id, uid).await;
    delete_quietly(&key.client, &copy).await;
}

/// Review Focus 4.
async fn rf_4_recurring_instance(test: &TestServer, key: &Party, plain: &Account) {
    let plain_id = plain.id().document_id();
    let plain_client = plain_client(plain);
    let card = add_contact(&plain_client, PLAIN, ORG).await;
    let uid = "imip-in-rf4";
    let attendees = [key.name, PLAIN];
    // A weekly series, one overridden instance, a CANCEL of another.
    let emails = [
        (
            "REQUEST",
            event(uid, ORG, &attendees, 0, "RRULE:FREQ=WEEKLY;COUNT=4"),
        ),
        (
            "REQUEST",
            instance_ics(
                uid,
                &attendees,
                1,
                "20990117T090000Z",
                "20990117T140000Z",
                "",
            )
            .replace(&format!("SUMMARY:imip {uid}"), "SUMMARY:imip-in-rf4 moved"),
        ),
        (
            "CANCEL",
            instance_ics(
                uid,
                &attendees,
                1,
                "20990124T090000Z",
                "20990124T090000Z",
                "STATUS:CANCELLED",
            ),
        ),
    ];
    for (method, ics) in &emails {
        // The same email to both accounts, in one transaction.
        SmtpConnection::connect()
            .await
            .ingest(
                "dmarc-org@remote.test",
                &[key.name, PLAIN],
                &itip_mail(ORG, key.name, method, ics),
            )
            .await;
        wait_for_queue(test).await;
    }
    let _drain = DrainOn::new();
    drain(key).await;
    let key_copy = copy_href(test, key.id, uid).await;
    let plain_copy = copy_href(test, plain_id, uid).await;
    let key_body = body(&key.client, &key_copy).await;
    let plain_body = body(&plain_client, &plain_copy).await;
    assert_eq!(
        comparable(&key_body),
        comparable(&plain_body),
        "the key account's copy differs from upstream's:\n{key_body}\n---\n{plain_body}"
    );
    delete_quietly(&key.client, &key_copy).await;
    delete_quietly(&plain_client, &plain_copy).await;
    delete_contact(&plain_client, &card).await;
}

/// An event body as Review Focus 4 compares it: unfolded, without DTSTAMP
/// lines or SCHEDULE-STATUS parameters, the account addresses replaced.
fn comparable(body: &str) -> Vec<String> {
    body.replace("\r\n ", "")
        .split("\r\n")
        .filter(|line| !line.is_empty() && !line.starts_with("DTSTAMP"))
        .map(|line| {
            let mut line = line.to_string();
            while let Some(at) = line.find(";SCHEDULE-STATUS=") {
                let rest = &line[at + 1..];
                let end = rest.find([';', ':']).map_or(line.len(), |end| at + 1 + end);
                line.replace_range(at..end, "");
            }
            line.replace(KEY, "account@example.com")
                .replace(PLAIN, "account@example.com")
        })
        .collect()
}

/// imip-plain's CalDAV client.
fn plain_client(plain: &Account) -> DummyWebDavClient {
    DummyWebDavClient::new(plain.id().document_id(), PLAIN, STRONG, PLAIN)
}

/// Deletes the module's items in every party's inbox (the drain cases'
/// included), key's contacts and imip-plain. Each drain case deleted the
/// events it created.
async fn cleanup(
    test: &TestServer,
    admin: &Account,
    key: &Party,
    parties: [(&Party, &[u32]); 2],
    cards: Vec<String>,
    plain: Account,
) {
    for (party, start) in parties {
        party.delete_items_since(test, start).await;
        assert!(
            mail_borne_items(test, party.id).await.is_empty(),
            "a mail-borne item of {} outlived the module",
            party.name
        );
        assert_eq!(
            uid_copies_of_module(test, party.id).await,
            0,
            "an event of {} outlived the module",
            party.name
        );
    }
    for card in cards {
        delete_contact(&key.client, &card).await;
    }
    admin.destroy_account(plain).await;
    test.wait_for_tasks().await;
}

/// `email_ingest` as local delivery calls it, for a message the SMTP layer
/// classified as spam, from a DMARC-passing sender.
async fn ingest_junk(
    test: &TestServer,
    account_id: u32,
    to: &str,
    envelope_from: &str,
    message: &str,
) {
    let access_token = test.server.access_token(account_id).await.unwrap().build();
    test.server
        .email_ingest(IngestEmail {
            raw_message: message.as_bytes(),
            blob_hash: None,
            message: MessageParser::new().parse(message.as_bytes()),
            access_token: &access_token,
            mailbox_ids: vec![INBOX_ID],
            keywords: vec![],
            received_at: None,
            source: IngestSource::Smtp {
                deliver_to: to,
                envelope_from,
                sender_authentication: SenderAuthentication::DmarcPass,
                is_spam: true,
            },
            metadata: None,
            session_id: 0,
        })
        .await
        .unwrap();
}

/// ORG's REQUEST for `imip-in-big` to `to` whose calendar part is exactly
/// `limit` bytes.
fn oversized_request(to: &str, limit: usize) -> String {
    let ics = |pad: &str| with_method(&event("imip-in-big", ORG, &[to], 0, pad), "REQUEST");
    let mut remaining = limit - ics("").len();
    let mut pad = String::new();
    while remaining > 152 {
        pad_line(&mut pad, 76);
        remaining -= 76;
    }
    if remaining > 76 {
        let half = remaining / 2;
        pad_line(&mut pad, half);
        pad_line(&mut pad, remaining - half);
    } else {
        pad_line(&mut pad, remaining);
    }
    let ics = ics(&pad);
    assert_eq!(ics.len(), limit);
    let message = itip_mail(ORG, to, "REQUEST", &ics);
    let parsed = MessageParser::new().parse(message.as_bytes()).unwrap();
    assert_eq!(
        parsed
            .root()
            .parts()
            .find(|part| part.is_content_type("text", "calendar"))
            .and_then(|part| part.text())
            .map(|text| text.len()),
        Some(limit),
        "the calendar part is the limit's size"
    );
    message
}

/// One padding line of `len` bytes, its line break included.
fn pad_line(pad: &mut String, len: usize) {
    const PREFIX: &str = "X-IMIP-PAD:";
    assert!(len >= PREFIX.len() + 2);
    pad.push_str(PREFIX);
    pad.push_str(&"a".repeat(len - PREFIX.len() - 2));
    pad.push_str("\r\n");
}

/// Review Focus 1's message, as real providers compose an invitation:
/// `multipart/mixed` holding a `multipart/alternative` (text, HTML and a
/// base64 `text/calendar; method=REQUEST` part) and an `application/ics`
/// attachment of the same ICS.
fn provider_mail(to: &str, ics: &str) -> String {
    let base64 = |bytes: &[u8]| {
        STANDARD
            .encode(bytes)
            .as_bytes()
            .chunks(76)
            .map(|line| std::str::from_utf8(line).unwrap())
            .collect::<Vec<_>>()
            .join("\r\n")
    };
    format!(
        concat!(
            "From: {org}\r\nTo: <{to}>\r\nSubject: Invitation: quarterly review\r\n",
            "Message-ID: <{id:016x}.za-imip-rf1@remote.test>\r\n",
            "Date: Mon, 02 Nov 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"rf1-mixed\"\r\n\r\n",
            "--rf1-mixed\r\n",
            "Content-Type: multipart/alternative; boundary=\"rf1-alt\"\r\n\r\n",
            "--rf1-alt\r\n",
            "Content-Type: text/plain; charset=\"UTF-8\"; format=flowed; delsp=yes\r\n",
            "Content-Transfer-Encoding: base64\r\n\r\n{text}\r\n",
            "--rf1-alt\r\n",
            "Content-Type: text/html; charset=\"UTF-8\"\r\n",
            "Content-Transfer-Encoding: quoted-printable\r\n\r\n",
            "<p>You have been invited.</p>\r\n",
            "--rf1-alt\r\n",
            "Content-Type: text/calendar; charset=\"UTF-8\"; method=REQUEST\r\n",
            "Content-Transfer-Encoding: base64\r\n\r\n{cal}\r\n",
            "--rf1-alt--\r\n\r\n",
            "--rf1-mixed\r\n",
            "Content-Type: application/ics; name=\"invite.ics\"\r\n",
            "Content-Disposition: attachment; filename=\"invite.ics\"\r\n",
            "Content-Transfer-Encoding: base64\r\n\r\n{cal}\r\n",
            "--rf1-mixed--\r\n"
        ),
        org = ORG,
        to = to,
        id = store::rand::random::<u64>(),
        text = base64(b"You have been invited to a quarterly review."),
        cal = base64(ics.as_bytes()),
    )
}

/// The first VEVENT's SUMMARY.
fn summary_of(ical: &ICalendar) -> Option<String> {
    ical.components
        .iter()
        .filter(|comp| comp.component_type == ICalendarComponentType::VEvent)
        .flat_map(|comp| comp.entries.iter())
        .find(|entry| entry.name == ICalendarProperty::Summary)
        .and_then(|entry| entry.values.first())
        .and_then(|value| value.as_text())
        .map(str::to_string)
}
