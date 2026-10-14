/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Spec 11: decodes every stored record and asserts the sealing boundary.
//! A regression test, not a proof.

use super::{
    STRONG,
    dav_seal::{CANARIES, EVENT, mkcalendar_body, raw_event},
    fanout::NO_REPLY,
};
use crate::utils::{
    server::TestServer,
    webdav::DummyWebDavClient,
    za::{
        PlantedItem, hold_task_consumer, inbox_ids, itip_tasks_for, new_inbox_items,
        plant_inbox_item, plant_sealed_event, wait_for_delivery, za_unlock,
    },
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD},
};
use calcard::icalendar::{ICalendar, ICalendarEntry, ICalendarProperty};
use email::cache::MessageCacheFetch;
use groupware::{
    cache::GroupwareCache,
    calendar::{
        Calendar, CalendarEvent, CalendarEventData, CalendarEventNotification,
        EVENT_NOTIFICATION_ZA_PENDING, Timezone,
        seal::{
            TokenPairs,
            collection::COLLECTION_MARKER,
            event_token_pairs,
            item::RSVP_TOKEN_PROP,
            policy::{is_visible_parameter, is_visible_property},
            tree::{EXTRA_PROP, KEY_PROP, SEALED_PROP},
        },
    },
};
use hyper::StatusCode;
use mail_parser::{DateTime, MessageParser};
use store::{
    IterateParams, SUBSPACE_ACL, SUBSPACE_BLOB_LINK, SUBSPACE_BLOBS, SUBSPACE_COUNTER,
    SUBSPACE_DELETED_ITEMS, SUBSPACE_DIRECTORY, SUBSPACE_IN_MEMORY_COUNTER,
    SUBSPACE_IN_MEMORY_VALUE, SUBSPACE_INDEXES, SUBSPACE_LOGS, SUBSPACE_PROPERTY,
    SUBSPACE_QUEUE_EVENT, SUBSPACE_QUEUE_MESSAGE, SUBSPACE_QUOTA, SUBSPACE_REGISTRY,
    SUBSPACE_REGISTRY_IDX, SUBSPACE_REGISTRY_PK, SUBSPACE_REPORT_IN, SUBSPACE_REPORT_OUT,
    SUBSPACE_SEARCH_INDEX, SUBSPACE_SPAM_SAMPLES, SUBSPACE_TASK_QUEUE, SUBSPACE_TELEMETRY_METRIC,
    SUBSPACE_TELEMETRY_SPAN, U32_LEN, U64_LEN,
    write::{AlignedBytes, AnyKey, Archive, SearchIndex},
};
use types::{
    blob_hash::BLOB_HASH_LEN,
    collection::{Collection, SyncCollection},
    field::Field,
};

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
const EVENT_PATH: &str = "/dav/cal/key2%40example.com/leak/e.ics";

/// Canaries written by this module, on top of `dav_seal::CANARIES` (which
/// `EVENT` carries).
const EXTRA_CANARIES: &[&str] = &[
    "leakcal-canary",
    "leakdesc-canary",
    "leakcolor-canary",
    "leaktz-canary",
    "leaktzcomment-canary",
    "leakevtname-canary",
    "leakdead-canary",
    "alarm-event-canary",
    "alarm-summary-canary",
    "alarm-description-canary",
    "alarm-location-canary",
    "alarm-attendee-canary",
    "todo-canary",
    "todo-description-canary",
    "negative-canary",
    "sched-canary",
    "itip-request-canary",
    "itip-reply-canary",
    "itip-drained-canary",
    "itip-pair-canary",
    "itip-neg-item-canary",
    "fanoutsum-canary",
    "fanoutloc-canary",
    "fanoutdesc-canary",
    "fanoutcn-canary",
    "fanoutaddr-canary",
    "fanoutextcn-canary",
];

/// A future invitation organized by the key account to a local non-key
/// account: with iTIP gated it is stored sealed and nothing is sent.
const SCHED_EVENT: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\n",
    "UID:leak-sched\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\n",
    "DTEND:20990102T100000Z\r\nSUMMARY:sched-canary\r\n",
    "ORGANIZER:mailto:key2@example.com\r\n",
    "ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:plain@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);

/// A key organizer's invitation to the module's key account (plan 9's
/// fan-out): canaries in the title, location, description, the attendees'
/// display names and an external attendee's address (status 5.3, nothing
/// sent).
const FANOUT_EVENT: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\n",
    "UID:leak-fanout\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990104T090000Z\r\n",
    "DTEND:20990104T100000Z\r\nSUMMARY:fanoutsum-canary\r\n",
    "LOCATION:fanoutloc-canary\r\nDESCRIPTION:fanoutdesc-canary\r\n",
    "ORGANIZER:mailto:key1@example.com\r\n",
    "ATTENDEE;CN=fanoutcn-canary;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com\r\n",
    "ATTENDEE;CN=fanoutextcn-canary;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:",
    "mailto:fanoutaddr-canary@remote.test\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);

/// Inbox items planted for the scan (invitations spec 11): a pending REQUEST
/// to key2 with key2's pair, key2's pending REPLY from plain, a drained item.
const ITIP_REQUEST: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\n",
    "UID:leak-itip-request\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\n",
    "DTEND:20990102T100000Z\r\nSEQUENCE:0\r\nSUMMARY:itip-request-canary\r\n",
    "ORGANIZER:mailto:plain@example.com\r\n",
    "ATTENDEE;CN=itip-request-canary:mailto:key2@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);
const ITIP_REPLY: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REPLY\r\nBEGIN:VEVENT\r\n",
    "UID:leak-sched\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\n",
    "SEQUENCE:0\r\nORGANIZER:mailto:key2@example.com\r\n",
    "ATTENDEE;PARTSTAT=ACCEPTED:mailto:plain@example.com\r\n",
    "COMMENT:itip-reply-canary\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);
const ITIP_DRAINED: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\n",
    "UID:leak-itip-drained\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990103T090000Z\r\n",
    "DTEND:20990103T100000Z\r\nSEQUENCE:0\r\nSUMMARY:itip-drained-canary\r\n",
    "ORGANIZER:mailto:plain@example.com\r\n",
    "ATTENDEE:mailto:key2@example.com\r\n",
    "END:VEVENT\r\nEND:VCALENDAR\r\n"
);
/// Tokens planted with them (16 ASCII bytes each).
const TOKEN_REQUEST: [u8; 16] = *b"za-tok-request-1";
const TOKEN_REPLY: [u8; 16] = *b"za-tok-reply-002";
const TOKEN_EVENT_A: [u8; 16] = *b"za-tok-event-a03";
const TOKEN_EVENT_B: [u8; 16] = *b"za-tok-event-b04";

/// Archive marker bits (`store/src/write/serialize.rs`, private there).
const ARCHIVE_MAGIC_MARKER: u8 = 1 << 7;
const ARCHIVE_LZ4_COMPRESSED: u8 = 1 << 4;
const ARCHIVE_VERSIONED: u8 = 1 << 6;
const ARCHIVE_HASHED: u8 = 1 << 5;
/// Largest decompressed size the scanner accepts for a value that merely
/// looks like an LZ4 archive, so arbitrary bytes cannot ask for gigabytes.
const MAX_DECODED_LEN: usize = 64 * 1024 * 1024;

/// Every subspace `assert_is_empty` knows about, plus the change log.
const SUBSPACES: &[u8] = &[
    SUBSPACE_ACL,
    SUBSPACE_TASK_QUEUE,
    SUBSPACE_IN_MEMORY_VALUE,
    SUBSPACE_IN_MEMORY_COUNTER,
    SUBSPACE_PROPERTY,
    SUBSPACE_QUEUE_MESSAGE,
    SUBSPACE_QUEUE_EVENT,
    SUBSPACE_REPORT_OUT,
    SUBSPACE_REPORT_IN,
    SUBSPACE_DELETED_ITEMS,
    SUBSPACE_SPAM_SAMPLES,
    SUBSPACE_BLOB_LINK,
    SUBSPACE_BLOBS,
    SUBSPACE_COUNTER,
    SUBSPACE_QUOTA,
    SUBSPACE_INDEXES,
    SUBSPACE_TELEMETRY_SPAN,
    SUBSPACE_TELEMETRY_METRIC,
    SUBSPACE_SEARCH_INDEX,
    SUBSPACE_REGISTRY,
    SUBSPACE_REGISTRY_IDX,
    SUBSPACE_REGISTRY_PK,
    SUBSPACE_DIRECTORY,
    SUBSPACE_LOGS,
];

/// What one scan saw. The counters let the caller prove the scanner was
/// not blind: a wrong key layout would decode nothing and find nothing.
#[derive(Debug, Default)]
pub struct Scan {
    pub violations: Vec<String>,
    pub records: usize,
    pub archives: usize,
    pub blobs: usize,
    pub search_records: usize,
    pub search_account_records: usize,
    pub events: usize,
    pub calendars: usize,
    pub items: usize,
    pub item_keys: usize,
    pub item_extras: usize,
    pub emails: usize,
}

fn all_canaries() -> Vec<&'static str> {
    CANARIES
        .iter()
        .chain(EXTRA_CANARIES.iter())
        .copied()
        .collect()
}

fn find_canaries(bytes: &[u8], what: &str, how: &str, violations: &mut Vec<String>) {
    let text = String::from_utf8_lossy(bytes);
    for canary in all_canaries() {
        if text.contains(canary) {
            violations.push(format!("{what}: {how} contains {canary}"));
        }
    }
}

/// Which of the three carriers `entry` is, if any.
fn carrier_kind(entry: &ICalendarEntry) -> Option<&'static str> {
    match &entry.name {
        ICalendarProperty::Other(n) => [SEALED_PROP, KEY_PROP, EXTRA_PROP]
            .into_iter()
            .find(|c| n.eq_ignore_ascii_case(c)),
        _ => None,
    }
}

/// Visibility of every non-carrier entry and parameter, and the carrier
/// layout the sealer writes: no parameters on a carrier, `X-ZA-SEALED` only
/// as the last entry of its component, and (events) the root ending with
/// `X-ZA-EXTRA` (if any) then `X-ZA-KEY`. A custom timezone has no
/// `X-ZA-KEY` or `X-ZA-EXTRA`.
fn check_tree(tree: &ICalendar, what: &str, is_event: bool, violations: &mut Vec<String>) {
    if is_event && tree.components.is_empty() {
        violations.push(format!("{what}: missing {KEY_PROP}"));
    }
    for (index, component) in tree.components.iter().enumerate() {
        let mut body = component.entries.as_slice();
        if is_event && index == 0 {
            match body.split_last() {
                Some((last, rest)) if carrier_kind(last) == Some(KEY_PROP) => {
                    body = rest;
                    if let Some((last, rest)) = body.split_last()
                        && carrier_kind(last) == Some(EXTRA_PROP)
                    {
                        body = rest;
                    }
                }
                _ => violations.push(format!("{what}: missing {KEY_PROP}")),
            }
        }
        for (position, entry) in body.iter().enumerate() {
            match carrier_kind(entry) {
                Some(SEALED_PROP) if position + 1 == body.len() => {}
                Some(kind) => violations.push(format!(
                    "{what}: component {index} has misplaced carrier {kind} at entry {position}"
                )),
                None => {
                    if !is_visible_property(&component.component_type, &entry.name) {
                        violations.push(format!(
                            "{what}: component {index} ({:?}) has sealed-class property {:?}",
                            component.component_type, entry.name
                        ));
                    }
                    for param in entry.params.iter() {
                        if !is_visible_parameter(&param.name) {
                            violations.push(format!(
                                "{what}: component {index} {:?} has sealed-class parameter {:?}",
                                entry.name, param.name
                            ));
                        }
                    }
                }
            }
        }
        for entry in component.entries.iter() {
            if let Some(kind) = carrier_kind(entry)
                && !entry.params.is_empty()
            {
                violations.push(format!(
                    "{what}: component {index} carrier {kind} has parameters"
                ));
            }
        }
    }
}

fn check_event(event: &CalendarEvent, what: &str, violations: &mut Vec<String>) {
    if event.display_name.is_some() {
        violations.push(format!("{what}: display_name not empty"));
    }
    if !event.dead_properties.0.is_empty() {
        violations.push(format!("{what}: dead_properties not empty"));
    }
    // Per-account properties and alerts are not sealed (decision 7).
    if !event.preferences.is_empty() {
        violations.push(format!("{what}: event preferences not empty"));
    }
    check_tree(&event.data.event, what, true, violations);
}

/// Whether the root's last two entries are `X-ZA-EXTRA` then `X-ZA-KEY`.
fn ends_extra_then_key(tree: &ICalendar) -> bool {
    tree.components.first().is_some_and(|root| {
        matches!(
            root.entries.as_slice(),
            [.., extra, key]
                if carrier_kind(extra) == Some(EXTRA_PROP) && carrier_kind(key) == Some(KEY_PROP)
        )
    })
}

/// An inbox item (invitations spec 9): the root ends with `X-ZA-KEY` and,
/// before it, at most one of `X-ZA-RSVP-TOKEN` (visible, a pending reply)
/// or `X-ZA-EXTRA`, and neither once drained (pending bit clear); the rest
/// as `check_tree` checks any component. Returns whether the item carries
/// its key envelope.
fn check_item(item: &CalendarEventNotification, what: &str, violations: &mut Vec<String>) -> bool {
    let mut tree = item.event.clone();
    let has_key = tree
        .components
        .first()
        .and_then(|root| root.entries.last())
        .is_some_and(|e| carrier_kind(e) == Some(KEY_PROP));
    if !has_key {
        violations.push(format!("{what}: missing {KEY_PROP}"));
    } else {
        let root = &mut tree.components[0].entries;
        root.pop();
        let carrier = root.last().and_then(|e| {
            if carrier_kind(e) == Some(EXTRA_PROP) {
                Some(EXTRA_PROP)
            } else if matches!(&e.name, ICalendarProperty::Other(n) if n.eq_ignore_ascii_case(RSVP_TOKEN_PROP))
            {
                Some(RSVP_TOKEN_PROP)
            } else {
                None
            }
        });
        if let Some(carrier) = carrier {
            if item.flags & EVENT_NOTIFICATION_ZA_PENDING == 0 {
                violations.push(format!("{what}: drained item carries {carrier}"));
            }
            root.pop();
        }
    }
    check_tree(&tree, what, false, violations);
    has_key
}

fn check_calendar(calendar: &Calendar, account_id: u32, what: &str, violations: &mut Vec<String>) {
    if calendar.preferences.is_empty() {
        violations.push(format!("{what}: collection without preferences"));
        return;
    }
    let pref = calendar.preferences(account_id);
    if !pref.name.starts_with(COLLECTION_MARKER) {
        // Plaintext collections are allowed only if nothing user-supplied is in them.
        if calendar.preferences.iter().any(|p| {
            p.description.is_some()
                || p.color.is_some()
                || matches!(p.time_zone, Timezone::Custom(_))
        }) || !calendar.dead_properties.0.is_empty()
        {
            violations.push(format!("{what}: unsealed collection with user data"));
        }
        return;
    }
    // Spec 7.2: a sealed collection holds only the owner's entry.
    if calendar.preferences.len() != 1 || calendar.preferences[0].account_id != account_id {
        violations.push(format!(
            "{what}: sealed collection with preferences other than the owner's"
        ));
    }
    if pref.description.is_some() || pref.color.is_some() || !calendar.dead_properties.0.is_empty()
    {
        violations.push(format!("{what}: sealed collection with plaintext fields"));
    }
    if let Timezone::Custom(tz) = &pref.time_zone {
        check_tree(tz, what, false, violations);
    }
}

/// Decodes a value as a stored archive if it carries the archive marker.
/// An LZ4 archive starts with its decompressed size (little endian u32);
/// values that only look like one and claim a huge size are skipped, as are
/// values too short for the trailer their marker implies (the store's
/// deserializer would underflow on them).
fn try_archive(value: &[u8]) -> Option<Archive<AlignedBytes>> {
    let marker = *value.last()?;
    if marker & ARCHIVE_MAGIC_MARKER == 0 {
        return None;
    }
    let min_len = 1 + if marker & ARCHIVE_VERSIONED != 0 {
        U64_LEN + U32_LEN
    } else if marker & ARCHIVE_HASHED != 0 {
        U32_LEN
    } else {
        0
    };
    if value.len() < min_len {
        return None;
    }
    if marker & ARCHIVE_LZ4_COMPRESSED != 0 {
        let size = u32::from_le_bytes(value.get(..U32_LEN)?.try_into().ok()?) as usize;
        if size > MAX_DECODED_LEN {
            return None;
        }
    }
    <Archive<AlignedBytes> as store::Deserialize>::deserialize(value).ok()
}

/// Walks every subspace and the account's mail. Returns what it saw and the
/// violations found for `account_id`.
pub async fn scan(test: &TestServer, account_id: u32) -> Scan {
    let store = test.server.store().clone();
    let archive_field = u8::from(Field::ARCHIVE);
    let mut scan = Scan::default();
    let mut blob_hashes: Vec<Vec<u8>> = Vec::new();

    for &subspace in SUBSPACES {
        if subspace == SUBSPACE_SEARCH_INDEX && store.is_pg_or_mysql() {
            continue;
        }
        let from = AnyKey {
            subspace,
            key: vec![0u8],
        };
        let to = AnyKey {
            subspace,
            key: vec![u8::MAX; 64],
        };
        let mut records: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        store
            .iterate(IterateParams::new(from, to), |key, value| {
                records.push((key.to_vec(), value.to_vec()));
                Ok(true)
            })
            .await
            .unwrap();
        for (key, value) in records {
            scan.records += 1;
            let what = format!("subspace {:?} key {:?}", char::from(subspace), key);
            find_canaries(&key, &what, "key", &mut scan.violations);
            find_canaries(&value, &what, "raw value", &mut scan.violations);

            // Blob contents are decoded after the walk, through the server's
            // configured blob store: the data store's own blob subspace, or a
            // filesystem or S3 store whose blobs only the link records name
            // (a link key starts with the blob hash).
            if subspace == SUBSPACE_BLOBS {
                blob_hashes.push(key.clone());
            } else if subspace == SUBSPACE_BLOB_LINK && key.len() >= BLOB_HASH_LEN {
                blob_hashes.push(key[..BLOB_HASH_LEN].to_vec());
            }

            // Term keys are hashes of tokens, so a canary never appears in
            // them whole: assert structurally that no calendar data of the
            // account was indexed (spec 9). Key: class (index | type << 6),
            // then the account id (u32 BE) for every index but tracing,
            // which is global (`store/src/write/key.rs`). Only calendar
            // indexing is gated: the account's mail (here the generic alarm
            // email) is indexed as upstream does, and its entries prove the
            // key layout is read correctly.
            if subspace == SUBSPACE_SEARCH_INDEX {
                scan.search_records += 1;
                if let Some(index) = key.first().and_then(|c| SearchIndex::try_from_u8(c & 0x3f))
                    && index != SearchIndex::Tracing
                    && key.get(1..1 + U32_LEN) == Some(account_id.to_be_bytes().as_slice())
                {
                    scan.search_account_records += 1;
                    if index == SearchIndex::Calendar {
                        scan.violations.push(format!(
                            "{what}: calendar search index entry of the account"
                        ));
                    }
                }
            }

            let Some(archive) = try_archive(&value) else {
                continue;
            };
            scan.archives += 1;
            find_canaries(
                archive.as_bytes(),
                &what,
                "decoded archive",
                &mut scan.violations,
            );

            // Property key: account_id (u32 BE) | collection | property | document_id (u32 BE).
            if subspace != SUBSPACE_PROPERTY
                || key.len() != U32_LEN + 2 + U32_LEN
                || key[U32_LEN + 1] != archive_field
                || u32::from_be_bytes(key[..U32_LEN].try_into().unwrap()) != account_id
            {
                continue;
            }
            let collection = key[U32_LEN];
            if collection == Collection::CalendarEvent as u8 {
                scan.events += 1;
                match archive.deserialize::<CalendarEvent>() {
                    Ok(event) => check_event(&event, &what, &mut scan.violations),
                    Err(err) => scan
                        .violations
                        .push(format!("{what}: event archive does not decode: {err:?}")),
                }
            } else if collection == Collection::Calendar as u8 {
                scan.calendars += 1;
                match archive.deserialize::<Calendar>() {
                    Ok(calendar) => {
                        check_calendar(&calendar, account_id, &what, &mut scan.violations)
                    }
                    Err(err) => scan
                        .violations
                        .push(format!("{what}: calendar archive does not decode: {err:?}")),
                }
            } else if collection == Collection::CalendarEventNotification as u8 {
                scan.items += 1;
                match archive.deserialize::<CalendarEventNotification>() {
                    Ok(item) => {
                        if check_item(&item, &what, &mut scan.violations) {
                            scan.item_keys += 1;
                        }
                        if ends_extra_then_key(&item.event) {
                            scan.item_extras += 1;
                        }
                    }
                    Err(err) => scan.violations.push(format!(
                        "{what}: inbox item archive does not decode: {err:?}"
                    )),
                }
            }
        }
    }

    // Blobs are stored compressed; decode them the way readers do.
    blob_hashes.sort_unstable();
    blob_hashes.dedup();
    for hash in blob_hashes {
        let what = format!("blob {hash:?}");
        // A link record can briefly outlive its blob around purge, so a
        // missing blob stays silent; a blob that fails to read or
        // decompress must not go unscanned unnoticed.
        match test
            .server
            .blob_store()
            .get_blob(&hash, 0..usize::MAX)
            .await
        {
            Ok(Some(blob)) => {
                scan.blobs += 1;
                find_canaries(&blob, &what, "decoded blob", &mut scan.violations);
            }
            Ok(None) => {}
            Err(err) => scan
                .violations
                .push(format!("{what}: blob does not decode: {err:?}")),
        }
    }

    // Generated alarm emails, decoded: transfer encodings would hide a
    // canary from the raw scan.
    if let Ok(messages) = test.server.get_cached_messages(account_id).await {
        for message in messages.emails.items.iter() {
            scan.emails += 1;
            let what = format!("email {}", message.document_id);
            let contents = test.fetch_email(account_id, message.document_id).await;
            find_canaries(&contents, &what, "raw message", &mut scan.violations);
            let parsed = MessageParser::new()
                .parse(&contents)
                .unwrap_or_else(|| panic!("{what} does not parse"));
            find_canaries(
                parsed.subject().unwrap_or_default().as_bytes(),
                &what,
                "subject",
                &mut scan.violations,
            );
            for part in parsed.parts.iter() {
                find_canaries(part.contents(), &what, "decoded part", &mut scan.violations);
            }
        }
    }
    scan
}

/// The property-subspace key of an archive: account id (u32 BE) |
/// collection | `Field::ARCHIVE` | document id (u32 BE).
fn archive_key(account_id: u32, collection: Collection, document_id: u32) -> Vec<u8> {
    let mut key = account_id.to_be_bytes().to_vec();
    key.push(collection as u8);
    key.push(u8::from(Field::ARCHIVE));
    key.extend_from_slice(&document_id.to_be_bytes());
    key
}

/// Where any of `tokens` appears in the clear (invitations spec 11): as raw
/// bytes, base64url without padding (the `X-ZA-RSVP-TOKEN` form), standard
/// base64 or standard base64 without padding, in a raw key, a raw value, a
/// decoded archive or a decoded blob. Returns (subspace, key, token index)
/// per hit; a blob hit carries `SUBSPACE_BLOBS` and the blob hash. A blob
/// that fails to read panics: it must not go unsearched unnoticed.
pub async fn token_hits(test: &TestServer, tokens: &[[u8; 16]]) -> Vec<(u8, Vec<u8>, usize)> {
    let forms: Vec<(usize, Vec<u8>)> = tokens
        .iter()
        .enumerate()
        .flat_map(|(i, token)| {
            [
                (i, token.to_vec()),
                (i, URL_SAFE_NO_PAD.encode(token).into_bytes()),
                (i, STANDARD.encode(token).into_bytes()),
                (i, STANDARD_NO_PAD.encode(token).into_bytes()),
            ]
        })
        .collect();
    let find = |bytes: &[u8]| -> Vec<usize> {
        forms
            .iter()
            .filter(|(_, form)| bytes.windows(form.len()).any(|w| w == form.as_slice()))
            .map(|(i, _)| *i)
            .collect()
    };
    let store = test.server.store().clone();
    let mut hits = Vec::new();
    let mut blob_hashes: Vec<Vec<u8>> = Vec::new();
    for &subspace in SUBSPACES {
        if subspace == SUBSPACE_SEARCH_INDEX && store.is_pg_or_mysql() {
            continue;
        }
        let mut records: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        store
            .iterate(
                IterateParams::new(
                    AnyKey {
                        subspace,
                        key: vec![0u8],
                    },
                    AnyKey {
                        subspace,
                        key: vec![u8::MAX; 64],
                    },
                ),
                |key, value| {
                    records.push((key.to_vec(), value.to_vec()));
                    Ok(true)
                },
            )
            .await
            .unwrap();
        for (key, value) in records {
            let mut found = find(&key);
            found.extend(find(&value));
            if let Some(archive) = try_archive(&value) {
                found.extend(find(archive.as_bytes()));
            }
            if subspace == SUBSPACE_BLOBS {
                blob_hashes.push(key.clone());
            } else if subspace == SUBSPACE_BLOB_LINK && key.len() >= BLOB_HASH_LEN {
                blob_hashes.push(key[..BLOB_HASH_LEN].to_vec());
            }
            found.sort_unstable();
            found.dedup();
            hits.extend(found.into_iter().map(|i| (subspace, key.clone(), i)));
        }
    }
    blob_hashes.sort_unstable();
    blob_hashes.dedup();
    for hash in blob_hashes {
        // A missing blob stays silent, as in `scan`.
        match test
            .server
            .blob_store()
            .get_blob(&hash, 0..usize::MAX)
            .await
        {
            Ok(Some(blob)) => hits.extend(
                find(&blob)
                    .into_iter()
                    .map(|i| (SUBSPACE_BLOBS, hash.clone(), i)),
            ),
            Ok(None) => {}
            Err(err) => panic!("blob {hash:?} does not decode: {err:?}"),
        }
    }
    hits
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access leak regression test...");
    let name = "key2@example.com";
    let id = test.account(name).id().document_id();
    let client = DummyWebDavClient::new(id, name, STRONG, name);
    let cal = "/dav/cal/key2%40example.com/leak/";
    let copy = "/dav/cal/key2%40example.com/leak-copy/";

    // Everything the spec lists, written through CalDAV: collection name,
    // description, colour, custom timezone; event tree, display name, dead
    // property; a VTODO; an email alarm.
    client
        .request(
            "MKCALENDAR",
            cal,
            mkcalendar_body(&[
                ("D:displayname", "leakcal-canary"),
                ("A:calendar-description", "leakdesc-canary"),
                ("C:calendar-color", "leakcolor-canary"),
            ]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let tz = crate::webdav::TEST_VTIMEZONE_1
        .replace("Eastern Standard Time (US Canada)", "leaktz-canary")
        .replace(
            "TZOFFSETTO:-0500\n",
            "TZOFFSETTO:-0500\nCOMMENT:leaktzcomment-canary\n",
        )
        .replace('\n', "\r\n");
    client
        .proppatch(cal, [("A:calendar-timezone", tz.as_str())], [], [])
        .await
        .with_status(StatusCode::MULTI_STATUS);
    // Every slot really holds its canary, so a clean scan means sealed, not
    // absent.
    let props = client
        .propfind(
            cal,
            [
                "D:displayname",
                "A:calendar-description",
                "C:calendar-color",
                "A:calendar-timezone",
            ],
        )
        .await;
    let body = props.response.body.as_deref().unwrap_or_default();
    assert!(!body.contains("X-ZA-"), "{body}");
    let props = props.properties(cal);
    props.get("D:displayname").with_values(["leakcal-canary"]);
    props
        .get("A:calendar-description")
        .with_values(["leakdesc-canary"]);
    props
        .get("calendar-color")
        .with_values(["leakcolor-canary", "[xmlns]:http://calendarserver.org/ns/"]);
    let tz_back = props.get("A:calendar-timezone").value().to_string();
    assert!(
        tz_back.contains("leaktz-canary") && tz_back.contains("leaktzcomment-canary"),
        "{tz_back}"
    );
    client
        .request_with_headers("PUT", EVENT_PATH, [CONTENT_TYPE], EVENT)
        .await
        .with_status(StatusCode::CREATED);
    client
        .proppatch(
            EVENT_PATH,
            [
                ("D:displayname", "leakevtname-canary"),
                ("C:leak-dead", "leakdead-canary"),
            ],
            [],
            [],
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    let event_props = client
        .propfind(EVENT_PATH, ["D:displayname", "C:leak-dead"])
        .await;
    let body = event_props.response.body.as_deref().unwrap_or_default();
    assert!(!body.contains("X-ZA-"), "{body}");
    let event_props = event_props.properties(EVENT_PATH);
    event_props
        .get("D:displayname")
        .with_values(["leakevtname-canary"]);
    event_props
        .get("leak-dead")
        .with_values(["leakdead-canary", "[xmlns]:http://calendarserver.org/ns/"]);
    let todo = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VTODO\r\n",
        "UID:leak-todo\r\nDTSTAMP:20240101T000000Z\r\nDUE:20240105T000000Z\r\n",
        "SUMMARY:todo-canary\r\nDESCRIPTION:todo-description-canary\r\n",
        "END:VTODO\r\nEND:VCALENDAR\r\n"
    );
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/key2%40example.com/leak/t.ics",
            [CONTENT_TYPE],
            todo,
        )
        .await
        .with_status(StatusCode::CREATED);
    let alarm = format!(
        concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\n",
            "UID:leak-alarm\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:{}\r\n",
            "DTEND;TZID=America/New_York:21250221T180000\r\n",
            "SUMMARY:alarm-event-canary\r\nLOCATION:alarm-location-canary\r\n",
            "BEGIN:VALARM\r\nTRIGGER:-P2S\r\nACTION:EMAIL\r\n",
            "ATTENDEE;CN=alarm-attendee-canary:mailto:key2@example.com\r\n",
            "SUMMARY:alarm-summary-canary\r\nDESCRIPTION:alarm-description-canary\r\n",
            "END:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        ),
        DateTime::from_timestamp(store::write::now() as i64 + 3)
            .to_rfc3339()
            .replace(['-', ':'], "")
    );
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/key2%40example.com/leak/alarm.ics",
            [CONTENT_TYPE],
            alarm,
        )
        .await
        .with_status(StatusCode::CREATED);
    // Hold the consumer across both invitations and the scan: a regression
    // that queued either as a task leaves it in the task queue here, where
    // the scan and the task count see it.
    let key1_id = test.account("key1@example.com").id().document_id();
    let key1 = DummyWebDavClient::new(key1_id, "key1@example.com", STRONG, "key1@example.com");
    let key2_items = inbox_ids(test, id).await;
    let held = hold_task_consumer(test).await;
    client
        .request_with_headers(
            "PUT",
            "/dav/cal/key2%40example.com/leak/sched.ics",
            [CONTENT_TYPE],
            SCHED_EVENT,
        )
        .await
        .with_status(StatusCode::CREATED);
    // Plan 9: key1's invitation reaches key2 as a sealed pending item.
    key1.request_with_headers(
        "PUT",
        "/dav/cal/key1%40example.com/default/fo-leak.ics",
        [CONTENT_TYPE],
        FANOUT_EVENT,
    )
    .await
    .with_status(StatusCode::CREATED);
    let fan_out_items = new_inbox_items(test, id, &key2_items).await;
    assert_eq!(
        fan_out_items.len(),
        1,
        "key1's invitation reached key2 once"
    );
    let fan_out_item = fan_out_items[0].0;
    let pending = scan(test, id).await;
    assert!(
        pending.violations.is_empty(),
        "leaks found before the task queue drained:\n{}",
        pending.violations.join("\n")
    );
    assert_eq!(
        itip_tasks_for(test, key1_id).await + itip_tasks_for(test, id).await,
        0,
        "an invitation was queued as a task"
    );
    // The organizer's stored event passes the structural check (`scan`
    // checks only key2's records) and ends with its pair bundle, then its
    // key envelope.
    let (organizer_event, _) = raw_event(test, key1_id, "default/fo-leak.ics").await;
    let mut organizer_violations = Vec::new();
    let organizer = organizer_event.deserialize::<CalendarEvent>().unwrap();
    check_event(&organizer, "key1 fo-leak.ics", &mut organizer_violations);
    assert!(
        organizer_violations.is_empty(),
        "the organizer's event:\n{}",
        organizer_violations.join("\n")
    );
    assert!(
        ends_extra_then_key(&organizer.data.event),
        "the organizer's root does not end {EXTRA_PROP} then {KEY_PROP}"
    );
    // The fan-out's token for key2 is in the clear nowhere, the held task
    // queue included: its item carries it in the sealed bundle, and no
    // reply item exists here.
    let keys1 = za_unlock(test, key1_id, STRONG).await;
    let fan_out_token = *event_token_pairs(&organizer_event, &keys1, key1_id)
        .unwrap()
        .expect("key1's bundle holds pairs")
        .get("key2@example.com")
        .expect("key2's pair");
    let fan_out_hits = token_hits(test, &[fan_out_token]).await;
    assert!(
        fan_out_hits.is_empty(),
        "the fan-out's token in the clear: {fan_out_hits:?}"
    );
    drop(held);
    test.wait_for_tasks().await;
    client
        .request_with_headers(
            "COPY",
            cal,
            [("destination", copy), ("depth", "infinity")],
            "",
        )
        .await
        .with_status(StatusCode::CREATED);
    // Wait for the alarm to fire and its email to be delivered: the task
    // queue holds the alarm until it is due, then the SMTP queue drains.
    wait_for_delivery(test).await;

    // No carrier ever reaches a client.
    for path in [cal, copy] {
        let body = client
            .request_with_headers(
                "PROPFIND",
                path,
                [("depth", "1")],
                "<?xml version=\"1.0\"?><D:propfind xmlns:D=\"DAV:\"><D:allprop/></D:propfind>",
            )
            .await
            .with_status(StatusCode::MULTI_STATUS)
            .body
            .unwrap();
        assert!(
            !body.contains("X-ZA-") && !body.contains(COLLECTION_MARKER),
            "{body}"
        );
    }
    for (path, canary) in [
        (EVENT_PATH, "summary-canary"),
        (
            "/dav/cal/key2%40example.com/leak/t.ics",
            "todo-description-canary",
        ),
        (
            "/dav/cal/key2%40example.com/leak/alarm.ics",
            "alarm-description-canary",
        ),
        (
            "/dav/cal/key2%40example.com/leak-copy/e.ics",
            "xprop-canary",
        ),
        ("/dav/cal/key2%40example.com/leak/sched.ics", "sched-canary"),
    ] {
        let body = client
            .request("GET", path, "")
            .await
            .with_status(StatusCode::OK)
            .body
            .unwrap();
        assert!(
            body.contains(canary) && !body.contains("X-ZA-") && !body.contains(COLLECTION_MARKER),
            "{body}"
        );
    }

    // Invitations spec 11: inbox items in both states and an event whose
    // bundle holds token pairs, as plans 9 and 10 will write them.
    let sender = test.account("plain@example.com").id().document_id();
    let request_id = plant_inbox_item(
        test,
        id,
        ITIP_REQUEST,
        PlantedItem::Pending {
            pair: Some(("itip-pair-canary@example.com", TOKEN_REQUEST)),
            reply_token: None,
        },
        sender,
        None,
    )
    .await;
    let reply_id = plant_inbox_item(
        test,
        id,
        ITIP_REPLY,
        PlantedItem::Pending {
            pair: None,
            reply_token: Some(TOKEN_REPLY),
        },
        sender,
        None,
    )
    .await;
    let drained_id = plant_inbox_item(
        test,
        id,
        ITIP_DRAINED,
        PlantedItem::Drained { password: STRONG },
        sender,
        None,
    )
    .await;
    let mut pairs = TokenPairs::new();
    pairs.insert("itip-pair-canary@example.com", TOKEN_EVENT_A);
    pairs.insert("key2@example.com", TOKEN_EVENT_B);
    plant_sealed_event(
        test,
        id,
        "leak",
        "pairs.ics",
        &EVENT.replace("za-event-1", "za-leak-pairs"),
        STRONG,
        &pairs,
    )
    .await;

    let result = scan(test, id).await;
    println!(
        "Leak scan: {} records, {} archives, {} blobs, {} search index records ({} of the account, none calendar), {} events, {} calendars, {} items, {} emails",
        result.records,
        result.archives,
        result.blobs,
        result.search_records,
        result.search_account_records,
        result.events,
        result.calendars,
        result.items,
        result.emails
    );
    assert!(
        result.violations.is_empty(),
        "leaks found:\n{}",
        result.violations.join("\n")
    );
    // Not blind: e.ics, t.ics, alarm.ics and sched.ics (an in-account
    // collection COPY adds a name to each event document instead of
    // duplicating it), both collections, the alarm email, and its blob
    // (read through the configured blob store, so a filesystem store is
    // covered too).
    assert!(result.events >= 4, "{result:?}");
    assert!(result.calendars >= 2, "{result:?}");
    assert!(result.emails >= 1, "the alarm email was not delivered");
    assert!(result.blobs > 0, "no blob decoded: {result:?}");
    assert!(
        result.search_account_records > 0,
        "no search index entry of the account: the key layout is misread: {result:?}"
    );

    // Inbox items were decoded (the three planted ones and the fan-out's)
    // and each carries its key envelope; the pending requests' pair bundles
    // were seen before their envelopes.
    assert!(result.items >= 4, "{result:?}");
    assert_eq!(result.item_keys, result.items, "{result:?}");
    assert!(result.item_extras >= 2, "{result:?}");
    // The paired event's stored root ends with its pair bundle, then its key
    // envelope: the scan above checked an event that carries pairs.
    let (paired, _) = raw_event(test, id, "leak/pairs.ics").await;
    let paired = paired.deserialize::<CalendarEvent>().unwrap();
    assert!(
        ends_extra_then_key(&paired.data.event),
        "the paired event's root does not end {EXTRA_PROP} then {KEY_PROP}"
    );

    // Tokens are in the clear only on the pending reply item, whose
    // `X-ZA-RSVP-TOKEN` is visible by design (spec 3.3); that hit is the
    // positive control.
    let hits = token_hits(
        test,
        &[TOKEN_REQUEST, TOKEN_REPLY, TOKEN_EVENT_A, TOKEN_EVENT_B],
    )
    .await;
    let reply_key = archive_key(id, Collection::CalendarEventNotification, reply_id);
    let on_reply = |subspace: u8, key: &[u8], token: usize| {
        subspace == SUBSPACE_PROPERTY && key == reply_key.as_slice() && token == 1
    };
    assert!(
        hits.iter().any(|(s, k, t)| on_reply(*s, k, *t)),
        "the visible reply token was not found: {hits:?}"
    );
    let stray: Vec<_> = hits
        .iter()
        .filter(|(s, k, t)| !on_reply(*s, k, *t))
        .collect();
    assert!(
        stray.is_empty(),
        "a token in the clear outside the pending reply item: {stray:?}"
    );

    // Negative control: an unsealed event planted directly must be caught,
    // both by the canary search and by the structural check.
    {
        let resources = test
            .server
            .fetch_dav_resources(id, id, SyncCollection::Calendar)
            .await
            .unwrap();
        let calendar_id = resources.by_path("leak").unwrap().document_id();
        let ical = match calcard::Parser::new(&EVENT.replace("summary-canary", "negative-canary"))
            .entry()
        {
            calcard::Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        };
        let mut next = None;
        let event = CalendarEvent {
            names: vec![common::DavName {
                name: "planted.ics".into(),
                parent_id: calendar_id,
            }],
            data: CalendarEventData::new(
                ical,
                calcard::common::timezone::Tz::Floating,
                100,
                &mut next,
            ),
            size: 1,
            ..Default::default()
        };
        let account_info = test.server.account_info(id).await.unwrap();
        let document_id = test
            .server
            .store()
            .assign_document_ids(id, Collection::CalendarEvent, 1)
            .await
            .unwrap();
        let mut batch = store::write::BatchBuilder::new();
        event
            .insert(
                account_info.account_tenant_ids(),
                id,
                document_id,
                None,
                &mut batch,
            )
            .unwrap();
        test.server.commit_batch(batch).await.unwrap();
        // A direct store write does not wake the task manager the way the
        // DAV handlers do; without this the index task waits for its poll.
        test.server.notify_task_queue();
        test.wait_for_tasks().await;
        let planted = scan(test, id).await;
        let report = planted.violations.join("\n");
        assert!(
            planted
                .violations
                .iter()
                .any(|v| v.contains("negative-canary")),
            "the scanner must catch the planted canary:\n{report}"
        );
        assert!(
            planted
                .violations
                .iter()
                .any(|v| v.ends_with(&format!("missing {KEY_PROP}"))),
            "the scanner must catch the unsealed event structure:\n{report}"
        );
        assert!(
            planted
                .violations
                .iter()
                .any(|v| v.contains("sealed-class property Summary")),
            "the scanner must catch the plaintext SUMMARY:\n{report}"
        );
        println!(
            "Negative control caught {} violations:\n{report}",
            planted.violations.len()
        );
        test.server
            .invalidate_local_caches(&[common::ipc::CacheInvalidation::DavResources(id)])
            .await;
        client
            .request("DELETE", "/dav/cal/key2%40example.com/leak/planted.ics", "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }

    // Negative control for items: a plaintext item with the pending bit is
    // caught by the canary search and by the item check.
    {
        let negative = plant_inbox_item(
            test,
            id,
            &ITIP_DRAINED.replace("itip-drained-canary", "itip-neg-item-canary"),
            PlantedItem::Plain {
                flags: EVENT_NOTIFICATION_ZA_PENDING,
            },
            sender,
            None,
        )
        .await;
        let key = format!(
            "{:?}",
            archive_key(id, Collection::CalendarEventNotification, negative)
        );
        let planted = scan(test, id).await;
        let report = planted.violations.join("\n");
        assert!(
            planted
                .violations
                .iter()
                .any(|v| v.contains("itip-neg-item-canary")),
            "the scanner must catch the planted item canary:\n{report}"
        );
        assert!(
            planted
                .violations
                .iter()
                .any(|v| v.contains(&key) && v.ends_with(&format!("missing {KEY_PROP}"))),
            "the scanner must catch the unsealed item:\n{report}"
        );
        client
            .request(
                "DELETE",
                &format!("/dav/itip/key2%40example.com/inbox/{negative}.ics"),
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    key1.request_with_headers(
        "DELETE",
        "/dav/cal/key1%40example.com/default/fo-leak.ics",
        [NO_REPLY],
        "",
    )
    .await
    .with_status(StatusCode::NO_CONTENT);
    for item in [request_id, reply_id, drained_id, fan_out_item] {
        client
            .request(
                "DELETE",
                &format!("/dav/itip/key2%40example.com/inbox/{item}.ics"),
                "",
            )
            .await
            .with_status(StatusCode::NO_CONTENT);
    }

    test.wait_for_tasks().await;
    for path in [copy, cal] {
        client
            .request("DELETE", path, "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    test.destroy_all_mailboxes(test.account(name)).await;
}

/// Positive control for the scanner's calendar search-index check: entries
/// planted with the store's own key serializer are flagged exactly when they
/// are Calendar entries of the scanned account.
pub async fn test_index_control(test: &mut TestServer) {
    use store::write::{
        BatchBuilder, SearchIndexClass, SearchIndexId, SearchIndexType, ValueClass,
    };
    println!("Running zero-access leak scanner index control...");
    // `leak::test` ends by deleting key2's calendars and mailboxes without
    // waiting: their unindex tasks may still be running. Drain them so the
    // baseline below is stable.
    test.wait_for_tasks().await;
    let id = test.account("key2@example.com").id().document_id();
    let plain_id = test.account("plain@example.com").id().document_id();
    let entry = |index, account_id, typ| {
        ValueClass::SearchIndex(SearchIndexClass {
            index,
            id: SearchIndexId::Account {
                account_id,
                document_id: u32::MAX - 1,
            },
            typ,
        })
    };
    let term = || SearchIndexType::Term {
        field: 0,
        hash: utils::cheeky_hash::CheekyHash::new(b"za-control"),
    };
    let planted = [
        // Flagged: Calendar entries of the scanned account (document and term layouts).
        entry(SearchIndex::Calendar, id, SearchIndexType::Document),
        entry(SearchIndex::Calendar, id, term()),
        // Not flagged: an Email entry of the account, a Calendar entry of another.
        entry(SearchIndex::Email, id, SearchIndexType::Document),
        entry(SearchIndex::Calendar, plain_id, SearchIndexType::Document),
    ];
    let before = scan(test, id).await;
    assert!(before.violations.is_empty(), "{:?}", before.violations);
    let mut batch = BatchBuilder::new();
    for class in &planted {
        batch.set(class.clone(), Vec::<u8>::new());
    }
    test.server.store().write(batch.build_all()).await.unwrap();

    let result = scan(test, id).await;
    let flagged = result
        .violations
        .iter()
        .filter(|v| v.ends_with("calendar search index entry of the account"))
        .count();
    assert_eq!(flagged, 2, "{:?}", result.violations);
    assert_eq!(
        result.violations.len(),
        2,
        "unexpected violations: {:?}",
        result.violations
    );
    assert_eq!(
        result.search_account_records,
        before.search_account_records + 3,
        "the three entries of the account were not all counted"
    );

    let mut batch = BatchBuilder::new();
    for class in planted {
        batch.clear(class);
    }
    test.server.store().write(batch.build_all()).await.unwrap();
    let after = scan(test, id).await;
    assert!(after.violations.is_empty(), "{:?}", after.violations);
}

#[cfg(test)]
mod tests {
    use super::try_archive;

    #[test]
    fn try_archive_short_values_are_not_archives() {
        for marker in [0xC0u8, 0xA0, 0xE0] {
            assert!(try_archive(&[marker]).is_none());
        }
        // A bare unversioned marker is a valid empty archive; it must only
        // not panic.
        let _ = try_archive(&[0x80]);
        let mut versioned = vec![0u8; 11];
        versioned.push(0xC0);
        assert!(try_archive(&versioned).is_none());
        assert!(try_archive(&[0, 0, 0, 0xA0]).is_none());
        assert!(try_archive(&[0, 0, 0, 0, 0x01]).is_none());
        // Unversioned and long enough: must not panic, result is irrelevant.
        let _ = try_archive(&[0, 0, 0, 0, 0x80]);
    }
}
