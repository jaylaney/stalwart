/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 5, inbound (iMIP spec 4): a key account's incoming iMIP mail
//! becomes a sealed, mail-borne pending item at delivery, sealed to the
//! served public key with the sender in `X-ZA-SENDER`, beside the sealed
//! mail. The drain stays skipped here (the suite's `DRAIN_SKIP_ALL`), so
//! every item is read at store level.

use super::{
    ATT, NODMARC, ORG, STRANGER, add_contact, delete_contact, deliver, event, itip_mail,
    with_method,
};
use crate::{
    utils::{
        account::Account,
        server::TestServer,
        za::{
            envelope_wrap, inbox_ids, item_method, mail_borne_items, mail_count, new_inbox_items,
            open_pending_item_as, wait_for_queue,
        },
    },
    za::{STRONG, fanout::Party, user_permissions},
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
use groupware::calendar::{ChangedBy, EVENT_NOTIFICATION_PRIVATE, EVENT_NOTIFICATION_ZA_PENDING};
use mail_parser::MessageParser;

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

    // The items of `imip-in-1` and `imip-in-d3` are still pending here;
    // every other case deleted its own.
    cleanup(test, &admin, &key, &start, vec![org_card], plain).await;
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
        // Step 3: a method that does not continue.
        case(
            "PUBLISH",
            "dmarc-org@remote.test",
            itip_mail(
                ORG,
                to,
                "PUBLISH",
                &event("imip-in-publish", ORG, &[to], 0, ""),
            ),
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

/// Deletes the module's items (the drain cases' included), its contacts and
/// imip-plain.
async fn cleanup(
    test: &TestServer,
    admin: &Account,
    key: &Party,
    start: &[u32],
    cards: Vec<String>,
    plain: Account,
) {
    key.delete_items_since(test, start).await;
    assert!(
        mail_borne_items(test, key.id).await.is_empty(),
        "a mail-borne item outlived the module"
    );
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
