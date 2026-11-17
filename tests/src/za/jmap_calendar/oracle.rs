/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6, the differential oracle (spec 3.5, 3.6, 9.3; review focus 4).
//! One edit script runs step by step on two fresh key accounts: the CalDAV
//! side (`jmapcal-oracle-cd`) performs each step over CalDAV, the JMAP side
//! (`jmapcal-oracle-jm`) over JMAP, each with its own key invitee and its own
//! outside address. Each step's iCalendar text is written once; the JMAP
//! input is upstream's JSCalendar of that text (`jscalendar_of`): a create
//! sends the whole object with `calendarIds`, an update the top-level
//! difference from the previous step's text. Only spec 3.2's inputs differ
//! between the sides: whether to schedule (`sendSchedulingMessages`, absent
//! unless the step is marked S), the resource name and the placement.
//!
//! After each step the sides are compared, the CalDAV side first: the
//! step's `Expect` on each side, then the JSCalendar of the CalDAV GET and of
//! `CalendarEvent/get` (`normalize_jscal`), the new inbox items and the sink
//! mail by recipient, method, UID, SEQUENCE and recurrence ids, each item
//! tree and each message part (`Tally::same`, CalDAV as "plain", JMAP as
//! "key"), each attendee's `SCHEDULE-STATUS` and the stored token pairs'
//! recipients. The names `jmapcal-oracle-cd*` become `jmapcal-oracle-jm*` in
//! the CalDAV side's values only (`map_names`, and `map_jscal` for the
//! participant ids upstream's converter derives from an address). The two
//! names have equal length, so the mapping moves no fold point.
//!
//! The stopping rule (spec 3.6): a step that differs is the oracle's result.
//! No normalization beyond `normalize_jscal`'s is added to make one pass.

use super::{finish, key_account, make_calendar};
use crate::{
    utils::{
        account::Account,
        http::HttpRequest,
        server::TestServer,
        smtp_sink::{SinkMessage, sink},
        webdav::DummyWebDavClient,
        za::{
            copy_href, inbox_ids, inbox_item_is_pending, item_method, item_partstat, item_sequence,
            jscalendar_of, new_inbox_items, normalize_jscal, open_pending_item_as, pin_record,
            schedule_statuses, trigger_drain, wait_for_index_queue, wait_for_queue,
            write_pin_record, za_unlock,
        },
    },
    za::{
        STRONG,
        drain::{DrainOn, body, set_partstat},
        fanout::{NO_REPLY, Party, attendee_statuses, stored_pairs, uid_copies},
        imip::{
            deliver, itip_mail,
            oracle::{Kind, Tally, mail_key, mail_parts},
        },
        pinning::withheld,
        uid_lease::put,
        user_permissions,
    },
};
use calcard::{
    common::IanaString,
    icalendar::{ICalendar, ICalendarProperty, ICalendarValue},
};
use hyper::{Method, StatusCode};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};
use store::write::now;
use types::{collection::Collection, field::CalendarEventField, id::Id};
use vault::pins::PinSource;

/// The CalDAV side's name prefix, and the JMAP side's (equal length).
const CD: &str = "jmapcal-oracle-cd";
const JM: &str = "jmapcal-oracle-jm";
/// The ordinary account of `round_trip_keeps_what_an_ordinary_account_keeps`.
const PLAIN: &str = "jmapcal-oracle-plain@example.com";
const PLAIN_NAME: &str = "jmapcal-oracle-plain";

const QUIET_UID: &str = "jmapcal-oracle-quiet";
const MAIN_UID: &str = "jmapcal-oracle-main";
const IN_UID: &str = "jmapcal-oracle-in";
const RT_UID: &str = "jmapcal-oracle-rt";
const RT2_UID: &str = "jmapcal-oracle-rt2";
/// The second calendar of each side.
const SECOND: &str = "oracle-b";
/// The DTSTAMP of every script text: an update never changes it.
const STAMP: &str = "20260101T000000Z";
/// The key `round_trip`'s Part B pins for the withheld attendee in place of
/// its served one: inert, synthetic, never served by any account.
const OTHER_KEY: [u8; 32] = [0x5a; 32];

/// The CalDAV side's names as the JMAP side's (`jmapcal-oracle-cd*` to
/// `jmapcal-oracle-jm*`), for the CalDAV side's values only.
fn map_names(text: &str) -> String {
    text.replace(CD, JM)
}

/// `map_names` on a JSCalendar object: every string and key, and every
/// participant id upstream's converter derived from a mapped address
/// (`ICalendarEntry::participant_id`: `uuid5` of the address when the
/// property carries no `JSID`), which becomes the id of the mapped address.
fn map_jscal(value: Value) -> Value {
    fn names(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(text) if text.contains(CD) => out.push(text.clone()),
            Value::Object(map) => {
                for (key, value) in map {
                    if key.contains(CD) {
                        out.push(key.clone());
                    }
                    names(value, out);
                }
            }
            Value::Array(values) => values.iter().for_each(|value| names(value, out)),
            _ => {}
        }
    }
    fn remap(value: Value, ids: &[(String, String)]) -> Value {
        let text = |text: String| {
            ids.iter()
                .find(|(from, _)| *from == text)
                .map(|(_, to)| to.clone())
                .unwrap_or_else(|| map_names(&text))
        };
        match value {
            Value::String(value) => Value::String(text(value)),
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(key, value)| (text(key), remap(value, ids)))
                    .collect(),
            ),
            Value::Array(values) => {
                Value::Array(values.into_iter().map(|value| remap(value, ids)).collect())
            }
            value => value,
        }
    }
    let mut found = Vec::new();
    names(&value, &mut found);
    let mut ids = Vec::new();
    for name in found {
        let mut forms = vec![name.clone()];
        if let Some(address) = name.strip_prefix("mailto:") {
            forms.push(address.to_string());
        }
        for form in forms {
            ids.push((
                calcard::jscalendar::uuid5(&form),
                calcard::jscalendar::uuid5(map_names(&form)),
            ));
        }
    }
    remap(value, &ids)
}

/// `value` as pretty JSON with every object's keys sorted, so equal objects
/// print equal whatever order a side produced.
fn canonical(value: &Value) -> String {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut entries: Vec<(&String, &Value)> = map.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                Value::Object(
                    entries
                        .into_iter()
                        .map(|(key, value)| (key.clone(), sorted(value)))
                        .collect(),
                )
            }
            Value::Array(values) => Value::Array(values.iter().map(sorted).collect()),
            value => value.clone(),
        }
    }
    serde_json::to_string_pretty(&sorted(value)).unwrap()
}

/// Compares a CalDAV-side and a JMAP-side iCalendar text as their
/// projections, `normalize_jscal(jscalendar_of(text))`, the CalDAV one after
/// the mapping (the brief's Step 4, items 2, 4 and 5, as ruled for Task 38:
/// by D1 the JMAP side's tree is upstream's export, and by D17 it carries
/// JMAP's stamps, so trees are not compared as text). A failure prints both
/// projections under the step's label, and the comparison counts in the
/// step's tally.
fn same_jscal(tally: &mut Tally<'_>, what: impl std::fmt::Display, caldav: &str, jmap: &str) {
    let caldav = canonical(&normalize_jscal(map_jscal(jscalendar_of(caldav))));
    let jmap = canonical(&normalize_jscal(jscalendar_of(jmap)));
    tally.same(
        format!("{what} (JSCalendar projection)"),
        Kind::Body,
        Some(&caldav),
        Some(&jmap),
    );
}

/// The top-level difference of `next` from `previous`: each added or changed
/// property with its new value, each removed one as `null`.
fn top_level_diff(previous: &Value, next: &Value) -> Value {
    let (previous, next) = (
        previous.as_object().expect("an object"),
        next.as_object().expect("an object"),
    );
    let mut patch = serde_json::Map::new();
    for (key, value) in next {
        if previous.get(key) != Some(value) {
            patch.insert(key.clone(), value.clone());
        }
    }
    for key in previous.keys() {
        if !next.contains_key(key) {
            patch.insert(key.clone(), Value::Null);
        }
    }
    Value::Object(patch)
}

/// A side's three addresses.
struct Names {
    account: String,
    invitee: String,
    outside: String,
}

impl Names {
    fn of(prefix: &str) -> Self {
        Names {
            account: format!("{prefix}@example.com"),
            invitee: format!("{prefix}-inv@example.com"),
            outside: format!("{prefix}-out@remote.test"),
        }
    }

    fn address(&self, who: Who) -> &str {
        match who {
            Who::Account => &self.account,
            Who::Invitee => &self.invitee,
            Who::Outside => &self.outside,
        }
    }
}

/// A party of the exchanges.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Who {
    Account,
    Invitee,
    Outside,
}

/// An expected `SCHEDULE-STATUS`.
#[derive(Clone, Copy, Debug)]
enum Status {
    /// Every value is this one.
    Is(&'static str),
    /// Any value, not empty.
    Any,
}

/// What the script says a step produces, asserted on each side before the
/// sides are compared, so two sides that both did nothing cannot pass. An
/// unset field is not asserted.
#[derive(Clone, Copy, Default)]
struct Expect {
    /// Whether the account holds the event.
    present: Option<bool>,
    /// The calendar that holds it.
    collection: Option<&'static str>,
    /// A calendar that does not.
    absent_from: Option<&'static str>,
    /// Substrings of the unfolded CalDAV GET of the account's copy.
    contains: &'static [&'static str],
    /// One party's PARTSTAT in the account's copy.
    partstat: Option<(Who, &'static str)>,
    /// The new inbox items as (whose inbox, METHOD), account first, each
    /// inbox by document id.
    items: Option<&'static [(Who, &'static str)]>,
    /// The new items in the account's inbox are drained.
    drained: bool,
    /// The sink messages as (recipient, METHOD).
    sink: Option<&'static [(Who, &'static str)]>,
    /// Attendees' SCHEDULE-STATUS in the organizer's copy.
    statuses: &'static [(Who, Status)],
    /// The recipients of the organizer's stored token pairs.
    pairs: Option<&'static [Who]>,
}

/// The script's three exchanges.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Exchange {
    /// Spec 3.2's one input that is off on both protocols.
    Quiet,
    /// The account organizes.
    Main,
    /// The invitee organizes over CalDAV on both sides; the account attends.
    Inbound,
}

impl Exchange {
    fn uid(self) -> &'static str {
        match self {
            Exchange::Quiet => QUIET_UID,
            Exchange::Main => MAIN_UID,
            Exchange::Inbound => IN_UID,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Exchange::Quiet => "quiet",
            Exchange::Main => "main",
            Exchange::Inbound => "inbound",
        }
    }

    fn code(self) -> char {
        match self {
            Exchange::Quiet => 'Q',
            Exchange::Main => 'M',
            Exchange::Inbound => 'I',
        }
    }
}

/// One side of the oracle.
struct Side {
    /// Whether this side writes over JMAP.
    jmap: bool,
    names: Names,
    user: Account,
    account: Party,
    invitee: Party,
    /// The JMAP ids of `default` and `oracle-b`.
    default_id: String,
    second_id: String,
}

impl Side {
    fn tag(&self) -> &'static str {
        if self.jmap { "JMAP" } else { "CalDAV" }
    }

    fn party(&self, who: Who) -> &Party {
        match who {
            Who::Account => &self.account,
            Who::Invitee => &self.invitee,
            Who::Outside => unreachable!("the outside address is no account"),
        }
    }

    /// A value read from this side, as the comparison sees it.
    fn mapped(&self, text: &str) -> String {
        if self.jmap {
            text.to_string()
        } else {
            map_names(text)
        }
    }

    /// The party a sink address names.
    fn who_is(&self, address: &str) -> Who {
        [Who::Account, Who::Invitee, Who::Outside]
            .into_iter()
            .find(|who| self.names.address(*who).eq_ignore_ascii_case(address))
            .unwrap_or_else(|| panic!("{address} is no party of the {} side", self.tag()))
    }

    /// The organizer of `exchange`, and the attendees whose statuses are read.
    fn organizer(&self, exchange: Exchange) -> &Party {
        match exchange {
            Exchange::Inbound => &self.invitee,
            Exchange::Quiet | Exchange::Main => &self.account,
        }
    }

    fn attendees(&self, exchange: Exchange) -> &'static [Who] {
        match exchange {
            Exchange::Inbound => &[Who::Account],
            Exchange::Quiet | Exchange::Main => &[Who::Invitee, Who::Outside],
        }
    }
}

fn vcalendar(components: &[Vec<String>]) -> String {
    let mut ics = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//oracle//EN\r\n");
    for lines in components {
        ics.push_str("BEGIN:VEVENT\r\n");
        for line in lines {
            ics.push_str(line);
            ics.push_str("\r\n");
        }
        ics.push_str("END:VEVENT\r\n");
    }
    ics.push_str("END:VCALENDAR\r\n");
    ics
}

/// The quiet exchange's event.
fn quiet_text(names: &Names) -> String {
    vcalendar(&[vec![
        format!("UID:{QUIET_UID}"),
        format!("DTSTAMP:{STAMP}"),
        "SEQUENCE:0".into(),
        "DTSTART:20990305T100000Z".into(),
        "DTEND:20990305T110000Z".into(),
        "SUMMARY:oracle quiet".into(),
        format!("ORGANIZER:mailto:{}", names.account),
        format!(
            "ATTENDEE;RSVP=TRUE;PARTSTAT=NEEDS-ACTION:mailto:{}",
            names.invitee
        ),
        format!(
            "ATTENDEE;RSVP=TRUE;PARTSTAT=NEEDS-ACTION:mailto:{}",
            names.outside
        ),
    ]])
}

/// The main exchange's event after step `n`. Steps 8 and 9 are not written
/// by the account: their texts are the event as the replies leave it, so
/// step 10's text and difference start from it. Every VEVENT carries the
/// organizer's SEQUENCE: `0` at the create, one higher at each organizer
/// step that changes the text (M2, M4, M5, M6, M7, M10); the replies (M8,
/// M9) keep it (Task 38's M2 ruling: the text an organizer's client writes).
fn main_text(n: u32, names: &Names) -> String {
    let sequence = [2, 4, 5, 6, 7, 10]
        .into_iter()
        .filter(|step| *step <= n)
        .count();
    let invitee = if n >= 8 { "ACCEPTED" } else { "NEEDS-ACTION" };
    let outside = if n >= 9 { "ACCEPTED" } else { "NEEDS-ACTION" };
    let scheduling = || {
        vec![
            format!("ORGANIZER:mailto:{}", names.account),
            format!(
                "ATTENDEE;RSVP=TRUE;PARTSTAT={invitee}:mailto:{}",
                names.invitee
            ),
            format!(
                "ATTENDEE;RSVP=TRUE;PARTSTAT={outside}:mailto:{}",
                names.outside
            ),
        ]
    };
    let mut master = vec![
        format!("UID:{MAIN_UID}"),
        format!("DTSTAMP:{STAMP}"),
        format!("SEQUENCE:{sequence}"),
    ];
    if n == 1 {
        master.extend([
            "DTSTART:20990301T100000Z".to_string(),
            "DTEND:20990301T110000Z".to_string(),
            "SUMMARY:oracle one".to_string(),
        ]);
    } else {
        master.extend([
            "DTSTART:20990302T100000Z".to_string(),
            "DTEND:20990302T110000Z".to_string(),
            "SUMMARY:oracle two".to_string(),
        ]);
    }
    if n >= 4 {
        master.push("RRULE:FREQ=WEEKLY;COUNT=4".to_string());
    }
    if n >= 6 {
        master.extend(scheduling());
    }
    if n >= 7 {
        master.push("LOCATION:room seven".to_string());
    }
    if n >= 10 {
        master.push("EXDATE:20990316T100000Z".to_string());
    }
    let mut components = vec![master];
    if n >= 5 {
        let mut instance = vec![
            format!("UID:{MAIN_UID}"),
            format!("DTSTAMP:{STAMP}"),
            format!("SEQUENCE:{sequence}"),
            "RECURRENCE-ID:20990309T100000Z".to_string(),
            "DTSTART:20990309T120000Z".to_string(),
            "DTEND:20990309T130000Z".to_string(),
            "SUMMARY:oracle two moved".to_string(),
        ];
        if n >= 6 {
            instance.extend(scheduling());
        }
        components.push(instance);
    }
    vcalendar(&components)
}

/// The outside address's REPLY to the main event at `sequence`, accepting
/// the series and its moved instance.
fn outside_reply(names: &Names, sequence: i64) -> String {
    let lines = |instance: &[&str]| {
        let mut lines = vec![
            format!("UID:{MAIN_UID}"),
            format!("DTSTAMP:{STAMP}"),
            format!("SEQUENCE:{sequence}"),
        ];
        lines.extend(instance.iter().map(|line| line.to_string()));
        lines.extend([
            format!("ORGANIZER:mailto:{}", names.account),
            format!(
                "ATTENDEE;RSVP=TRUE;PARTSTAT=ACCEPTED:mailto:{}",
                names.outside
            ),
        ]);
        lines
    };
    vcalendar(&[
        lines(&["DTSTART:20990302T100000Z"]),
        lines(&["RECURRENCE-ID:20990309T100000Z", "DTSTART:20990309T120000Z"]),
    ])
    .replacen(
        "BEGIN:VCALENDAR\r\n",
        "BEGIN:VCALENDAR\r\nMETHOD:REPLY\r\n",
        1,
    )
}

/// The inbound exchange's event, organized by the invitee; the account's
/// PARTSTAT is `ACCEPTED` once it accepted.
fn inbound_text(names: &Names, accepted: bool) -> String {
    vcalendar(&[vec![
        format!("UID:{IN_UID}"),
        format!("DTSTAMP:{STAMP}"),
        "SEQUENCE:0".into(),
        "DTSTART:20990320T100000Z".into(),
        "DTEND:20990320T110000Z".into(),
        "SUMMARY:oracle inbound".into(),
        format!("ORGANIZER:mailto:{}", names.invitee),
        format!(
            "ATTENDEE;RSVP=TRUE;PARTSTAT={}:mailto:{}",
            if accepted { "ACCEPTED" } else { "NEEDS-ACTION" },
            names.account
        ),
    ]])
}

/// The script, in order, with each step's `Expect`.
fn script() -> Vec<(Exchange, u32, Expect)> {
    use Exchange::*;
    use Who::*;
    vec![
        (
            Quiet,
            1,
            Expect {
                present: Some(true),
                items: Some(&[(Invitee, "REQUEST")]),
                sink: Some(&[(Outside, "REQUEST")]),
                ..Default::default()
            },
        ),
        (
            Quiet,
            2,
            Expect {
                present: Some(false),
                items: Some(&[]),
                sink: Some(&[]),
                ..Default::default()
            },
        ),
        (
            Main,
            1,
            Expect {
                present: Some(true),
                collection: Some("default"),
                contains: &["SUMMARY:oracle one"],
                ..Default::default()
            },
        ),
        (
            Main,
            2,
            Expect {
                contains: &["SUMMARY:oracle two", "DTSTART:20990302T100000Z"],
                ..Default::default()
            },
        ),
        (
            Main,
            3,
            Expect {
                present: Some(true),
                collection: Some(SECOND),
                absent_from: Some("default"),
                ..Default::default()
            },
        ),
        (
            Main,
            4,
            Expect {
                contains: &["RRULE:FREQ=WEEKLY;COUNT=4"],
                ..Default::default()
            },
        ),
        (
            Main,
            5,
            Expect {
                contains: &["SUMMARY:oracle two moved"],
                ..Default::default()
            },
        ),
        (
            Main,
            6,
            Expect {
                items: Some(&[(Invitee, "REQUEST")]),
                sink: Some(&[(Outside, "REQUEST")]),
                statuses: &[(Outside, Status::Is("1.1")), (Invitee, Status::Any)],
                // Every scheduled attendee not local to the sender holds a
                // pair (`token_union`; Task 38's M6 ruling).
                pairs: Some(&[Invitee, Outside]),
                ..Default::default()
            },
        ),
        (
            Main,
            7,
            Expect {
                items: Some(&[(Invitee, "REQUEST")]),
                sink: Some(&[(Outside, "REQUEST")]),
                ..Default::default()
            },
        ),
        (
            Main,
            8,
            Expect {
                items: Some(&[(Account, "REPLY")]),
                drained: true,
                partstat: Some((Invitee, "ACCEPTED")),
                ..Default::default()
            },
        ),
        (
            Main,
            9,
            Expect {
                partstat: Some((Outside, "ACCEPTED")),
                ..Default::default()
            },
        ),
        (
            Main,
            10,
            Expect {
                items: Some(&[(Invitee, "CANCEL")]),
                sink: Some(&[(Outside, "CANCEL")]),
                ..Default::default()
            },
        ),
        (
            Main,
            11,
            Expect {
                present: Some(false),
                items: Some(&[(Invitee, "CANCEL")]),
                sink: Some(&[(Outside, "CANCEL")]),
                ..Default::default()
            },
        ),
        (
            Inbound,
            1,
            Expect {
                present: Some(true),
                items: Some(&[(Account, "REQUEST")]),
                drained: true,
                partstat: Some((Account, "NEEDS-ACTION")),
                ..Default::default()
            },
        ),
        (
            Inbound,
            2,
            Expect {
                items: Some(&[(Invitee, "REPLY")]),
                partstat: Some((Account, "ACCEPTED")),
                ..Default::default()
            },
        ),
        (
            Inbound,
            3,
            Expect {
                present: Some(false),
                items: Some(&[(Invitee, "REPLY")]),
                ..Default::default()
            },
        ),
    ]
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access JMAP calendar oracle tests...");
    self_check();
    sink().clear();
    for local in ["oracle-cd", "oracle-jm", "oracle-cd-inv", "oracle-jm-inv"] {
        key_account(test, local, user_permissions()).await;
    }
    let test: &TestServer = test;
    let cd = side(test, false, CD).await;
    let jm = side(test, true, JM).await;
    let parties = [&cd.account, &cd.invitee, &jm.account, &jm.invitee];
    // Spec 9.3's precondition: both runs start from equal accounts with
    // equal pins, none.
    for party in parties {
        assert!(
            pin_record(test, party.id, STRONG).await.is_none(),
            "{} starts with a pin record",
            party.name
        );
    }
    let starts = [
        inbox_ids(test, cd.account.id).await,
        inbox_ids(test, cd.invitee.id).await,
        inbox_ids(test, jm.account.id).await,
        inbox_ids(test, jm.invitee.id).await,
    ];
    let drain = DrainOn::new();

    for (exchange, n, expect) in script() {
        run_step(test, &cd, &jm, exchange, n, &expect).await;
    }
    round_trip_keeps_what_an_ordinary_account_keeps(test, &cd, &jm).await;

    // Cleanup, with the drain off again, so a cleanup request imports
    // nothing: every copy of the script's events (the invitees' imported
    // ones and the inbound organizer's), the inbox items, the delivered
    // REPLY mail and `oracle-b`.
    drop(drain);
    wait_for_queue(test).await;
    for party in parties {
        for uid in [QUIET_UID, MAIN_UID, IN_UID, RT_UID, RT2_UID] {
            if uid_copies(test, party.id, uid).await > 0 {
                let href = copy_href(test, party.id, uid).await;
                party
                    .client
                    .request_with_headers("DELETE", &href, [NO_REPLY], "")
                    .await
                    .with_status(StatusCode::NO_CONTENT);
            }
            assert_eq!(
                uid_copies(test, party.id, uid).await,
                0,
                "{uid} outlived the module in {}",
                party.name
            );
        }
    }
    for (party, start) in parties.into_iter().zip(&starts) {
        party.delete_items_since(test, start).await;
    }
    for side in [&cd, &jm] {
        delete_delivered_mail(side).await;
        side.account
            .client
            .request(
                "DELETE",
                &format!("{}{SECOND}/", home(side.account.name)),
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    sink().clear();
    finish(test, "oracle").await;
}

/// A side's four parties' accounts are made; this builds the side, its
/// second calendar (setup, not a compared step) and its calendar ids.
async fn side(test: &TestServer, jmap: bool, prefix: &str) -> Side {
    let names = Names::of(prefix);
    let account = Party::new(test, names.account.clone().leak());
    let invitee = Party::new(test, names.invitee.clone().leak());
    let user = test.account(account.name).clone();
    let second = make_calendar(test, &user, &account.client, SECOND).await;
    let second_id = Id::from(second).to_string();
    let reply = user
        .jmap_method_call(
            "Calendar/get",
            json!({ "accountId": user.id_string(), "ids": null }),
        )
        .await;
    let ids: Vec<(String, bool)> = reply
        .list()
        .iter()
        .filter_map(|calendar| {
            calendar["id"]
                .as_str()
                .map(|id| (id.to_string(), calendar["isDefault"] == true))
        })
        .collect();
    assert!(
        ids.iter().any(|(id, _)| *id == second_id),
        "{prefix}: Calendar/get does not list {SECOND}: {reply:?}"
    );
    let default_id = ids
        .iter()
        .find(|(id, is_default)| *is_default && *id != second_id)
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| panic!("{prefix}: no default calendar: {reply:?}"));
    Side {
        jmap,
        names,
        user,
        account,
        invitee,
        default_id,
        second_id,
    }
}

fn home(name: &str) -> String {
    format!("/dav/cal/{}/", name.replace('@', "%40"))
}

/// `<calendar>/<name>` of an event's href, for `stored_pairs`.
fn relative_path(href: &str) -> String {
    let mut segments = href.trim_end_matches('/').rsplit('/');
    let name = segments.next().unwrap();
    let calendar = segments.next().unwrap();
    format!("{calendar}/{name}")
}

/// The JMAP id of the account's one event with `uid`.
async fn event_id(test: &TestServer, account_id: u32, uid: &str) -> String {
    let ids = test
        .server
        .document_ids_matching(
            account_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .unwrap();
    assert_eq!(ids.len(), 1, "{uid}: {} copies", ids.len());
    Id::from(ids.min().unwrap()).to_string()
}

fn send_args(send: bool) -> Vec<(&'static str, Value)> {
    if send {
        vec![("sendSchedulingMessages", Value::Bool(true))]
    } else {
        Vec::new()
    }
}

async fn jmap_create(side: &Side, ics: &str, send: bool) {
    let mut object = jscalendar_of(ics);
    object["calendarIds"] = json!({ side.default_id.as_str(): true });
    let reply = side
        .user
        .jmap_create("CalendarEvent", [object], send_args(send))
        .await;
    reply.created_id(0);
}

async fn jmap_update(test: &TestServer, side: &Side, uid: &str, patch: Value, send: bool) {
    let id = event_id(test, side.account.id, uid).await;
    let reply = side
        .user
        .jmap_update("CalendarEvent", [(id.clone(), patch)], send_args(send))
        .await;
    reply.updated(&id);
}

async fn jmap_destroy(test: &TestServer, side: &Side, uid: &str, args: Vec<(&str, Value)>) {
    let id = event_id(test, side.account.id, uid).await;
    let reply = side.user.jmap_destroy("CalendarEvent", [&id], args).await;
    assert_eq!(
        reply.destroyed().collect::<Vec<_>>(),
        [id.as_str()],
        "{reply:?}"
    );
}

/// A CalDAV PUT through the raw client (5 s): a write that mails can outlast
/// `DummyWebDavClient`'s 500 ms.
async fn dav_put(name: &str, href: String, ics: String, status: u16) {
    assert_eq!(
        put(name, STRONG, href.clone(), ics).await,
        status,
        "PUT {href}"
    );
}

/// A CalDAV DELETE with scheduling, through the raw client.
async fn dav_delete(name: &str, href: &str) {
    let response = HttpRequest::with_credentials(8899, name, STRONG)
        .send_full(Method::DELETE, href, None, None)
        .await;
    assert_eq!(response.status, StatusCode::NO_CONTENT, "DELETE {href}");
}

/// The update of the main event from step `n - 1`'s text to step `n`'s.
async fn main_update(test: &TestServer, side: &Side, n: u32, send: bool) {
    let text = main_text(n, &side.names);
    if side.jmap {
        let patch = top_level_diff(
            &jscalendar_of(&main_text(n - 1, &side.names)),
            &jscalendar_of(&text),
        );
        jmap_update(test, side, MAIN_UID, patch, send).await;
    } else {
        let href = copy_href(test, side.account.id, MAIN_UID).await;
        dav_put(side.account.name, href, text, 204).await;
    }
}

/// One step on one side, without its drains.
async fn perform(test: &TestServer, side: &Side, exchange: Exchange, n: u32) {
    let account = side.account.name;
    match (exchange, n) {
        // Q1: the invitation, S.
        (Exchange::Quiet, 1) => {
            let text = quiet_text(&side.names);
            if side.jmap {
                jmap_create(side, &text, true).await;
            } else {
                let href = format!("{}default/{QUIET_UID}.ics", home(account));
                dav_put(account, href, text, 201).await;
            }
        }
        // Q2: the quiet delete.
        (Exchange::Quiet, 2) => {
            if side.jmap {
                jmap_destroy(
                    test,
                    side,
                    QUIET_UID,
                    vec![("sendSchedulingMessages", Value::Bool(false))],
                )
                .await;
            } else {
                let href = copy_href(test, side.account.id, QUIET_UID).await;
                side.account
                    .client
                    .request_with_headers("DELETE", &href, [NO_REPLY], "")
                    .await
                    .with_status(StatusCode::NO_CONTENT);
            }
        }
        // M1: the create in `default`.
        (Exchange::Main, 1) => {
            let text = main_text(1, &side.names);
            if side.jmap {
                jmap_create(side, &text, false).await;
            } else {
                let href = format!("{}default/{MAIN_UID}.ics", home(account));
                dav_put(account, href, text, 201).await;
            }
        }
        // M2, M4, M5: updates; M6, M7, M10: updates, S.
        (Exchange::Main, 2 | 4 | 5) => main_update(test, side, n, false).await,
        (Exchange::Main, 6 | 7 | 10) => main_update(test, side, n, true).await,
        // M3: the placement.
        (Exchange::Main, 3) => {
            if side.jmap {
                let patch = json!({ "calendarIds": { side.second_id.as_str(): true } });
                jmap_update(test, side, MAIN_UID, patch, false).await;
            } else {
                let href = copy_href(test, side.account.id, MAIN_UID).await;
                let destination = format!("{}{SECOND}/{MAIN_UID}.ics", home(account));
                side.account
                    .client
                    .request_with_headers(
                        "MOVE",
                        &href,
                        [("destination", destination.as_str())],
                        "",
                    )
                    .await
                    .with_status(StatusCode::CREATED);
            }
        }
        // M8: the invitee accepts in its own copy over CalDAV, on both sides
        // (its request drains its inbox first, which imports the invitation).
        (Exchange::Main, 8) => {
            let invitee = &side.invitee;
            assert_eq!(trigger_drain(invitee.name, STRONG).await, 207);
            let href = copy_href(test, invitee.id, MAIN_UID).await;
            set_partstat(&invitee.client, &href, invitee.name, "ACCEPTED").await;
        }
        // M9: the outside address's emailed REPLY, over LMTP.
        (Exchange::Main, 9) => {
            let href = copy_href(test, side.account.id, MAIN_UID).await;
            let stored = body(&side.account.client, &href).await;
            let sequence = ICalendar::parse(&stored)
                .unwrap()
                .components
                .iter()
                .flat_map(|component| component.entries.iter())
                .filter(|entry| entry.name == ICalendarProperty::Sequence)
                .filter_map(|entry| match entry.values.first() {
                    Some(ICalendarValue::Integer(sequence)) => Some(*sequence),
                    _ => None,
                })
                .max()
                .unwrap_or_default();
            let outside = side.names.outside.as_str();
            deliver(
                &format!("dmarc-{outside}"),
                account,
                &itip_mail(
                    outside,
                    account,
                    "REPLY",
                    &outside_reply(&side.names, sequence),
                ),
            )
            .await;
        }
        // M11: the delete, S.
        (Exchange::Main, 11) => {
            if side.jmap {
                jmap_destroy(test, side, MAIN_UID, send_args(true)).await;
            } else {
                let href = copy_href(test, side.account.id, MAIN_UID).await;
                dav_delete(account, &href).await;
            }
        }
        // I1: the invitee's invitation, over CalDAV on both sides.
        (Exchange::Inbound, 1) => {
            let invitee = side.invitee.name;
            let href = format!("{}default/{IN_UID}.ics", home(invitee));
            dav_put(invitee, href, inbound_text(&side.names, false), 201).await;
        }
        // I2: the account accepts, S.
        (Exchange::Inbound, 2) => {
            let text = inbound_text(&side.names, true);
            if side.jmap {
                let patch = top_level_diff(
                    &jscalendar_of(&inbound_text(&side.names, false)),
                    &jscalendar_of(&text),
                );
                jmap_update(test, side, IN_UID, patch, true).await;
            } else {
                let href = copy_href(test, side.account.id, IN_UID).await;
                dav_put(account, href, text, 204).await;
            }
        }
        // I3: the account deletes its copy, S.
        (Exchange::Inbound, 3) => {
            if side.jmap {
                jmap_destroy(test, side, IN_UID, send_args(true)).await;
            } else {
                let href = copy_href(test, side.account.id, IN_UID).await;
                dav_delete(account, &href).await;
            }
        }
        _ => unreachable!(),
    }
}

/// The step's drains: M8, M9 and I1 drain the account, over CalDAV on the
/// CalDAV side and by a keyed `Calendar/get` on the JMAP side.
async fn drains(side: &Side, exchange: Exchange, n: u32) {
    if !matches!(
        (exchange, n),
        (Exchange::Main, 8 | 9) | (Exchange::Inbound, 1)
    ) {
        return;
    }
    if side.jmap {
        let reply = side
            .user
            .jmap_method_call(
                "Calendar/get",
                json!({ "accountId": side.user.id_string(), "ids": null }),
            )
            .await;
        assert!(
            reply.pointer("/methodResponses/0/1/list").is_some(),
            "the draining Calendar/get: {reply:?}"
        );
    } else {
        assert_eq!(trigger_drain(side.account.name, STRONG).await, 207);
    }
}

/// A new inbox item, opened as the drain will before any drain of the step
/// ran (a drained item is resealed under the master key).
struct Item {
    who: Who,
    document_id: u32,
    /// The inbox owner's address, mapped.
    recipient: String,
    method: String,
    uid: String,
    sequence: Option<i64>,
    recurrence_ids: Vec<String>,
    tree: String,
}

/// The UID, the first SEQUENCE and the RECURRENCE-ID lines of an iCalendar
/// text, unfolded.
fn ics_fields(text: &str) -> (String, Option<String>, Vec<String>) {
    let unfolded = text
        .replace("\r\n ", "")
        .replace("\r\n\t", "")
        .replace("\n ", "");
    let mut uid = String::new();
    let mut sequence = None;
    let mut recurrence_ids = Vec::new();
    for line in unfolded.lines().map(str::trim_end) {
        if let Some(value) = line.strip_prefix("UID:")
            && uid.is_empty()
        {
            uid = value.to_string();
        } else if let Some(value) = line.strip_prefix("SEQUENCE:")
            && sequence.is_none()
        {
            sequence = Some(value.to_string());
        } else if line.starts_with("RECURRENCE-ID") {
            recurrence_ids.push(line.to_string());
        }
    }
    (uid, sequence, recurrence_ids)
}

async fn new_items(test: &TestServer, side: &Side, before: &[Vec<u32>; 2]) -> Vec<Item> {
    let mut items = Vec::new();
    for (who, before) in [(Who::Account, &before[0]), (Who::Invitee, &before[1])] {
        let party = side.party(who);
        for (document_id, _) in new_inbox_items(test, party.id, before).await {
            let (tree, _) = open_pending_item_as(test, party.id, STRONG, document_id).await;
            let text = tree.to_string();
            let (uid, _, recurrence_ids) = ics_fields(&text);
            items.push(Item {
                who,
                document_id,
                recipient: side.mapped(party.name),
                method: item_method(&tree)
                    .map(|method| method.as_str().to_string())
                    .unwrap_or_default(),
                uid,
                sequence: item_sequence(&tree),
                recurrence_ids: recurrence_ids
                    .into_iter()
                    .map(|line| side.mapped(&line))
                    .collect(),
                tree: text,
            });
        }
    }
    items
}

/// The messages a side's parties sent.
fn sent_by<'x>(side: &Side, messages: &'x [SinkMessage]) -> Vec<&'x SinkMessage> {
    messages
        .iter()
        .filter(|message| {
            message.mail_from.eq_ignore_ascii_case(side.account.name)
                || message.mail_from.eq_ignore_ascii_case(side.invitee.name)
        })
        .collect()
}

/// A sink message's `mail_key`, mapped, with the UID, SEQUENCE and
/// recurrence ids of its iCalendar part.
type MailEntry = (String, String, String, Option<String>, Vec<String>);

fn mail_entry(side: &Side, message: &SinkMessage) -> MailEntry {
    let (rcpt, method) = mail_key(message);
    let (uid, sequence, recurrence_ids) = ics_fields(&message.calendar_part().unwrap_or_default());
    (
        side.mapped(&rcpt),
        method,
        uid,
        sequence,
        recurrence_ids
            .into_iter()
            .map(|line| side.mapped(&line))
            .collect(),
    )
}

/// The recipients of the organizer's stored token pairs, `None` without the
/// organizer's copy.
async fn pair_recipients(
    test: &TestServer,
    side: &Side,
    exchange: Exchange,
    label: &str,
) -> Option<Vec<Who>> {
    let organizer = side.organizer(exchange);
    let uid = exchange.uid();
    if uid_copies(test, organizer.id, uid).await == 0 {
        return None;
    }
    let href = copy_href(test, organizer.id, uid).await;
    let keys = za_unlock(test, organizer.id, STRONG).await;
    let pairs = stored_pairs(test, &keys, organizer.id, &relative_path(&href))
        .await
        .unwrap_or_default();
    let recipients: Vec<Who> = [Who::Account, Who::Invitee, Who::Outside]
        .into_iter()
        .filter(|who| pairs.get(side.names.address(*who)).is_some())
        .collect();
    assert_eq!(
        recipients.len(),
        pairs.len(),
        "{label}: {}: a stored pair names an address outside the exchange",
        side.tag()
    );
    Some(recipients)
}

/// Each attendee's SCHEDULE-STATUS values in the organizer's copy, `None`
/// without it.
async fn statuses(
    test: &TestServer,
    side: &Side,
    exchange: Exchange,
) -> Option<Vec<(Who, Vec<String>)>> {
    let organizer = side.organizer(exchange);
    let uid = exchange.uid();
    if uid_copies(test, organizer.id, uid).await == 0 {
        return None;
    }
    let href = copy_href(test, organizer.id, uid).await;
    let text = body(&organizer.client, &href).await;
    Some(
        side.attendees(exchange)
            .iter()
            .map(|who| {
                (
                    *who,
                    schedule_statuses(&text, ICalendarProperty::Attendee, side.names.address(*who)),
                )
            })
            .collect(),
    )
}

/// The step's `Expect` on one side.
async fn check_expect(
    test: &TestServer,
    side: &Side,
    exchange: Exchange,
    label: &str,
    items: &[Item],
    sent: &[&SinkMessage],
    expect: &Expect,
) {
    let tag = side.tag();
    let uid = exchange.uid();
    let present = uid_copies(test, side.account.id, uid).await > 0;
    if let Some(expected) = expect.present {
        assert_eq!(present, expected, "{label}: {tag}: the event's presence");
    }
    let reads_copy = expect.collection.is_some()
        || expect.absent_from.is_some()
        || !expect.contains.is_empty()
        || expect.partstat.is_some();
    if reads_copy {
        assert!(present, "{label}: {tag}: the account holds no event {uid}");
        let href = copy_href(test, side.account.id, uid).await;
        if let Some(calendar) = expect.collection {
            assert!(
                href.contains(&format!("/{calendar}/")),
                "{label}: {tag}: {href} is not in {calendar}"
            );
        }
        if let Some(calendar) = expect.absent_from {
            assert!(
                !href.contains(&format!("/{calendar}/")),
                "{label}: {tag}: {href} is still in {calendar}"
            );
        }
        let text = body(&side.account.client, &href).await;
        let unfolded = text.replace("\r\n ", "");
        for needle in expect.contains {
            assert!(
                unfolded.contains(needle),
                "{label}: {tag}: the event lacks {needle}:\n{text}"
            );
        }
        if let Some((who, partstat)) = expect.partstat {
            let address = side.names.address(who);
            assert_eq!(
                item_partstat(&ICalendar::parse(&text).unwrap(), address).as_deref(),
                Some(partstat),
                "{label}: {tag}: the PARTSTAT of {address}:\n{text}"
            );
        }
    }
    if let Some(expected) = expect.items {
        let got: Vec<(Who, &str)> = items
            .iter()
            .map(|item| (item.who, item.method.as_str()))
            .collect();
        assert_eq!(got, expected, "{label}: {tag}: the new inbox items");
    }
    if expect.drained {
        for item in items.iter().filter(|item| item.who == Who::Account) {
            assert!(
                !inbox_item_is_pending(test, side.account.id, item.document_id).await,
                "{label}: {tag}: item {} is still pending",
                item.document_id
            );
        }
    }
    if let Some(expected) = expect.sink {
        let mut got: Vec<(Who, String)> = sent
            .iter()
            .flat_map(|message| {
                let method = message.calendar_method().unwrap_or_default();
                message
                    .rcpt_to
                    .iter()
                    .map(move |rcpt| (side.who_is(rcpt), method.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        got.sort();
        let mut expected: Vec<(Who, String)> = expected
            .iter()
            .map(|(who, method)| (*who, method.to_string()))
            .collect();
        expected.sort();
        assert_eq!(got, expected, "{label}: {tag}: the sink messages");
    }
    if !expect.statuses.is_empty() {
        let read = statuses(test, side, exchange)
            .await
            .unwrap_or_else(|| panic!("{label}: {tag}: no organizer's copy"));
        for (who, status) in expect.statuses {
            let values = read
                .iter()
                .find(|(attendee, _)| attendee == who)
                .map(|(_, values)| values.clone())
                .unwrap_or_default();
            let holds = match status {
                Status::Is(value) => !values.is_empty() && values.iter().all(|v| v == value),
                Status::Any => !values.is_empty(),
            };
            assert!(
                holds,
                "{label}: {tag}: the SCHEDULE-STATUS of {who:?} is {values:?}, expected {status:?}"
            );
        }
    }
    if let Some(expected) = expect.pairs {
        let recipients = pair_recipients(test, side, exchange, label)
            .await
            .unwrap_or_else(|| panic!("{label}: {tag}: no organizer's copy"));
        assert_eq!(
            recipients, expected,
            "{label}: {tag}: the stored pairs' recipients"
        );
    }
}

/// One step on both sides, then the comparison (spec 3.5).
async fn run_step(
    test: &TestServer,
    cd: &Side,
    jm: &Side,
    exchange: Exchange,
    n: u32,
    expect: &Expect,
) {
    let label = format!(
        "{} step {}{n} (first CalDAV, second JMAP)",
        exchange.name(),
        exchange.code()
    );
    sink().clear();
    let mut before = Vec::new();
    for side in [cd, jm] {
        before.push([
            inbox_ids(test, side.account.id).await,
            inbox_ids(test, side.invitee.id).await,
        ]);
    }
    for side in [cd, jm] {
        perform(test, side, exchange, n).await;
    }
    wait_for_queue(test).await;
    let items = [
        new_items(test, cd, &before[0]).await,
        new_items(test, jm, &before[1]).await,
    ];
    for side in [cd, jm] {
        drains(side, exchange, n).await;
    }
    wait_for_queue(test).await;
    compare(test, cd, jm, exchange, &label, &items, expect).await;
}

async fn compare(
    test: &TestServer,
    cd: &Side,
    jm: &Side,
    exchange: Exchange,
    label: &str,
    items: &[Vec<Item>; 2],
    expect: &Expect,
) {
    let uid = exchange.uid();
    let mut tally = Tally {
        label,
        compared: 0,
        absorbed: Vec::new(),
    };
    let messages = sink().messages();
    let sent = [sent_by(cd, &messages), sent_by(jm, &messages)];
    let strays: Vec<&str> = messages
        .iter()
        .filter(|message| {
            !sent
                .iter()
                .any(|sent| sent.iter().any(|kept| std::ptr::eq(*kept, *message)))
        })
        .map(|message| message.mail_from.as_str())
        .collect();
    assert!(
        strays.is_empty(),
        "{label}: stray sink messages from {strays:?}"
    );

    // 1. The step's Expect, on each side.
    for (index, side) in [cd, jm].into_iter().enumerate() {
        check_expect(
            test,
            side,
            exchange,
            label,
            &items[index],
            &sent[index],
            expect,
        )
        .await;
    }
    let present = [
        uid_copies(test, cd.account.id, uid).await > 0,
        uid_copies(test, jm.account.id, uid).await > 0,
    ];
    assert_eq!(present[0], present[1], "{label}: the event on both sides");

    if present[1] {
        // 2. The JSCalendar of the CalDAV GET.
        let mut gets = Vec::new();
        for side in [cd, jm] {
            let href = copy_href(test, side.account.id, uid).await;
            gets.push(body(&side.account.client, &href).await);
        }
        same_jscal(
            &mut tally,
            "the JSCalendar of the CalDAV GET",
            &gets[0],
            &gets[1],
        );

        // 3. CalendarEvent/get.
        let mut objects = Vec::new();
        for side in [cd, jm] {
            let id = event_id(test, side.account.id, uid).await;
            let reply = side
                .user
                .jmap_method_call(
                    "CalendarEvent/get",
                    json!({ "accountId": side.user.id_string(), "ids": [id] }),
                )
                .await;
            let [object] = reply.list() else {
                panic!("{label}: {}: CalendarEvent/get: {reply:?}", side.tag());
            };
            let object = if side.jmap {
                object.clone()
            } else {
                map_jscal(object.clone())
            };
            objects.push(canonical(&normalize_jscal(object)));
        }
        tally.same(
            "CalendarEvent/get",
            Kind::Body,
            Some(&objects[0]),
            Some(&objects[1]),
        );
    }

    // 4. The new inbox items, then each pair of trees.
    let mut sorted: Vec<Vec<&Item>> = items.iter().map(|items| items.iter().collect()).collect();
    for items in &mut sorted {
        items.sort_by(|a, b| {
            (
                &a.recipient,
                &a.method,
                &a.uid,
                a.sequence,
                &a.recurrence_ids,
            )
                .cmp(&(
                    &b.recipient,
                    &b.method,
                    &b.uid,
                    b.sequence,
                    &b.recurrence_ids,
                ))
        });
    }
    let keys: Vec<Vec<_>> = sorted
        .iter()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    (
                        item.recipient.clone(),
                        item.method.clone(),
                        item.uid.clone(),
                        item.sequence,
                        item.recurrence_ids.clone(),
                    )
                })
                .collect()
        })
        .collect();
    assert_eq!(keys[0], keys[1], "{label}: the new inbox items");
    for (index, (c, j)) in sorted[0].iter().zip(&sorted[1]).enumerate() {
        same_jscal(
            &mut tally,
            format!("item tree {index} ({}, {})", j.recipient, j.method),
            &c.tree,
            &j.tree,
        );
    }

    // 5. The sink mail, then each pair part by part.
    let mut mail: Vec<Vec<(MailEntry, &SinkMessage)>> = [cd, jm]
        .into_iter()
        .zip(&sent)
        .map(|(side, sent)| {
            sent.iter()
                .map(|message| (mail_entry(side, message), *message))
                .collect()
        })
        .collect();
    for messages in &mut mail {
        messages.sort_by(|a, b| a.0.cmp(&b.0));
    }
    assert_eq!(
        mail[0].iter().map(|(entry, _)| entry).collect::<Vec<_>>(),
        mail[1].iter().map(|(entry, _)| entry).collect::<Vec<_>>(),
        "{label}: the sink messages"
    );
    for ((_, c), (entry, j)) in mail[0].iter().zip(&mail[1]) {
        let (rcpt, method) = (&entry.0, &entry.1);
        for ((part, kind, c_value), (_, _, j_value)) in mail_parts(c).iter().zip(&mail_parts(j)) {
            let what = format!("{part} of the {method} to {rcpt}");
            // The iCalendar part (`Kind::Ics`, or `Kind::IcsCopy` for a
            // CANCEL's) is compared as an item tree is; every other part
            // through `Tally::same`.
            match (kind, c_value, j_value) {
                (Kind::Ics | Kind::IcsCopy, Some(c_value), Some(j_value)) => {
                    same_jscal(&mut tally, what, c_value, j_value)
                }
                _ => tally.same(
                    what,
                    *kind,
                    c_value.as_deref().map(|value| cd.mapped(value)).as_deref(),
                    j_value.as_deref(),
                ),
            }
        }
    }

    // 6. Each attendee's SCHEDULE-STATUS and the stored pairs' recipients.
    let read = [
        statuses(test, cd, exchange).await,
        statuses(test, jm, exchange).await,
    ];
    assert_eq!(
        read[0].is_some(),
        read[1].is_some(),
        "{label}: the organizer's copy on both sides"
    );
    if let (Some(c), Some(j)) = (&read[0], &read[1]) {
        for ((who, c_values), (_, j_values)) in c.iter().zip(j) {
            assert_eq!(
                c_values, j_values,
                "{label}: the SCHEDULE-STATUS of {who:?}"
            );
            tally.compared += 1;
        }
    }
    let pairs = [
        pair_recipients(test, cd, exchange, label).await,
        pair_recipients(test, jm, exchange, label).await,
    ];
    assert_eq!(pairs[0], pairs[1], "{label}: the stored pairs' recipients");
    tally.compared += 1;

    println!(
        "{label}: the sides are equal ({} values compared; differed before normalizing: {})",
        tally.compared,
        tally.absorbed.join(", ")
    );
}

/// The unfolded lines of the CalDAV GET of `uid` in `party`'s account, less
/// DTSTAMP, CREATED and LAST-MODIFIED, with the ordinary account's name read
/// as the key account's.
async fn lines(test: &TestServer, party: &Party, uid: &str) -> BTreeSet<String> {
    let href = copy_href(test, party.id, uid).await;
    body(&party.client, &href)
        .await
        .replace("\r\n ", "")
        .replace("\r\n\t", "")
        .split("\r\n")
        .filter(|line| {
            !line.is_empty()
                && !["DTSTAMP", "CREATED", "LAST-MODIFIED"]
                    .iter()
                    .any(|name| line.starts_with(name))
        })
        .map(|line| line.replace(PLAIN_NAME, JM))
        .collect()
}

/// Review focus 4: an event a native client annotated, edited in the
/// browser, loses no more than the same steps lose on an ordinary account,
/// and keeps the fork's own state.
async fn round_trip_keeps_what_an_ordinary_account_keeps(test: &TestServer, cd: &Side, jm: &Side) {
    // Part A, what the conversion keeps.
    let admin = test.account("admin@example.com");
    let plain = admin
        .create_user_account(PLAIN, STRONG, PLAIN_NAME, &[], user_permissions())
        .await;
    let plain_id = plain.id().document_id();
    let plain_party = Party {
        id: plain_id,
        name: PLAIN,
        client: DummyWebDavClient::new(plain_id, PLAIN, STRONG, PLAIN),
    };
    let rt = vcalendar(&[vec![
        format!("UID:{RT_UID}"),
        "DTSTART:20990401T100000Z".into(),
        "DTEND:20990401T110000Z".into(),
        "SUMMARY:round trip".into(),
        "X-JMAPCAL-KEEP:native value".into(),
        "BEGIN:VALARM".into(),
        "ACTION:DISPLAY".into(),
        "TRIGGER:-PT10M".into(),
        "DESCRIPTION:alarm text".into(),
        "X-JMAPCAL-ALARM:native alarm value".into(),
        "END:VALARM".into(),
    ]]);
    for (party, user) in [(&jm.account, &jm.user), (&plain_party, &plain)] {
        let href = format!("{}default/{RT_UID}.ics", home(party.name));
        dav_put(party.name, href, rt.clone(), 201).await;
        let id = event_id(test, party.id, RT_UID).await;
        let reply = user
            .jmap_update(
                "CalendarEvent",
                [(id.clone(), json!({ "title": "round trip edited" }))],
                send_args(false),
            )
            .await;
        reply.updated(&id);
    }
    let key_lines = lines(test, &jm.account, RT_UID).await;
    let plain_lines = lines(test, &plain_party, RT_UID).await;
    for (what, lines) in [("key", &key_lines), ("plain", &plain_lines)] {
        assert!(
            lines.contains("SUMMARY:round trip edited"),
            "round_trip: the {what} account's event: {lines:?}"
        );
    }
    let survived = |prefix: &str| key_lines.iter().any(|line| line.starts_with(prefix));
    println!(
        "round_trip: on the key account X-JMAPCAL-KEEP {}, DESCRIPTION:alarm text {}, X-JMAPCAL-ALARM {}",
        if survived("X-JMAPCAL-KEEP") {
            "survived"
        } else {
            "was dropped"
        },
        if key_lines.contains("DESCRIPTION:alarm text") {
            "survived"
        } else {
            "was dropped"
        },
        if survived("X-JMAPCAL-ALARM") {
            "survived"
        } else {
            "was dropped"
        },
    );
    assert_eq!(
        key_lines, plain_lines,
        "round_trip: the key account's lines (left) and the ordinary account's (right)"
    );

    // Part B, the fork's own state, on the JMAP side's account alone. The
    // CalDAV side's invitee is refused: its pin in the organizer's pin
    // record is replaced by another key (as `pinning::case_02_refusal`
    // arranges a refusal, there by substituting the served key).
    let organizer = &jm.account;
    let withheld_address = cd.names.invitee.as_str();
    let mut record = pin_record(test, organizer.id, STRONG)
        .await
        .expect("the main exchange pinned the JMAP side's invitee");
    record.set_pin(
        withheld_address,
        OTHER_KEY,
        PinSource::FirstUse,
        now() as i64,
    );
    write_pin_record(test, organizer.id, STRONG, &record).await;
    let href = format!("{}default/{RT2_UID}.ics", home(organizer.name));
    let rt2 = vcalendar(&[vec![
        format!("UID:{RT2_UID}"),
        format!("DTSTAMP:{STAMP}"),
        "SEQUENCE:0".into(),
        "DTSTART:20990405T100000Z".into(),
        "DTEND:20990405T110000Z".into(),
        "SUMMARY:round trip two".into(),
        format!("ORGANIZER:mailto:{}", organizer.name),
        format!(
            "ATTENDEE;RSVP=TRUE;PARTSTAT=NEEDS-ACTION:mailto:{}",
            jm.invitee.name
        ),
        format!("ATTENDEE;RSVP=TRUE;PARTSTAT=NEEDS-ACTION:mailto:{withheld_address}"),
    ]]);
    dav_put(organizer.name, href.clone(), rt2, 201).await;
    organizer
        .client
        .proppatch(&href, [("D:displayname", "rt2 name")], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    wait_for_queue(test).await;
    let display_name = || async {
        organizer
            .client
            .propfind(&href, ["D:displayname"])
            .await
            .properties(&href)
            .get("D:displayname")
            .with_values(["rt2 name"]);
    };
    let status = || attendee_statuses(&organizer.client, &href, withheld_address);
    let pairs = || async {
        let keys = za_unlock(test, organizer.id, STRONG).await;
        let pairs = stored_pairs(test, &keys, organizer.id, &relative_path(&href))
            .await
            .unwrap_or_default();
        let recipients: Vec<&str> = [jm.invitee.name, withheld_address]
            .into_iter()
            .filter(|address| pairs.get(address).is_some())
            .collect();
        assert_eq!(recipients.len(), pairs.len(), "round_trip: a stray pair");
        recipients
    };
    let refusals = || async {
        withheld(test, organizer.id, withheld_address)
            .await
            .into_iter()
            .map(|entry| entry.refusal_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(status().await, ["3.8"], "round_trip: the withheld status");
    let refusal_ids = refusals().await;
    assert!(!refusal_ids.is_empty(), "round_trip: nothing withheld");
    let recipients = pairs().await;
    display_name().await;

    jmap_update(
        test,
        jm,
        RT2_UID,
        json!({ "description": "edited in the browser" }),
        false,
    )
    .await;
    assert_eq!(
        pairs().await,
        recipients,
        "round_trip: the pairs after the edit"
    );
    display_name().await;
    assert_eq!(
        refusals().await,
        refusal_ids,
        "round_trip: the withheld entries after the edit"
    );
    assert_eq!(
        status().await,
        ["3.8"],
        "round_trip: the withheld status after the edit"
    );

    jmap_update(
        test,
        jm,
        RT2_UID,
        json!({ "locations": { "rt2-room": { "@type": "Location", "name": "rt2 room" } } }),
        true,
    )
    .await;
    wait_for_queue(test).await;
    assert_eq!(
        pairs().await,
        recipients,
        "round_trip: the pairs after the scheduled edit"
    );
    display_name().await;
    assert!(
        !refusals().await.is_empty(),
        "round_trip: nothing withheld after the scheduled edit"
    );

    // Cleanup: both events, quietly; the invitee's items go with the
    // module's; the ordinary account.
    for (party, uid) in [
        (&jm.account, RT_UID),
        (&plain_party, RT_UID),
        (&jm.account, RT2_UID),
    ] {
        let href = copy_href(test, party.id, uid).await;
        party
            .client
            .request_with_headers("DELETE", &href, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    wait_for_index_queue(test, plain_id, Duration::from_secs(30)).await;
    admin.destroy_account(plain).await;
}

/// The REPLY of step M9 delivered to the account's mailbox.
async fn delete_delivered_mail(side: &Side) {
    let account = side.user.id_string();
    let reply = side
        .user
        .jmap_method_call(
            "Email/get",
            json!({ "accountId": account, "ids": null, "properties": ["id", "subject"] }),
        )
        .await;
    let mails: Vec<Value> = reply
        .list()
        .iter()
        .filter(|email| email["subject"] == "iMIP REPLY")
        .map(|email| email["id"].clone())
        .collect();
    assert_eq!(
        mails.len(),
        1,
        "{}: the delivered REPLY: {reply:?}",
        side.tag()
    );
    let reply = side
        .user
        .jmap_method_call(
            "Email/set",
            json!({ "accountId": account, "destroy": mails.clone() }),
        )
        .await;
    assert_eq!(reply.response_at(0)["destroyed"], json!(mails), "{reply:?}");
}

/// The oracle must not be blind: `normalize_jscal` removes ids and
/// timestamps and nothing else, and the mapping is what makes the sides
/// equal. Runs before any server work.
fn self_check() {
    let (cd, jm) = (Names::of(CD), Names::of(JM));
    let base = jscalendar_of(&main_text(6, &jm));
    let normalized = normalize_jscal(base.clone());

    type Edit = Box<dyn Fn(&mut Value)>;
    let ids: Vec<(&str, Edit)> = vec![
        (
            "id",
            Box::new(|value: &mut Value| value["id"] = json!("zz")),
        ),
        (
            "baseEventId",
            Box::new(|value: &mut Value| value["baseEventId"] = json!("zz")),
        ),
        (
            "calendarIds",
            Box::new(|value: &mut Value| value["calendarIds"] = json!({ "zz": true })),
        ),
        (
            "created",
            Box::new(|value: &mut Value| value["created"] = json!("2001-01-01T00:00:00Z")),
        ),
        (
            "updated",
            Box::new(|value: &mut Value| value["updated"] = json!("2001-01-01T00:00:00Z")),
        ),
    ];
    for (what, edit) in &ids {
        let mut edited = base.clone();
        edit(&mut edited);
        assert_ne!(
            edited, base,
            "self_check: the {what} edit changes the object"
        );
        assert_eq!(
            normalize_jscal(edited),
            normalized,
            "self_check: {what} alone"
        );
    }
    let mut all = base.clone();
    for (_, edit) in &ids {
        edit(&mut all);
    }
    assert_eq!(normalize_jscal(all), normalized, "self_check: all five");

    let invitee_address = format!("mailto:{}", jm.invitee);
    let invitee = base["participants"]
        .as_object()
        .and_then(|participants| {
            participants
                .iter()
                .find(|(_, participant)| {
                    participant["calendarAddress"]
                        .as_str()
                        .is_some_and(|address| address.eq_ignore_ascii_case(&invitee_address))
                })
                .map(|(key, _)| key.clone())
        })
        .unwrap_or_else(|| panic!("self_check: no invitee participant in {base}"));
    assert!(
        base.get("duration").is_some(),
        "self_check: no duration in {base}"
    );
    let changes: Vec<(&str, Edit)> = vec![
        (
            "title",
            Box::new(|value: &mut Value| value["title"] = json!("another title")),
        ),
        (
            "the invitee's participationStatus",
            Box::new({
                let invitee = invitee.clone();
                move |value: &mut Value| {
                    value["participants"][invitee.as_str()]["participationStatus"] =
                        json!("accepted")
                }
            }),
        ),
        (
            "the invitee's calendarAddress",
            Box::new({
                let invitee = invitee.clone();
                move |value: &mut Value| {
                    value["participants"][invitee.as_str()]["calendarAddress"] =
                        json!("mailto:other@example.com")
                }
            }),
        ),
        (
            "a locations entry",
            Box::new(|value: &mut Value| {
                if !value["locations"].is_object() {
                    value["locations"] = json!({});
                }
                value["locations"]["added"] = json!({ "@type": "Location", "name": "added" });
            }),
        ),
        (
            "duration removed",
            Box::new(|value: &mut Value| {
                value.as_object_mut().unwrap().remove("duration");
            }),
        ),
    ];
    for (what, change) in &changes {
        let mut edited = base.clone();
        change(&mut edited);
        assert_ne!(
            normalize_jscal(edited.clone()),
            normalized,
            "self_check: {what} is not removed"
        );
        for (_, edit) in &ids {
            edit(&mut edited);
        }
        assert_ne!(
            normalize_jscal(edited),
            normalized,
            "self_check: {what} beside the five removed properties"
        );
    }

    assert_eq!(
        map_names("mailto:jmapcal-oracle-cd-inv@example.com"),
        "mailto:jmapcal-oracle-jm-inv@example.com"
    );
    assert_eq!(
        map_names("jmapcal-oracle-cd-out@remote.test"),
        "jmapcal-oracle-jm-out@remote.test"
    );
    let caldav = jscalendar_of(&main_text(6, &cd));
    assert_ne!(
        normalize_jscal(caldav.clone()),
        normalized,
        "self_check: the two sides' M6 objects without the mapping"
    );
    assert_eq!(
        normalize_jscal(map_jscal(caldav)),
        normalized,
        "self_check: the two sides' M6 objects with the mapping"
    );
}
