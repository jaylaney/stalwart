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
    fanout::{NO_REPLY, uid_copies},
    mail::{Traced, key_mail_account, message_ids, stored_metadata},
    pinning::{ALICE, drop_entries, forget, parties, refuse_once_at},
    raw_input::{SUBMISSION_PORT, strings},
};
use crate::{
    utils::{
        imap::{ImapConnection, Type},
        pop3::{Pop3Connection, ResponseType as Pop3Reply},
        server::TestServer,
        smtp::SmtpConnection,
        webdav::DummyWebDavClient,
        za::{
            Conn, PlantedItem, copy_href, hold_task_consumer, inbox_ids, itip_tasks_for,
            new_inbox_items, pending_ids, pin_record, plant_inbox_item, plant_plain_message,
            plant_sealed_event, queued_recipients, substitute_public_key, trigger_drain,
            wait_for_delivery, za_unlock,
        },
    },
    za::drain::{DrainOn, body, set_partstat},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD},
};
use calcard::icalendar::{ICalendar, ICalendarEntry, ICalendarProperty};
use email::{
    cache::MessageCacheFetch,
    message::{
        metadata::{MESSAGE_HAS_ATTACHMENT, MessageMetadata, MetadataPartType},
        za::MAIL_FORMAT_V1,
    },
};
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
use imap_proto::ResponseType;
use mail_parser::{DateTime, MessageParser};
use registry::{
    pickle::{Pickle, PickledStream},
    schema::structs::SpamTrainingSample,
};
use serde_json::json;
use store::{
    IterateParams, SUBSPACE_ACL, SUBSPACE_BLOB_LINK, SUBSPACE_BLOBS, SUBSPACE_COUNTER,
    SUBSPACE_DELETED_ITEMS, SUBSPACE_DIRECTORY, SUBSPACE_IN_MEMORY_COUNTER,
    SUBSPACE_IN_MEMORY_VALUE, SUBSPACE_INDEXES, SUBSPACE_LOGS, SUBSPACE_PROPERTY,
    SUBSPACE_QUEUE_EVENT, SUBSPACE_QUEUE_MESSAGE, SUBSPACE_QUOTA, SUBSPACE_REGISTRY,
    SUBSPACE_REGISTRY_IDX, SUBSPACE_REGISTRY_PK, SUBSPACE_REPORT_IN, SUBSPACE_REPORT_OUT,
    SUBSPACE_SEARCH_INDEX, SUBSPACE_SPAM_SAMPLES, SUBSPACE_TASK_QUEUE, SUBSPACE_TELEMETRY_METRIC,
    SUBSPACE_TELEMETRY_SPAN, Serialize as _, U32_LEN, U64_LEN,
    write::{AlignedBytes, AnyKey, Archive, Archiver, BatchBuilder, SearchIndex},
};
use trc::EventType;
use types::{
    blob_hash::BLOB_HASH_LEN,
    collection::{Collection, SyncCollection},
    field::{EmailField, Field},
    id::Id,
};
use utils::cheeky_hash::CheekyHash;

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
    "draintitle-canary",
    "drainloc-canary",
    "draindesc-canary",
    "draincn-canary",
    "drainaddr-canary",
    "pinaddr-canary",
    // Mail spec 11: `test_mail`'s messages. `mail-sub` is the subject's
    // first word (`mail-sub canary`, 15 bytes, the length a verbatim index
    // key would hold whole).
    "mail-sub",
    "mailfrom-canary",
    "mailname-canary",
    "mailrcpt-canary",
    "mailmsgid-canary",
    "mailbody-canary",
    "mailattach-canary",
    "mailattname-canary",
    "mailhdrname-canary",
    "mailhdrval-canary",
    "mailpreview-canary",
];

/// Mail spec 5.6: a key account's keyword and mailbox names are visible by
/// structure. The scan reports them in `Scan::found`, not as violations: the
/// positive controls of `test_mail`.
const VISIBLE_CANARIES: &[&str] = &["mailkw-canary", "mailbox-canary"];

/// The search fields a key account's message documents hold (mail spec 5.5):
/// `EmailSearchField::ReceivedAt` and `Size`, as `SearchField::u8_id`
/// (`store/src/search/term.rs`, crate-private) numbers them.
const SEARCH_FIELD_RECEIVED_AT: u8 = 10;
const SEARCH_FIELD_SIZE: u8 = 12;

/// The layout of the store's search document (`TermIndex`,
/// `store/src/search/term.rs`, crate-private), mirrored to decode the value
/// of a `Document`-layout search entry.
#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
struct ScannedDocument {
    terms: Vec<ScannedTerm>,
    fields: Vec<ScannedField>,
}

#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
struct ScannedTerm {
    hash: CheekyHash,
    fields: u32,
}

#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
struct ScannedField {
    field_id: u8,
    data: Vec<u8>,
}

/// Plan 10: key1's invitation to key2 with every canary the drain handles,
/// plus an external attendee whose address is a canary.
const DRAIN_EVENT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//za//EN\r\nBEGIN:VEVENT\r\nUID:leak-drain\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990105T090000Z\r\nDTEND:20990105T100000Z\r\nSEQUENCE:0\r\nSUMMARY:draintitle-canary\r\nLOCATION:drainloc-canary\r\nDESCRIPTION:draindesc-canary\r\nORGANIZER:mailto:key1@example.com\r\nATTENDEE;CN=draincn-canary;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:key2@example.com\r\nATTENDEE:mailto:drainaddr-canary@remote.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

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
    // Under 75 octets, so no fold can split the address canary in a text form.
    "ATTENDEE;CN=fanoutextcn-canary:mailto:fanoutaddr-canary@remote.test\r\n",
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
    /// Message metadata archives of the account that are stubs.
    pub stubs: usize,
    /// Hits of `VISIBLE_CANARIES`, which are not violations.
    pub found: Vec<String>,
}

fn all_canaries() -> Vec<&'static str> {
    CANARIES
        .iter()
        .chain(EXTRA_CANARIES.iter())
        .copied()
        .collect()
}

fn find_canaries(bytes: &[u8], what: &str, how: &str, scan: &mut Scan) {
    let text = String::from_utf8_lossy(bytes);
    for canary in all_canaries() {
        if text.contains(canary) {
            scan.violations
                .push(format!("{what}: {how} contains {canary}"));
        }
    }
    for canary in VISIBLE_CANARIES {
        if text.contains(canary) {
            scan.found.push(format!("{what}: {how} contains {canary}"));
        }
    }
}

/// Mail spec 5.2 and 6.1: the metadata archive (`EmailField::Metadata`) of a
/// key account's message is a stub: `raw_headers` opens with the format
/// byte, and the visible fields are the arrival time, the blob hash and one
/// empty root part. Counts the stubs.
fn check_email(
    archive: &Archive<AlignedBytes>,
    what: &str,
    violations: &mut Vec<String>,
    stubs: &mut usize,
) {
    let metadata = match archive.deserialize::<MessageMetadata>() {
        Ok(metadata) => metadata,
        Err(err) => {
            violations.push(format!("{what}: metadata archive does not decode: {err:?}"));
            return;
        }
    };
    if metadata.raw_headers.first() != Some(&MAIL_FORMAT_V1) {
        violations.push(format!("{what}: plaintext metadata archive"));
        return;
    }
    *stubs += 1;
    if !metadata.preview.is_empty() {
        violations.push(format!("{what}: stub with a preview"));
    }
    if metadata.blob_body_offset != 0 || metadata.rcvd_attach & MESSAGE_HAS_ATTACHMENT != 0 {
        violations.push(format!("{what}: stub with a body offset or attachment bit"));
    }
    let empty_root = matches!(metadata.contents.as_ref(), [contents]
        if contents.html_body.is_empty()
            && contents.text_body.is_empty()
            && contents.attachments.is_empty()
            && matches!(contents.parts.as_ref(), [part]
                if part.headers.is_empty()
                    && matches!(part.body, MetadataPartType::Binary)
                    && part.flags == 0
                    && part.offset_header == 0
                    && part.offset_body == 0
                    && part.offset_end == 0));
    if !empty_root {
        violations.push(format!("{what}: stub with content parts"));
    }
}

/// Mail spec 5.5: a key account's Email search entries are one document per
/// message holding the arrival time and the size, and those two fields'
/// entries; no term. The class byte is the index, plus 0 for a term, 1 << 6
/// for a field, 2 << 6 for a document (`store/src/write/key.rs`); a field
/// key continues with the account id and the field id.
fn check_email_search(key: &[u8], value: &[u8], what: &str, violations: &mut Vec<String>) {
    let is_visible = |field: u8| matches!(field, SEARCH_FIELD_RECEIVED_AT | SEARCH_FIELD_SIZE);
    match key[0] >> 6 {
        0 => violations.push(format!("{what}: search term of the account")),
        1 => {
            if !key.get(1 + U32_LEN).is_some_and(|field| is_visible(*field)) {
                violations.push(format!("{what}: search field of the account"));
            }
        }
        _ => {
            // A value that is not an archive holds no document (the index
            // control plants empty ones).
            let Some(archive) = try_archive(value) else {
                return;
            };
            match archive.unarchive_untrusted::<ScannedDocument>() {
                Ok(document) => {
                    if !document.terms.is_empty() {
                        violations.push(format!("{what}: search document with terms"));
                    }
                    if document
                        .fields
                        .iter()
                        .any(|field| !is_visible(field.field_id))
                    {
                        violations.push(format!(
                            "{what}: search document with fields other than the arrival time and the size"
                        ));
                    }
                }
                Err(err) => {
                    violations.push(format!("{what}: search document does not decode: {err:?}"))
                }
            }
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
            find_canaries(&key, &what, "key", &mut scan);
            find_canaries(&value, &what, "raw value", &mut scan);
            let of_account = key.get(..U32_LEN) == Some(account_id.to_be_bytes().as_slice());

            // Blob contents are decoded after the walk, through the server's
            // configured blob store: the data store's own blob subspace, or a
            // filesystem or S3 store whose blobs only the link records name
            // (a link key starts with the blob hash).
            if subspace == SUBSPACE_BLOBS {
                blob_hashes.push(key.clone());
            } else if subspace == SUBSPACE_BLOB_LINK && key.len() >= BLOB_HASH_LEN {
                blob_hashes.push(key[..BLOB_HASH_LEN].to_vec());
                // Mail spec 11: once delivery completes the account holds no
                // temporary link (hash | account id | until): no staged blob
                // and no training sample's hold.
                if key.len() == BLOB_HASH_LEN + U32_LEN + U64_LEN
                    && key[BLOB_HASH_LEN..BLOB_HASH_LEN + U32_LEN] == account_id.to_be_bytes()
                {
                    scan.violations
                        .push(format!("{what}: temporary blob link held by the account"));
                }
            }

            // Mail spec 5.4: no threading key of the account (account id |
            // collection | `EmailField::Threading` | hash | document id).
            if (subspace == SUBSPACE_PROPERTY || subspace == SUBSPACE_INDEXES)
                && of_account
                && key.get(U32_LEN) == Some(&(Collection::Email as u8))
                && key.get(U32_LEN + 1) == Some(&u8::from(EmailField::Threading))
            {
                scan.violations
                    .push(format!("{what}: threading key of the account"));
            }

            // Mail spec 9: no spam training sample of the account.
            if subspace == SUBSPACE_SPAM_SAMPLES {
                match PickledStream::new(&value)
                    .and_then(|mut stream| SpamTrainingSample::unpickle(&mut stream))
                {
                    Some(sample) if sample.account_id == Some(Id::from(account_id)) => scan
                        .violations
                        .push(format!("{what}: spam training sample of the account")),
                    Some(_) => {}
                    None => scan
                        .violations
                        .push(format!("{what}: spam training sample does not decode")),
                }
            }

            // Term keys are hashes of tokens, so a canary never appears in
            // them whole: assert structurally that no calendar data of the
            // account was indexed (spec 9) and that the account's mail is
            // indexed by its visible fields only (mail spec 5.5). Key: class
            // (index | type << 6), then the account id (u32 BE) for every
            // index but tracing, which is global (`store/src/write/key.rs`).
            // The account's Email entries prove the key layout is read
            // correctly.
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
                    } else if index == SearchIndex::Email {
                        check_email_search(&key, &value, &what, &mut scan.violations);
                    }
                }
            }

            let Some(archive) = try_archive(&value) else {
                continue;
            };
            scan.archives += 1;
            find_canaries(archive.as_bytes(), &what, "decoded archive", &mut scan);

            // Property key: account_id (u32 BE) | collection | property | document_id (u32 BE).
            if subspace != SUBSPACE_PROPERTY || key.len() != U32_LEN + 2 + U32_LEN || !of_account {
                continue;
            }
            if key[U32_LEN] == Collection::Email as u8
                && key[U32_LEN + 1] == u8::from(EmailField::Metadata)
            {
                check_email(&archive, &what, &mut scan.violations, &mut scan.stubs);
                continue;
            }
            if key[U32_LEN + 1] != archive_field {
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
                find_canaries(&blob, &what, "decoded blob", &mut scan);
            }
            Ok(None) => {}
            Err(err) => scan
                .violations
                .push(format!("{what}: blob does not decode: {err:?}")),
        }
    }

    // The account's messages as stored. A sealed blob (mail spec 5.1) is
    // searched as the ciphertext it is; a plaintext one is decoded too, as
    // transfer encodings would hide a canary from the raw search.
    if let Ok(messages) = test.server.get_cached_messages(account_id).await {
        for message in messages.emails.items.iter() {
            scan.emails += 1;
            let what = format!("email {}", message.document_id);
            let contents = test.fetch_email(account_id, message.document_id).await;
            find_canaries(&contents, &what, "raw message", &mut scan);
            if contents.first() == Some(&MAIL_FORMAT_V1) {
                continue;
            }
            let parsed = MessageParser::new()
                .parse(&contents)
                .unwrap_or_else(|| panic!("{what} does not parse"));
            find_canaries(
                parsed.subject().unwrap_or_default().as_bytes(),
                &what,
                "subject",
                &mut scan,
            );
            for part in parsed.parts.iter() {
                find_canaries(part.contents(), &what, "decoded part", &mut scan);
            }
        }
    }

    // Mail spec 11: once delivery completes nothing for the account is
    // queued (a sub-address is the account's).
    if let Ok(info) = test.server.account_info(account_id).await {
        let mut addresses = info
            .addresses()
            .iter()
            .map(|address| address.to_lowercase())
            .collect::<Vec<_>>();
        addresses.push(info.name().to_lowercase());
        for recipient in queued_recipients(test).await {
            let recipient = recipient.to_lowercase();
            let base = match recipient.split_once('@') {
                Some((local, domain)) => {
                    format!("{}@{domain}", local.split('+').next().unwrap_or(local))
                }
                None => recipient.clone(),
            };
            if addresses.contains(&base) {
                scan.violations
                    .push(format!("queued message for the account ({recipient})"));
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

/// Spec 11, leak tests: a drained invitation, the attendee's copy and the
/// reply leave no canary and no token in the clear anywhere, in either
/// account's records; the reply's visible token is the one exception, and
/// it is gone once the organizer drains.
pub async fn test_drain(test: &mut TestServer) {
    println!("Running zero-access leak test of the drain...");
    let _drain = DrainOn::new();
    let key1 = DummyWebDavClient::new(
        test.account("key1@example.com").id().document_id(),
        "key1@example.com",
        STRONG,
        "key1@example.com",
    );
    let key1_id = key1.account_id;
    let key2_id = test.account("key2@example.com").id().document_id();
    let key2 = DummyWebDavClient::new(key2_id, "key2@example.com", STRONG, "key2@example.com");
    let key1_items = inbox_ids(test, key1_id).await;
    let key2_items = inbox_ids(test, key2_id).await;
    let held = hold_task_consumer(test).await;
    key1.request_with_headers(
        "PUT",
        "/dav/cal/key1%40example.com/default/leak-drain.ics",
        [CONTENT_TYPE],
        DRAIN_EVENT,
    )
    .await
    .with_status(StatusCode::CREATED);
    let (organizer_event, _) = raw_event(test, key1_id, "default/leak-drain.ics").await;
    let keys1 = za_unlock(test, key1_id, STRONG).await;
    let token = *event_token_pairs(&organizer_event, &keys1, key1_id)
        .unwrap()
        .expect("key1's pairs")
        .get("key2@example.com")
        .expect("key2's pair");
    assert_eq!(pending_ids(test, key2_id).await.len(), 1);

    // key2 drains: the copy and the drained item are sealed; the token is
    // in the clear nowhere; the task queue holds nothing of the invitation.
    assert_eq!(trigger_drain("key2@example.com", STRONG).await, 207);
    assert!(pending_ids(test, key2_id).await.is_empty());
    let copy = copy_href(test, key2_id, "leak-drain").await;
    let shown = body(&key2, &copy).await;
    assert!(
        shown.contains("draintitle-canary") && shown.contains("draincn-canary"),
        "{shown}"
    );
    let drained = scan(test, key2_id).await;
    assert!(
        drained.violations.is_empty(),
        "leaks after the drain:\n{}",
        drained.violations.join("\n")
    );
    assert!(drained.events >= 1 && drained.items >= 1, "{drained:?}");
    assert_eq!(drained.item_keys, drained.items, "{drained:?}");
    let organizer_scan = scan(test, key1_id).await;
    assert!(
        organizer_scan.violations.is_empty(),
        "leaks in the organizer's records:\n{}",
        organizer_scan.violations.join("\n")
    );
    assert_eq!(
        itip_tasks_for(test, key1_id).await + itip_tasks_for(test, key2_id).await,
        0
    );
    assert!(
        token_hits(test, &[token]).await.is_empty(),
        "key2's token in the clear after the drain"
    );

    // key2 accepts: the pending reply in key1's inbox carries the token
    // visibly (spec 3.3), the one place it may appear; key1's drain
    // reseals the item without it.
    set_partstat(&key2, &copy, "key2@example.com", "ACCEPTED").await;
    assert_eq!(pending_ids(test, key1_id).await.len(), 1);
    assert!(
        !token_hits(test, &[token]).await.is_empty(),
        "the positive control: the pending reply's visible token"
    );
    let pending_scan = scan(test, key1_id).await;
    assert!(
        pending_scan.violations.is_empty(),
        "leaks while the reply is pending:\n{}",
        pending_scan.violations.join("\n")
    );
    assert_eq!(trigger_drain("key1@example.com", STRONG).await, 207);
    assert!(pending_ids(test, key1_id).await.is_empty());
    assert!(
        token_hits(test, &[token]).await.is_empty(),
        "the token in the clear after the organizer's drain"
    );
    let after_reply = scan(test, key1_id).await;
    assert!(
        after_reply.violations.is_empty(),
        "leaks after the reply:\n{}",
        after_reply.violations.join("\n")
    );
    assert_eq!(
        itip_tasks_for(test, key1_id).await + itip_tasks_for(test, key2_id).await,
        0,
        "the reply reached the task queue"
    );
    drop(held);
    test.wait_for_tasks().await;

    // Cleanup, as `test` does: events with `Schedule-Reply: F`, items by
    // href.
    key2.request_with_headers("DELETE", &copy, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    key1.request_with_headers(
        "DELETE",
        "/dav/cal/key1%40example.com/default/leak-drain.ics",
        [NO_REPLY],
        "",
    )
    .await
    .with_status(StatusCode::NO_CONTENT);
    for (client, account_id, before) in
        [(&key1, key1_id, &key1_items), (&key2, key2_id, &key2_items)]
    {
        for id in inbox_ids(test, account_id).await {
            if !before.contains(&id) {
                client
                    .request(
                        "DELETE",
                        &format!(
                            "/dav/itip/{}/inbox/{id}.ics",
                            client.name.replace('@', "%40")
                        ),
                        "",
                    )
                    .await
                    .with_status(StatusCode::NO_CONTENT);
            }
        }
    }
    test.wait_for_tasks().await;
}

/// Key pinning spec 9, the leak scan: a pin record holding a pin and a
/// withheld entry for a canary-bearing address leaves no canary in the
/// clear in either account's records. A key account's own address is in
/// the registry in the clear, so the canary rides in a sub-address, which
/// pins separately (spec 4.3).
pub async fn test_pins(test: &mut TestServer) {
    println!("Running zero-access leak test of the pin record...");
    const ADDRESS: &str = "key2+pinaddr-canary@example.com";
    let _drain = DrainOn::new();
    let (alice, bob, _) = parties(test);
    let (key1_id, key2_id) = (alice.id, bob.id);
    let bob_start = inbox_ids(test, key2_id).await;
    // alice invites the sub-address (a `FirstUse` pin); key2's key is
    // substituted with invalidation and alice's SUMMARY edit is refused
    // (an entry for that address).
    let (path, original, _) = refuse_once_at(test, &alice, &bob, ADDRESS, "pin-leak").await;
    let record = pin_record(test, key1_id, STRONG).await.expect("the record");
    assert!(
        record.pin(ADDRESS).is_some() && record.withheld.len() >= 1,
        "the positive control"
    );
    for account_id in [key1_id, key2_id] {
        let found = scan(test, account_id).await;
        assert!(
            found.violations.is_empty(),
            "pin record leaks:\n{}",
            found.violations.join("\n")
        );
    }

    // Cleanup: key2's key restored; the event deleted with `Schedule-Reply:
    // F`; key2's new items deleted, and its copy if the drain made one (as
    // `pinning`'s cleanup does: a sub-address invitation may leave none);
    // `pin` for the sub-address drops the entry of the deleted event, then
    // `forget` removes the pin.
    substitute_public_key(test, key2_id, &original, true).await;
    alice
        .client
        .request_with_headers("DELETE", &path, [NO_REPLY], "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    if uid_copies(test, key2_id, "pin-leak").await > 0 {
        let copy = copy_href(test, key2_id, "pin-leak").await;
        bob.client
            .request_with_headers("DELETE", &copy, [NO_REPLY], "")
            .await
            .with_status(StatusCode::NO_CONTENT);
    }
    bob.delete_items_since(test, &bob_start).await;
    drop_entries(ALICE, ADDRESS).await;
    forget(ALICE, Some(ADDRESS)).await.expect(200);
    test.wait_for_tasks().await;
}

/// `test_mail`'s key account, the recipient of its submission (a key account
/// too, so the submitted copy is sealed as well), and the sub-address that
/// carries the recipient canary (the account's address is in the registry
/// in the clear, as `test_pins` notes, so the canary rides in the detail).
const MAIL_KEY: &str = "keymail12@example.com";
const MAIL_RECIPIENT: &str = "keymail13@example.com";
const MAIL_RCPT: &str = "keymail12+mailrcpt-canary@example.com";

/// A message carrying every mail canary: the sender's name and address, the
/// recipient, the subject (`mail-sub canary`, 15 bytes), the Message-ID, a
/// custom header's name and value (the name in the canary's own case: the
/// search is case-sensitive), the preview (the text's opening), the body
/// text, and a base64 attachment's name and content.
fn mail_canary_message(n: u32) -> String {
    format!(
        "From: mailname-canary <mailfrom-canary@example.com>\r\n\
         To: <{MAIL_RCPT}>\r\n\
         Subject: mail-sub canary\r\n\
         Message-ID: <mailmsgid-canary.{n}@example.com>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         X-mailhdrname-canary: mailhdrval-canary\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"za-leak\"\r\n\
         \r\n\
         --za-leak\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         mailpreview-canary opens the text, which goes on to mailbody-canary.\r\n\
         --za-leak\r\n\
         Content-Type: text/plain; name=\"mailattname-canary.txt\"\r\n\
         Content-Disposition: attachment; filename=\"mailattname-canary.txt\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         \r\n\
         {}\r\n\
         --za-leak--\r\n",
        STANDARD.encode("mailattach-canary")
    )
}

/// Deletes every unlinked blob, as the purge does: a delivered message's
/// queue blob is unlinked once delivery completes (mail spec 11, "no queue
/// blob linked after delivery") and stays in the blob store until then.
async fn purge_blobs(test: &TestServer) {
    test.server
        .store()
        .purge_blobs_all_shards(test.server.blob_store().clone())
        .await
        .unwrap();
}

/// The property-subspace key of a message's metadata archive.
fn metadata_key(account_id: u32, document_id: u32) -> Vec<u8> {
    let mut key = account_id.to_be_bytes().to_vec();
    key.push(Collection::Email as u8);
    key.push(u8::from(EmailField::Metadata));
    key.extend_from_slice(&document_id.to_be_bytes());
    key
}

/// Writes `metadata` as the message's metadata archive (field 71).
async fn write_metadata(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
    metadata: MessageMetadata,
) {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Email)
        .with_document(document_id)
        .set(
            EmailField::Metadata,
            Archiver::new(metadata).serialize().unwrap(),
        );
    test.server.store().write(batch.build_all()).await.unwrap();
}

/// Every string a captured event carries, lower-cased.
fn event_text(keys: &[(trc::Key, trc::Value)]) -> String {
    let mut out = Vec::new();
    for (_, value) in keys {
        strings(value, &mut out);
    }
    out.join(" ").to_lowercase()
}

/// Mail spec 11, the leak test's mail extension: a key account receives a
/// message carrying every mail canary over LMTP and appends a second one
/// over IMAP, and a session of the account FETCHes, RETRs and submits it.
/// Then the account's records hold two stubs and no plaintext metadata
/// archive, its blobs and every other record no canary, its search
/// documents the arrival time and the size only, and nothing of it is
/// threaded, queued, held or sampled; no IMAP, POP3 or SMTP event of the
/// session carries a canary. Positive controls: the keyword and the mailbox
/// name the account set are found, visible by structure. Negative controls:
/// a planted plaintext message, and a stub rewritten with plaintext headers
/// alone, are flagged.
pub async fn test_mail(test: &mut TestServer) {
    println!("Running zero-access leak test of mail...");
    let key = key_mail_account(test, MAIL_KEY, STRONG).await;
    let recipient = key_mail_account(test, MAIL_RECIPIENT, STRONG).await;
    let id = key.id().document_id();
    let recipient_id = recipient.id().document_id();

    // Delivered over LMTP to a sub-address, and appended over IMAP.
    SmtpConnection::connect()
        .await
        .ingest("sender@example.org", &[MAIL_RCPT], &mail_canary_message(1))
        .await;
    wait_for_delivery(test).await;
    let mut imap = ImapConnection::connect(b"_l ").await;
    imap.authenticate(MAIL_KEY, STRONG).await;
    imap.append("INBOX", &mail_canary_message(2)).await;
    imap.send("LOGOUT").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    let documents = message_ids(test, id).await;
    assert_eq!(documents.len(), 2, "the delivery and the APPEND");

    // The positive controls: a keyword and a mailbox name (spec 5.6).
    let email_ids = key
        .jmap_method_call("Email/query", json!({ "accountId": key.id_string() }))
        .await
        .ids()
        .map(String::from)
        .collect::<Vec<_>>();
    let reply = key
        .jmap_method_calls(json!([
            ["Mailbox/set", {
                "accountId": key.id_string(),
                "create": { "m": { "name": "mailbox-canary" } }
            }, "m"],
            ["Email/set", {
                "accountId": key.id_string(),
                "update": { (email_ids[0].as_str()): { "keywords/$mailkw-canary": true } }
            }, "e"],
        ]))
        .await;
    assert!(
        reply.pointer("/methodResponses/0/1/created/m/id").is_some()
            && reply
                .pointer(&format!("/methodResponses/1/1/updated/{}", email_ids[0]))
                .is_some(),
        "{reply:?}"
    );

    // A session that FETCHes, RETRs and submits the message, its IMAP, POP3
    // and SMTP events captured.
    let types = EventType::variants()
        .iter()
        .copied()
        .filter(|typ| {
            matches!(
                typ,
                EventType::Imap(_) | EventType::Pop3(_) | EventType::Smtp(_)
            )
        })
        .collect::<Vec<_>>();
    let traced = Traced::start(&types);
    let mut imap = ImapConnection::connect(b"_l ").await;
    imap.authenticate(MAIL_KEY, STRONG).await;
    imap.send("SELECT INBOX").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("FETCH 1:* (BODY.PEEK[])").await;
    let fetched = imap
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .join("\n");
    imap.send("LOGOUT").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    let mut pop3 = Pop3Connection::connect().await;
    pop3.authenticate(MAIL_KEY, STRONG).await;
    pop3.send("RETR 1").await;
    let retrieved = pop3.assert_read(Pop3Reply::Multiline).await.join("\n");
    pop3.send("QUIT").await;
    pop3.assert_read(Pop3Reply::Ok).await;
    let mut smtp = Conn::open_smtp(SUBMISSION_PORT).await;
    smtp.send("EHLO submit.test\r\n").await;
    smtp.until(&["250 "]).await;
    smtp.exchange(
        &format!(
            "AUTH PLAIN {}\r\n",
            STANDARD.encode(format!("\0{MAIL_KEY}\0{STRONG}"))
        ),
        "235",
    )
    .await;
    smtp.exchange(&format!("MAIL FROM:<{MAIL_KEY}>\r\n"), "250")
        .await;
    smtp.exchange(&format!("RCPT TO:<{MAIL_RECIPIENT}>\r\n"), "250")
        .await;
    smtp.exchange("DATA\r\n", "354").await;
    smtp.exchange(&format!("{}\r\n.\r\n", mail_canary_message(3)), "250")
        .await;
    smtp.exchange("QUIT\r\n", "221").await;
    let events = traced.finish().await;
    wait_for_delivery(test).await;
    // The client receives everything.
    for (what, answer) in [("FETCH", &fetched), ("RETR", &retrieved)] {
        for canary in ["mail-sub canary", "mailhdrval-canary", "mailbody-canary"] {
            assert!(answer.contains(canary), "the {what} answer: {answer}");
        }
    }
    for protocol in ["Imap", "Pop3", "Smtp"] {
        assert!(
            events
                .iter()
                .any(|event| format!("{:?}", event.typ).starts_with(protocol)),
            "no {protocol} event captured"
        );
    }
    for event in &events {
        let text = event_text(&event.keys);
        for canary in all_canaries() {
            assert!(
                !text.contains(canary),
                "{:?} carries {canary}: {}",
                event.typ,
                event.text
            );
        }
    }

    // The scan: two stubs, nothing in the clear, the controls found.
    purge_blobs(test).await;
    let result = scan(test, id).await;
    println!(
        "Mail leak scan: {} records, {} archives, {} blobs, {} search index records ({} of the account), {} emails, {} stubs, {} visible hits",
        result.records,
        result.archives,
        result.blobs,
        result.search_records,
        result.search_account_records,
        result.emails,
        result.stubs,
        result.found.len()
    );
    assert!(
        result.violations.is_empty(),
        "mail leaks:\n{}",
        result.violations.join("\n")
    );
    assert_eq!(result.stubs, 2, "{result:?}");
    assert_eq!(result.emails, 2, "{result:?}");
    assert!(
        result.search_account_records > 0,
        "no search entry of the account: {result:?}"
    );
    for canary in VISIBLE_CANARIES {
        assert!(
            result.found.iter().any(|hit| hit.ends_with(canary)),
            "the positive control {canary} was not found: {result:?}"
        );
    }
    // The submitted copy is sealed for its recipient.
    let submitted = scan(test, recipient_id).await;
    assert!(
        submitted.violations.is_empty(),
        "leaks of the submitted copy:\n{}",
        submitted.violations.join("\n")
    );
    assert_eq!(submitted.stubs, 1, "{submitted:?}");

    // Negative control: a plaintext message, as an account that held mail
    // before it became a key account holds one (spec 6.6).
    let planted = plant_plain_message(
        test,
        id,
        "From: Planted <planted@example.org>\r\nSubject: planted\r\n\r\nmailbody-canary, planted in the clear\r\n",
    )
    .await;
    test.wait_for_tasks().await;
    let found = scan(test, id).await;
    let planted_key = format!("{:?}", metadata_key(id, planted));
    let report = found.violations.join("\n");
    assert!(
        found
            .violations
            .iter()
            .any(|v| v.contains(&planted_key) && v.ends_with("plaintext metadata archive")),
        "the scanner must flag the planted metadata archive:\n{report}"
    );
    assert!(
        found
            .violations
            .iter()
            .any(|v| v.starts_with("blob ") && v.ends_with("decoded blob contains mailbody-canary")),
        "the scanner must find the planted blob's canary:\n{report}"
    );
    let planted_id = Id::from_parts(planted, planted).to_string();
    let reply = key
        .jmap_method_call(
            "Email/set",
            json!({ "accountId": key.id_string(), "destroy": [&planted_id] }),
        )
        .await;
    assert_eq!(
        reply.method_response()["destroyed"],
        json!([planted_id]),
        "{reply:?}"
    );
    test.wait_for_tasks().await;
    purge_blobs(test).await;

    // Negative control: a stub rewritten with a plaintext header block alone
    // (its blob still sealed) is flagged by structure.
    let document = documents[0];
    let original = stored_metadata(test, id, document)
        .await
        .deserialize::<MessageMetadata>()
        .unwrap();
    let mut rewritten = stored_metadata(test, id, document)
        .await
        .deserialize::<MessageMetadata>()
        .unwrap();
    rewritten.raw_headers = b"Subject: rewritten\r\n\r\n".to_vec().into_boxed_slice();
    write_metadata(test, id, document, rewritten).await;
    let found = scan(test, id).await;
    let rewritten_key = format!("{:?}", metadata_key(id, document));
    assert_eq!(
        found.violations,
        vec![format!(
            "subspace {:?} key {rewritten_key}: plaintext metadata archive",
            char::from(SUBSPACE_PROPERTY)
        )],
        "the scanner must flag the rewritten stub, and only it"
    );
    assert_eq!(found.stubs, 1);
    write_metadata(test, id, document, original).await;
    let restored = scan(test, id).await;
    assert!(restored.violations.is_empty(), "{:?}", restored.violations);
    assert_eq!(restored.stubs, 2);

    // Cleanup: the messages and the mailbox, and their blobs.
    for account in [&key, &recipient] {
        test.destroy_all_mailboxes(account).await;
    }
    wait_for_delivery(test).await;
    purge_blobs(test).await;
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
