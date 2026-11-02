/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Spec 11: decodes every stored record and asserts the sealing boundary.
//! A regression test, not a proof.
//!
//! At 1.0 the walk is keyed on the `Subspace` enum (`Subspace::ALL`), never
//! on subspace bytes: `m`, `y` and `u` changed meaning between 0.16 and 1.0
//! (Task 6). Each subspace that can hold a key account's data has a stated
//! rule for what the account may hold there in the clear; every record of
//! every subspace is also searched for the canaries, raw and decoded:
//!
//! - `Property` (`p`): the calendar, event and inbox-item records (the
//!   structural checks below), and the object-metadata containers
//!   (`Collection::Metadata`): no `Private` container (ruling 66), on the
//!   account's objects or of the account on another owner's (the walk reads
//!   every owner's `Private` records whose viewer is the account); a
//!   calendar's or event's `Shared` container is a sealed wrapper
//!   (`is_sealed_container`; rulings 16, 23 and 29); an address book's,
//!   card's or file's `Shared` container holds DAV dead properties, which
//!   DAV keeps in the clear for contacts and files (design, "Other object
//!   kinds"); any other `Shared` container (a mailbox's, message's or Sieve
//!   script's, a server-level IMAP annotation) is object metadata, refused
//!   for a key account (ruling 66); the `Viewer` and `Owner` counters are
//!   well formed (an 11-byte key naming a collection, a 12-byte value that
//!   decodes as a `ViewerState`: a change id and a container count,
//!   `merge_viewer_state`) and count no container. Names are never in
//!   metadata keys, so the rule reads the class byte, not a name.
//! - `Immutable` (`c`): a message's metadata row is a stub (mail spec 5.2),
//!   exactly the row `seal_stub` writes for its bundle and blob; its sort
//!   keys are the empty ones `za_ingest` writes (`MessageSortKeys::default`);
//!   nothing else of the account is stored there (`ValueClass::Immutable`
//!   holds only `EmailField::Metadata` and `SortKeys`).
//! - `IndexProperty` (`v`): no threading hash (mail spec 5.4); a mailbox's
//!   name index (its parent and lower-cased name: visible by structure, mail
//!   spec 5.6, reported in `Scan::found`), a Sieve script's seen-id hashes
//!   (keyed hashes of an id, `SeenIdHash`) and a submission's integer
//!   indexes (send time, queue id; values hold ids and the undo status, and
//!   the delivery results' recipients and replies) are permitted; any other
//!   index property of the account is unclassified and flagged (the
//!   writers are `email` and `merge_threads` only, by source).
//! - `SearchTerm` (`z`) and `SearchDocument` (`m`), in the internal search
//!   store: no Calendar entry of the account (calendar spec 9); no Email
//!   term, and every Email document holds its ids only (ruling 64:
//!   `IDS_ONLY_DOCUMENT`; 1.0's Email index has no arrival-time or size
//!   field); Contacts and File entries are permitted (ruling 65: their
//!   builders index plaintext for every account, as at 0.16).
//! - `SearchQueue` (`y`, in the data store): the account's queue entries
//!   carry ids only (the key: index, account, document, creation id; the
//!   value: one action byte, 0 or 1); a partition's queued-work marker is
//!   empty and its status key decodes as a `SearchIndexStatus` (a retry
//!   time, an attempt count and an error reason, searched for canaries).
//! - `System` (`u`): the schema version (`0x00`), the node-id leases
//!   (`0x01 || node id`) and the UID leases (`ZA_UID_LEASE_PREFIX ||
//!   account || SHA-256 of the UID`, ruling 24) only; any other record is
//!   unclassified and flagged. None names content.
//! - `BlobLink`, `Blobs`, `SpamSamples`: as at 0.16 (no temporary link held
//!   once delivery completes, every blob searched decoded, no spam sample
//!   of the account).

use super::{
    STRONG,
    dav_seal::{CANARIES, EVENT, mkcalendar_body, raw_event, raw_event_records},
    fanout::{NO_REPLY, uid_copies},
    mail::{Traced, key_mail_account, message_ids, stored_metadata_bytes},
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
            Conn, IDS_ONLY_DOCUMENT, PlantedItem, copy_href, hold_index_partition,
            hold_task_consumer, inbox_ids, internal_search, itip_tasks_for, new_inbox_items,
            pending_ids, pin_record, plain_dav_container, plant_container, plant_event,
            plant_inbox_item, plant_plain_message, plant_sealed_event, queued_recipients,
            release_index_partition, substitute_public_key, trigger_drain, wait_for_delivery,
            za_unlock,
        },
    },
    za::drain::{DrainOn, body, set_partstat},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD},
};
use calcard::icalendar::{ICalendar, ICalendarEntry, ICalendarProperty};
use common::storage::search::SearchIndexStatus;
use email::message::{
    metadata::MetadataRow,
    sortkeys::MessageSortKeys,
    za::{MAIL_FORMAT_V1, is_stub, stub_structure},
};
use groupware::cache::GroupwareCache;
use groupware::calendar::{
    Calendar, CalendarEvent, CalendarEventContent, CalendarEventNotification,
    CalendarEventNotificationContent, EVENT_NOTIFICATION_ZA_PENDING, Timezone,
    seal::{
        TokenPairs, ZaEvent,
        collection::COLLECTION_MARKER,
        event_token_pairs, is_sealed_container,
        item::{RSVP_TOKEN_PROP, item_message},
        policy::{is_visible_entry, is_visible_parameter},
        tree::{EXTRA_PROP, KEY_PROP, SEALED_PROP},
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
use std::collections::{BTreeMap, BTreeSet};
use store::{
    Deserialize as _, IterateParams, Serialize as _, Subspace, U32_LEN, U64_LEN, ValueKey,
    ZA_UID_LEASE_PREFIX,
    write::{
        AnyKey, Archive, ArchiveBytes, Archiver, BatchBuilder, IndexPropertyClass, QueueDocumentId,
        SearchIndex, SearchIndexClass, ValueClass,
        metadata::{MetadataClass, ViewerState},
        serialize::RawValue,
    },
};
use trc::EventType;
use types::{
    blob_hash::BLOB_HASH_LEN,
    collection::{Collection, SyncCollection},
    field::{
        CalendarEventField, CalendarNotificationField, EmailField, EmailSubmissionField, Field,
        MailboxField, SieveField,
    },
    id::Id,
};
use utils::{cheeky_hash::CheekyHash, hash128::Hash128};

const CONTENT_TYPE: (&str, &str) = ("content-type", "text/calendar; charset=utf-8");
const EVENT_PATH: &str = "/dav/cal/key2%40example.com/leak/e.ics";

/// Canaries written by this module, on top of `dav_seal::CANARIES` (which
/// `EVENT` carries).
const EXTRA_CANARIES: &[&str] = &[
    "shared-owner-name-canary",
    "shared-grantee-name-canary",
    "shared-colour-canary",
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
/// Largest decompressed size the scanner accepts for a value that merely
/// looks like an LZ4 archive, so arbitrary bytes cannot ask for gigabytes.
const MAX_DECODED_LEN: usize = 64 * 1024 * 1024;

/// `MetadataClass::Shared` and `Private`'s key bytes (`store/src/write/
/// metadata.rs`, crate-private there); `VIEWER` and `OWNER` are public.
const METADATA_SHARED: u8 = 0;
const METADATA_PRIVATE: u8 = 1;
/// The length of a `Viewer` or `Owner` counter (`ViewerState`: a change id
/// and a container count).
const METADATA_COUNTER_LEN: usize = U64_LEN + U32_LEN;

/// The `System` subspace's kinds (`SystemKind`, `store/src/write/key.rs`).
const SYSTEM_SCHEMA_VERSION: u8 = 0x00;
const SYSTEM_NODE_ID: u8 = 0x01;
/// A UID lease's key length (`common::za::lease::LEASE_KEY_LEN`).
const LEASE_KEY_LEN: usize = 1 + U32_LEN + 32;

/// A search-queue entry's key length: the index byte, the partition and the
/// document id (u32 BE) and the creation id (u64 BE).
const QUEUE_ENTRY_LEN: usize = 1 + U32_LEN * 2 + U64_LEN;

/// What one scan saw. The counters let the caller prove the scanner was
/// not blind: a wrong key layout would decode nothing and find nothing.
#[derive(Debug, Default)]
pub struct Scan {
    pub violations: Vec<String>,
    pub records: usize,
    pub archives: usize,
    pub blobs: usize,
    /// Records of the internal search store's `SearchTerm` and
    /// `SearchDocument` subspaces, and those of the account.
    pub search_records: usize,
    pub search_account_records: usize,
    /// The account's search-queue entries, markers and status keys.
    pub queue_records: usize,
    /// The account's object-metadata records (containers and counters).
    pub metadata_records: usize,
    /// The account's `IndexProperty` and `Immutable` records.
    pub index_properties: usize,
    pub immutables: usize,
    pub events: usize,
    pub calendars: usize,
    pub items: usize,
    pub item_keys: usize,
    pub item_extras: usize,
    pub emails: usize,
    /// Message metadata rows of the account that are stubs.
    pub stubs: usize,
    /// Hits of `VISIBLE_CANARIES`, which are not violations.
    pub found: Vec<String>,
    /// Temporary blob links held by the account and queued messages for it
    /// (mail spec 11: no staged blob, sample hold or queue blob once
    /// delivery completes). Violations only once the caller has waited for
    /// delivery (`wait_for_delivery`), which the scan cannot know: a scan
    /// taken while mail is in flight sees them legitimately.
    pub undelivered: Vec<String>,
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

/// A subspace's character in a violation's text (`'p'`, `'c'`, ...).
fn subspace_char(subspace: Subspace) -> char {
    char::from(subspace.byte())
}

/// The key of a per-document value of the account (`Property`,
/// `Immutable`): account id (u32 BE) | collection | field | document id
/// (u32 BE).
pub fn document_key(
    account_id: u32,
    collection: Collection,
    field: u8,
    document_id: u32,
) -> Vec<u8> {
    let mut key = account_id.to_be_bytes().to_vec();
    key.push(collection as u8);
    key.push(field);
    key.extend_from_slice(&document_id.to_be_bytes());
    key
}

/// The `Property` key of an archive (`Field::ARCHIVE`).
fn archive_key(account_id: u32, collection: Collection, document_id: u32) -> Vec<u8> {
    document_key(
        account_id,
        collection,
        u8::from(Field::ARCHIVE),
        document_id,
    )
}

/// The `Immutable` key of a message's metadata row (`EmailField::Metadata`).
fn metadata_row_key(account_id: u32, document_id: u32) -> Vec<u8> {
    document_key(
        account_id,
        Collection::Email,
        u8::from(EmailField::Metadata),
        document_id,
    )
}

/// Mail spec 5.2 and 6.1: the metadata row (`EmailField::Metadata`, in the
/// immutable class at 1.0) of a key account's message is a stub: its header
/// section is the sealed bundle (the format byte first) and its structure is
/// exactly `stub_structure` for that bundle and the sealed blob, the row
/// `seal_stub` writes: one binary root part, no preview, no attachment, no
/// header entry. Counts the stubs.
fn check_email(value: &[u8], what: &str, violations: &mut Vec<String>, stubs: &mut usize) {
    let row = match MetadataRow::deserialize(value) {
        Ok(row) => row,
        Err(err) => {
            violations.push(format!("{what}: metadata row does not decode: {err:?}"));
            return;
        }
    };
    if !is_stub(&row).unwrap_or(false) {
        violations.push(format!("{what}: plaintext metadata row"));
        return;
    }
    *stubs += 1;
    let rebuilt =
        row.raw_headers()
            .ok()
            .zip(row.unarchive().ok())
            .and_then(|(bundle, metadata)| {
                MetadataRow::encode(
                    stub_structure(metadata.blob_hash(), bundle.len(), metadata.blob_size()),
                    &bundle,
                )
                .ok()
            });
    if rebuilt.as_deref() != Some(value) {
        violations.push(format!(
            "{what}: stub whose structure is not the sealed stub's"
        ));
    }
}

/// Mail spec 5.4 and the `IndexProperty` rule (module documentation): the
/// account's index properties, keyed account id | collection | property |
/// kind | ... (`store/src/write/key.rs`).
fn check_index_property(key: &[u8], what: &str, violations: &mut Vec<String>) {
    let collection = key.get(U32_LEN).copied().map(Collection::from);
    let property = key.get(U32_LEN + 1).copied();
    let kind = key.get(U32_LEN + 2).copied();
    let hash = Some(IndexPropertyClass::KIND_HASH);
    let integer = Some(IndexPropertyClass::KIND_INTEGER);
    match (collection, property, kind) {
        (Some(Collection::Email), Some(p), k)
            if p == u8::from(EmailField::Threading) && k == hash =>
        {
            violations.push(format!("{what}: threading key of the account"))
        }
        (Some(Collection::Mailbox), Some(p), k)
            if p == u8::from(MailboxField::Name) && k == hash => {}
        (Some(Collection::SieveScript), Some(p), k)
            if p == u8::from(SieveField::SeenId) && k == hash => {}
        (Some(Collection::EmailSubmission), Some(p), k)
            if (p == u8::from(EmailSubmissionField::Metadata)
                || p == u8::from(EmailSubmissionField::DeliveryResult))
                && k == integer => {}
        _ => violations.push(format!(
            "{what}: unclassified index property of the account"
        )),
    }
}

/// The `Immutable` rule: the account's message rows are stubs and its sort
/// keys empty; nothing else of the account is immutable.
fn check_immutable(key: &[u8], value: &[u8], what: &str, scan: &mut Scan) {
    let is_email = key.get(U32_LEN) == Some(&(Collection::Email as u8));
    let field = key.get(U32_LEN + 1).copied();
    if is_email && field == Some(u8::from(EmailField::Metadata)) {
        check_email(value, what, &mut scan.violations, &mut scan.stubs);
    } else if is_email && field == Some(u8::from(EmailField::SortKeys)) {
        if value != MessageSortKeys::default().serialize().as_slice() {
            scan.violations
                .push(format!("{what}: sort keys with content"));
        }
    } else {
        scan.violations.push(format!(
            "{what}: unclassified immutable record of the account"
        ));
    }
}

/// The search store's rules (module documentation). A key of an
/// account-partitioned index starts with the index byte and the account id
/// (u32 BE); a `Document` key then holds the document id, a `Term` key the
/// term group, the term, its length, the field and the block id. Tracing
/// keys are global and carry no account.
fn check_search(
    subspace: Subspace,
    key: &[u8],
    value: &[u8],
    account_id: u32,
    what: &str,
    scan: &mut Scan,
) {
    scan.search_records += 1;
    let Some(index) = key.first().copied().and_then(SearchIndex::try_from_u8) else {
        scan.violations
            .push(format!("{what}: search record of an unknown index"));
        return;
    };
    if index == SearchIndex::Tracing
        || key.get(1..1 + U32_LEN) != Some(account_id.to_be_bytes().as_slice())
    {
        return;
    }
    scan.search_account_records += 1;
    match (index, subspace) {
        (SearchIndex::Calendar, _) => scan.violations.push(format!(
            "{what}: calendar search index entry of the account"
        )),
        (SearchIndex::Email, Subspace::SearchTerm) => scan
            .violations
            .push(format!("{what}: search term of the account")),
        (SearchIndex::Email, _) => {
            if key.len() != 1 + U32_LEN * 2 {
                scan.violations
                    .push(format!("{what}: search document key shape"));
            } else if value != IDS_ONLY_DOCUMENT {
                scan.violations
                    .push(format!("{what}: search document with terms"));
            }
        }
        // Ruling 65: the Contacts and File builders index plaintext for
        // every account, as at 0.16.
        _ => {}
    }
}

/// The `SearchQueue` rule (module documentation): an entry's key is the
/// index byte, the partition, the document id and the creation id, and its
/// value the action; a control key is `QUEUE_CONTROL`, the index byte, the
/// partition and the kind.
fn check_search_queue(key: &[u8], value: &[u8], account_id: u32, what: &str, scan: &mut Scan) {
    let account = account_id.to_be_bytes();
    if key.first() == Some(&SearchIndexClass::QUEUE_CONTROL) {
        let index = key.get(1).copied().and_then(SearchIndex::try_from_u8);
        if index.is_none_or(|index| index == SearchIndex::Tracing)
            || key.get(2..2 + U32_LEN) != Some(account.as_slice())
        {
            return;
        }
        scan.queue_records += 1;
        match key.get(2 + U32_LEN).copied() {
            Some(SearchIndexClass::CONTROL_INDEX) if key.len() == U32_LEN + 3 => {
                if !value.is_empty() {
                    scan.violations
                        .push(format!("{what}: queue marker with a value"));
                }
            }
            Some(SearchIndexClass::CONTROL_STATUS) if key.len() == U32_LEN + 3 => {
                if SearchIndexStatus::deserialize(value).is_err() {
                    scan.violations
                        .push(format!("{what}: queue status does not decode"));
                }
            }
            _ => scan
                .violations
                .push(format!("{what}: unclassified queue control key")),
        }
        return;
    }
    let index = key.first().copied().and_then(SearchIndex::try_from_u8);
    if index.is_none_or(|index| index == SearchIndex::Tracing)
        || key.get(1..1 + U32_LEN) != Some(account.as_slice())
    {
        return;
    }
    scan.queue_records += 1;
    if key.len() != QUEUE_ENTRY_LEN {
        scan.violations
            .push(format!("{what}: queue entry key shape"));
    }
    if !matches!(value, [0] | [1]) {
        scan.violations
            .push(format!("{what}: queue entry carries more than its action"));
    }
}

/// The `System` rule (module documentation).
fn check_system(key: &[u8], what: &str, violations: &mut Vec<String>) {
    match key.first().copied() {
        Some(SYSTEM_SCHEMA_VERSION) | Some(SYSTEM_NODE_ID) => {}
        Some(ZA_UID_LEASE_PREFIX) if key.len() == LEASE_KEY_LEN => {}
        _ => violations.push(format!("{what}: unclassified system record")),
    }
}

/// An object-metadata record of the account (`Collection::Metadata` in the
/// `Property` subspace): account id | `Collection::Metadata` | class byte,
/// then for `Shared` the collection and the document id, for `Private` the
/// viewer, the collection and the document id, for `Viewer` the collection
/// and the viewer, for `Owner` the owner and the collection.
struct MetadataRecord {
    what: String,
    key: Vec<u8>,
    value: Vec<u8>,
    /// A `Private` record of another owner whose viewer is the account.
    foreign: bool,
}

/// A metadata key's length for `Shared`, `Viewer` and `Owner` records
/// (account | `Collection::Metadata` | class | collection and document,
/// collection and viewer, or owner and collection).
const METADATA_KEY_LEN: usize = U32_LEN + 3 + U32_LEN;
/// A `Private` record's key length (owner | `Collection::Metadata` | class
/// | viewer | collection | document).
const METADATA_PRIVATE_KEY_LEN: usize = U32_LEN * 3 + 3;

/// Whether `key` (a `Property` record) is another owner's `Private`
/// container whose viewer is `account_id`.
fn is_foreign_private(key: &[u8], account_id: u32) -> bool {
    key.len() == METADATA_PRIVATE_KEY_LEN
        && key.get(U32_LEN) == Some(&(Collection::Metadata as u8))
        && key.get(U32_LEN + 1) == Some(&METADATA_PRIVATE)
        && key.get(U32_LEN + 2..U32_LEN * 2 + 2) == Some(account_id.to_be_bytes().as_slice())
}

/// The object-metadata rule (module documentation). A `Shared` container is
/// read through the server's container reader, which decompresses it.
async fn check_metadata(
    test: &TestServer,
    account_id: u32,
    record: &MetadataRecord,
    scan: &mut Scan,
) {
    let what = record.what.as_str();
    let key = record.key.as_slice();
    scan.metadata_records += 1;
    if record.foreign {
        scan.violations.push(format!(
            "{what}: private metadata container of a key account on another account's object"
        ));
        return;
    }
    match key.get(U32_LEN + 1).copied() {
        Some(METADATA_SHARED) if key.len() == U32_LEN + 3 + U32_LEN => {
            let collection = Collection::from(key[U32_LEN + 2]);
            let document_id = u32::from_be_bytes(key[U32_LEN + 3..].try_into().unwrap());
            match collection {
                Collection::Calendar | Collection::CalendarEvent => {
                    match test
                        .server
                        .metadata_container(account_id, collection, document_id)
                        .await
                    {
                        Ok(Some(container)) => {
                            let bytes = container.view().as_bytes().to_vec();
                            find_canaries(&bytes, what, "decoded container", scan);
                            if !is_sealed_container(&bytes) {
                                scan.violations
                                    .push(format!("{what}: unsealed shared container"));
                            }
                        }
                        Ok(None) => {}
                        Err(err) => scan
                            .violations
                            .push(format!("{what}: shared container does not decode: {err:?}")),
                    }
                }
                // DAV dead properties of contacts and files, in the clear.
                Collection::AddressBook | Collection::ContactCard | Collection::FileNode => {}
                // A mailbox's, message's or Sieve script's container, a
                // server-level IMAP annotation (`Collection::Principal`) or
                // any other: object metadata, refused for a key account.
                _ => scan
                    .violations
                    .push(format!("{what}: object metadata of a key account")),
            }
        }
        Some(METADATA_PRIVATE) => scan.violations.push(format!(
            "{what}: private metadata container of a key account"
        )),
        Some(class @ (MetadataClass::VIEWER | MetadataClass::OWNER)) => {
            // `Viewer`: collection | viewer; `Owner`: owner | collection.
            let collection = match class {
                MetadataClass::VIEWER => key.get(U32_LEN + 2),
                _ => key.get(U32_LEN * 2 + 2),
            }
            .map(|collection| Collection::from(*collection));
            let state = (record.value.len() == METADATA_COUNTER_LEN)
                .then(|| ViewerState::deserialize(&record.value).ok())
                .flatten();
            match (key.len(), collection, state) {
                (METADATA_KEY_LEN, Some(collection), Some(state))
                    if !matches!(collection, Collection::None | Collection::Metadata) =>
                {
                    if state.containers > 0 {
                        scan.violations.push(format!(
                            "{what}: metadata counter of private containers ({} counted)",
                            state.containers
                        ));
                    }
                }
                _ => scan
                    .violations
                    .push(format!("{what}: metadata counter shape")),
            }
        }
        _ => scan
            .violations
            .push(format!("{what}: unclassified metadata record")),
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
                    // Field policy 2 (upgrade design, "Privacy flags"): the
                    // predicate takes the entry, since CLASS is visible only
                    // with a standard value on VEVENT, VTODO and VJOURNAL.
                    if !is_visible_entry(&component.component_type, entry) {
                        violations.push(format!(
                            "{what}: component {index} ({:?}) has sealed-class property {:?}",
                            component.component_type, entry.name
                        ));
                    }
                    // The scanner's own CLASS rule, independent of the
                    // product's predicate: a visible CLASS holds exactly one
                    // standard classification, on an event, task or journal.
                    if entry.name == calcard::icalendar::ICalendarProperty::Class
                        && !(matches!(
                            component.component_type,
                            calcard::icalendar::ICalendarComponentType::VEvent
                                | calcard::icalendar::ICalendarComponentType::VTodo
                                | calcard::icalendar::ICalendarComponentType::VJournal
                        ) && matches!(
                            entry.values.as_slice(),
                            [calcard::icalendar::ICalendarValue::Classification(
                                calcard::icalendar::ICalendarClassification::Public
                                    | calcard::icalendar::ICalendarClassification::Private
                                    | calcard::icalendar::ICalendarClassification::Confidential
                            )]
                        ))
                    {
                        violations.push(format!(
                            "{what}: component {index} has a visible non-standard CLASS {:?}",
                            entry.values
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

/// An event of a key account at 1.0 (Task 6: the 1.0 event pair,
/// `ZaEvent`): the row holds no display name (it rides in the sealed
/// bundle), the content no per-account preference (decision 7, part 4c),
/// and the content's tree is sealed. Dead properties left the records at
/// 1.0; the object-metadata rule checks their containers.
fn check_event(event: &ZaEvent, what: &str, violations: &mut Vec<String>) {
    if event.meta.display_name.is_some() {
        violations.push(format!("{what}: display_name not empty"));
    }
    if !event.content.preferences.is_empty() {
        violations.push(format!("{what}: event preferences not empty"));
    }
    check_tree(&event.content.data.event, what, true, violations);
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

/// An inbox item (invitations spec 9) at 1.0: a row and a content record
/// (ruling 13). The content is `Itip` with no plaintext snapshot
/// (`item_message`), and its message tree ends with `X-ZA-KEY` and, before
/// it, at most one of `X-ZA-RSVP-TOKEN` (visible, a pending reply) or
/// `X-ZA-EXTRA`, and neither once drained (pending bit clear); the rest as
/// `check_tree` checks any component. Returns whether the item carries its
/// key envelope.
fn check_item(
    item: &CalendarEventNotification,
    content: &CalendarEventNotificationContent,
    what: &str,
    violations: &mut Vec<String>,
) -> bool {
    let Some(message) = item_message(content) else {
        violations.push(format!(
            "{what}: inbox item with a plaintext snapshot or upstream content"
        ));
        return false;
    };
    let mut tree = message.clone();
    let has_key = tree
        .components
        .first()
        .and_then(|root| root.entries.last())
        .is_some_and(|e| carrier_kind(e) == Some(KEY_PROP));
    if !has_key {
        violations.push(format!("{what}: missing {KEY_PROP}"));
    } else {
        let pending = item.flags & EVENT_NOTIFICATION_ZA_PENDING != 0;
        let expected = if pending { (2, 106) } else { (1, 74) };
        if !key_envelope(message)
            .is_some_and(|b| b.len() == expected.1 && b[..2] == [1, expected.0])
        {
            violations.push(format!("{what}: inbox envelope state mismatch"));
        }
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
            if !pending {
                violations.push(format!("{what}: drained item carries {carrier}"));
            }
            root.pop();
        }
    }
    check_tree(&tree, what, false, violations);
    has_key
}

/// Keyless correspondence only: this does not authenticate encrypted ring payloads,
/// the owner ledger, keys, or associated data.
type CalendarHeaders = BTreeMap<u32, Option<groupware::calendar::sharing::format::OwnerRingHeader>>;

fn key_envelope(tree: &ICalendar) -> Option<Vec<u8>> {
    let entry = tree.components.first()?.entries.last()?;
    if carrier_kind(entry) != Some(KEY_PROP) || entry.values.len() != 1 {
        return None;
    }
    STANDARD.decode(entry.values[0].as_text()?).ok()
}

fn check_event_domain(
    event: &ZaEvent,
    calendars: &CalendarHeaders,
    what: &str,
    violations: &mut Vec<String>,
) {
    let envelope = key_envelope(&event.content.data.event);
    let names = &event.meta.names;
    if names.is_empty() {
        violations.push(format!("{what}: event parent calendar missing"));
    }
    for name in names {
        let Some(header) = calendars.get(&name.parent_id) else {
            violations.push(format!("{what}: event parent calendar missing"));
            continue;
        };
        if let Some(header) = header {
            if names.len() != 1 {
                violations.push(format!("{what}: shared event must have one parent"));
            }
            match envelope.as_deref() {
                Some(bytes) if bytes.len() == 82 && bytes[..2] == [1, 3] => {
                    let calendar = u32::from_be_bytes(bytes[2..6].try_into().unwrap());
                    let generation = u32::from_be_bytes(bytes[6..10].try_into().unwrap());
                    if calendar != name.parent_id {
                        violations.push(format!("{what}: shared event calendar mismatch"));
                    }
                    if !header.generations.contains(&generation) {
                        violations.push(format!("{what}: shared event generation not listed"));
                    }
                }
                _ => violations.push(format!("{what}: shared event requires CK envelope")),
            }
        } else if !envelope
            .as_deref()
            .is_some_and(|bytes| bytes.len() == 74 && bytes[..2] == [1, 1])
        {
            violations.push(format!("{what}: unshared event requires account envelope"));
        }
    }
}

fn check_calendar(
    calendar: &Calendar,
    account_id: u32,
    calendar_id: u32,
    key_accounts: &std::collections::BTreeSet<u32>,
    what: &str,
    violations: &mut Vec<String>,
) -> Option<groupware::calendar::sharing::format::OwnerRingHeader> {
    use groupware::calendar::sharing::{
        format::{parse_grant_header, parse_owner_header},
        mutations::ZaShareLevel,
    };
    let mut principals = std::collections::BTreeSet::new();
    for p in &calendar.preferences {
        if !principals.insert(p.account_id) {
            violations.push(format!("{what}: duplicate preference principal"));
        }
    }
    let Some(pref) = calendar
        .preferences
        .iter()
        .find(|p| p.account_id == account_id)
    else {
        violations.push(format!("{what}: collection without owner preferences"));
        return None;
    };
    if !pref.name.starts_with(COLLECTION_MARKER) {
        if calendar.preferences.iter().any(|p| {
            p.description.is_some()
                || p.color.is_some()
                || matches!(p.time_zone, Timezone::Custom(_))
        }) || calendar.preferences.len() != 1
            || !calendar.acls.is_empty()
        {
            violations.push(format!("{what}: unsealed collection with user data"));
        }
        return None;
    }
    if calendar
        .preferences
        .iter()
        .any(|p| p.description.is_some() || p.color.is_some())
    {
        violations.push(format!("{what}: sealed collection with plaintext fields"));
    }
    for p in &calendar.preferences {
        if let Timezone::Custom(tz) = &p.time_zone {
            check_tree(tz, what, false, violations);
        }
    }
    let parts = pref
        .name
        .strip_prefix(COLLECTION_MARKER)
        .unwrap()
        .split('|')
        .collect::<Vec<_>>();
    let bytes = STANDARD.decode(parts[0]).unwrap_or_default();
    if parts.len() == 2 && bytes.len() == 74 && bytes[..2] == [1, 1] {
        if calendar.preferences.len() != 1 || !calendar.acls.is_empty() {
            violations.push(format!(
                "{what}: unshared collection with foreign preferences or ACL"
            ));
        }
        return None;
    }
    if parts.len() != 5 {
        violations.push(format!("{what}: owner carrier shape"));
        return None;
    }
    let header = match parse_owner_header(&bytes) {
        Ok(h) => h,
        Err(_) => {
            violations.push(format!("{what}: owner ring header malformed"));
            return None;
        }
    };
    if header.calendar_id != calendar_id {
        violations.push(format!("{what}: owner ring calendar mismatch"));
    }
    let mut acl_principals = std::collections::BTreeSet::new();
    for acl in &calendar.acls {
        if !acl_principals.insert(acl.account_id) {
            violations.push(format!("{what}: duplicate ACL principal"));
        }
        if !key_accounts.contains(&acl.account_id) {
            violations.push(format!(
                "{what}: ACL principal is not a current key account"
            ));
        }
        if acl.account_id == account_id || ZaShareLevel::from_bitmap(acl.grants).is_none() {
            violations.push(format!("{what}: shared ACL bits"));
        }
    }
    principals.remove(&account_id);
    if principals != acl_principals {
        violations.push(format!("{what}: shared preference/ACL mismatch"));
    }
    for entry in &calendar.preferences {
        let Some(text) = entry.name.strip_prefix(COLLECTION_MARKER) else {
            violations.push(format!("{what}: grant carrier shape"));
            continue;
        };
        let entry_parts = text.split('|').collect::<Vec<_>>();
        if entry.account_id != account_id {
            if entry_parts.len() != 3 {
                violations.push(format!("{what}: grant carrier shape"));
                continue;
            }
            let grant = STANDARD
                .decode(entry_parts[0])
                .ok()
                .and_then(|b| parse_grant_header(&b).ok());
            if !grant.is_some_and(|g| {
                g.calendar_id == calendar_id
                    && g.domain_id == header.domain_id
                    && g.grantee_id == entry.account_id
                    && g.generations == header.generations
                    && g.rename_change_id == header.rename_change_id
                    && (!header.boundary_pending || g.boundary_pending)
            }) {
                violations.push(format!("{what}: grant header mismatch"));
            }
        }
        for index in if entry.account_id == account_id {
            vec![1, 3]
        } else {
            vec![1]
        } {
            let envelope = entry_parts.get(index).and_then(|p| STANDARD.decode(p).ok());
            if !envelope.is_some_and(|b| {
                b.len() == 82
                    && b[..2] == [1, 3]
                    && u32::from_be_bytes(b[2..6].try_into().unwrap()) == calendar_id
                    && header
                        .generations
                        .contains(&u32::from_be_bytes(b[6..10].try_into().unwrap()))
            }) {
                violations.push(format!("{what}: shared bundle envelope mismatch"));
            }
            if !entry_parts
                .get(index + 1)
                .and_then(|p| STANDARD.decode(p).ok())
                .is_some_and(|b| b.len() >= 41 && b[0] == 1)
            {
                violations.push(format!("{what}: sealed bundle shape"));
            }
        }
    }
    Some(header)
}

/// Decodes a value as a stored archive if it carries the archive marker.
/// An LZ4 archive starts with its decompressed size (little endian u32);
/// values that only look like one and claim a huge size are skipped. At 1.0
/// the store's decoder checks every trailer length itself
/// (`validate_marker_and_contents`), so a short value is not an archive.
fn try_archive(value: &[u8]) -> Option<Archive<ArchiveBytes>> {
    let marker = *value.last()?;
    if marker & ARCHIVE_MAGIC_MARKER == 0 {
        return None;
    }
    if marker & ARCHIVE_LZ4_COMPRESSED != 0 {
        let size = u32::from_le_bytes(value.get(..U32_LEN)?.try_into().ok()?) as usize;
        if size > MAX_DECODED_LEN {
            return None;
        }
    }
    <Archive<ArchiveBytes> as store::Deserialize>::deserialize(value).ok()
}

/// Every record of `subspace`, from the store that holds it: the internal
/// search store for `SearchTerm` and `SearchDocument` (none without one),
/// the data store for every other subspace.
async fn subspace_records(test: &TestServer, subspace: Subspace) -> Vec<(Vec<u8>, Vec<u8>)> {
    let store = match subspace {
        Subspace::SearchTerm | Subspace::SearchDocument => {
            match test.server.search_store().internal_fts() {
                Some(store) => store.clone(),
                None => return Vec::new(),
            }
        }
        _ => test.server.store().clone(),
    };
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
    records
}

/// The two records of a split document (1.0: an event or an inbox item is
/// a row, `Field::ARCHIVE`, and a content archive), as the walk finds them.
#[derive(Default)]
struct SplitRecords {
    row: Option<(String, Archive<ArchiveBytes>)>,
    content: Option<(String, Archive<ArchiveBytes>)>,
}

/// Walks every subspace and the account's mail. Returns what it saw and the
/// violations found for `account_id`.
pub async fn scan(test: &TestServer, account_id: u32) -> Scan {
    let store = test.server.store().clone();
    let archive_field = u8::from(Field::ARCHIVE);
    let event_content_field = u8::from(CalendarEventField::Content);
    let item_content_field = u8::from(CalendarNotificationField::Content);
    let mut scan = Scan::default();
    let mut blob_hashes: Vec<Vec<u8>> = Vec::new();
    // Build the complete owner/calendar/header map before validating any event.
    let mut calendar_records = Vec::new();
    store
        .iterate(
            IterateParams::new(
                AnyKey {
                    subspace: Subspace::Property,
                    key: archive_key(account_id, Collection::Calendar, 0),
                },
                AnyKey {
                    subspace: Subspace::Property,
                    key: archive_key(account_id, Collection::Calendar, u32::MAX),
                },
            ),
            |key, value| {
                calendar_records.push((key.to_vec(), value.to_vec()));
                Ok(true)
            },
        )
        .await
        .unwrap();
    let mut stored_calendars = Vec::new();
    for (key, value) in calendar_records {
        let id = u32::from_be_bytes(key[key.len() - U32_LEN..].try_into().unwrap());
        // A malformed archive is scanner evidence, not a reason to panic before
        // the negative-control owner can restore its exact bytes.
        let calendar = try_archive(&value)
            .and_then(|archive| archive.deserialize_untrusted::<Calendar>().ok());
        if let Some(calendar) = calendar {
            stored_calendars.push((id, calendar));
        } else {
            scan.violations.push(format!(
                "calendar {account_id}/{id}: calendar archive does not decode"
            ));
        }
    }
    let mut key_accounts = BTreeSet::new();
    for (_, calendar) in &stored_calendars {
        for acl in &calendar.acls {
            if groupware::calendar::sharing::mutations::za_share_key_account(
                &test.server,
                acl.account_id,
            )
            .await
            .unwrap()
            {
                key_accounts.insert(acl.account_id);
            }
        }
    }
    let mut calendar_headers = CalendarHeaders::new();
    for (id, calendar) in &stored_calendars {
        let what = format!("calendar {account_id}/{id}");
        calendar_headers.insert(
            *id,
            check_calendar(
                calendar,
                account_id,
                *id,
                &key_accounts,
                &what,
                &mut scan.violations,
            ),
        );
    }

    let mut events: BTreeMap<u32, SplitRecords> = BTreeMap::new();
    let mut items: BTreeMap<u32, SplitRecords> = BTreeMap::new();
    let mut metadata = Vec::new();
    for &subspace in Subspace::ALL {
        for (key, value) in subspace_records(test, subspace).await {
            scan.records += 1;
            let what = format!("subspace {:?} key {:?}", subspace_char(subspace), key);
            find_canaries(&key, &what, "key", &mut scan);
            find_canaries(&value, &what, "raw value", &mut scan);
            let of_account = key.get(..U32_LEN) == Some(account_id.to_be_bytes().as_slice());

            match subspace {
                // Blob contents are decoded after the walk, through the
                // server's configured blob store: the data store's own blob
                // subspace, or a filesystem or S3 store whose blobs only the
                // link records name (a link key starts with the blob hash).
                Subspace::Blobs => blob_hashes.push(key.clone()),
                Subspace::BlobLink if key.len() >= BLOB_HASH_LEN => {
                    blob_hashes.push(key[..BLOB_HASH_LEN].to_vec());
                    // Mail spec 11: once delivery completes the account
                    // holds no temporary link (hash | account id | until):
                    // no staged blob and no training sample's hold.
                    if key.len() == BLOB_HASH_LEN + U32_LEN + U64_LEN
                        && key[BLOB_HASH_LEN..BLOB_HASH_LEN + U32_LEN] == account_id.to_be_bytes()
                    {
                        scan.undelivered
                            .push(format!("{what}: temporary blob link held by the account"));
                    }
                }
                Subspace::IndexProperty if of_account => {
                    scan.index_properties += 1;
                    check_index_property(&key, &what, &mut scan.violations);
                }
                Subspace::Immutable if of_account => {
                    scan.immutables += 1;
                    check_immutable(&key, &value, &what, &mut scan);
                }
                // Mail spec 9: no spam training sample of the account.
                Subspace::SpamSamples => {
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
                // Term keys hold a token's `CheekyHash`, the token itself
                // for one of up to 16 bytes, so a long canary never appears
                // in them whole: the rules are structural (calendar spec 9,
                // mail spec 5.5, rulings 64 and 65). The account's Email
                // documents prove the key layout is read correctly.
                Subspace::SearchTerm | Subspace::SearchDocument => {
                    check_search(subspace, &key, &value, account_id, &what, &mut scan);
                }
                Subspace::SearchQueue => {
                    check_search_queue(&key, &value, account_id, &what, &mut scan);
                }
                Subspace::System => check_system(&key, &what, &mut scan.violations),
                Subspace::Property
                    if of_account && key.get(U32_LEN) == Some(&(Collection::Metadata as u8)) =>
                {
                    metadata.push(MetadataRecord {
                        what: what.clone(),
                        key: key.clone(),
                        value: value.clone(),
                        foreign: false,
                    });
                }
                // The account's private containers on another owner's
                // objects (ruling 66's caller rule).
                Subspace::Property if !of_account && is_foreign_private(&key, account_id) => {
                    metadata.push(MetadataRecord {
                        what: what.clone(),
                        key: key.clone(),
                        value: value.clone(),
                        foreign: true,
                    });
                }
                _ => {}
            }

            let Some(archive) = try_archive(&value) else {
                continue;
            };
            scan.archives += 1;
            find_canaries(archive.as_bytes(), &what, "decoded archive", &mut scan);

            // Property key: account_id (u32 BE) | collection | field | document_id (u32 BE).
            if subspace != Subspace::Property || key.len() != U32_LEN + 2 + U32_LEN || !of_account {
                continue;
            }
            let collection = Collection::from(key[U32_LEN]);
            let field = key[U32_LEN + 1];
            let document_id = u32::from_be_bytes(key[U32_LEN + 2..].try_into().unwrap());
            match collection {
                Collection::CalendarEvent if field == archive_field => {
                    events.entry(document_id).or_default().row = Some((what, archive));
                }
                Collection::CalendarEvent if field == event_content_field => {
                    events.entry(document_id).or_default().content = Some((what, archive));
                }
                Collection::CalendarEventNotification if field == archive_field => {
                    items.entry(document_id).or_default().row = Some((what, archive));
                }
                Collection::CalendarEventNotification if field == item_content_field => {
                    items.entry(document_id).or_default().content = Some((what, archive));
                }
                Collection::Calendar if field == archive_field => {
                    scan.calendars += 1;
                    if let Err(err) = archive.deserialize_untrusted::<Calendar>() {
                        scan.violations
                            .push(format!("{what}: calendar archive does not decode: {err:?}"));
                    }
                }
                _ => {}
            }
        }
    }

    // Events: the row and the content, checked as one `ZaEvent`.
    for (document_id, records) in events {
        scan.events += 1;
        match (records.row, records.content) {
            (Some((what, row)), Some((_, content))) => {
                match (
                    row.deserialize::<CalendarEvent>(),
                    content.deserialize::<CalendarEventContent>(),
                ) {
                    (Ok(meta), Ok(content)) => {
                        let event = ZaEvent::new(meta, content);
                        check_event(&event, &what, &mut scan.violations);
                        check_event_domain(&event, &calendar_headers, &what, &mut scan.violations);
                    }
                    (Err(err), _) | (_, Err(err)) => scan
                        .violations
                        .push(format!("{what}: event records do not decode: {err:?}")),
                }
            }
            (Some((what, _)), None) | (None, Some((what, _))) => scan.violations.push(format!(
                "{what}: event {account_id}/{document_id} has one of its two records"
            )),
            (None, None) => {}
        }
    }

    // Inbox items: the row and the content, as one item.
    for (document_id, records) in items {
        scan.items += 1;
        match (records.row, records.content) {
            (Some((what, row)), Some((_, content))) => {
                match (
                    row.deserialize::<CalendarEventNotification>(),
                    content.deserialize::<CalendarEventNotificationContent>(),
                ) {
                    (Ok(item), Ok(content)) => {
                        if check_item(&item, &content, &what, &mut scan.violations) {
                            scan.item_keys += 1;
                        }
                        if item_message(&content).is_some_and(ends_extra_then_key) {
                            scan.item_extras += 1;
                        }
                    }
                    (Err(err), _) | (_, Err(err)) => scan
                        .violations
                        .push(format!("{what}: inbox item records do not decode: {err:?}")),
                }
            }
            (Some((what, _)), None) | (None, Some((what, _))) => scan.violations.push(format!(
                "{what}: inbox item {account_id}/{document_id} has one of its two records"
            )),
            (None, None) => {}
        }
    }

    // Object metadata of the account.
    for record in &metadata {
        check_metadata(test, account_id, record, &mut scan).await;
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

    // The account's messages as stored: each one's blob, named by its
    // metadata row. A sealed blob (mail spec 5.1) is searched as the
    // ciphertext it is; a plaintext one is decoded too, as transfer
    // encodings would hide a canary from the raw search.
    for document_id in cached_message_ids(test, account_id).await {
        scan.emails += 1;
        let what = format!("email {document_id}");
        let Some(row) = test
            .server
            .store()
            .get_value::<RawValue>(ValueKey::immutable(
                account_id,
                Collection::Email,
                document_id,
                EmailField::Metadata,
            ))
            .await
            .unwrap()
            .and_then(|value| MetadataRow::deserialize(&value.0).ok())
        else {
            scan.violations
                .push(format!("{what}: message without a metadata row"));
            continue;
        };
        let Ok(metadata) = row.unarchive() else {
            continue;
        };
        let contents = match test
            .server
            .blob_store()
            .get_blob(metadata.blob_hash().as_slice(), 0..usize::MAX)
            .await
        {
            Ok(Some(contents)) => contents,
            Ok(None) => continue,
            Err(err) => {
                scan.violations
                    .push(format!("{what}: message blob does not decode: {err:?}"));
                continue;
            }
        };
        find_canaries(&contents, &what, "raw message", &mut scan);
        if contents.first() == Some(&MAIL_FORMAT_V1) {
            continue;
        }
        // Mail spec 5.1, 11: every message blob opens only with the keys.
        scan.violations
            .push(format!("{what}: plaintext message blob"));
        let parsed = MessageParser::new()
            .parse(&contents)
            .unwrap_or_else(|| panic!("{what} does not parse"));
        find_canaries(
            parsed.subject().unwrap_or_default().as_bytes(),
            &what,
            "subject",
            &mut scan,
        );
        for part in parsed.parts() {
            find_canaries(&part.decoded(), &what, "decoded part", &mut scan);
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
                scan.undelivered
                    .push(format!("queued message for the account ({recipient})"));
            }
        }
    }
    scan
}

/// The account's message document ids from the message cache, empty for an
/// account without one (a destroyed account).
async fn cached_message_ids(test: &TestServer, account_id: u32) -> Vec<u32> {
    use email::cache::MessageCacheFetch;
    match test.server.get_cached_messages(account_id).await {
        Ok(messages) => {
            let mut ids = messages.emails.document_ids().collect::<Vec<_>>();
            ids.sort_unstable();
            ids
        }
        Err(_) => Vec::new(),
    }
}

/// Where any of `tokens` appears in the clear (invitations spec 11): as raw
/// bytes, base64url without padding (the `X-ZA-RSVP-TOKEN` form), standard
/// base64 or standard base64 without padding, in a raw key, a raw value, a
/// decoded archive or a decoded blob. Returns (subspace, key, token index)
/// per hit; a blob hit carries `Subspace::Blobs` and the blob hash. A blob
/// that fails to read panics: it must not go unsearched unnoticed.
pub async fn token_hits(test: &TestServer, tokens: &[[u8; 16]]) -> Vec<(Subspace, Vec<u8>, usize)> {
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
    let mut hits = Vec::new();
    let mut blob_hashes: Vec<Vec<u8>> = Vec::new();
    for &subspace in Subspace::ALL {
        for (key, value) in subspace_records(test, subspace).await {
            let mut found = find(&key);
            found.extend(find(&value));
            if let Some(archive) = try_archive(&value) {
                found.extend(find(archive.as_bytes()));
            }
            if subspace == Subspace::Blobs {
                blob_hashes.push(key.clone());
            } else if subspace == Subspace::BlobLink && key.len() >= BLOB_HASH_LEN {
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
                    .map(|i| (Subspace::Blobs, hash.clone(), i)),
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
                crate::utils::za::delete_inbox_item(test, client, account_id, id).await;
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

/// Writes `row` as the message's metadata row (field 71, immutable at 1.0).
async fn write_metadata_row(test: &TestServer, account_id: u32, document_id: u32, row: Vec<u8>) {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Email)
        .with_document(document_id)
        .set(EmailField::Metadata, row);
    test.server.store().write_batch(&mut batch).await.unwrap();
}

/// `test_mail`'s ordinary account, the rules' positive control.
const MAIL_PLAIN: &str = "plainmail26@example.com";

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
/// row, its blobs and every other record no canary, its search documents
/// their ids only (ruling 64), its sort keys none, and nothing of it is
/// threaded, queued, held or sampled; no IMAP, POP3 or SMTP event of the
/// session carries a canary. Positive controls: the keyword and the mailbox
/// name the account set are found, visible by structure; an ordinary
/// account that receives a message is flagged by every mail rule, so the
/// rules read 1.0's real records. Negative controls: a planted plaintext
/// message, and a stub rewritten with plaintext headers alone, are flagged.
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
    // Delivery has completed (`wait_for_delivery` above).
    assert!(
        result.undelivered.is_empty(),
        "held or queued after delivery:\n{}",
        result.undelivered.join("\n")
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
        submitted.violations.is_empty() && submitted.undelivered.is_empty(),
        "leaks of the submitted copy:\n{}\n{}",
        submitted.violations.join("\n"),
        submitted.undelivered.join("\n")
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
    let planted_key = format!("{:?}", metadata_row_key(id, planted));
    let report = found.violations.join("\n");
    assert!(
        found
            .violations
            .iter()
            .any(|v| v.contains(&planted_key) && v.ends_with("plaintext metadata row")),
        "the scanner must flag the planted metadata row:\n{report}"
    );
    assert!(
        found
            .violations
            .iter()
            .any(|v| v.starts_with("blob ") && v.ends_with("decoded blob contains mailbody-canary")),
        "the scanner must find the planted blob's canary:\n{report}"
    );
    assert!(
        found
            .violations
            .contains(&format!("email {planted}: plaintext message blob")),
        "the scanner must flag the planted message's plaintext blob:\n{report}"
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
    // At 1.0 the row is re-encoded around a plaintext header section with
    // the stub's own structure, so only the header section changes.
    let document = documents[0];
    let original = stored_metadata_bytes(test, id, document).await;
    let headers = b"Subject: rewritten\r\n\r\n";
    let rewritten = {
        let row = MetadataRow::deserialize(&original).unwrap();
        let metadata = row.unarchive().unwrap();
        MetadataRow::encode(
            stub_structure(metadata.blob_hash(), headers.len(), metadata.blob_size()),
            headers,
        )
        .unwrap()
    };
    write_metadata_row(test, id, document, rewritten).await;
    let found = scan(test, id).await;
    let rewritten_key = format!("{:?}", metadata_row_key(id, document));
    assert_eq!(
        found.violations,
        vec![format!(
            "subspace {:?} key {rewritten_key}: plaintext metadata row",
            subspace_char(Subspace::Immutable)
        )],
        "the scanner must flag the rewritten stub, and only it"
    );
    assert_eq!(found.stubs, 1);
    write_metadata_row(test, id, document, original).await;
    let restored = scan(test, id).await;
    assert!(restored.violations.is_empty(), "{:?}", restored.violations);
    assert_eq!(restored.stubs, 2);

    // Positive control: an ordinary account's message, delivered and
    // indexed by 1.0's ordinary paths, is flagged by every mail rule.
    let plain = test
        .account("admin@example.com")
        .create_user_account(
            MAIL_PLAIN,
            STRONG,
            "Plain mail",
            &[],
            super::user_permissions(),
        )
        .await;
    let plain_id = plain.id().document_id();
    SmtpConnection::connect()
        .await
        .ingest(
            "sender@example.org",
            &[MAIL_PLAIN],
            "From: Ordinary <ordinary@example.org>\r\nSubject: ordinary control\r\n\r\nordinary body text\r\n",
        )
        .await;
    wait_for_delivery(test).await;
    let ordinary = scan(test, plain_id).await;
    let report = ordinary.violations.join("\n");
    for rule in [
        "plaintext metadata row",
        "sort keys with content",
        "threading key of the account",
        "search term of the account",
        "search document with terms",
        "plaintext message blob",
    ] {
        assert!(
            ordinary.violations.iter().any(|v| v.ends_with(rule)),
            "the ordinary account's records do not trip the rule {rule:?}:\n{report}"
        );
    }
    assert!(
        ordinary.search_account_records > 0 && ordinary.immutables > 0,
        "{ordinary:?}"
    );

    // Cleanup: the messages and the mailbox, and their blobs.
    for account in [&key, &recipient, &plain] {
        test.destroy_all_mailboxes(account).await;
    }
    wait_for_delivery(test).await;
    test.account("admin@example.com")
        .destroy_account(plain)
        .await;
    test.wait_for_tasks().await;
    purge_blobs(test).await;
}

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access leak regression test...");
    test_shared_structure(test).await;
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
    // Both of 1.0's records, as one `ZaEvent` (row and content); the
    // scanner's `check_event` takes the pair at 1.0 (Task 6).
    let (organizer_event, _) = raw_event_records(test, key1_id, "default/fo-leak.ics").await;
    let mut organizer_violations = Vec::new();
    let organizer = organizer_event.za_event();
    check_event(&organizer, "key1 fo-leak.ics", &mut organizer_violations);
    assert!(
        organizer_violations.is_empty(),
        "the organizer's event:\n{}",
        organizer_violations.join("\n")
    );
    assert!(
        ends_extra_then_key(&organizer.content.data.event),
        "the organizer's root does not end {EXTRA_PROP} then {KEY_PROP}"
    );
    // The fan-out's token for key2 is in the clear nowhere, the held task
    // queue included: its item carries it in the sealed bundle, and no
    // reply item exists here.
    let keys1 = za_unlock(test, key1_id, STRONG).await;
    let fan_out_token = *event_token_pairs(&organizer_event.content, &keys1, key1_id)
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
    // The alarm email was delivered above: no held or queued mail remains.
    assert!(
        result.undelivered.is_empty(),
        "held or queued mail after delivery:\n{}",
        result.undelivered.join("\n")
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
    // `raw_event` returns the content record, which holds the tree at 1.0.
    let (paired, _) = raw_event(test, id, "leak/pairs.ics").await;
    let paired = paired
        .deserialize::<groupware::calendar::CalendarEventContent>()
        .unwrap();
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
    // At 1.0 the reply's message tree, and so its visible token, is in the
    // item's content record (ruling 13).
    let reply_key = document_key(
        id,
        Collection::CalendarEventNotification,
        u8::from(CalendarNotificationField::Content),
        reply_id,
    );
    let on_reply = |subspace: Subspace, key: &[u8], token: usize| {
        subspace == Subspace::Property && key == reply_key.as_slice() && token == 1
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
        plant_event(
            test,
            id,
            "leak",
            "planted.ics",
            &EVENT.replace("summary-canary", "negative-canary"),
            None,
        )
        .await;
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
        crate::utils::za::delete_inbox_item(test, &client, id, negative).await;
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
        crate::utils::za::delete_inbox_item(test, &client, id, item).await;
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

/// Positive and planted-violation controls for the scanner's rules over
/// 1.0's spaces, with records planted through the store's own key
/// serializer: each new space has a record that its rule flags and one it
/// permits, and every record is the scanned account's except where the
/// control is another account's. The Email partition of the account is held
/// while the queue entries are planted, so the index worker never reads
/// them.
pub async fn test_index_control(test: &mut TestServer) {
    println!("Running zero-access leak scanner index control...");
    // `leak::test` ends by deleting key2's calendars and mailboxes without
    // waiting: their index-queue entries may still be pending. Drain them so
    // the baseline below is stable.
    test.wait_for_tasks().await;
    let id = test.account("key2@example.com").id().document_id();
    let plain_id = test.account("plain@example.com").id().document_id();
    let document_id = u32::MAX - 1;
    let term = |index, account_id| {
        ValueClass::SearchIndex(SearchIndexClass::Term {
            index,
            account_id,
            field: 0,
            term: CheekyHash::new(b"za-control"),
            block_id: 0,
        })
    };
    let document = |index, account_id, document_id| {
        ValueClass::SearchIndex(SearchIndexClass::Document {
            index,
            account_id,
            document_id,
        })
    };
    // (class, value, the violation it trips, or `None`), in the internal
    // search store.
    let search_plants = [
        (
            document(SearchIndex::Calendar, id, document_id),
            IDS_ONLY_DOCUMENT.to_vec(),
            Some("calendar search index entry of the account"),
        ),
        (
            term(SearchIndex::Calendar, id),
            vec![],
            Some("calendar search index entry of the account"),
        ),
        (
            term(SearchIndex::Email, id),
            vec![],
            Some("search term of the account"),
        ),
        // One term listed: not ids only.
        (
            document(SearchIndex::Email, id, document_id),
            vec![1, 0, 0],
            Some("search document with terms"),
        ),
        // Permitted: an ids-only Email document of the account (ruling 64),
        // a Contacts term of the account (ruling 65), another account's
        // Calendar document.
        (
            document(SearchIndex::Email, id, document_id - 1),
            IDS_ONLY_DOCUMENT.to_vec(),
            None,
        ),
        (term(SearchIndex::Contacts, id), vec![], None),
        (
            document(SearchIndex::Calendar, plain_id, document_id),
            IDS_ONLY_DOCUMENT.to_vec(),
            None,
        ),
    ];
    let queue_entry = |created_at| {
        ValueClass::SearchIndex(SearchIndexClass::Queue {
            index: SearchIndex::Email,
            id_prefix: id,
            id_suffix: QueueDocumentId::Assigned(document_id),
            created_at,
        })
    };
    let index_property = |property: u8| {
        ValueClass::IndexProperty(IndexPropertyClass::Hash {
            property,
            hash: Hash128::from(0x5a5a_u128),
        })
    };
    let sort_keys = MessageSortKeys {
        subject: "control".into(),
        ..Default::default()
    }
    .serialize();
    // A `ViewerState` (change id, then container count, big-endian) that
    // counts one container.
    let mut counting_counter = vec![0u8; U64_LEN];
    counting_counter.extend_from_slice(&1u32.to_be_bytes());
    // (collection, class, value, the violation it trips, or `None`), in
    // the data store, keyed on the account and `document_id`.
    let data_plants = [
        (
            Collection::Email,
            queue_entry(1),
            vec![1, 0xAA],
            Some("queue entry carries more than its action"),
        ),
        (Collection::Email, queue_entry(2), vec![0], None),
        (
            Collection::Email,
            index_property(u8::from(EmailField::Threading)),
            vec![],
            Some("threading key of the account"),
        ),
        (
            Collection::Mailbox,
            index_property(u8::from(MailboxField::Name)),
            vec![],
            None,
        ),
        (
            Collection::Email,
            ValueClass::Immutable(u8::from(EmailField::SortKeys)),
            sort_keys,
            Some("sort keys with content"),
        ),
        (
            Collection::Email,
            ValueClass::Immutable(u8::from(EmailField::SortKeys)),
            MessageSortKeys::default().serialize(),
            None,
        ),
        (
            Collection::Calendar,
            ValueClass::Metadata(MetadataClass::Private { viewer: plain_id }),
            vec![1, 2, 3],
            Some("private metadata container of a key account"),
        ),
        (
            Collection::Calendar,
            ValueClass::Metadata(MetadataClass::Viewer { viewer: plain_id }),
            vec![0; METADATA_COUNTER_LEN],
            None,
        ),
        (
            Collection::Calendar,
            ValueClass::Metadata(MetadataClass::Owner { owner: plain_id }),
            vec![0; 5],
            Some("metadata counter shape"),
        ),
        // A well-formed counter that counts one private container of the
        // account on `plain`'s mailboxes (the Task 6 review's F2).
        (
            Collection::Mailbox,
            ValueClass::Metadata(MetadataClass::Owner { owner: plain_id }),
            counting_counter.clone(),
            Some("metadata counter of private containers (1 counted)"),
        ),
        // Shared containers (the review's F8): a mailbox's and a
        // server-level annotation's are flagged by their collection, an
        // address book's (DAV dead properties) is permitted.
        (
            Collection::Mailbox,
            ValueClass::Metadata(MetadataClass::Shared),
            vec![1, 2, 3],
            Some("object metadata of a key account"),
        ),
        (
            Collection::Principal,
            ValueClass::Metadata(MetadataClass::Shared),
            vec![1, 2, 3],
            Some("object metadata of a key account"),
        ),
        (
            Collection::AddressBook,
            ValueClass::Metadata(MetadataClass::Shared),
            vec![1, 2, 3],
            None,
        ),
        (
            Collection::None,
            ValueClass::Any(store::write::AnyClass {
                subspace: Subspace::System,
                key: vec![0x7F, 0x5A],
            }),
            vec![],
            Some("unclassified system record"),
        ),
    ];
    // The account's private container on `plain`'s mailbox, stored under
    // `plain` (the review's F2: the walk reads other owners' records).
    const FOREIGN_RULE: &str =
        "private metadata container of a key account on another account's object";
    let write_foreign = async |set: bool| {
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(plain_id)
            .with_collection(Collection::Mailbox)
            .with_document(document_id);
        let class = ValueClass::Metadata(MetadataClass::Private { viewer: id });
        if set {
            batch.set(class, vec![1, 2, 3]);
        } else {
            batch.clear(class);
        }
        test.server.store().write_batch(&mut batch).await.unwrap();
    };
    let expected = search_plants
        .iter()
        .filter_map(|(_, _, rule)| *rule)
        .chain(data_plants.iter().filter_map(|(_, _, _, rule)| *rule))
        .chain([FOREIGN_RULE])
        .collect::<Vec<_>>();

    let before = scan(test, id).await;
    assert!(before.violations.is_empty(), "{:?}", before.violations);
    hold_index_partition(test, SearchIndex::Email, id).await;
    // The two sort-key plants share one key: the flagged one is written in
    // a second batch, after the permitted one is scanned with the rest.
    let write = async |plants: &[(Collection, ValueClass, Vec<u8>, Option<&str>)], set: bool| {
        let mut batch = BatchBuilder::new();
        for (collection, class, value, _) in plants {
            batch
                .with_account_id(id)
                .with_collection(*collection)
                .with_document(document_id);
            if set {
                batch.set(class.clone(), value.clone());
            } else {
                batch.clear(class.clone());
            }
        }
        test.server.store().write_batch(&mut batch).await.unwrap();
    };
    let search = internal_search(test);
    let mut batch = BatchBuilder::new();
    for (class, value, _) in &search_plants {
        batch.set(class.clone(), value.clone());
    }
    search.write_batch(&mut batch).await.unwrap();
    let (flagged_sort_keys, rest): (Vec<_>, Vec<_>) =
        data_plants
            .iter()
            .cloned()
            .partition(|(_, class, _, rule)| {
                matches!(class, ValueClass::Immutable(_)) && rule.is_some()
            });
    write(&rest, true).await;
    write_foreign(true).await;

    // The permitted sort keys: every rule but that one's trips.
    let result = scan(test, id).await;
    let mut tripped = expected.clone();
    tripped.retain(|rule| *rule != "sort keys with content");
    for rule in &tripped {
        assert!(
            result.violations.iter().any(|v| v.ends_with(rule)),
            "the planted violation {rule:?} was not flagged: {:?}",
            result.violations
        );
    }
    assert_eq!(
        result.violations.len(),
        tripped.len(),
        "unexpected violations (a permitted plant was flagged): {:?}",
        result.violations
    );
    assert_eq!(
        result.search_account_records,
        before.search_account_records + 6,
        "the six search entries of the account were not all counted"
    );
    // The two entries and the hold's status key.
    assert_eq!(result.queue_records, before.queue_records + 3, "{result:?}");

    // The flagged sort keys over the permitted ones.
    write(&flagged_sort_keys, true).await;
    let result = scan(test, id).await;
    assert!(
        result
            .violations
            .iter()
            .any(|v| v.ends_with("sort keys with content")),
        "the planted sort keys were not flagged: {:?}",
        result.violations
    );
    assert_eq!(
        result.violations.len(),
        expected.len(),
        "{:?}",
        result.violations
    );

    let mut batch = BatchBuilder::new();
    for (class, _, _) in search_plants {
        batch.clear(class);
    }
    search.write_batch(&mut batch).await.unwrap();
    write(&rest, false).await;
    write_foreign(false).await;
    release_index_partition(&test.server, SearchIndex::Email, id).await;
    test.wait_for_tasks().await;
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

/// Keep a real multi-generation shared calendar alive while inspecting storage.
/// Each bad archive is restored before any assertion can abort this fixture.
async fn test_shared_structure(test: &mut TestServer) {
    use crate::utils::za::za_post;
    use calcard::icalendar::ICalendarValue;
    use types::acl::AclGrant;
    const OWNER: &str = "leak-shared-owner@example.com";
    const BOB: &str = "leak-shared-bob@example.com";
    const CAROL: &str = "leak-shared-carol@example.com";
    const HOME: &str = "/dav/cal/leak-shared-owner%40example.com/";
    const PATH: &str = "/dav/cal/leak-shared-owner%40example.com/shared/";
    for name in [OWNER, BOB, CAROL] {
        let account = test
            .account("admin@example.com")
            .create_key_user_account(name, STRONG, "Leak sharing", &[], super::user_permissions())
            .await;
        test.insert_account(account);
    }
    let owner = test.account(OWNER).id().document_id();
    let bob = test.account(BOB).id().document_id();
    let carol = test.account(CAROL).id().document_id();
    let alice = DummyWebDavClient::new(owner, OWNER, STRONG, OWNER);
    let reader = DummyWebDavClient::new(bob, BOB, STRONG, BOB);
    alice.request("MKCALENDAR", PATH, "<A:mkcalendar xmlns:A=\"urn:ietf:params:xml:ns:caldav\" xmlns:D=\"DAV:\" xmlns:I=\"http://apple.com/ns/ical/\"><D:set><D:prop><D:displayname>shared-owner-name-canary</D:displayname><I:calendar-color>shared-colour-canary</I:calendar-color></D:prop></D:set></A:mkcalendar>").await.with_status(StatusCode::CREATED);
    for (name, id) in [(BOB, bob), (CAROL, carol)] {
        let public = test.server.za_served_public_key(id).await.unwrap().unwrap();
        super::pinning::pin(OWNER, name, &vault::keys::display_fingerprint(&public))
            .await
            .expect(200);
        za_post("shares/grant", &json!({"username":OWNER,"password":STRONG,"calendar":"shared","address":name,"level":"edit"})).await.expect(200);
    }
    alice
        .request_with_headers(
            "PUT",
            &format!("{PATH}g1.ics"),
            [CONTENT_TYPE],
            EVENT.replace("UID:", "UID:shared-g1-"),
        )
        .await
        .with_status(StatusCode::CREATED);
    za_post(
        "shares/revoke",
        &json!({"username":OWNER,"password":STRONG,"calendar":"shared","grant":carol}),
    )
    .await
    .expect(200);
    alice
        .request_with_headers(
            "PUT",
            &format!("{PATH}g2.ics"),
            [CONTENT_TYPE],
            EVENT.replace("UID:", "UID:shared-g2-"),
        )
        .await
        .with_status(StatusCode::CREATED);
    reader
        .proppatch(
            PATH,
            [("D:displayname", "shared-grantee-name-canary")],
            [],
            [],
        )
        .await
        .with_status(StatusCode::MULTI_STATUS);
    for (client, name) in [
        (&alice, "shared-owner-name-canary"),
        (&reader, "shared-grantee-name-canary"),
    ] {
        let response = client.request("PROPFIND", PATH, "<D:propfind xmlns:D=\"DAV:\" xmlns:I=\"http://apple.com/ns/ical/\"><D:prop><D:displayname/><I:calendar-color/></D:prop></D:propfind>").await.with_status(StatusCode::MULTI_STATUS);
        let body = response.body.unwrap();
        assert!(
            body.contains(name)
                && body.contains("shared-colour-canary")
                && body.contains("http://apple.com/ns/ical/"),
            "personal name and actual Apple shared colour must be returned"
        );
    }
    let resources = test
        .server
        .fetch_groupware_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap();
    let calendar = resources.by_path("shared").unwrap().document_id();
    let event_id = resources.by_path("shared/g1.ics").unwrap().document_id();
    let original: Calendar = test
        .server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            owner,
            Collection::Calendar,
            calendar,
        ))
        .await
        .unwrap()
        .unwrap()
        .deserialize()
        .unwrap();
    // At 1.0 an event is a row (its names) and a content record (its tree).
    let (original_records, _) = raw_event_records(test, owner, "shared/g1.ics").await;
    let original_event = original_records
        .meta
        .deserialize::<CalendarEvent>()
        .unwrap();
    let original_content = original_records
        .content
        .deserialize::<CalendarEventContent>()
        .unwrap();
    let archive_field = u8::from(Field::ARCHIVE);
    let content_field = u8::from(CalendarEventField::Content);
    // A malformed archive must return diagnostics and permit exact restoration.
    shared_control(
        test,
        owner,
        Collection::Calendar,
        calendar,
        archive_field,
        Archiver::with_compression(1u8, store::write::Compression::None)
            .untrusted()
            .serialize()
            .unwrap(),
        "malformed calendar archive",
        "calendar archive does not decode",
    )
    .await;
    alice
        .request(
            "MKCALENDAR",
            &format!("{HOME}unshared/"),
            mkcalendar_body(&[("D:displayname", "Unshared control")]),
        )
        .await
        .with_status(StatusCode::CREATED);
    let unshared = test
        .server
        .fetch_groupware_resources(owner, owner, SyncCollection::Calendar)
        .await
        .unwrap()
        .by_path("unshared")
        .unwrap()
        .document_id();
    let mut controls = Vec::new();
    let mut c = original.clone();
    c.acls.clear();
    controls.push(("stale grant entry", c, "shared preference/ACL mismatch"));
    let mut c = original.clone();
    c.preferences.retain(|p| p.account_id != bob);
    controls.push(("ACL without ring", c, "shared preference/ACL mismatch"));
    let owner_index = original
        .preferences
        .iter()
        .position(|p| p.account_id == owner)
        .unwrap();
    let bob_index = original
        .preferences
        .iter()
        .position(|p| p.account_id == bob)
        .unwrap();
    for (label, index, byte, expected) in [
        (
            "owner calendar identity",
            owner_index,
            5,
            "owner ring calendar mismatch",
        ),
        (
            "grant calendar identity",
            bob_index,
            5,
            "grant header mismatch",
        ),
        (
            "grant domain identity",
            bob_index,
            6,
            "grant header mismatch",
        ),
        (
            "grant principal identity",
            bob_index,
            25,
            "grant header mismatch",
        ),
    ] {
        let mut c = original.clone();
        let mut parts = c.preferences[index]
            .name
            .strip_prefix(COLLECTION_MARKER)
            .unwrap()
            .split('|')
            .map(str::to_string)
            .collect::<Vec<_>>();
        let mut ring = STANDARD.decode(&parts[0]).unwrap();
        ring[byte] ^= 0x40;
        parts[0] = STANDARD.encode(ring);
        c.preferences[index].name = format!("{COLLECTION_MARKER}{}", parts.join("|"));
        controls.push((label, c, expected));
    }
    for (label, index, expected) in [
        ("owner carrier", owner_index, "owner carrier shape"),
        ("grant carrier", bob_index, "grant carrier shape"),
    ] {
        let mut c = original.clone();
        c.preferences[index].name.push_str("|extra");
        controls.push((label, c, expected));
    }
    let mut c = original.clone();
    let mut parts = c.preferences[bob_index]
        .name
        .strip_prefix(COLLECTION_MARKER)
        .unwrap()
        .split('|')
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut ring = STANDARD.decode(&parts[0]).unwrap();
    assert_eq!(u16::from_be_bytes(ring[43..45].try_into().unwrap()), 2);
    ring[43..45].copy_from_slice(&1u16.to_be_bytes());
    ring.truncate(45 + 108);
    parts[0] = STANDARD.encode(ring);
    c.preferences[bob_index].name = format!("{COLLECTION_MARKER}{}", parts.join("|"));
    controls.push(("missing generation", c, "grant header mismatch"));
    let mut c = original.clone();
    c.acls[0].grants.bitmap |= 1 << 20;
    controls.push(("ACL bits", c, "shared ACL bits"));
    let mut c = original.clone();
    c.preferences.push(c.preferences[bob_index].clone());
    controls.push(("duplicate preference", c, "duplicate preference principal"));
    let mut c = original.clone();
    c.acls.push(c.acls[0].clone());
    controls.push(("duplicate ACL", c, "duplicate ACL principal"));
    let mut c = original.clone();
    c.acls.push(AclGrant {
        account_id: u32::MAX - 1,
        grants: c.acls[0].grants,
    });
    controls.push((
        "gone ACL principal",
        c,
        "ACL principal is not a current key account",
    ));
    for field in ["description", "color"] {
        let mut c = original.clone();
        match field {
            "description" => c.preferences[bob_index].description = Some("plain".into()),
            _ => c.preferences[bob_index].color = Some("plain".into()),
        }
        controls.push((field, c, "sealed collection with plaintext fields"));
    }
    for (label, bad, expected) in controls {
        shared_control(
            test,
            owner,
            Collection::Calendar,
            calendar,
            archive_field,
            Archiver::new(bad).serialize().unwrap(),
            label,
            expected,
        )
        .await;
    }
    // 0.16's plaintext dead-property control: at 1.0 dead properties live in
    // the calendar's shared metadata container (rulings 16 and 29), so the
    // control is an unsealed container written by upstream's keyless writer.
    for (collection, document) in [
        (Collection::Calendar, calendar),
        (Collection::CalendarEvent, event_id),
    ] {
        shared_container_control(test, owner, collection, document).await;
    }
    let mut events = Vec::new();
    let mut contents = Vec::new();
    let mut e = original_event.clone();
    let mut second = e.names[0].clone();
    second.parent_id = unshared;
    e.names.push(second);
    events.push((
        "two distinct parents",
        e,
        "shared event must have one parent",
    ));
    let mut e = original_event.clone();
    e.names[0].parent_id = unshared;
    events.push((
        "CK on unshared event",
        e,
        "unshared event requires account envelope",
    ));
    let mut e = original_event.clone();
    e.names[0].parent_id = u32::MAX;
    events.push(("missing parent", e, "event parent calendar missing"));
    for (label, envelope, expected) in [
        (
            "account envelope",
            vec![1, 1].into_iter().chain([0u8; 72]).collect::<Vec<_>>(),
            "shared event requires CK envelope",
        ),
        (
            "unlisted generation",
            {
                let mut v = vec![1, 3];
                v.extend(calendar.to_be_bytes());
                v.extend(999u32.to_be_bytes());
                v.extend([0u8; 72]);
                v
            },
            "shared event generation not listed",
        ),
        (
            "pending envelope",
            vec![1, 2].into_iter().chain([0u8; 104]).collect::<Vec<_>>(),
            "shared event requires CK envelope",
        ),
    ] {
        let mut e = original_content.clone();
        e.data.event.components[0]
            .entries
            .last_mut()
            .unwrap()
            .values = vec![ICalendarValue::Text(STANDARD.encode(envelope))].into();
        contents.push((label, e, expected));
    }
    for (label, bad, expected) in events {
        shared_control(
            test,
            owner,
            Collection::CalendarEvent,
            event_id,
            archive_field,
            Archiver::new(bad).serialize().unwrap(),
            label,
            expected,
        )
        .await;
    }
    for (label, bad, expected) in contents {
        shared_control(
            test,
            owner,
            Collection::CalendarEvent,
            event_id,
            content_field,
            Archiver::new(bad).serialize().unwrap(),
            label,
            expected,
        )
        .await;
    }
    let healthy = scan(test, owner).await;
    assert!(
        healthy.violations.is_empty(),
        "healthy shared fixture: {:?}",
        healthy.violations
    );
    assert!(healthy.calendars > 0 && healthy.events == 2);
    println!(
        "Task11 healthy multi-generation shared fixture and all isolated structural controls passed"
    );
    alice
        .request("DELETE", PATH, "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    alice
        .request("DELETE", &format!("{HOME}unshared/"), "")
        .await
        .with_status(StatusCode::NO_CONTENT);
    for (client, root) in [
        (&alice, HOME),
        (&reader, "/dav/cal/leak-shared-bob%40example.com/"),
    ] {
        let _ = client
            .request("DELETE", &format!("{root}default/"), "")
            .await;
    }
    // ACL notifications are independent disposable inbox records.
    for (name, id) in [(BOB, bob), (CAROL, carol)] {
        let client = DummyWebDavClient::new(id, name, STRONG, name);
        for item in inbox_ids(test, id).await {
            crate::utils::za::delete_inbox_item(test, &client, id, item).await;
        }
    }
    test.wait_for_tasks().await;
    let clean = scan(test, owner).await;
    assert!(
        clean.violations.is_empty(),
        "shared fixture cleanup: {:?}",
        clean.violations
    );
}

/// The `Property` value `field` of the document (a row, `Field::ARCHIVE`,
/// or an event's content record).
fn property_value_key(
    owner: u32,
    collection: Collection,
    document: u32,
    field: u8,
) -> ValueKey<ValueClass> {
    ValueKey {
        account_id: owner,
        collection: collection.into(),
        document_id: document,
        class: ValueClass::Property(field),
    }
}

/// Writes `bad` as the document's record `field`, scans, restores the exact
/// original bytes and checks that the bad scan named `expected` and the
/// restored fixture scans clean.
#[allow(clippy::too_many_arguments)]
async fn shared_control(
    test: &TestServer,
    owner: u32,
    collection: Collection,
    document: u32,
    field: u8,
    bad: Vec<u8>,
    label: &str,
    expected: &str,
) {
    let original = test
        .server
        .store()
        .get_value::<RawValue>(property_value_key(owner, collection, document, field))
        .await
        .unwrap()
        .unwrap()
        .0;
    async fn write(
        test: &TestServer,
        owner: u32,
        collection: Collection,
        document: u32,
        field: u8,
        bytes: Vec<u8>,
    ) {
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(owner)
            .with_collection(collection)
            .with_document(document)
            .set(ValueClass::Property(field), bytes);
        test.server.store().write_batch(&mut batch).await.unwrap();
    }
    write(test, owner, collection, document, field, bad).await;
    use futures::FutureExt;
    let bad_scan = std::panic::AssertUnwindSafe(scan(test, owner))
        .catch_unwind()
        .await;
    write(test, owner, collection, document, field, original.clone()).await;
    let restored_bytes = test
        .server
        .store()
        .get_value::<RawValue>(property_value_key(owner, collection, document, field))
        .await
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(original, restored_bytes, "{label}: raw archive restoration");
    let restored = scan(test, owner).await;
    assert!(
        bad_scan.is_ok(),
        "{label}: scanner must return violations instead of panicking"
    );
    let bad_scan = bad_scan.unwrap();
    assert!(
        bad_scan.violations.iter().any(|v| v.contains(expected)),
        "{label}: expected {expected}: {:?}",
        bad_scan.violations
    );
    assert!(
        restored.violations.is_empty(),
        "{label}: restored fixture: {:?}",
        restored.violations
    );
    println!("Task11 corruption control {label}: {expected}; exact raw restoration and clean scan");
}

/// The object-metadata rule's control on a key account's calendar or event:
/// a plaintext shared container (a DAV dead property, as a legacy writer or
/// an ordinary account stores one) written by upstream's keyless writer is
/// flagged; the exact stored container and row bytes are then restored and
/// the fixture scans clean.
async fn shared_container_control(
    test: &TestServer,
    owner: u32,
    collection: Collection,
    document: u32,
) {
    let container_key = ValueKey {
        account_id: owner,
        collection: collection.into(),
        document_id: document,
        class: ValueClass::Metadata(MetadataClass::Shared),
    };
    let row_key = property_value_key(owner, collection, document, u8::from(Field::ARCHIVE));
    let raw = async |key: &ValueKey<ValueClass>| {
        test.server
            .store()
            .get_value::<RawValue>(key.clone())
            .await
            .unwrap()
            .map(|value| value.0)
    };
    let container = raw(&container_key).await;
    let row = raw(&row_key).await.expect("the container's document");
    plant_container(
        test,
        owner,
        collection,
        document,
        Some(plain_dav_container(&[(
            "http://apple.com/ns/ical/",
            "calendar-color",
            "plain-container-colour",
        )])),
    )
    .await;
    let bad = scan(test, owner).await;
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(owner)
        .with_collection(collection)
        .with_document(document)
        .set(ValueClass::Property(u8::from(Field::ARCHIVE)), row.clone());
    match &container {
        Some(container) => batch.set(
            ValueClass::Metadata(MetadataClass::Shared),
            container.clone(),
        ),
        None => batch.clear(ValueClass::Metadata(MetadataClass::Shared)),
    };
    test.server.store().write_batch(&mut batch).await.unwrap();
    test.server
        .invalidate_local_caches(&[common::ipc::CacheInvalidation::GroupwareResources(owner)]);
    assert_eq!(
        raw(&container_key).await,
        container,
        "{collection:?}: container restoration"
    );
    assert_eq!(
        raw(&row_key).await,
        Some(row),
        "{collection:?}: row restoration"
    );
    assert!(
        bad.violations
            .iter()
            .any(|v| v.ends_with("unsealed shared container")),
        "{collection:?}: the plaintext container was not flagged: {:?}",
        bad.violations
    );
    let restored = scan(test, owner).await;
    assert!(
        restored.violations.is_empty(),
        "{collection:?}: restored fixture: {:?}",
        restored.violations
    );
    println!("Task 6 container control on {collection:?}: flagged, restored, clean scan");
}
