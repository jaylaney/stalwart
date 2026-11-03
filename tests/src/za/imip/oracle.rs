/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 5, the differential oracle (iMIP spec 3.3, 3.4, 8.3). One
//! scheduling exchange with outside parties runs step by step, identically,
//! for a key account and for a plain account, in both directions: inbound
//! (`ORG` organizes) and outbound (the account organizes, `ATT` attends).
//! After each step the two sides must be equal, up to exactly the permitted
//! differences of spec 3.3 that `normalize` applies and nothing more:
//!
//! 1. The sealing: the key account is read through its unsealed views (a
//!    DAV GET), so no normalization is needed for it.
//! 2. Addresses, display names and hrefs, mapped between the accounts
//!    (`PLAIN` on the plain side becomes `KEY`; the key side is not mapped,
//!    so a key-side value that names the plain account still differs). The
//!    mapping changes a line's length, hence its fold point, so iCalendar
//!    content and header values are unfolded first.
//! 3. Volatile values: Message-ID, Date, DTSTAMP, the logo CID and imported
//!    file names are replaced by placeholders (MIME boundaries decode away in
//!    the compared parts). The module runs with `httpRsvpEnable` off, so the
//!    email bodies otherwise compare exactly.
//! 4. `SCHEDULE-STATUS` parameters, removed in stored copies and in the
//!    CANCELs that copy them (Jay's decision 9).
//!
//! Spec 3.3's other permitted differences are not normalizations: the
//! sealing (item 1) is the unsealed view, timing (item 5) is the drain
//! trigger after every step, the inbox row (item 6) is not compared (only the
//! item trees are, and every key-side item must be drained), and item 7's
//! cases (the SEQUENCE reconciliation, the `SENT-BY` scrub, a failed
//! submission) are not exercised.

use super::{ATT, ORG, add_contact, delete_contact, deliver, event, itip_mail};
use crate::{
    utils::{
        account::Account,
        http::HttpRequest,
        server::TestServer,
        smtp_sink::{SinkMessage, sink},
        webdav::DummyWebDavClient,
        za::{
            copy_href, inbox_ids, inbox_items, item_partstat, mail_borne_items, new_inbox_items,
            trigger_drain, wait_for_delivery,
        },
    },
    za::{
        STRONG,
        drain::{DrainOn, body, set_partstat},
        fanout::{NO_REPLY, Party, uid_copies},
        uid_lease::put,
        user_permissions,
    },
};
use ::registry::schema::{
    enums::MtaInboundThrottleKey,
    prelude::{ObjectType, Property},
    structs::{CalendarScheduling, MtaInboundThrottle},
};
use calcard::icalendar::ICalendar;
use groupware::calendar::EVENT_NOTIFICATION_ZA_PENDING;
use hyper::{Method, StatusCode};
use serde_json::json;
use std::fmt;
use types::id::Id;

/// The key account of the oracle (spec 8.3).
const KEY: &str = "imip-oracle-key@example.com";
/// The plain account of the oracle.
const PLAIN: &str = "imip-oracle-plain@example.com";
/// The local parts, which are also the accounts' display names, so the one
/// mapping covers addresses, display names and (percent-encoded) hrefs.
const KEY_NAME: &str = "imip-oracle-key";
const PLAIN_NAME: &str = "imip-oracle-plain";

/// The inbound exchange's UID (ORG organizes) and the outbound one's (the
/// account organizes).
const IN_UID: &str = "imip-oracle-in";
const OUT_UID: &str = "imip-oracle-out";

/// Which account a value was read from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Plain,
    Key,
}

/// What a value is, which decides the normalizations that apply to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// iCalendar content: unfolded; DTSTAMP and `SCHEDULE-STATUS` removed.
    Ics,
    /// A header value or an envelope address: unfolded.
    Header,
    /// A decoded body part: nothing is unfolded.
    Body,
    /// Message-ID and Date: replaced by a placeholder when present.
    Volatile,
}

/// Spec 3.3's permitted differences, and nothing more (see the module
/// documentation for the item each step cites).
fn normalize(side: Side, kind: Kind, text: &str) -> String {
    if kind == Kind::Volatile {
        // Item 3: Message-ID and Date.
        return "<volatile>".to_string();
    }
    let mut text = text.to_string();
    if matches!(kind, Kind::Ics | Kind::Header) {
        // Item 2's consequence: a fold point follows the mapped line length.
        for fold in ["\r\n ", "\r\n\t", "\n ", "\n\t"] {
            text = text.replace(fold, "");
        }
    }
    if side == Side::Plain {
        // Item 2: addresses, display names and hrefs.
        text = text.replace(PLAIN_NAME, KEY_NAME);
    }
    if kind == Kind::Ics {
        text = text
            .split('\n')
            .map(|line| {
                let (line, end) = match line.strip_suffix('\r') {
                    Some(line) => (line, "\r"),
                    None => (line, ""),
                };
                // Item 3: DTSTAMP.
                let line = if line.starts_with("DTSTAMP") {
                    "DTSTAMP:<volatile>".to_string()
                } else {
                    // Item 4: SCHEDULE-STATUS.
                    without_schedule_status(line)
                };
                line + end
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    // Item 3: the logo CID and imported file names.
    imported_names(&logo_cids(&text))
}

/// `line` without its `SCHEDULE-STATUS` parameters, the value running to the
/// next `;` or `:` outside double quotes.
fn without_schedule_status(line: &str) -> String {
    const PARAM: &str = ";SCHEDULE-STATUS=";
    let mut line = line.to_string();
    while let Some(at) = line.find(PARAM) {
        let value = at + PARAM.len();
        let mut quoted = false;
        let end = line[value..]
            .char_indices()
            .find(|(_, c)| match c {
                '"' => {
                    quoted = !quoted;
                    false
                }
                ';' | ':' => !quoted,
                _ => false,
            })
            .map_or(line.len(), |(offset, _)| value + offset);
        line.replace_range(at..end, "");
    }
    line
}

/// The length of the run of ASCII digits at the start of `bytes`.
fn digits(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|b| b.is_ascii_digit()).count()
}

/// `logo.<digits>@` (the logo CID, `logo.<time>@<domain>`) with the time
/// replaced.
fn logo_cids(text: &str) -> String {
    const PREFIX: &str = "logo.";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(PREFIX) {
        let after = &rest[at + PREFIX.len()..];
        let run = digits(after.as_bytes());
        out.push_str(&rest[..at + PREFIX.len()]);
        if run > 0 && after[run..].starts_with('@') {
            out.push_str("<cid>");
            rest = &after[run..];
        } else {
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// `<digits>_<digits>.ics` (an imported event's name, `<time>_<random>.ics`)
/// replaced.
fn imported_names(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() && (i == 0 || !bytes[i - 1].is_ascii_digit()) {
            let first = digits(&bytes[i..]);
            if bytes[i + first..].first() == Some(&b'_') {
                let second = digits(&bytes[i + first + 1..]);
                if second > 0 && bytes[i + first + 1 + second..].starts_with(b".ics") {
                    out.push_str("<imported>.ics");
                    i += first + 1 + second + 4;
                    continue;
                }
            }
        }
        let c = text[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Which exchange a step belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    /// ORG organizes; the account attends.
    Inbound,
    /// The account organizes; ATT attends.
    Outbound,
}

impl Direction {
    fn uid(self) -> &'static str {
        match self {
            Direction::Inbound => IN_UID,
            Direction::Outbound => OUT_UID,
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Direction::Inbound => "inbound",
            Direction::Outbound => "outbound",
        })
    }
}

/// What the exchange itself says a step produces, asserted on each side, so
/// that two sides that are equal because both did nothing cannot pass (spec
/// 3.4).
#[derive(Clone, Copy)]
struct Expect {
    /// Whether the account holds the event afterwards.
    present: bool,
    /// Substrings of the stored event, unfolded.
    contains: &'static [&'static str],
    /// An attendee's PARTSTAT in the stored event: the address (`None` for
    /// the account itself) and its value.
    partstat: Option<(Option<&'static str>, &'static str)>,
    /// The METHODs of the new inbox items, in order.
    items: &'static [&'static str],
    /// The recipient and METHOD of each message the account sent.
    sink: &'static [(&'static str, &'static str)],
}

/// The oracle's two accounts.
struct Oracle {
    plain: Party,
    key: Party,
}

impl Oracle {
    fn party(&self, side: Side) -> &Party {
        match side {
            Side::Plain => &self.plain,
            Side::Key => &self.key,
        }
    }
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access iMIP oracle tests...");
    self_check();

    let admin = test.account("admin@example.com").clone();
    let key = admin
        .create_key_user_account(KEY, STRONG, KEY_NAME, &[], user_permissions())
        .await;
    test.insert_account(key);
    let plain = admin
        .create_user_account(PLAIN, STRONG, PLAIN_NAME, &[], user_permissions())
        .await;
    let oracle = Oracle {
        plain: Party {
            id: plain.id().document_id(),
            name: PLAIN,
            client: DummyWebDavClient::new(plain.id().document_id(), PLAIN, STRONG, PLAIN),
        },
        key: Party::new(test, KEY),
    };
    let key_start = inbox_ids(test, oracle.key.id).await;

    // The email bodies compare exactly without RSVP links (spec 3.3 item 3),
    // and the module mails ATT and ORG more often than upstream's default
    // inbound throttle on a sender domain and recipient allows (25 an hour):
    // both are restored at the end.
    let rsvp = set_http_rsvp(&admin, false).await;
    let throttles = rcpt_throttles(&admin).await;
    set_throttles(&admin, &throttles, false).await;
    // The precondition (spec 8.3): ORG is a ContactCard of both accounts, so
    // the first REQUEST imports on both sides without auto-add.
    let cards = [
        (
            &oracle.plain,
            add_contact(&oracle.plain.client, PLAIN, ORG).await,
        ),
        (&oracle.key, add_contact(&oracle.key.client, KEY, ORG).await),
    ];
    // The key side imports and merges in its drain.
    let drain = DrainOn::new();

    exchange(test, &oracle, Direction::Inbound).await;
    exchange(test, &oracle, Direction::Outbound).await;

    // Cleanup: the inbound event's copies (quietly: no REPLY), the key
    // account's inbox items, the ContactCards, the plain account.
    for party in [&oracle.plain, &oracle.key] {
        let href = copy_href(test, party.id, IN_UID).await;
        party
            .client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
        for uid in [IN_UID, OUT_UID] {
            assert_eq!(
                uid_copies(test, party.id, uid).await,
                0,
                "an event of {} outlived the module",
                party.name
            );
        }
    }
    oracle.key.delete_items_since(test, &key_start).await;
    assert!(
        mail_borne_items(test, oracle.key.id).await.is_empty(),
        "a mail-borne item outlived the module"
    );
    for (party, card) in cards {
        delete_contact(&party.client, &card).await;
    }
    drop(drain);
    sink().clear();
    admin.destroy_account(plain).await;
    set_throttles(&admin, &throttles, true).await;
    set_http_rsvp(&admin, rsvp).await;
    test.wait_for_tasks().await;
}

/// The exchange of `direction`, one step at a time, both sides compared after
/// each (spec 3.3, 8.3).
async fn exchange(test: &TestServer, oracle: &Oracle, direction: Direction) {
    let expectations = match direction {
        // ORG's REQUEST, ORG's update, the account's acceptance (a REPLY to
        // ORG), ORG's CANCEL.
        Direction::Inbound => [
            Expect {
                present: true,
                contains: &["SUMMARY:imip imip-oracle-in", "DTSTART:20990110T090000Z"],
                partstat: Some((None, "NEEDS-ACTION")),
                items: &["REQUEST"],
                sink: &[],
            },
            Expect {
                present: true,
                contains: &["SUMMARY:imip-oracle-in updated", "DTSTART:20990111T100000Z"],
                partstat: None,
                items: &["REQUEST"],
                sink: &[],
            },
            Expect {
                present: true,
                contains: &["SUMMARY:imip-oracle-in updated"],
                partstat: Some((None, "ACCEPTED")),
                items: &[],
                sink: &[(ORG, "REPLY")],
            },
            Expect {
                present: true,
                contains: &["STATUS:CANCELLED"],
                partstat: None,
                items: &["CANCEL"],
                sink: &[],
            },
        ],
        // The invitation (a REQUEST to ATT), ATT's REPLY, the update (a
        // REQUEST), the DELETE (a CANCEL).
        Direction::Outbound => [
            Expect {
                present: true,
                contains: &["SUMMARY:imip imip-oracle-out"],
                partstat: Some((Some(ATT), "NEEDS-ACTION")),
                items: &[],
                sink: &[(ATT, "REQUEST")],
            },
            Expect {
                present: true,
                contains: &["SUMMARY:imip imip-oracle-out"],
                partstat: Some((Some(ATT), "ACCEPTED")),
                items: &["REPLY"],
                sink: &[],
            },
            Expect {
                present: true,
                contains: &["SUMMARY:imip-oracle-out updated"],
                partstat: None,
                items: &[],
                sink: &[(ATT, "REQUEST")],
            },
            Expect {
                present: false,
                contains: &[],
                partstat: None,
                items: &[],
                sink: &[(ATT, "CANCEL")],
            },
        ],
    };
    for (index, expect) in expectations.into_iter().enumerate() {
        let n = index + 1;
        let label = format!("{direction} step {n}");
        sink().clear();
        let before = [
            inbox_ids(test, oracle.plain.id).await,
            inbox_ids(test, oracle.key.id).await,
        ];
        for side in [Side::Plain, Side::Key] {
            perform(test, direction, n, oracle.party(side)).await;
        }
        // Spec 3.3 item 5: the key account's inbound effects follow its next
        // DAV request.
        assert_eq!(
            trigger_drain(oracle.key.name, STRONG).await,
            207,
            "{label}: the drain"
        );
        wait_for_delivery(test).await;
        compare(test, oracle, direction, &label, &before, expect).await;
    }
}

/// One step of `direction`, as the account's side of the exchange.
async fn perform(test: &TestServer, direction: Direction, n: usize, party: &Party) {
    let account = party.name;
    match (direction, n) {
        (Direction::Inbound, 1) => {
            inbound_mail(account, "REQUEST", &inbound_event(account, 0)).await
        }
        (Direction::Inbound, 2) => {
            let update = inbound_event(account, 1)
                .replace(
                    "SUMMARY:imip imip-oracle-in",
                    "SUMMARY:imip-oracle-in updated",
                )
                .replace("DTSTART:20990110T090000Z", "DTSTART:20990111T100000Z")
                .replace("DTEND:20990110T100000Z", "DTEND:20990111T110000Z");
            assert!(
                update.contains("updated") && update.contains("20990111T100000Z"),
                "{update}"
            );
            inbound_mail(account, "REQUEST", &update).await
        }
        (Direction::Inbound, 3) => {
            // The account accepts in its client.
            let href = copy_href(test, party.id, IN_UID).await;
            set_partstat(&party.client, &href, account, "ACCEPTED").await;
        }
        (Direction::Inbound, 4) => {
            inbound_mail(
                account,
                "CANCEL",
                &event(IN_UID, ORG, &[account], 2, "STATUS:CANCELLED"),
            )
            .await
        }
        (Direction::Outbound, 1) => {
            let status = put(
                account,
                STRONG,
                own_href(account),
                event(OUT_UID, account, &[ATT], 0, ""),
            )
            .await;
            assert_eq!(status, 201, "{account}: the invitation");
        }
        (Direction::Outbound, 2) => {
            // ATT's emailed REPLY.
            let reply = event(
                OUT_UID,
                account,
                &[],
                0,
                &format!("ATTENDEE;PARTSTAT=ACCEPTED:mailto:{ATT}"),
            );
            deliver(
                "dmarc-att@remote.test",
                account,
                &itip_mail(ATT, account, "REPLY", &reply),
            )
            .await;
        }
        (Direction::Outbound, 3) => {
            let updated = body(&party.client, &own_href(account)).await.replace(
                "SUMMARY:imip imip-oracle-out",
                "SUMMARY:imip-oracle-out updated",
            );
            assert!(updated.contains("updated"), "{updated}");
            assert_eq!(
                put(account, STRONG, own_href(account), updated).await,
                204,
                "{account}: the update"
            );
        }
        (Direction::Outbound, 4) => {
            // The DELETE's CANCEL goes to ATT: no `Schedule-Reply: F`.
            let status = HttpRequest::with_credentials(8899, account, STRONG)
                .send_full(Method::DELETE, &own_href(account), None, None)
                .await
                .status
                .as_u16();
            assert_eq!(status, 204, "{account}: the delete");
        }
        _ => unreachable!(),
    }
}

/// ORG's event for `account` at `sequence`.
fn inbound_event(account: &str, sequence: u32) -> String {
    event(IN_UID, ORG, &[account], sequence, "")
}

/// ORG's iTIP `method` message holding `ics`, delivered to `account` over
/// LMTP with a DMARC-passing MAIL FROM.
async fn inbound_mail(account: &str, method: &str, ics: &str) {
    deliver(
        "dmarc-org@remote.test",
        account,
        &itip_mail(ORG, account, method, ics),
    )
    .await;
}

/// `account`'s href for its outbound event.
fn own_href(account: &str) -> String {
    format!(
        "/dav/cal/{}/default/{OUT_UID}.ics",
        account.replace('@', "%40")
    )
}

/// The href of an inbox item.
fn item_href(party: &Party, id: u32) -> String {
    format!("{}{id}.ics", party.inbox())
}

/// The comparisons of one step: every value is normalized per side, must be
/// present and non-empty on both, and must then be equal; the values that
/// differed before normalizing are listed, as evidence of what the permitted
/// differences absorbed.
struct Tally<'a> {
    label: &'a str,
    compared: usize,
    absorbed: Vec<String>,
}

impl Tally<'_> {
    fn same(
        &mut self,
        what: impl fmt::Display,
        kind: Kind,
        plain: Option<&str>,
        key: Option<&str>,
    ) {
        let label = self.label;
        let normalized = |side, value: Option<&str>| value.map(|v| normalize(side, kind, v));
        let (p, k) = (normalized(Side::Plain, plain), normalized(Side::Key, key));
        for (side, value) in [("plain", &p), ("key", &k)] {
            assert!(
                value.as_deref().is_some_and(|v| !v.is_empty()),
                "{label}: {what}: nothing on the {side} side"
            );
        }
        assert_eq!(p, k, "{label}: {what}");
        self.compared += 1;
        if plain != key {
            self.absorbed.push(what.to_string());
        }
    }
}

/// Compares the two sides after a step (spec 8.3): the object, the drained
/// state, the new inbox items, the stored event, each item's tree and each
/// message the sink got.
async fn compare(
    test: &TestServer,
    oracle: &Oracle,
    direction: Direction,
    label: &str,
    before: &[Vec<u32>; 2],
    expect: Expect,
) {
    let (plain, key) = (&oracle.plain, &oracle.key);
    let uid = direction.uid();
    let mut tally = Tally {
        label,
        compared: 0,
        absorbed: Vec::new(),
    };

    // The object on both sides: present after each step, absent after a
    // DELETE.
    let present = |count: u64| count > 0;
    let (plain_present, key_present) = (
        present(uid_copies(test, plain.id, uid).await),
        present(uid_copies(test, key.id, uid).await),
    );
    assert_eq!(
        plain_present, key_present,
        "{label}: the object on both sides"
    );
    assert_eq!(key_present, expect.present, "{label}: the object");

    // Every key-side item is drained.
    assert!(
        inbox_items(test, key.id)
            .await
            .iter()
            .all(|(_, item)| item.flags & EVENT_NOTIFICATION_ZA_PENDING == 0),
        "{label}: drained"
    );

    // The inbox items this step wrote.
    let plain_items = new_inbox_items(test, plain.id, &before[0]).await;
    let key_items = new_inbox_items(test, key.id, &before[1]).await;
    assert_eq!(plain_items.len(), key_items.len(), "{label}: inbox items");
    assert_eq!(
        key_items.len(),
        expect.items.len(),
        "{label}: the exchange's items"
    );

    // The stored event, each read with a DAV GET.
    if key_present {
        let mut stored = Vec::new();
        for party in [plain, key] {
            let href = copy_href(test, party.id, uid).await;
            stored.push(body(&party.client, &href).await);
        }
        tally.same(
            "stored event",
            Kind::Ics,
            Some(&stored[0]),
            Some(&stored[1]),
        );
        // What the exchange says it holds, on both sides.
        for (party, body) in [plain, key].into_iter().zip(&stored) {
            let unfolded = body.replace("\r\n ", "");
            for needle in expect.contains {
                assert!(
                    unfolded.contains(needle),
                    "{label}: the stored event of {} lacks {needle}:\n{body}",
                    party.name
                );
            }
            if let Some((who, partstat)) = expect.partstat {
                let who = who.unwrap_or(party.name);
                assert_eq!(
                    item_partstat(&ICalendar::parse(body).unwrap(), who).as_deref(),
                    Some(partstat),
                    "{label}: the PARTSTAT of {who} in the event of {}",
                    party.name
                );
            }
        }
    }

    // Each item's tree, read with a DAV GET of the inbox item.
    for (index, ((plain_id, _), (key_id, _))) in plain_items.iter().zip(&key_items).enumerate() {
        let p = body(&plain.client, &item_href(plain, *plain_id)).await;
        let k = body(&key.client, &item_href(key, *key_id)).await;
        tally.same("item tree", Kind::Ics, Some(&p), Some(&k));
        for (party, tree) in [(plain, &p), (key, &k)] {
            assert!(
                tree.contains(&format!("METHOD:{}", expect.items[index])),
                "{label}: the METHOD of item {index} of {}:\n{tree}",
                party.name
            );
        }
    }

    // The sink: what each account sent, matched by recipient and method.
    let mut sent = [Vec::new(), Vec::new()];
    let mut strays = Vec::new();
    for message in sink().messages() {
        if message.mail_from.eq_ignore_ascii_case(PLAIN) {
            sent[0].push(message);
        } else if message.mail_from.eq_ignore_ascii_case(KEY) {
            sent[1].push(message);
        } else {
            strays.push(message.mail_from);
        }
    }
    assert!(
        strays.is_empty(),
        "{label}: stray sink messages from {strays:?}"
    );
    assert_eq!(sent[0].len(), sent[1].len(), "{label}: sink messages");
    for messages in &mut sent {
        messages.sort_by_key(mail_key);
    }
    let mut expected = expect
        .sink
        .iter()
        .map(|(rcpt, method)| (rcpt.to_string(), method.to_string()))
        .collect::<Vec<_>>();
    expected.sort();
    for (party, messages) in [plain, key].into_iter().zip(&sent) {
        assert_eq!(
            messages.iter().map(mail_key).collect::<Vec<_>>(),
            expected,
            "{label}: the exchange's messages from {}",
            party.name
        );
    }
    for (p, k) in sent[0].iter().zip(&sent[1]) {
        let (rcpt, method) = mail_key(k);
        let (p_parts, k_parts) = (mail_parts(p), mail_parts(k));
        for ((part, kind, p_value), (_, _, k_value)) in p_parts.iter().zip(&k_parts) {
            tally.same(
                format!("{part} of the {method} to {rcpt}"),
                *kind,
                p_value.as_deref(),
                k_value.as_deref(),
            );
        }
    }
    println!(
        "{label}: the sides are equal ({} values compared; differed before normalizing: {})",
        tally.compared,
        tally.absorbed.join(", ")
    );
}

/// How a sink message is matched across the sides: its recipients and its
/// iTIP method.
fn mail_key(message: &SinkMessage) -> (String, String) {
    let mut rcpt = message
        .rcpt_to
        .iter()
        .map(|address| address.to_ascii_lowercase())
        .collect::<Vec<_>>();
    rcpt.sort();
    (
        rcpt.join(","),
        message.calendar_method().unwrap_or_default(),
    )
}

/// The parts of a sink message the oracle compares (spec 8.3): the envelope,
/// From, To, Subject, Auto-Submitted, Reply-To, the method, the decoded
/// text, HTML and iCalendar parts, and the presence of Message-ID and Date.
fn mail_parts(message: &SinkMessage) -> Vec<(&'static str, Kind, Option<String>)> {
    let header = |name| raw_header(message, name);
    vec![
        ("MAIL FROM", Kind::Header, Some(message.mail_from.clone())),
        ("RCPT TO", Kind::Header, Some(message.rcpt_to.join(","))),
        ("From", Kind::Header, header("From")),
        ("To", Kind::Header, header("To")),
        ("Subject", Kind::Header, header("Subject")),
        ("Auto-Submitted", Kind::Header, header("Auto-Submitted")),
        ("Reply-To", Kind::Header, header("Reply-To")),
        ("Message-ID", Kind::Volatile, header("Message-ID")),
        ("Date", Kind::Volatile, header("Date")),
        ("Method", Kind::Body, message.calendar_method()),
        ("Text", Kind::Body, message.text_part()),
        ("Html", Kind::Body, message.html_part()),
        ("Ics", Kind::Ics, message.calendar_part()),
    ]
}

/// The raw value of the message's top-level header `name`, unfolded by the
/// caller's normalization.
fn raw_header(message: &SinkMessage, name: &str) -> Option<String> {
    let data = String::from_utf8_lossy(&message.data);
    let head = data.split("\r\n\r\n").next()?;
    let mut value: Option<String> = None;
    for line in head.split("\r\n") {
        if line.starts_with([' ', '\t']) {
            if let Some(value) = value.as_mut() {
                value.push_str("\r\n");
                value.push_str(line);
            }
        } else if value.is_some() {
            break;
        } else if let Some((field, rest)) = line.split_once(':')
            && field.eq_ignore_ascii_case(name)
        {
            value = Some(rest.trim_start().to_string());
        }
    }
    value
}

/// Turns `httpRsvpEnable` to `enable` and reloads the settings; returns the
/// prior value.
async fn set_http_rsvp(admin: &Account, enable: bool) -> bool {
    let prior = admin
        .registry_get::<CalendarScheduling>(Id::singleton())
        .await
        .http_rsvp_enable;
    admin
        .registry_update_setting(
            CalendarScheduling {
                http_rsvp_enable: enable,
                ..Default::default()
            },
            &[Property::HttpRsvpEnable],
        )
        .await;
    admin.reload_settings().await;
    prior
}

/// The enabled inbound throttles keyed by recipient (upstream's default
/// "Sender address to recipient throttle" among them), as `imip::inbound`
/// reads them.
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

/// The oracle must not be blind (spec 3.4): each normalization equates what
/// spec 3.3 permits, alone and together, and nothing else. Runs before the
/// exchange.
fn self_check() {
    // A stored event as an account's copy reads, for `account`: its address
    // and display name in a long ORGANIZER line that folds (RFC 5545 3.1) at
    // a point that follows the name's length.
    let stored = |account: &str, stamp: &str, schedule_status: &str, summary: &str| {
        let fold = |line: String| {
            let mut out = String::new();
            let mut rest = line.as_str();
            let mut limit = 75;
            while rest.len() > limit {
                let (head, tail) = rest.split_at(limit);
                out.push_str(head);
                out.push_str("\r\n ");
                rest = tail;
                limit = 74;
            }
            out.push_str(rest);
            out
        };
        format!(
            concat!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:{uid}\r\n",
                "DTSTAMP:{stamp}\r\nSUMMARY:{summary}\r\n{organizer}\r\n{attendee}\r\n",
                "END:VEVENT\r\nEND:VCALENDAR\r\n"
            ),
            uid = OUT_UID,
            stamp = stamp,
            summary = summary,
            organizer = fold(format!(
                "ORGANIZER;CN=\"{account} organizer of the exchange\":mailto:{account}@example.com"
            )),
            attendee = fold(format!(
                "ATTENDEE;PARTSTAT=ACCEPTED{schedule_status}:mailto:{ATT}"
            ))
        )
    };
    let (stamp, later) = ("20260101T000000Z", "20261102T101500Z");
    let base = stored(KEY_NAME, stamp, "", "same");
    let ics = |side, text: &str| normalize(side, Kind::Ics, text);
    // Every sample is read as the key side unless it is the mapped one, so
    // each rule is exercised alone.
    let same = |a: &str, b: &str| ics(Side::Key, a) == ics(Side::Key, b);

    // Item 3: DTSTAMP alone.
    assert!(same(&base, &stored(KEY_NAME, later, "", "same")), "DTSTAMP");
    // Item 4: SCHEDULE-STATUS alone, and a quoted value with a colon.
    assert!(
        same(
            &base,
            &stored(KEY_NAME, stamp, ";SCHEDULE-STATUS=2.0", "same")
        ),
        "SCHEDULE-STATUS"
    );
    assert_eq!(
        without_schedule_status("ATTENDEE;SCHEDULE-STATUS=\"3.7;x:y\";CN=A:mailto:a@b"),
        "ATTENDEE;CN=A:mailto:a@b"
    );
    // Item 2: the mapping alone (the plain copy's address, display name and
    // fold points), and its consequence, a fold alone.
    let plain_copy = stored(PLAIN_NAME, stamp, "", "same");
    assert!(plain_copy.contains("\r\n "), "the sample folds a line");
    assert_eq!(
        ics(Side::Plain, &plain_copy),
        ics(Side::Key, &base),
        "the mapping"
    );
    assert!(same(&base, &base.replace("\r\n ", "")), "a fold");
    // All of them together.
    assert_eq!(
        ics(
            Side::Plain,
            &stored(PLAIN_NAME, stamp, ";SCHEDULE-STATUS=2.0", "same")
        ),
        ics(Side::Key, &stored(KEY_NAME, later, "", "same")),
        "the permitted differences together"
    );

    // And nothing else: a different SUMMARY, PARTSTAT, address, property or
    // account name stays different, with or without the permitted
    // differences beside it.
    let other = stored(KEY_NAME, later, ";SCHEDULE-STATUS=2.0", "same");
    for (what, changed) in [
        ("SUMMARY", stored(KEY_NAME, later, "", "changed")),
        (
            "PARTSTAT",
            other.replace("PARTSTAT=ACCEPTED", "PARTSTAT=DECLINED"),
        ),
        (
            "an attendee address",
            other.replace(ATT, "imip-other@remote.test"),
        ),
        (
            "an added property",
            other.replace("UID:", "DTSTART:20990101T000000Z\r\nUID:"),
        ),
        ("a removed property", other.replace("SUMMARY:same\r\n", "")),
        (
            "the organizer's name",
            other.replace("organizer of the exchange", "another organizer"),
        ),
    ] {
        assert!(
            !same(&base, &changed),
            "{what} is not a permitted difference"
        );
    }
    // The key side is not mapped: a key copy that names the plain account
    // differs from one that names itself.
    assert_ne!(
        ics(Side::Key, &plain_copy),
        ics(Side::Key, &base),
        "the key side naming the plain account"
    );

    // Body kinds: the logo CID and imported names are placeholders; nothing
    // else is, and nothing is unfolded.
    let body = |side, text: &str| normalize(side, Kind::Body, text);
    assert_eq!(
        body(Side::Plain, "<img src=\"cid:logo.1730000000@example.com\">"),
        body(Side::Key, "<img src=\"cid:logo.1730000099@example.com\">"),
        "the logo CID"
    );
    assert_ne!(
        body(Side::Plain, "cid:logo.1730000000@example.com"),
        body(Side::Key, "cid:logo.1730000000@other.example"),
        "the logo CID's domain"
    );
    assert_eq!(
        body(Side::Plain, "/default/1730000000_9182736455.ics"),
        body(Side::Key, "/default/1730000099_5546372819.ics"),
        "an imported name"
    );
    assert_ne!(
        body(Side::Plain, "/default/1730000000_9182736455.ics"),
        body(Side::Key, "/default/event.ics"),
        "a name that is not an imported one"
    );
    assert_ne!(
        body(Side::Plain, "a\r\n b"),
        body(Side::Key, "ab"),
        "a body is not unfolded"
    );
    // Header kinds: unfolded and mapped; Message-ID and Date reduce to their
    // presence.
    let header = |side, text: &str| normalize(side, Kind::Header, text);
    assert_eq!(
        header(
            Side::Plain,
            "\"imip-oracle-plain\" <imip-oracle-pla\r\n in@example.com>"
        ),
        header(
            Side::Key,
            "\"imip-oracle-key\" <imip-oracle-key@example.com>"
        ),
        "a folded header"
    );
    assert_ne!(
        header(
            Side::Plain,
            "\"imip-oracle-plain\" <imip-oracle-plain@example.com>"
        ),
        header(
            Side::Key,
            "\"imip-oracle-key\" <imip-oracle-other@example.com>"
        ),
        "another address"
    );
    assert_eq!(
        normalize(Side::Plain, Kind::Volatile, "<1@a>"),
        normalize(Side::Key, Kind::Volatile, "<2@b>"),
        "Message-ID"
    );

    // The header reader: a folded top-level header is whole, a header of a
    // MIME part is not one.
    let message = SinkMessage {
        mail_from: KEY.to_string(),
        rcpt_to: vec![ATT.to_string()],
        data: concat!(
            "From: \"a\"\r\n <a@b>\r\nSubject: top\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n",
            "--x\r\nSubject: inner\r\n\r\ntext\r\n--x--\r\n"
        )
        .as_bytes()
        .to_vec(),
    };
    assert_eq!(
        raw_header(&message, "from").as_deref(),
        Some("\"a\"\r\n <a@b>")
    );
    assert_eq!(raw_header(&message, "Subject").as_deref(), Some("top"));
    assert_eq!(raw_header(&message, "Reply-To"), None);
}
