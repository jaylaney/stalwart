/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6: the drain. A key account's own DAV request, which
//! holds the account's session keys, opens the pending items of the
//! account's scheduling inbox before the request's own work (6.1), so no
//! background code needs a key. The first pass of every session runs the
//! self-check (key pinning spec 3.3) before enumerating, items pending or
//! not. A pass (6.2-6.3) takes the session's lock,
//! reads the inbox change id (the snapshot), enumerates the items whose
//! pending bit is set in document id order, unwraps the private key once
//! (3.2), and for each item: opens the `pk` wrap (the only place one is
//! opened, invariant 11), checks the sender (6.4 step 2), finds the event
//! by UID, imports under the creation lease or merges under the event's
//! archive assertion, adopts or keeps token pairs (3.3), authenticates a
//! REPLY by its token and rejects a stale one, fans out the organizer's
//! REQUEST to delegates the merge added (compared per instance), and
//! rewrites the item resealed under the master key with the pending bit
//! cleared (6.4 step 5). Each item is one store write holding the event
//! write and the item rewrite (its batch stays far below
//! `BatchBuilder::is_large_batch`, the only point at which a commit point
//! splits a batch).
//! Upstream's pure iTIP functions run unchanged (invariant 15); only the
//! commit is the fork's. The drain never fails the request that triggered
//! it (invariant 14): `za_drain` traces and returns.
//!
//! Item errors (6.5): a transient one (a store error, a lost assertion,
//! the lease's fence included) aborts the pass and the item stays pending;
//! a busy UID lease leaves that item pending and stops the pass there, so
//! later items for the same UID keep their order (a CANCEL never overtakes
//! the REQUEST it cancels); a permanent one (the wrap does not open, a stray carrier, an unparsable
//! tree, an unknown METHOD) deletes the item, since nothing can ever read
//! it; a forgery, a REPLY from a participant (a sender address holding a
//! pair in the event's bundle) whose token is wrong, deletes the item with
//! a security trace, as does a REPLY carrying no token at all (checked
//! before the bundle is read), while a REPLY with a token from a sender
//! holding no pair is not a participant and is kept as history; an
//! iTIP-level rejection keeps the item as history, resealed, bit cleared.
//! A pass that leaves any item pending does not advance the cursor and
//! defers the next pass by `DRAIN_RETRY_SECS`. A pass attempts at most
//! `DRAIN_PASS_CAP` items and, with more remaining, ends without a deferral
//! so the next request continues. An item that failed
//! `DRAIN_ITEM_FAILURE_LIMIT` times in the session is skipped for the
//! session, traced once, and later items of its UID wait with it; neither
//! holds the cursor nor is re-opened in this session. A private key that
//! does not open suspends the session's drain for good (spec 10).
//! `contacts/pin` drains through `za_drain_settled` (key pinning spec 6.4),
//! which runs the next pass at once, past the deferral, while a pass ends on
//! an item's own counted failure, so a persistently failing item reaches the
//! limit within the request; DAV requests run one pass (`za_drain`).
//!
//! Lease order: a drain holds its session lock and then may take a per-UID
//! lease (an import, or a REPLY merge that fans out to delegates); the PUT
//! path takes the lease without any drain lock, so there is no cycle.
//! Both lease holders re-read the item once the lease is theirs and skip
//! it, writing nothing, when another session finished it in the meantime.
//! The lease fences the item's commit and, for a delegating merge, each
//! delegate item the fan-out writes: a holder whose lease was taken over
//! delivers nothing more and defers, as on a lost commit fence.
//!
//! A mail-borne item (iMIP spec 4.3, the sender in `X-ZA-SENDER`) is opened,
//! its UID read and checked against the skipped items like any other, then
//! goes to the mail arm (`apply_mail`): delivery's checks repeated on the
//! opened tree with the carrier's sender, then upstream's import or merge
//! with that sender, untokened, through the commits above. ADD and REFRESH
//! come only by mail. It ends as upstream's ingest leaves the inbox (D7): an
//! applied import or merge rewrites it drained; any other outcome deletes
//! it, a REFRESH after mailing nothing until its deletion commits (D14).
//! Mail never adopts, replaces or removes a pair, and only a REPLY into the
//! account's own organizer copy runs the delegating fan-out (D15).

use crate::{
    DestroyArchive, SizeWriter,
    cache::GroupwareCache,
    calendar::{
        CalendarEvent, CalendarEventContent, CalendarEventData, CalendarEventNotification,
        CalendarEventNotificationContent, ChangedBy, EVENT_NOTIFICATION_IS_CHANGE,
        alerts::{DefaultAlerts, DefaultAlertsResolver},
        fanout::{
            ITIP_STATUS_KEY_REFUSED, ZaFanOut, ZaFanOutKind, ZaFanOutMessage, ZaScheduleStatus,
            ZaSource, za_imip_mail, za_mail_reachable, za_mail_withheld, za_set_schedule_status,
        },
        itip::{itip_auto_add_allowed, itip_fill_organizer, itip_sender_snapshot},
        privacy::{EventPrivacy, ICalendarPrivacy},
        schedule::{AlarmTasks, EventAlarmScheduler, EventAlarmUsers},
        seal::{
            PendingCarriers, SealError, TOKEN_LEN, TokenPairs, ZaEvent, archived_item_is_pending,
            item::{into_item_message, sealed_item_record},
            open_pending_item, seal_drained_item, seal_error, seal_event_in_domain,
            unseal_event_in_domain,
        },
        sequence_floor::za_set_sequences,
        sharing::resolve::{za_owned_calendar_context, za_owned_event_contexts},
    },
    scheduling::{
        Attendee, InstanceId, ItipError, ItipMessage, ItipSnapshots,
        inbound::{
            MergeAction, MergeResult, itip_import_message, itip_merge_changes, itip_method,
            itip_process_message,
        },
        itip::ITIP_STATUS_DELIVERY_FAILED,
        organizer::organizer_request_full,
        recipient::{RecipientPolicy, itip_messages_per_recipient},
        snapshot::itip_snapshot,
    },
};
use ahash::{AHashMap, AHashSet};
use calcard::{
    common::timezone::Tz,
    icalendar::{ICalendar, ICalendarMethod, ICalendarProperty},
};
use common::{
    DavName, Server,
    auth::AccountInfo,
    storage::index::SplitUpdate,
    za::{
        imip::{
            ZA_IMIP_ITEM_DELETED, ZA_IMIP_KEY_SENDER, ZA_IMIP_LOOKUP_FAILED, ZaImipPending,
            za_imip_trace_unrouted, za_imip_trace_withheld,
        },
        lease::{ZaUidLease, ZaUidLeaseGuard},
    },
};
use registry::schema::enums::StorageQuota;
use std::{future::Future, sync::Arc};
use store::{
    rand,
    write::{Archive, ArchiveBytes, BatchBuilder, Slot, now},
};
use trc::AddContext;
use types::{
    collection::{Collection, SyncCollection},
    field::{CalendarEventField, CalendarNotificationField, Field},
};
use vault::session::{PassState, SessionKeys, ZaDrainState};
use zeroize::Zeroizing;

/// Spec 6.2: a pass that left an item pending is not retried before this
/// many seconds have passed. A constant with a test-mode override, not a
/// setting (spec 13).
pub const DRAIN_RETRY_SECS: u64 = 30;
/// Spec 6.2, "The cap": items one pass handles at most. A constant (spec 13).
pub const DRAIN_PASS_CAP: usize = 32;
/// Spec 6.2, "Skipped items": failures of one item in one session before
/// the session skips it. A constant (spec 13).
pub const DRAIN_ITEM_FAILURE_LIMIT: u8 = 3;
/// Spec 10: a deferral no later request of the session passes.
const PERMANENT_DEFERRAL: u64 = u64::MAX;

/// What one item's drain left behind.
enum ItemOutcome {
    /// Rewritten or deleted.
    Done,
    /// Left pending; the pass stops here (a busy UID lease, spec 6.3).
    LeftPending,
    /// Left pending behind a skipped item of its UID; the pass goes on and
    /// the session does not re-open it.
    Waiting,
}

/// What a pass left behind, as `za_drain` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassOutcome {
    /// Every item is done, skipped, or waiting behind a skipped one; or the
    /// cursor is at the snapshot.
    Complete,
    /// An item stays pending for a reason a later pass can clear; also an
    /// error, a deferral in force and the test-mode drain skip.
    LeftPending,
    /// `DRAIN_PASS_CAP` items attempted, more remain: the next request
    /// continues.
    More,
    /// The private key does not open: no later pass of this session (also
    /// a pass that permanent deferral keeps out).
    Suspended,
}

/// A pass's error, traced by the caller.
struct PassError {
    err: trc::Error,
    /// An item's own failure, counted toward `DRAIN_ITEM_FAILURE_LIMIT`
    /// without reaching it: the item is not skipped yet, so a pass run at
    /// once retries it (`za_drain_settled`).
    counted: bool,
}

impl From<trc::Error> for PassError {
    fn from(err: trc::Error) -> Self {
        PassError {
            err,
            counted: false,
        }
    }
}

/// What a settled drain holds back from an attendee-side redelivery (key
/// pinning spec 6.4): the UIDs of the items the session skipped, which the
/// later items of each UID wait behind, or everything when the drain is
/// incomplete or a skipped item named no UID.
pub enum ZaHeldBack {
    Uids(Vec<String>),
    All,
}

impl ZaHeldBack {
    /// Whether a REPLY for `uid` would be built behind a pending item.
    pub fn holds(&self, uid: &str) -> bool {
        match self {
            ZaHeldBack::Uids(uids) => uids.iter().any(|held| held == uid),
            ZaHeldBack::All => true,
        }
    }
}

/// An item's error with the UID it named, if the item got that far.
struct ItemFailure {
    err: trc::Error,
    uid: Option<String>,
    /// Whether the error is the item's own and counts toward
    /// `DRAIN_ITEM_FAILURE_LIMIT` (spec 6.2, "Skipped items").
    counts: bool,
}

pub trait ZaDrain: Sync + Send {
    /// Spec 6.1: drains the inbox of the account whose session keys these
    /// are, before the request's own work, and returns the pass's outcome.
    /// Never fails the request (invariant 14): errors are traced here and
    /// are `LeftPending`.
    fn za_drain(&self, keys: &Arc<SessionKeys>) -> impl Future<Output = PassOutcome> + Send;

    /// Key pinning spec 6.4: drains as `za_drain` does, pass after pass,
    /// while a pass returns `More` or ends on an item's own counted failure
    /// (an item not yet skipped), so a persistently failing item reaches
    /// `DRAIN_ITEM_FAILURE_LIMIT` within the request and is skipped. The
    /// first pass honours the session's deferral; the later ones are this
    /// request's own and pass the deferral its failures set. Bounded: each
    /// retried pass recorded a new failure of an item not yet skipped, and
    /// an item stops counting once skipped. Any other `LeftPending`, and
    /// `Suspended`, ends the loop and holds back everything; `Complete`
    /// holds back the UIDs of the items the session skipped.
    fn za_drain_settled(&self, keys: &Arc<SessionKeys>) -> impl Future<Output = ZaHeldBack> + Send;
}

impl ZaDrain for Server {
    async fn za_drain(&self, keys: &Arc<SessionKeys>) -> PassOutcome {
        run_pass(self, keys, true).await.0
    }

    async fn za_drain_settled(&self, keys: &Arc<SessionKeys>) -> ZaHeldBack {
        let mut honour_deferral = true;
        loop {
            match run_pass(self, keys, honour_deferral).await {
                (PassOutcome::More, _) | (PassOutcome::LeftPending, true) => {}
                (PassOutcome::Complete, _) => break,
                (PassOutcome::LeftPending | PassOutcome::Suspended, _) => return ZaHeldBack::All,
            }
            honour_deferral = false;
        }
        // Items waiting behind a skipped one carry its UID.
        match keys
            .drain()
            .lock()
            .await
            .skipped
            .values()
            .cloned()
            .collect::<Option<Vec<String>>>()
        {
            Some(uids) => ZaHeldBack::Uids(uids),
            None => ZaHeldBack::All,
        }
    }
}

/// One pass as `za_drain` reports it: an error is traced and is
/// `LeftPending`, flagged when it was an item's own counted failure.
async fn run_pass(
    server: &Server,
    keys: &Arc<SessionKeys>,
    honour_deferral: bool,
) -> (PassOutcome, bool) {
    #[cfg(feature = "test_mode")]
    if common::za::test_hooks::drain_skipped(keys.account_id) {
        return (PassOutcome::LeftPending, false);
    }
    match drain_pass(server, keys, honour_deferral).await {
        Ok(outcome) => (outcome, false),
        Err(PassError { err, counted }) => {
            trc::error!(
                err.account_id(keys.account_id)
                    .details("zero-access: drain pass failed")
                    .caused_by(trc::location!())
            );
            (PassOutcome::LeftPending, counted)
        }
    }
}

fn drain_retry_secs() -> u64 {
    #[cfg(feature = "test_mode")]
    if let Some(secs) = common::za::test_hooks::drain_retry_override() {
        return secs;
    }
    DRAIN_RETRY_SECS
}

/// Spec 6.2-6.3: one pass under the session's lock. Nothing runs while a
/// deferral is in force (`LeftPending`, or `Suspended` when permanent;
/// without `honour_deferral` only a permanent one counts) or while the
/// cursor is at the inbox's change id (`Complete`). The session's first
/// pass runs the self-check first (key pinning spec 3.3), items pending or
/// not: a private key that does not open suspends the session's drain; any
/// other error is traced and the pass goes on.
async fn drain_pass(
    server: &Server,
    keys: &Arc<SessionKeys>,
    honour_deferral: bool,
) -> Result<PassOutcome, PassError> {
    let account_id = keys.account_id;
    let state = keys.drain();
    let mut pass = state.lock().await;
    let now = now();
    let retry_after = state.retry_after();
    if retry_after > now && (honour_deferral || retry_after == PERMANENT_DEFERRAL) {
        return Ok(if retry_after == PERMANENT_DEFERRAL {
            PassOutcome::Suspended
        } else {
            PassOutcome::LeftPending
        });
    }
    if keys.private_key().is_none()
        && let Err(err) = server.za_private_key(keys).await
    {
        // A recorded failure was traced once for the session (spec 10).
        if matches!(keys.private_key(), Some(Err(_))) {
            state.defer_until(PERMANENT_DEFERRAL);
            return Ok(PassOutcome::Suspended);
        }
        trc::error!(
            err.account_id(account_id)
                .details("zero-access: self-check failed")
                .caused_by(trc::location!())
        );
    }
    let Some((snapshot, mut pending)) = (match enumerate(server, account_id, state).await {
        Ok(enumerated) => enumerated,
        // Spec 10: a store error before the match defers like one after it.
        Err(err) => {
            state.defer_until(now + drain_retry_secs());
            return Err(err.into());
        }
    }) else {
        return Ok(PassOutcome::Complete);
    };
    // Spec 6.2, "Skipped items": neither a skipped item nor one waiting
    // behind it holds the cursor, and neither is re-opened in this session
    // (a fresh session retries both).
    pending.retain(|document_id| !pass.is_skipped(*document_id) && !pass.is_waiting(*document_id));
    if pending.is_empty() {
        state.complete(snapshot);
        return Ok(PassOutcome::Complete);
    }
    let outcome = match drain_items(server, keys, pending, &mut pass).await {
        Ok(outcome) => outcome,
        Err(err) => {
            state.defer_until(now + drain_retry_secs());
            return Err(err);
        }
    };
    match outcome {
        PassOutcome::Complete => state.complete(snapshot),
        PassOutcome::LeftPending => state.defer_until(now + drain_retry_secs()),
        // The next request continues from the cursor, at once.
        PassOutcome::More => {}
        PassOutcome::Suspended => state.defer_until(PERMANENT_DEFERRAL),
    }
    Ok(outcome)
}

/// Spec 6.2: the snapshot and the pending items, oldest first; `None` when
/// the cursor is at the snapshot.
async fn enumerate(
    server: &Server,
    account_id: u32,
    state: &ZaDrainState,
) -> trc::Result<Option<(u64, Vec<u32>)>> {
    #[cfg(feature = "test_mode")]
    if common::za::test_hooks::take_drain_enumerate_fail(account_id) {
        return Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access test hook: drain enumeration failure")
            .caused_by(trc::location!()));
    }
    // The snapshot: the inbox change id observed before enumeration, from
    // the same since-query upstream runs for a request's own collection.
    let snapshot = server
        .fetch_groupware_resources(
            account_id,
            account_id,
            SyncCollection::CalendarEventNotification,
        )
        .await
        .caused_by(trc::location!())?
        .highest_change_id;
    if state.cursor().is_some_and(|cursor| cursor >= snapshot) {
        return Ok(None);
    }
    // Every item row in document id order (the iteration's key order).
    let mut pending = Vec::new();
    server
        .archives(
            account_id,
            Collection::CalendarEventNotification,
            Field::ARCHIVE,
            &(),
            |document_id, stored| {
                if archived_item_is_pending(stored.unarchive::<CalendarEventNotification>()?) {
                    pending.push(document_id);
                }
                Ok(true)
            },
        )
        .await
        .caused_by(trc::location!())?;
    Ok(Some((snapshot, pending)))
}

/// An item's row and content archives, read together; `None` when either
/// is gone.
async fn read_item(
    server: &Server,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Option<(Archive<ArchiveBytes>, Archive<ArchiveBytes>)>> {
    let (row, content) = server
        .archive_and_field(
            account_id,
            Collection::CalendarEventNotification,
            document_id,
            CalendarNotificationField::Content.field(),
        )
        .await
        .caused_by(trc::location!())?;
    Ok(row.zip(content))
}

/// Spec 6.4: the private key once, then each item in document id order,
/// attempting at most `DRAIN_PASS_CAP` of them (an item found waiting
/// behind a skipped one is not an attempt). A busy UID lease leaves its
/// item pending and stops the pass there, so later items for the same UID
/// keep their order; an item's error aborts the pass (6.5) until the item
/// reaches `DRAIN_ITEM_FAILURE_LIMIT` failures in the session, when it is
/// skipped and the pass goes on (6.2, "Skipped items").
async fn drain_items(
    server: &Server,
    keys: &Arc<SessionKeys>,
    pending: Vec<u32>,
    pass: &mut PassState,
) -> Result<PassOutcome, PassError> {
    let account_id = keys.account_id;
    let private_key = match server.za_private_key(keys).await {
        Ok(key) => key,
        // A recorded failure (the wrap does not open, the record is missing
        // or unusable) was traced once for the session and suspends its
        // drain (spec 10); a store error before the key opened is not
        // recorded, so it is traced by the caller and the next pass retries
        // it.
        Err(err) => {
            return if keys.private_key().is_some() {
                Ok(PassOutcome::Suspended)
            } else {
                Err(err.into())
            };
        }
    };
    let account_info = server
        .account_info(account_id)
        .await
        .caused_by(trc::location!())?;
    // Items attempted (done, left pending or failed); waiting ones are not.
    let mut handled = 0;
    for document_id in pending {
        if handled == DRAIN_PASS_CAP {
            return Ok(PassOutcome::More);
        }
        let outcome = drain_item(server, &account_info, keys, private_key, document_id, pass).await;
        if !matches!(outcome, Ok(ItemOutcome::Waiting)) {
            handled += 1;
        }
        match outcome {
            Ok(ItemOutcome::Done) => {}
            Ok(ItemOutcome::Waiting) => {
                pass.waiting.insert(document_id);
            }
            Ok(ItemOutcome::LeftPending) => return Ok(PassOutcome::LeftPending),
            Err(ItemFailure { err, uid, counts }) => {
                // A failure before the UID is known is a store error on an
                // archive `enumerate` read: it defers the pass, uncounted.
                if !counts {
                    return Err(err.into());
                }
                // Only the item's own error counts: neither a busy lease
                // (`LeftPending`) nor `Waiting` reaches this arm.
                if pass.record_failure(document_id, uid, DRAIN_ITEM_FAILURE_LIMIT) {
                    // Spec 6.2, "Skipped items": traced once, then skipped
                    // for the session; its UID's later items wait.
                    trc::error!(
                        err.account_id(account_id)
                            .document_id(document_id)
                            .details("zero-access: drain item skipped after repeated failures")
                            .caused_by(trc::location!())
                    );
                } else {
                    return Err(PassError { err, counted: true });
                }
            }
        }
    }
    Ok(PassOutcome::Complete)
}

/// One pending item: what the rewrite, the deletion and the commit need.
struct ItemCtx<'a> {
    server: &'a Server,
    account_info: &'a AccountInfo,
    keys: &'a SessionKeys,
    /// The stored row as read; every write asserts it.
    stored: Archive<ArchiveBytes>,
    /// The stored content as read, the current content of the rewrite.
    stored_content: Archive<ArchiveBytes>,
    document_id: u32,
    changed_by: ChangedBy,
    created: i64,
    /// A mail-borne item (iMIP spec 4.2: it carried `X-ZA-SENDER`), set
    /// from the opened carriers before any write. Its iTIP-level rejections
    /// and a merge that changes nothing delete it (D7, `delete_mail`).
    mail: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Request,
    Cancel,
    Reply,
    /// iMIP spec D5: accepted from a mail-borne item only.
    Add,
    /// iMIP spec D5: accepted from a mail-borne item only.
    Refresh,
}

/// What a merge applies.
enum Incoming {
    /// The recipient's pair from the item (spec 3.3, "Attendee side").
    Request(Option<TokenPairs>),
    Cancel,
    /// The replier's token (spec 3.3, "Verification").
    Reply(Zeroizing<[u8; TOKEN_LEN]>),
    /// A mail-borne item of any method, applied with the carrier's sender
    /// (iMIP spec 4.3, D5): no token, no pair.
    Mail {
        sender: String,
    },
}

/// Spec 6.4 steps 1-5 for one item. An error carries the item's UID once
/// it is known, so a skipped item holds back the later items of its UID.
async fn drain_item(
    server: &Server,
    account_info: &AccountInfo,
    keys: &SessionKeys,
    private_key: &vault::keys::Secret,
    document_id: u32,
    pass: &PassState,
) -> Result<ItemOutcome, ItemFailure> {
    let account_id = keys.account_id;
    #[cfg(feature = "test_mode")]
    if common::za::test_hooks::drain_read_fails(account_id, document_id) {
        return Err(no_uid(
            trc::StoreEvent::UnexpectedError
                .into_err()
                .details("zero-access test hook: drain read failure"),
        ));
    }
    // Re-read (spec 6.3, "The pass"): an item that is gone, or that another
    // session drained, is skipped.
    let Some((stored, stored_content)) = read_item(server, account_id, document_id)
        .await
        .map_err(no_uid)?
    else {
        return Ok(ItemOutcome::Done);
    };
    let archived = stored
        .unarchive::<CalendarEventNotification>()
        .caused_by(trc::location!())
        .map_err(no_uid)?;
    if !archived_item_is_pending(archived) {
        return Ok(ItemOutcome::Done);
    }
    let item = stored
        .deserialize::<CalendarEventNotification>()
        .caused_by(trc::location!())
        .map_err(no_uid)?;
    let content = stored_content
        .deserialize::<CalendarEventNotificationContent>()
        .caused_by(trc::location!())
        .map_err(no_uid)?;
    let mut ctx = ItemCtx {
        server,
        account_info,
        keys,
        stored,
        stored_content,
        document_id,
        changed_by: item.changed_by.clone(),
        created: item.created,
        mail: false,
    };
    // A pending row the fan-out did not write (any other content shape)
    // cannot be opened: permanent, as an undecodable item.
    let Some(mut tree) = into_item_message(content) else {
        return item_outcome(ctx.delete_undecodable("not a sealed item").await, None);
    };
    // Step 1: open. A failure is permanent (spec 6.5): the private key
    // never changes, so nothing will ever read this item.
    let mut carriers = match open_pending_item(&mut tree, account_id, private_key) {
        Ok(carriers) => carriers,
        Err(err) => return item_outcome(ctx.delete_undecodable(seal_reason(&err)).await, None),
    };
    ctx.mail = carriers.sender.is_some();
    // An unknown or missing METHOD cannot come from the fan-out: permanent.
    // ADD and REFRESH come only by mail (iMIP spec D5).
    let method = match itip_method(&tree) {
        Ok(ICalendarMethod::Request) => Method::Request,
        Ok(ICalendarMethod::Cancel) => Method::Cancel,
        Ok(ICalendarMethod::Reply) => Method::Reply,
        Ok(ICalendarMethod::Add) if ctx.mail => Method::Add,
        Ok(ICalendarMethod::Refresh) if ctx.mail => Method::Refresh,
        Ok(_) => return item_outcome(ctx.delete_undecodable("UnsupportedMethod").await, None),
        Err(_) => return item_outcome(ctx.delete_undecodable("MissingMethod").await, None),
    };
    // iMIP spec D5 step 1: a mail-borne REQUEST or REPLY gets upstream's
    // Exchange fill again, before its first snapshot as upstream's parse
    // fills before its snapshot. Delivery filled with the envelope
    // recipient, which the item does not carry, so the repeat names the
    // account's first address (spec question 2); on every item delivery
    // wrote it adds nothing.
    if ctx.mail
        && matches!(method, Method::Request | Method::Reply)
        && let Some(address) = account_info.addresses().first()
    {
        itip_fill_organizer(&mut tree, address);
    }
    let (uid, candidates) = match itip_snapshot(&tree, account_info.addresses(), false) {
        Ok(snapshots) => (
            snapshots.uid.to_string(),
            sender_candidates(&snapshots, method),
        ),
        Err(err) => return item_outcome(ctx.reject(tree, None, &err).await, None),
    };
    // Spec 6.2, "Skipped items": behind a skipped item of this UID, so a
    // CANCEL never overtakes the REQUEST it cancels.
    if pass.blocks_uid(&uid) {
        return Ok(ItemOutcome::Waiting);
    }
    // iMIP spec 4.3: only now, on the carrier, so a mail-borne item waits
    // behind a skipped one and counts toward the failure limit as any.
    let result = match carriers.sender.take() {
        Some(sender) => apply_mail(&ctx, tree, sender, method, &uid).await,
        None => apply(&ctx, tree, carriers, method, &uid, candidates).await,
    };
    item_outcome(result, Some(uid))
}

/// A failure before the item's UID is known: not counted (spec 6.2).
fn no_uid(err: trc::Error) -> ItemFailure {
    ItemFailure {
        err,
        uid: None,
        counts: false,
    }
}

/// `Ok(true)`: done; `Ok(false)`: left pending (the lease was busy).
fn item_outcome(
    result: trc::Result<bool>,
    uid: Option<String>,
) -> Result<ItemOutcome, ItemFailure> {
    match result {
        Ok(true) => Ok(ItemOutcome::Done),
        Ok(false) => Ok(ItemOutcome::LeftPending),
        Err(err) => Err(ItemFailure {
            err,
            uid,
            counts: true,
        }),
    }
}

/// Spec 6.4 steps 2-5 for an opened item. `Ok(true)`: done (rewritten or
/// deleted); `Ok(false)`: left pending (the lease was busy).
async fn apply(
    ctx: &ItemCtx<'_>,
    tree: ICalendar,
    carriers: PendingCarriers,
    method: Method,
    uid: &str,
    candidates: Vec<ZaSenderCandidate>,
) -> trc::Result<bool> {
    let server = ctx.server;
    let account_id = ctx.keys.account_id;
    // Step 2: the sending address must resolve to the principal in
    // `changed_by`, the check upstream's ingest makes against a mail's
    // sender. A REQUEST or CANCEL names one candidate, its ORGANIZER; a
    // REPLY names every ATTENDEE it carries, since upstream's attendee
    // builder lists the replier together with its delegates and
    // delegators, so the sender's addresses are the candidates that
    // resolve to `changed_by`. The lookup is the quiet one (spec 10).
    let mut sender_addresses = Vec::new();
    let has_delegation = candidates
        .iter()
        .any(|candidate| candidate.sent_by.is_some());
    if let ChangedBy::PrincipalId(sender_id) = &ctx.changed_by {
        for candidate in candidates {
            let verified = if let Some(sent_by) = &candidate.sent_by {
                // Empty is the private invalid-present sentinel. Never treat
                // malformed or non-mailto SENT-BY as ordinary absence.
                if sent_by.is_empty() {
                    false
                } else {
                    let sent_by_id = server
                        .za_account_id_from_email(sent_by)
                        .await
                        .caused_by(trc::location!())?;
                    let named_id = server
                        .za_account_id_from_email(&candidate.named)
                        .await
                        .caused_by(trc::location!())?;
                    sent_by_id == Some(*sender_id)
                        && named_id.is_some_and(|named_id| named_id != *sender_id)
                        && server.za_is_key_account(*sender_id).await?
                        && server.za_is_key_account(named_id.unwrap()).await?
                }
            } else {
                server
                    .za_account_id_from_email(&candidate.named)
                    .await
                    .caused_by(trc::location!())?
                    == Some(*sender_id)
            };
            if verified && !sender_addresses.contains(&candidate.named) {
                sender_addresses.push(candidate.named);
            }
        }
    }
    if sender_addresses.is_empty() {
        return if has_delegation {
            ctx.reject_delegated(tree).await
        } else {
            ctx.reject(tree, None, &ItipError::SenderIsNotOrganizerNorAttendee)
                .await
        };
    }
    // Step 3, by METHOD. The UID lookup is upstream's (the fresh index);
    // the lowest document id is the copy that merges, as `itip_ingest`
    // takes the first hit.
    let existing = find_event(server, account_id, uid).await?;
    match (method, existing) {
        (Method::Request, None) => {
            import(
                ctx,
                tree,
                Incoming::Request(carriers.pair),
                uid,
                sender_addresses,
            )
            .await
        }
        (Method::Request, Some(event_id)) => {
            merge(
                ctx,
                event_id,
                tree,
                Incoming::Request(carriers.pair),
                sender_addresses,
            )
            .await
        }
        (Method::Cancel, Some(event_id)) => {
            merge(ctx, event_id, tree, Incoming::Cancel, sender_addresses).await
        }
        (Method::Reply, Some(event_id)) => match carriers.reply_token {
            Some(token) => {
                merge(
                    ctx,
                    event_id,
                    tree,
                    Incoming::Reply(token),
                    sender_addresses,
                )
                .await
            }
            // A fan-out REPLY always carries its token; one without cannot
            // be authenticated.
            None => ctx.drop_forgery().await,
        },
        (Method::Cancel | Method::Reply, None) => {
            ctx.reject(tree, None, &ItipError::EventNotFound).await
        }
        // `drain_item` reads ADD and REFRESH only from a mail-borne item,
        // which never comes here; a fan-out item never carries them.
        (Method::Add | Method::Refresh, _) => ctx.delete_undecodable("UnsupportedMethod").await,
    }
}

/// iMIP spec 4.3 (D5, D7): a mail-borne item, applied as `itip_ingest`
/// applies the mail, with the carrier's sender. Delivery's checks are
/// repeated on the opened tree (4.1 steps 2-4; the Exchange fill ran in
/// `drain_item`), the UID looked up, then upstream's import (a REQUEST, no
/// event, under the auto-add predicate again, D2) or merge (an event
/// exists) through the drain's commits. Every outcome upstream's ingest
/// leaves no inbox item for deletes the item (`delete_mail`). A store error
/// from a lookup keyed by the sender's address is the item's own failure,
/// kept to its type (section 7).
async fn apply_mail(
    ctx: &ItemCtx<'_>,
    tree: ICalendar,
    sender: String,
    method: Method,
    uid: &str,
) -> trc::Result<bool> {
    let server = ctx.server;
    let account_id = ctx.keys.account_id;
    // 4.1 step 2: the snapshot and the method's sender rule.
    if let Err(err) = itip_sender_snapshot(&tree, ctx.account_info.addresses(), &sender).map(|_| ())
    {
        return ctx.reject(tree, None, &err).await;
    }
    // 4.1 step 4: key accounts reach each other only through the fan-out.
    let principal = server
        .za_account_id_from_email(&sender)
        .await
        .map_err(sender_lookup_error)?;
    if let Some(principal_id) = principal
        && server
            .za_is_key_account(principal_id)
            .await
            .map_err(sender_lookup_error)?
    {
        return ctx.delete_mail(ZA_IMIP_KEY_SENDER).await;
    }
    match (method, find_event(server, account_id, uid).await?) {
        (Method::Request, None) => {
            if !itip_auto_add_allowed(server, account_id, principal.is_some(), &sender)
                .await
                .map_err(sender_lookup_error)?
            {
                return ctx
                    .delete_mail(itip_reason(&ItipError::AutoAddDisabled))
                    .await;
            }
            import(
                ctx,
                tree,
                Incoming::Mail {
                    sender: sender.clone(),
                },
                uid,
                vec![sender],
            )
            .await
        }
        (_, Some(event_id)) => {
            // Spec question 3: holds one session while another drains the
            // same item (the two-session REFRESH case); `import` keeps its
            // own pause point.
            #[cfg(feature = "test_mode")]
            common::za::test_hooks::drain_lookup_pause_point(account_id).await;
            merge(
                ctx,
                event_id,
                tree,
                Incoming::Mail {
                    sender: sender.clone(),
                },
                vec![sender],
            )
            .await
        }
        (_, None) => {
            ctx.delete_mail(itip_reason(&ItipError::EventNotFound))
                .await
        }
    }
}

/// iMIP spec section 7: a store error from a lookup keyed by the sender's
/// address keeps only its type (the pass adds the account id where it
/// traces it), so no key it looked up reaches a trace.
fn sender_lookup_error(err: trc::Error) -> trc::Error {
    trc::Error::new(err.event_type())
        .details(ZA_IMIP_LOOKUP_FAILED)
        .caused_by(trc::location!())
}

async fn find_event(server: &Server, account_id: u32, uid: &str) -> trc::Result<Option<u32>> {
    Ok(server
        .document_ids_matching(
            account_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .caused_by(trc::location!())?
        .iter()
        .next())
}

/// The addresses an item may have been sent from (spec 6.4 step 2): the
/// ORGANIZER of a REQUEST or CANCEL; every ATTENDEE of a REPLY, because
/// upstream's attendee builder exports the replier's entry together with
/// its delegates' and delegators' (`attendee.rs`, `attendee_entry_uids`)
/// and a set's iteration order says nothing about which one replied.
/// Deduplicated case-insensitively, first occurrence kept.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
struct ZaSenderCandidate {
    named: String,
    // Some("") records present but invalid input, distinct from actual absence.
    sent_by: Option<String>,
}

fn sender_candidates(snapshots: &ItipSnapshots<'_>, method: Method) -> Vec<ZaSenderCandidate> {
    use calcard::icalendar::{ICalendarParameterName, ICalendarProperty};
    let mut seen = AHashSet::new();
    let mut candidates = Vec::new();
    for snapshot in snapshots.components.values() {
        for entry in &snapshot.comp.entries {
            let property = match method {
                Method::Reply | Method::Refresh => ICalendarProperty::Attendee,
                Method::Request | Method::Cancel | Method::Add => ICalendarProperty::Organizer,
            };
            if entry.name != property {
                continue;
            }
            let Some(named) = entry
                .values
                .first()
                .and_then(|value| value.as_text())
                .and_then(|value| crate::scheduling::Email::new(value, &[]))
            else {
                continue;
            };
            let sent_by = entry
                .params
                .iter()
                .find(|param| param.name == ICalendarParameterName::SentBy)
                .map(|param| {
                    // Only a location URI can assert a mailto sender. A
                    // data URI's as_text() is its content type, not its URI.
                    let uri = match &param.value {
                        calcard::icalendar::ICalendarParameterValue::Uri(
                            calcard::icalendar::Uri::Location(value),
                        ) => Some(value.as_str()),
                        _ => None,
                    };
                    uri.filter(|value| {
                        value
                            .split_once(':')
                            .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("mailto"))
                    })
                    .and_then(|value| {
                        common::auth::vault::za_normalize_address(&crate::decode_mailto_address(
                            value,
                        ))
                    })
                    .unwrap_or_default()
                });
            let candidate = ZaSenderCandidate {
                named: named.email,
                sent_by,
            };
            // Distinct SENT-BY assertions are distinct candidates, even for
            // the same named attendee across multiple components.
            if seen.insert(candidate.clone()) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

/// Spec 6.4, REPLY: stale when any instance of the reply carries a
/// SEQUENCE below the current SEQUENCE of the event component that
/// governs that instance (absent counts as 0). Upstream's `handle_reply`
/// makes no such check.
fn is_stale_reply(event: &ItipSnapshots<'_>, reply: &ItipSnapshots<'_>) -> bool {
    reply.components.iter().any(|(instance_id, instance)| {
        governing_sequence(event, instance_id)
            .is_some_and(|current| instance.sequence.unwrap_or(0) < current)
    })
}

/// The SEQUENCE of the event component that governs `instance_id`: its own
/// override when stored; otherwise, for an occurrence, the latest
/// `RANGE=THISANDFUTURE` override at or before it (a ranged override is a
/// different `InstanceId`, since `RecurrenceId` equality includes the
/// flag); otherwise the master, whose SEQUENCE a reschedule bumps. An
/// occurrence without any of these (a decline by EXDATE, which upstream's
/// merge adds as a new override) is therefore judged against what covers
/// it. `None` only for an event without a master component.
fn governing_sequence(event: &ItipSnapshots<'_>, instance_id: &InstanceId) -> Option<i64> {
    let instances: AHashSet<InstanceKey> = event.components.keys().map(instance_key).collect();
    let governing = governing_instance(instance_key(instance_id), &instances);
    event
        .components
        .iter()
        .find(|(id, _)| instance_key(id) == governing)
        .map(|(_, component)| component.sequence.unwrap_or(0))
}

/// The instance of `instances` that governs `instance` (spec 6.4): the
/// instance itself when present; otherwise, for an occurrence, the latest
/// `RANGE=THISANDFUTURE` override at or before it; otherwise the master
/// (`None`, which `instances` may lack). Shared by the stale-reply check
/// and the delegates' attendance.
fn governing_instance(instance: InstanceKey, instances: &AHashSet<InstanceKey>) -> InstanceKey {
    if instances.contains(&instance) {
        return instance;
    }
    let (occurrence, _) = instance?;
    instances
        .iter()
        .filter_map(|key| match key {
            Some((date, true)) if *date <= occurrence => Some(*date),
            _ => None,
        })
        .max()
        .map(|date| (date, true))
}

/// Spec 6.4, REQUEST with no event: upstream's import (`itip_ingest`) with
/// the commit replaced, under the creation lease (6.3). `incoming` is
/// `Request` with the item's pair, or `Mail` (iMIP spec 4.3). A local sender
/// always adds, so `itip_auto_add` does not apply; the mail arm checked it.
async fn import(
    ctx: &ItemCtx<'_>,
    tree: ICalendar,
    incoming: Incoming,
    uid: &str,
    sender_addresses: Vec<String>,
) -> trc::Result<bool> {
    let server = ctx.server;
    let account_id = ctx.keys.account_id;
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::drain_lookup_pause_point(account_id).await;
    // Spec 6.3 step 1. A lease that stays held through the retries leaves
    // the item pending for a later pass.
    let Some(lease) = ZaUidLease::acquire(server.store(), account_id, uid)
        .await
        .caused_by(trc::location!())?
    else {
        trc::event!(
            Store(trc::StoreEvent::AssertValueFailed),
            AccountId = account_id,
            DocumentId = ctx.document_id,
            Details = "zero-access: drain creation lease busy",
        );
        return Ok(false);
    };
    // Dropped on an early return, the guard releases from a spawned task.
    let lease = ZaUidLeaseGuard::new(server.store().clone(), lease);
    // Under the lease: another session that imported this item while this
    // one waited leaves nothing to do, as in the delegating merge.
    if !ctx.still_pending().await? {
        lease.release().await;
        return Ok(true);
    }
    // Under the lease the UID may have appeared (another session's drain
    // or PUT): merge instead.
    if let Some(event_id) = find_event(server, account_id, uid).await? {
        lease.release().await;
        return merge(ctx, event_id, tree, incoming, sender_addresses).await;
    }
    // Spec 3.3, "Attendee side": the copy takes the pair the item carried;
    // mail carries none (iMIP spec 4.3 point 4).
    let pair = match incoming {
        Incoming::Request(pair) => pair,
        Incoming::Cancel | Incoming::Reply(_) | Incoming::Mail { .. } => None,
    };
    let mut event_tree = tree.clone();
    if let Err(err) = itip_import_message(&mut event_tree) {
        lease.release().await;
        return ctx.reject(tree, None, &err).await;
    }
    // Upstream's checks (1.0.1 `itip_ingest`, import branch): the default
    // calendar, then the in-batch disk and object quotas below. The fork
    // also checks the quota for the plaintext size first, so an over-quota
    // item is kept as history. Only a quota limit is the iTIP-level
    // rejection; a failed counter read is a store error that keeps the
    // item pending (spec 6.5), as the fan-out classifies it.
    let account = server
        .account(account_id)
        .await
        .caused_by(trc::location!())?;
    if let Err(err) = server
        .has_available_quota(account.as_ref(), SizeWriter::ical(&tree) as u64)
        .await
    {
        lease.release().await;
        return if is_over_quota(&err) {
            ctx.reject(tree, None, &ItipError::QuotaExceeded).await
        } else {
            Err(err.caused_by(trc::location!()))
        };
    }
    let Some(parent_id) = server
        .get_or_create_default_calendar(ctx.account_info.account(), ctx.account_info.account())
        .await
        .caused_by(trc::location!())?
    else {
        lease.release().await;
        return ctx.reject(tree, None, &ItipError::NoDefaultCalendar).await;
    };
    let now = now() as i64;
    let meta = CalendarEvent {
        names: vec![DavName {
            name: format!("{}_{}.ics", now, rand::random::<u64>()),
            parent_id,
        }],
        schedule_tag: Some(1),
        ..Default::default()
    };
    let content = CalendarEventContent {
        data: CalendarEventData::new_for(
            event_tree,
            Tz::Floating,
            server.core.groupware.max_ical_instances,
            &DefaultAlerts::disabled(),
            true,
        ),
        ..Default::default()
    };
    // Computed on the plaintext view; the alarm data is visible, so sealing
    // (below) changes none of it.
    let next_email_alarms = server
        .next_event_alarms(
            account_id,
            &EventAlarmUsers::new(account_id, &content)
                .caused_by(trc::location!())?
                .with_event_flags(meta.flags),
            &content.data,
            &[parent_id],
            now,
            &mut DefaultAlertsResolver::default(),
        )
        .await
        .caused_by(trc::location!())?;
    let mut event = ZaEvent::new(meta, content);
    // Spec 3.3, "Attendee side": the copy takes the pair the item carried.
    let context = za_owned_calendar_context(server, ctx.keys, parent_id)
        .await
        .map_err(|error| error.into_store_error(account_id, parent_id))?;
    let context =
        crate::calendar::sharing::boundary::za_repair_owned_context(server, ctx.keys, context)
            .await
            .map_err(|error| error.into_store_error(account_id, parent_id))?;
    seal_event_in_domain(&mut event, &context.domain, pair.as_ref())
        .map_err(|err| seal_error(err, account_id, u32::MAX))?;
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::drain_lease_pause_point(account_id).await;
    let mut batch = BatchBuilder::new();
    batch.with_account_id(account_id);
    server.limit_disk_quota(&mut batch, &account);
    server.limit_object_quota(&mut batch, &account, StorageQuota::MaxCalendarEvents);
    // The new event's id is reserved in this batch: nothing before the
    // commit needs it (no fan-out here); the item records it as a pending
    // id, as upstream's ingest records its notification's event.
    let document_id = batch.reserve_document_id(account_id, Collection::CalendarEvent);
    // Spec 6.3 step 3: fenced on the lease, with the item rewrite ahead of
    // the insert (one write; see the module doc).
    lease.lease().assert_in(&mut batch);
    context.assert_in(&mut batch);
    ctx.rewrite_in(&mut batch, tree.clone(), ItemEvent::Pending(document_id), 0)?;
    let (meta, content) = event.into_parts();
    meta.insert(
        content,
        ctx.account_info.changed_by(),
        account_id,
        document_id,
        None,
        next_email_alarms,
        &mut batch,
    )
    .caused_by(trc::location!())?;
    ctx.commit_or_reject_quota(batch, Some(lease), tree, None)
        .await
}

enum Step {
    Reject(ItipError),
    Merge(Vec<MergeAction>),
    Nothing,
    /// A mail-borne REFRESH's answer, upstream's one message to the sender.
    Answer(ItipMessage<ICalendar>),
}

/// iMIP spec D14: a REFRESH is answered by mail only after the item's
/// deletion commits; its archive assertion admits one session, so one
/// answer goes out per REFRESH. No item, pin or status is written for it.
/// PR review ruling P1 (fail closed): a recipient that is not of the mail
/// class (`za_mail_reachable`, the fan-out's classification) gets no answer,
/// traced without the address. Final review ruling A-2 (fail closed): nor
/// does a recipient that holds a contact pin in the account's pin record,
/// or any when no usable record decides it (`za_mail_withheld`). The item
/// is deleted all the same. A store error reading the pin state or a
/// recipient's class is returned (ruling O-4: section 7's transient class),
/// so the item stays pending and counts toward `DRAIN_ITEM_FAILURE_LIMIT`.
/// The answers are held by the abandonment guard from before the deletion's
/// commit (rulings A-3 and A-4): submitted after it returns `Ok`, traced as
/// dropped after an error and as abandoned when the request ends first;
/// each submission is traced by `za_imip_submit`.
async fn answer_refresh(
    ctx: &ItemCtx<'_>,
    event_id: u32,
    message: ItipMessage<ICalendar>,
) -> trc::Result<bool> {
    let account_id = ctx.keys.account_id;
    // `za_pin_state` answers an unusable record as a state (no record, its
    // mark read), so its error is a store error.
    let state = ctx
        .server
        .za_pin_state(ctx.keys)
        .await
        .caused_by(trc::location!())?;
    let mut answers = ZaImipPending::default();
    for to in &message.to {
        if !za_mail_reachable(ctx.server, account_id, to)
            .await
            .caused_by(trc::location!())?
        {
            za_imip_trace_unrouted(account_id, event_id);
            continue;
        }
        match za_mail_withheld(Some(&state), to) {
            Some(status) => {
                za_imip_trace_withheld(account_id, event_id, status == ITIP_STATUS_KEY_REFUSED)
            }
            None => answers.push(za_imip_mail(account_id, event_id, &message, to)),
        }
    }
    match answers.on_err(ctx.delete_mail("Refresh").await)? {
        true => {
            answers.submit(ctx.server).await;
            Ok(true)
        }
        false => {
            answers.dropped();
            Ok(false)
        }
    }
}

/// An instance's identity across two snapshots of one event: `None` for
/// the master, the RECURRENCE-ID's date and range for an override (never
/// its entry id, which a merge may change).
pub type InstanceKey = Option<(i64, bool)>;

pub fn instance_key(instance: &InstanceId) -> InstanceKey {
    match instance {
        InstanceId::Main => None,
        InstanceId::Recurrence(recurrence) => Some((recurrence.date, recurrence.this_and_future)),
    }
}

/// An event's attendance before a merge, per instance (spec 6.4,
/// "Delegates"): for each attendee's lowercased address, the lowercased
/// addresses of its DELEGATED-FROM.
struct Attendance {
    instances: AHashSet<InstanceKey>,
    attendees: AHashMap<(InstanceKey, String), AHashSet<String>>,
}

impl Attendance {
    fn of(snapshots: &ItipSnapshots<'_>) -> Self {
        let mut attendance = Attendance {
            instances: AHashSet::new(),
            attendees: AHashMap::new(),
        };
        for (instance, snapshot) in &snapshots.components {
            let instance = instance_key(instance);
            attendance.instances.insert(instance);
            // An address listed twice in one instance keeps the union of
            // its DELEGATED-FROM sets.
            for attendee in &snapshot.attendees {
                attendance
                    .attendees
                    .entry((instance, attendee.email.email.to_ascii_lowercase()))
                    .or_default()
                    .extend(delegators(attendee));
            }
        }
        attendance
    }

    /// Whether `attendee`, as `instance` of the merged event lists it, is
    /// new to that instance: it did not attend it before, or its
    /// DELEGATED-FROM gained an address (a new delegation to an existing
    /// attendee). An override the merge created inherits the attendance
    /// of the instance that governs it (`governing_instance`: the latest
    /// range override at or before it, else the master), so a delegator's
    /// own per-occurrence delegation does not make the delegator new.
    /// PR review ruling P2: a mail-borne merge (`mail`) counts only an
    /// attendee absent from that instance before; a DELEGATED-FROM gain on
    /// an existing attendee waits for the organizer's next update, as
    /// upstream's plain path, so an outside attendee cannot re-invite an
    /// existing key co-attendee without its token or an organizer edit.
    fn is_new(&self, instance: InstanceKey, attendee: &Attendee<'_>, mail: bool) -> bool {
        match self.attendees.get(&(
            governing_instance(instance, &self.instances),
            attendee.email.email.to_ascii_lowercase(),
        )) {
            Some(_) if mail => false,
            Some(before) => !delegators(attendee).is_subset(before),
            None => true,
        }
    }
}

/// An attendee's DELEGATED-FROM addresses, lowercased.
fn delegators(attendee: &Attendee<'_>) -> AHashSet<String> {
    attendee
        .delegated_from
        .iter()
        .map(|delegator| delegator.email.to_ascii_lowercase())
        .collect()
}

/// Spec 6.4, an event exists: upstream's merge (`commit_itip_merge`'s
/// rules) on the unsealed copy, committed sealed under the assertion on
/// the event's stored archive, with the fork's checks around it.
async fn merge(
    ctx: &ItemCtx<'_>,
    event_id: u32,
    tree: ICalendar,
    incoming: Incoming,
    sender_addresses: Vec<String>,
) -> trc::Result<bool> {
    let server = ctx.server;
    let account_id = ctx.keys.account_id;
    let addresses = ctx.account_info.addresses();
    let (Some(archive), Some(content_archive)) = server
        .archive_and_field(
            account_id,
            Collection::CalendarEvent,
            event_id,
            CalendarEventField::Content.field(),
        )
        .await
        .caused_by(trc::location!())?
    else {
        return ctx.reject(tree, None, &ItipError::EventNotFound).await;
    };
    let event_ = archive
        .to_unarchived::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let content_ = content_archive
        .to_unarchived::<CalendarEventContent>()
        .caused_by(trc::location!())?;
    let mut event = ZaEvent::new(
        event_
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?,
        content_
            .deserialize::<CalendarEventContent>()
            .caused_by(trc::location!())?,
    );
    let parent_id = event
        .meta
        .names
        .first()
        .ok_or_else(|| trc::StoreEvent::DataCorruption.into_err())?
        .parent_id;
    let contexts = za_owned_event_contexts(server, ctx.keys, &event.meta, parent_id)
        .await
        .map_err(|error| error.into_store_error(account_id, event_id))?;
    let contexts =
        crate::calendar::sharing::boundary::za_repair_owned_contexts(server, ctx.keys, contexts)
            .await
            .map_err(|error| error.into_store_error(account_id, event_id))?;
    let mut pairs = unseal_event_in_domain(&mut event, &contexts[0].domain)
        .map_err(|err| seal_error(err, account_id, event_id))?;
    // Spec 3.3, "Verification": a REPLY is authenticated by the replier's
    // token in this event's bundle, matched by address. A sender none of
    // whose addresses holds a pair is not a participant (never invited, or
    // removed): kept as history, as upstream's participant check rejects
    // it. Otherwise the replier is the sender's address whose pair holds
    // the token; a token that matches none is a forgery.
    let sender_address = match &incoming {
        Incoming::Reply(token) => {
            let Some(pairs) = pairs.as_ref().filter(|pairs| {
                sender_addresses
                    .iter()
                    .any(|address| pairs.get(address).is_some())
            }) else {
                return ctx
                    .reject(
                        tree,
                        Some(event_id),
                        &ItipError::SenderIsNotParticipant(sender_addresses[0].clone()),
                    )
                    .await;
            };
            match sender_addresses
                .iter()
                .find(|address| pairs.matches(address, token))
            {
                Some(address) => address.clone(),
                None => return ctx.drop_forgery().await,
            }
        }
        Incoming::Request(_) | Incoming::Cancel => sender_addresses[0].clone(),
        // iMIP spec 4.3: the carrier's address, as upstream's ingest takes
        // the mail's sender; no token is read or required.
        Incoming::Mail { sender } => sender.clone(),
    };
    let (step, is_organizer_update, before, event_uid, mail_delegates) = {
        let snapshots = match itip_snapshot(&event.content.data.event, addresses, false) {
            Ok(snapshots) => snapshots,
            Err(err) => return ctx.reject(tree, Some(event_id), &err).await,
        };
        let itip_snapshots = match itip_snapshot(&tree, addresses, false) {
            Ok(snapshots) => snapshots,
            Err(err) => return ctx.reject(tree, Some(event_id), &err).await,
        };
        let is_organizer_update = !itip_snapshots.organizer.email.is_local;
        let event_uid = snapshots.uid.to_string();
        // iMIP spec D15's gate: a mail-borne merge fans out to delegates only
        // for a REPLY into the account's own organizer copy (the stored
        // ORGANIZER is one of its addresses).
        let mail_delegates = matches!(incoming, Incoming::Mail { .. })
            && snapshots.organizer.email.is_local
            && matches!(itip_method(&tree), Ok(ICalendarMethod::Reply));
        // The attendance before the merge, per instance: an attendee the
        // merge adds to an instance is a delegate (spec 6.4, "Delegates").
        let before = Attendance::of(&snapshots);
        let step = if matches!(incoming, Incoming::Reply(_))
            && is_stale_reply(&snapshots, &itip_snapshots)
        {
            Step::Reject(ItipError::OutOfSequence)
        } else {
            match itip_process_message(
                &event.content.data.event,
                snapshots,
                &tree,
                itip_snapshots,
                sender_address.clone(),
                RecipientPolicy::new(&server.core.groupware, event.meta.flags),
            ) {
                Ok(MergeResult::Actions(changes)) => Step::Merge(changes),
                Ok(MergeResult::None) => Step::Nothing,
                // Only REFRESH yields a message. By mail it is answered
                // (iMIP spec D14); it cannot come from the fan-out (spec
                // 6.4 step 3).
                Ok(MergeResult::Message(message)) if matches!(incoming, Incoming::Mail { .. }) => {
                    Step::Answer(message)
                }
                Ok(MergeResult::Message(_)) => {
                    Step::Reject(ItipError::UnsupportedMethod(ICalendarMethod::Refresh))
                }
                Err(err) => Step::Reject(err),
            }
        };
        (step, is_organizer_update, before, event_uid, mail_delegates)
    };
    let changes = match step {
        Step::Reject(err) => return ctx.reject(tree, Some(event_id), &err).await,
        // iMIP spec D7: upstream's ingest writes no inbox item when the
        // merge changes nothing.
        Step::Nothing if ctx.mail => return ctx.delete_mail("NothingToMerge").await,
        Step::Nothing => return ctx.rewrite_only(tree, Some(event_id), 0).await,
        Step::Answer(message) => return answer_refresh(ctx, event_id, message).await,
        Step::Merge(changes) => changes,
    };
    // 1.0.1's `commit_itip_merge`: the stored classification is kept when
    // the merge would change it. Upstream reads it from the row's flags,
    // which it derives from the stored tree (`refresh_from_content`); here
    // it is read from the unsealed stored tree itself, because a sealed
    // non-standard CLASS leaves the sealed row's flags clear (field policy
    // 2's recorded limit) while the tree an ordinary account stores would
    // have set them. So the kept classification, and the tree, are an
    // ordinary account's for every value.
    let privacy = stored_classification(&event.content.data.event);
    if let Err(err) = itip_merge_changes(&mut event.content.data.event, &tree, changes) {
        return ctx.reject(tree, Some(event_id), &err).await;
    }
    keep_classification(&mut event.content.data.event, privacy);
    // The plaintext size, as upstream's merge measures it; the stored size
    // is the sealed tree's (`refresh_from_content`).
    let new_size = SizeWriter::ical(&event.content.data.event) as u32;
    if new_size > server.core.groupware.max_ical_size as u32 {
        return ctx
            .reject(tree, Some(event_id), &ItipError::EventTooLarge)
            .await;
    }
    let account = server
        .account(account_id)
        .await
        .caused_by(trc::location!())?;
    let extra_bytes = (new_size as u64).saturating_sub(event_.inner.size.to_native() as u64);
    if extra_bytes > 0
        && let Err(err) = server
            .has_available_quota(account.as_ref(), extra_bytes)
            .await
    {
        return if is_over_quota(&err) {
            ctx.reject(tree, Some(event_id), &ItipError::QuotaExceeded)
                .await
        } else {
            Err(err.caused_by(trc::location!()))
        };
    }
    // Held only by a REPLY merge that fans out to delegates.
    let mut reply_lease = None;
    // Spec 3.3: the pairs the copy keeps.
    match &incoming {
        // An accepted REQUEST replaces this attendee's pair with the
        // incoming one, so a removed and re-added attendee follows the
        // organizer to the new token.
        Incoming::Request(Some(pair)) => {
            let pairs = pairs.get_or_insert_with(TokenPairs::new);
            // `to_vec` hands out a plain copy the caller zeroes.
            let incoming = Zeroizing::new(pair.to_vec());
            for (address, token) in incoming.iter() {
                pairs.insert(address, *token);
            }
        }
        Incoming::Request(None) | Incoming::Cancel => {}
        // iMIP spec 4.3 point 4: mail takes `Request(None)`'s rule, the
        // copy's pairs as stored, except a REPLY into the account's own
        // organizer copy, which fans out to its new delegates below (D15).
        Incoming::Mail { .. } if !mail_delegates => {}
        // Delegates the merge added get their token from the organizer,
        // whose session this is, through the same fan-out a PUT uses:
        // upstream's organizer request builder on the merged tree,
        // restricted to the new attendees (spec 6.4, "Delegates").
        Incoming::Reply(_) | Incoming::Mail { .. } => {
            // Upgrade ruling 154: the new delegates whose invitation's split
            // failed, for their `5.1`.
            let mut split_failed: Vec<String> = Vec::new();
            let mail = matches!(incoming, Incoming::Mail { .. });
            let new_attendees = match itip_snapshot(&event.content.data.event, addresses, false) {
                Ok(after) => {
                    // Compared per instance, so a delegate who already
                    // attends another instance is new to this one, and an
                    // attendee that became a delegate is new too; the set
                    // dedups the addresses for delivery.
                    let mut new_attendees: AHashSet<String> = AHashSet::new();
                    for (instance, snapshot) in &after.components {
                        let instance = instance_key(instance);
                        for attendee in &snapshot.attendees {
                            if before.is_new(instance, attendee, mail) {
                                new_attendees.insert(attendee.email.email.to_ascii_lowercase());
                            }
                        }
                    }
                    if new_attendees.is_empty() {
                        Ok(None)
                    } else {
                        // Restricted to the new attendees, then split per
                        // recipient under the event's policy as 1.0 splits
                        // upstream's own organizer messages (upgrade ruling
                        // 131): each delegate gets its slice. A split error
                        // is the builders' error, and the delegates it
                        // leaves uninvited get `5.1` (upgrade ruling 154).
                        organizer_request_full(&event.content.data.event, &after, None, false)
                            .and_then(|mut messages| {
                                for message in &mut messages {
                                    message.to.retain(|to| {
                                        new_attendees.contains(&to.to_ascii_lowercase())
                                    });
                                }
                                messages.retain(|message| !message.to.is_empty());
                                let delegates: Vec<String> = messages
                                    .iter()
                                    .flat_map(|message| message.to.iter().cloned())
                                    .collect();
                                itip_messages_per_recipient(
                                    messages,
                                    RecipientPolicy::new(&server.core.groupware, event.meta.flags),
                                )
                                .inspect_err(|_| split_failed = delegates)
                            })
                            .map(Some)
                    }
                }
                Err(err) => Err(err),
            };
            match new_attendees {
                Ok(Some(messages)) => {
                    if !messages.is_empty() {
                        // Two sessions of the organizer (two drain locks)
                        // can both pass the re-read of this REPLY, and the
                        // fan-out delivers its REQUEST outside the item's
                        // transaction: the loser's write rolls back but its
                        // delivered token stays and would replace the
                        // winner's at the delegate. The per-UID lease a PUT
                        // of this UID takes serializes them.
                        let Some(lease) =
                            ZaUidLease::acquire(server.store(), account_id, &event_uid)
                                .await
                                .caused_by(trc::location!())?
                        else {
                            trc::event!(
                                Store(trc::StoreEvent::AssertValueFailed),
                                AccountId = account_id,
                                DocumentId = ctx.document_id,
                                Details = "zero-access: drain creation lease busy",
                            );
                            return Ok(false);
                        };
                        let lease = ZaUidLeaseGuard::new(server.store().clone(), lease);
                        // Under the lease: another session that finished
                        // this item first leaves nothing to do.
                        if !ctx.still_pending().await? {
                            lease.release().await;
                            return Ok(true);
                        }
                        reply_lease = Some(lease);
                    }
                    // Each delegate item's write asserts the lease too: a
                    // session whose lease expired and was taken over while
                    // it waited inside the fan-out delivers nothing more,
                    // so a stale token never reaches a delegate after the
                    // successor's (spec 6.3).
                    let fan_out = match server
                        .za_fan_out(
                            ctx.account_info,
                            ctx.account_info,
                            ctx.account_info,
                            ctx.keys,
                            messages
                                .into_iter()
                                .map(ZaFanOutMessage::upstream)
                                .collect(),
                            pairs.take(),
                            &event.content.data.event,
                            ZaSource {
                                document_id: event_id,
                                uid: &event_uid,
                            },
                            ZaFanOutKind::Change,
                            // iMIP spec 5.5 (D15): internal delegates only,
                            // in the release 2 arm and the mail arm alike.
                            true,
                            RecipientPolicy::new(&server.core.groupware, event.meta.flags),
                            reply_lease.as_ref().map(|lease| lease.lease()),
                            None,
                        )
                        .await
                    {
                        Ok(fan_out) => fan_out,
                        // The key holds a successor's value, or is gone
                        // (the successor released it, or the sweep cleared
                        // it): nothing to release. The same outcome as a
                        // lost commit fence.
                        Err(err) if err.is_assertion_failure() => {
                            if let Some(lease) = reply_lease.take() {
                                lease.abandon();
                            }
                            return Err(err
                                .details("zero-access: drain fan-out lost its lease")
                                .caused_by(trc::location!()));
                        }
                        Err(err) => return Err(err.caused_by(trc::location!())),
                    };
                    za_set_schedule_status(&mut event.content.data.event, &fan_out.statuses);
                    za_set_sequences(&mut event.content.data.event, &fan_out.sequences);
                    pairs = fan_out.pairs;
                }
                Ok(None) => {}
                // The reply still applies; the delegate's invitation is
                // not built. Traced as upstream traces a builder error. A
                // split error (upgrade ruling 154) is each uninvited new
                // delegate's `5.1` on its ATTENDEE lines, so the organizer
                // sees the failure, as a failed delivery shows it.
                Err(err) => {
                    trc::event!(
                        Calendar(trc::CalendarEvent::ItipMessageError),
                        AccountId = account_id,
                        DocumentId = ctx.document_id,
                        Reason = itip_reason(&err),
                        Details = "zero-access: delegate invitation not built",
                    );
                    let statuses: Vec<ZaScheduleStatus> = split_failed
                        .into_iter()
                        .map(|address| ZaScheduleStatus {
                            address,
                            on_organizer: false,
                            status: ITIP_STATUS_DELIVERY_FAILED,
                        })
                        .collect();
                    za_set_schedule_status(&mut event.content.data.event, &statuses);
                }
            }
        }
    }
    // 1.0.1's `commit_itip_merge`: the calendar's default alerts, the
    // alarms before and after, on the plaintext view (their data is
    // visible, so sealing below changes none of it).
    let now = now() as i64;
    let mut resolver = DefaultAlertsResolver::default();
    let calendar_ids = event
        .meta
        .names
        .iter()
        .map(DavName::parent_id)
        .collect::<Vec<_>>();
    let default_alerts = resolver
        .resolve_for_content(
            server,
            account_id,
            account_id,
            &event.content,
            calendar_ids.iter().copied(),
        )
        .await
        .caused_by(trc::location!())?;
    let prev_email_alarms = server
        .next_event_alarms(
            account_id,
            &EventAlarmUsers::new(account_id, content_.inner)
                .caused_by(trc::location!())?
                .with_event_flags(event_.inner.flags.to_native()),
            &content_.inner.data,
            &calendar_ids,
            now,
            &mut resolver,
        )
        .await
        .caused_by(trc::location!())?;
    event.content.data = CalendarEventData::new_for(
        std::mem::take(&mut event.content.data.event),
        Tz::Floating,
        server.core.groupware.max_ical_instances,
        &default_alerts,
        true,
    );
    let next_email_alarms = server
        .next_event_alarms(
            account_id,
            &EventAlarmUsers::new(account_id, &event.content)
                .caused_by(trc::location!())?
                .with_event_flags(event.meta.flags),
            &event.content.data,
            &calendar_ids,
            now,
            &mut resolver,
        )
        .await
        .caused_by(trc::location!())?;
    if is_organizer_update {
        match &mut event.meta.schedule_tag {
            Some(schedule_tag) => *schedule_tag += 1,
            None => event.meta.schedule_tag = Some(1),
        }
    }
    seal_event_in_domain(&mut event, &contexts[0].domain, pairs.as_ref())
        .map_err(|err| seal_error(err, account_id, event_id))?;
    let mut batch = BatchBuilder::new();
    batch.with_account_id(account_id);
    server.limit_disk_quota(&mut batch, &account);
    // A delegating REPLY is fenced on its lease first, as `import` is.
    if let Some(lease) = &reply_lease {
        lease.lease().assert_in(&mut batch);
    }
    for context in &contexts {
        context.assert_in(&mut batch);
    }
    // The alarm tasks and the item rewrite first, ahead of the event update,
    // whose commit point closes the chunk: one transaction under the
    // fences (see the module doc).
    server
        .replace_event_alarms(
            account_id,
            event_id,
            prev_email_alarms,
            next_email_alarms,
            &mut AlarmTasks::None,
            &mut batch,
        )
        .await
        .caused_by(trc::location!())?;
    ctx.rewrite_in(
        &mut batch,
        tree.clone(),
        ItemEvent::Assigned(event_id),
        EVENT_NOTIFICATION_IS_CHANGE,
    )?;
    let (meta, content) = event.into_parts();
    meta.update_full(
        content,
        ctx.account_info.changed_by(),
        event_,
        content_.inner,
        account_id,
        event_id,
        None,
        &mut batch,
    )
    .caused_by(trc::location!())?;
    ctx.commit_or_reject_quota(batch, reply_lease, tree, Some(event_id))
        .await
}

/// The classification upstream's merge preserves, read from the unsealed
/// stored tree: what `refresh_from_content` derives for an ordinary
/// account's stored tree, so `EventPrivacy::from_flags` of its row.
fn stored_classification(stored: &ICalendar) -> EventPrivacy {
    stored.privacy()
}

/// `commit_itip_merge`'s preservation, verbatim: a scheduling component
/// whose merged CLASS reads differently from `stored` gets `stored` back.
fn keep_classification(merged: &mut ICalendar, stored: EventPrivacy) {
    if merged
        .components
        .iter()
        .filter(|component| component.component_type.is_scheduling_object())
        .any(|component| {
            component
                .entries
                .iter()
                .filter(|entry| entry.name == ICalendarProperty::Class)
                .filter_map(|entry| entry.values.first())
                .map(EventPrivacy::from_value)
                .max()
                .unwrap_or_default()
                != stored
        })
    {
        merged.set_privacy(stored);
    }
}

/// The event an item's rewrite records: assigned, or reserved in the same
/// batch (an import).
#[derive(Clone, Copy)]
enum ItemEvent {
    None,
    Assigned(u32),
    Pending(Slot),
}

impl From<Option<u32>> for ItemEvent {
    fn from(event_id: Option<u32>) -> Self {
        match event_id {
            Some(event_id) => ItemEvent::Assigned(event_id),
            None => ItemEvent::None,
        }
    }
}

impl ItemCtx<'_> {
    /// The re-read under a lease: whether the item is still the archive
    /// this pass read, pending. Another session that finished it (or a
    /// deletion) while this one waited for the lease leaves nothing to do.
    async fn still_pending(&self) -> trc::Result<bool> {
        let Some((current, current_content)) =
            read_item(self.server, self.keys.account_id, self.document_id).await?
        else {
            return Ok(false);
        };
        Ok(current.inner == self.stored.inner
            && current.version == self.stored.version
            && current_content.inner == self.stored_content.inner
            && archived_item_is_pending(
                current
                    .unarchive::<CalendarEventNotification>()
                    .caused_by(trc::location!())?,
            ))
    }

    /// Spec 6.4 step 5: the item resealed under the master key, bit
    /// cleared, no carrier, `event_id` and the change flag as upstream's
    /// ingest sets them, `created` kept. The record is
    /// `sealed_item_record`'s (upgrade ruling 13): the content is
    /// `Itip { message: <the drained sealed tree>, previous: None,
    /// current: None }`, never a plaintext snapshot. No commit point of its
    /// own: the callers put it ahead of the event write, or end the batch
    /// themselves.
    fn rewrite_in(
        &self,
        batch: &mut BatchBuilder,
        mut tree: ICalendar,
        event: impl Into<ItemEvent>,
        flags: u16,
    ) -> trc::Result<()> {
        let account_id = self.keys.account_id;
        seal_drained_item(&mut tree, self.keys, account_id)
            .map_err(|err| seal_error(err, account_id, self.document_id))?;
        let (event_id, pending_event) = match event.into() {
            ItemEvent::None => (None, None),
            ItemEvent::Assigned(event_id) => (Some(event_id), None),
            ItemEvent::Pending(slot) => (None, Some(slot)),
        };
        let (mut item, content) =
            sealed_item_record(tree, self.changed_by.clone(), event_id, false);
        item.flags |= flags;
        item.created = self.created;
        item.modified = now() as i64;
        let update = SplitUpdate::full(
            self.stored
                .to_unarchived::<CalendarEventNotification>()
                .caused_by(trc::location!())?,
            self.stored_content
                .unarchive::<CalendarEventNotificationContent>()
                .caused_by(trc::location!())?,
            item,
            content,
            (),
        )
        .caused_by(trc::location!())?;
        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEventNotification)
            .with_document(self.document_id)
            .custom(update.into_builder(self.account_info.changed_by(), pending_event))
            .caused_by(trc::location!())?;
        Ok(())
    }

    /// The rewrite alone (nothing to merge, or kept as history).
    async fn rewrite_only(
        &self,
        tree: ICalendar,
        event_id: Option<u32>,
        flags: u16,
    ) -> trc::Result<bool> {
        let mut batch = BatchBuilder::new();
        self.rewrite_in(&mut batch, tree, ItemEvent::from(event_id), flags)?;
        batch.commit_point();
        self.commit(batch, None).await
    }

    /// Spec 6.4 step 4: an iTIP-level rejection leaves the calendar
    /// unchanged; the item is kept as history. The reason is a fixed name.
    /// A mail-borne item is deleted instead (iMIP spec D7: upstream's ingest
    /// writes no inbox item then).
    async fn reject(
        &self,
        tree: ICalendar,
        event_id: Option<u32>,
        err: &ItipError,
    ) -> trc::Result<bool> {
        if self.mail {
            return self.delete_mail(itip_reason(err)).await;
        }
        trc::event!(
            Calendar(trc::CalendarEvent::ItipMessageError),
            AccountId = self.keys.account_id,
            DocumentId = self.document_id,
            Reason = itip_reason(err),
            Details = "zero-access: inbox item kept as history",
        );
        self.rewrite_only(tree, event_id, 0).await
    }

    async fn reject_delegated(&self, tree: ICalendar) -> trc::Result<bool> {
        trc::event!(
            Calendar(trc::CalendarEvent::ItipMessageError),
            AccountId = self.keys.account_id,
            DocumentId = self.document_id,
            Reason = "zero-access: delegated sender not verified",
            Details = "zero-access: inbox item kept as history",
        );
        self.rewrite_only(tree, None, 0).await
    }

    /// Spec 6.5, permanent: the item is deleted, since nothing can ever
    /// read it. `reason` is a fixed string.
    async fn delete_undecodable(&self, reason: &'static str) -> trc::Result<bool> {
        trc::event!(
            Calendar(trc::CalendarEvent::ItipMessageError),
            AccountId = self.keys.account_id,
            DocumentId = self.document_id,
            Reason = reason,
            Details = "zero-access: undecodable inbox item deleted",
        );
        self.delete().await
    }

    /// iMIP spec D7, section 7: a mail-borne item whose outcome upstream's
    /// ingest leaves no inbox item for (an iTIP-level rejection, the
    /// key-account sender, `AutoAddDisabled`, `EventNotFound`, a merge that
    /// changes nothing, a REFRESH) is deleted. `reason` is a fixed name.
    /// Traced once the deletion committed (final review, ledger 142): a
    /// session that loses the item's assertion, or a store error, traces no
    /// deletion.
    async fn delete_mail(&self, reason: &'static str) -> trc::Result<bool> {
        let deleted = self.delete().await?;
        if deleted {
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageError),
                AccountId = self.keys.account_id,
                DocumentId = self.document_id,
                Reason = reason,
                Details = ZA_IMIP_ITEM_DELETED,
            );
        }
        Ok(deleted)
    }

    /// Spec 6.5, token mismatch: a traced drop of a forgery, a REPLY from
    /// a participant (an address holding a pair) whose token is wrong or
    /// missing; the event is unchanged. A sender with no pair is not a
    /// participant: `merge` keeps its REPLY as history instead.
    async fn drop_forgery(&self) -> trc::Result<bool> {
        trc::event!(
            Security(trc::SecurityEvent::Unauthorized),
            AccountId = self.keys.account_id,
            DocumentId = self.document_id,
            Details = "zero-access: reply token mismatch",
        );
        self.delete().await
    }

    async fn delete(&self) -> trc::Result<bool> {
        let mut batch = BatchBuilder::new();
        DestroyArchive(
            self.stored
                .to_unarchived::<CalendarEventNotification>()
                .caused_by(trc::location!())?,
        )
        .delete(
            self.account_info.changed_by(),
            self.keys.account_id,
            self.document_id,
            &mut batch,
        )
        .caused_by(trc::location!())?;
        self.commit(batch, None).await
    }

    /// One store write per item. A lost assertion (the lease's fence, the
    /// item's or the event's archive) aborts the pass (spec 6.3); the
    /// lease, if held, is released either way.
    async fn commit(
        &self,
        batch: BatchBuilder,
        lease: Option<ZaUidLeaseGuard>,
    ) -> trc::Result<bool> {
        #[cfg(feature = "test_mode")]
        if common::za::test_hooks::take_drain_fail(self.keys.account_id)
            || common::za::test_hooks::drain_item_fails(self.keys.account_id, self.document_id)
        {
            if let Some(lease) = lease {
                lease.release().await;
            }
            return Err(trc::StoreEvent::UnexpectedError
                .into_err()
                .details("zero-access test hook: drain commit failure")
                .caused_by(trc::location!()));
        }
        let result = self.server.commit_batch(batch).await;
        if let Some(lease) = lease {
            lease.release().await;
        }
        match result {
            Ok(_) => Ok(true),
            Err(err) if err.is_assertion_failure() => Err(err
                .details("zero-access: drain commit lost its assertion")
                .caused_by(trc::location!())),
            Err(err) => Err(err.caused_by(trc::location!())),
        }
    }

    /// `commit`, except that the in-batch quota limits upstream's ingest
    /// applies (1.0.1: `limit_disk_quota`, `limit_object_quota`) reject the
    /// item as history, as the fork's own quota check does, instead of
    /// counting as the item's failure.
    async fn commit_or_reject_quota(
        &self,
        batch: BatchBuilder,
        lease: Option<ZaUidLeaseGuard>,
        tree: ICalendar,
        event_id: Option<u32>,
    ) -> trc::Result<bool> {
        match self.commit(batch, lease).await {
            Err(err) if is_over_quota(&err) => {
                self.reject(tree, event_id, &ItipError::QuotaExceeded).await
            }
            result => result,
        }
    }
}

/// Only a quota limit is "over quota", as `fanout::za_write_pending_item`
/// classifies it; any other error of the quota check is a store error.
fn is_over_quota(err: &trc::Error) -> bool {
    err.matches(trc::EventType::Limit(trc::LimitEvent::Quota))
}

/// A fixed name per `SealError`; its `Structure` text is a fixed string too.
fn seal_reason(err: &SealError) -> &'static str {
    match err {
        SealError::Structure(what) => what,
        _ => "unseal failure",
    }
}

/// A fixed name per `ItipError` variant: upstream traces `to_string()`,
/// whose `SenderIsNotParticipant` and `UnknownParticipant` texts carry an
/// address (spec 10).
pub fn itip_reason(err: &ItipError) -> &'static str {
    match err {
        ItipError::NoSchedulingInfo => "NoSchedulingInfo",
        ItipError::OtherSchedulingAgent => "OtherSchedulingAgent",
        ItipError::NotOrganizer => "NotOrganizer",
        ItipError::NotOrganizerNorAttendee => "NotOrganizerNorAttendee",
        ItipError::NothingToSend => "NothingToSend",
        ItipError::MissingUid => "MissingUid",
        ItipError::MultipleUid => "MultipleUid",
        ItipError::MultipleOrganizer => "MultipleOrganizer",
        ItipError::MultipleObjectTypes => "MultipleObjectTypes",
        ItipError::MultipleObjectInstances => "MultipleObjectInstances",
        ItipError::CannotModifyProperty(_) => "CannotModifyProperty",
        ItipError::CannotModifyInstance => "CannotModifyInstance",
        ItipError::CannotModifyAddress => "CannotModifyAddress",
        ItipError::OrganizerMismatch => "OrganizerMismatch",
        ItipError::MissingMethod => "MissingMethod",
        ItipError::InvalidComponentType => "InvalidComponentType",
        ItipError::OutOfSequence => "OutOfSequence",
        ItipError::OrganizerIsLocalAddress => "OrganizerIsLocalAddress",
        ItipError::SenderIsNotOrganizerNorAttendee => "SenderIsNotOrganizerNorAttendee",
        ItipError::SenderIsNotOrganizer => "SenderIsNotOrganizer",
        ItipError::SenderIsNotParticipant(_) => "SenderIsNotParticipant",
        ItipError::UnknownParticipant(_) => "UnknownParticipant",
        ItipError::UnsupportedMethod(_) => "UnsupportedMethod",
        ItipError::ICalendarParseError => "ICalendarParseError",
        ItipError::EventNotFound => "EventNotFound",
        ItipError::EventTooLarge => "EventTooLarge",
        ItipError::QuotaExceeded => "QuotaExceeded",
        ItipError::NoDefaultCalendar => "NoDefaultCalendar",
        ItipError::AutoAddDisabled => "AutoAddDisabled",
        ItipError::TooManyRecipients => "TooManyRecipients",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calcard::{Entry, Parser};

    fn parse(text: &str) -> ICalendar {
        match Parser::new(text).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        }
    }

    fn event(sequence: Option<i64>) -> ICalendar {
        let sequence = sequence.map_or(String::new(), |s| format!("SEQUENCE:{s}\r\n"));
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:stale-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\n{sequence}ORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        ))
    }

    fn reply(sequence: Option<i64>) -> ICalendar {
        let sequence = sequence.map_or(String::new(), |s| format!("SEQUENCE:{s}\r\n"));
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REPLY\r\nBEGIN:VEVENT\r\nUID:stale-1\r\nDTSTAMP:20240101T000000Z\r\n{sequence}ORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        ))
    }

    fn organizer() -> Vec<String> {
        vec!["org@example.com".to_string()]
    }

    /// The drain's merge path on the attendee's copy, as `merge` runs it:
    /// upstream's processing and merge on the unsealed tree, then the seal.
    fn merged_and_sealed(stored: &str, incoming: &str) -> (ZaEvent, ZaEvent) {
        use crate::calendar::seal::{ZaKeyDomain, seal_event, unseal_event};
        use vault::{keys::Secret, session::SessionKeys};
        let keys = SessionKeys::new(9, 1, Secret::random());
        let attendee = vec!["att@example.com".to_string()];
        let content = |ical| CalendarEventContent {
            data: CalendarEventData::new(ical, Tz::Floating, 100),
            ..Default::default()
        };
        let mut event = ZaEvent::new(
            CalendarEvent {
                names: vec![DavName {
                    name: "e.ics".into(),
                    parent_id: 1,
                }],
                ..Default::default()
            },
            content(parse(stored)),
        );
        seal_event(&mut event, &keys, 9, None).unwrap();
        unseal_event_in_domain(&mut event, &ZaKeyDomain::owner(&keys)).unwrap();
        let itip = parse(incoming);
        let snapshots = itip_snapshot(&event.content.data.event, &attendee, false).unwrap();
        let itip_snapshots = itip_snapshot(&itip, &attendee, false).unwrap();
        let policy = RecipientPolicy {
            visibility: crate::scheduling::recipient::AttendeeVisibility::All,
            max_recipients: usize::MAX,
            max_instances: usize::MAX,
        };
        let MergeResult::Actions(changes) = itip_process_message(
            &event.content.data.event,
            snapshots,
            &itip,
            itip_snapshots,
            "org@example.com".into(),
            policy,
        )
        .unwrap() else {
            panic!("no merge");
        };
        itip_merge_changes(&mut event.content.data.event, &itip, changes).unwrap();
        event.content.data = CalendarEventData::new_for(
            std::mem::take(&mut event.content.data.event),
            Tz::Floating,
            100,
            &DefaultAlerts::disabled(),
            true,
        );
        let plain = event.clone();
        seal_event(&mut event, &keys, 9, None).unwrap();
        let sealed = event.clone();
        unseal_event(&mut event, &keys, 9).unwrap();
        assert_eq!(
            event, plain,
            "the merged event round-trips through the seal"
        );
        (plain, sealed)
    }

    #[test]
    fn incoming_timezone_update_survives_seal() {
        // A REQUEST that moves one occurrence of a weekly series into a
        // timezone the stored copy does not have yet: the merge adds the
        // VTIMEZONE and the override, and the seal keeps both, their TZIDs
        // and the time ranges computed on the plaintext.
        const STORED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:tz-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990105T090000Z\r\nDTEND:20990105T100000Z\r\nRRULE:FREQ=WEEKLY;COUNT=4\r\nSEQUENCE:0\r\nSUMMARY:tz canary\r\nORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        const INCOMING: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VTIMEZONE\r\nTZID:Europe/Berlin\r\nBEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nRRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nTZNAME:CET\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:19700329T020000\r\nRRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=3\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0200\r\nTZNAME:CEST\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:tz-1\r\nDTSTAMP:20240102T000000Z\r\nRECURRENCE-ID:20990112T090000Z\r\nDTSTART;TZID=Europe/Berlin:20990112T150000\r\nDTEND;TZID=Europe/Berlin:20990112T160000\r\nSEQUENCE:1\r\nSUMMARY:tz canary moved\r\nORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (plain, sealed) = merged_and_sealed(STORED, INCOMING);
        let tz = |event: &ZaEvent| {
            event
                .content
                .data
                .event
                .components
                .iter()
                .filter(|c| {
                    c.component_type == calcard::icalendar::ICalendarComponentType::VTimezone
                })
                .count()
        };
        assert_eq!(tz(&plain), 1, "the merge added the VTIMEZONE");
        assert_eq!(tz(&sealed), 1, "the seal kept it");
        assert_eq!(
            plain.content.data.event.components.len(),
            sealed.content.data.event.components.len()
        );
        assert_eq!(
            plain.content.data.time_ranges,
            sealed.content.data.time_ranges
        );
        let dump = sealed.content.data.event.to_string();
        assert!(dump.contains("TZID=Europe/Berlin"), "{dump}");
        assert!(!dump.contains("canary"), "{dump}");
        assert!(!dump.contains("CEST"), "TZNAME is sealed: {dump}");
        // The sealed tree's time ranges, recomputed as upstream's builder
        // would from the stored tree, are the plaintext's.
        let recomputed =
            CalendarEventData::new(sealed.content.data.event.clone(), Tz::Floating, 100);
        assert_eq!(recomputed.time_ranges, plain.content.data.time_ranges);
    }

    /// Review M1: the drain's merge keeps the stored classification as an
    /// ordinary account's `commit_itip_merge` does, for every stored value,
    /// a sealed non-standard one included.
    #[test]
    fn the_merge_keeps_the_classification_an_ordinary_account_keeps() {
        use crate::calendar::seal::{ZaKeyDomain, seal_event};
        use common::storage::index::SplitObject;
        use vault::{keys::Secret, session::SessionKeys};
        let stored_tree = |class: &str| {
            parse(&format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:class-2\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990105T090000Z\r\nDTEND:20990105T100000Z\r\nSEQUENCE:0\r\nCLASS:{class}\r\nORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            ))
        };
        let incoming = |class: Option<&str>| {
            let class = class.map_or(String::new(), |class| format!("CLASS:{class}\r\n"));
            parse(&format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:class-2\r\nDTSTAMP:20240102T000000Z\r\nDTSTART:20990105T100000Z\r\nDTEND:20990105T110000Z\r\nSEQUENCE:1\r\n{class}ORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            ))
        };
        let attendee = vec!["att@example.com".to_string()];
        let policy = || RecipientPolicy {
            visibility: crate::scheduling::recipient::AttendeeVisibility::All,
            max_recipients: usize::MAX,
            max_instances: usize::MAX,
        };
        let merge = |stored: &ICalendar, itip: &ICalendar| {
            let mut merged = stored.clone();
            let MergeResult::Actions(changes) = itip_process_message(
                stored,
                itip_snapshot(stored, &attendee, false).unwrap(),
                itip,
                itip_snapshot(itip, &attendee, false).unwrap(),
                "org@example.com".into(),
                policy(),
            )
            .unwrap() else {
                panic!("no merge");
            };
            itip_merge_changes(&mut merged, itip, changes).unwrap();
            merged
        };
        let row_flags = |tree: &ICalendar| {
            let content = CalendarEventContent {
                data: CalendarEventData::new(tree.clone(), Tz::Floating, 100),
                ..Default::default()
            };
            let mut meta = CalendarEvent::default();
            meta.refresh_from_content(&content, ());
            meta.flags
        };
        let keys = SessionKeys::new(9, 1, Secret::random());
        let domain = ZaKeyDomain::owner(&keys);
        for class in ["PUBLIC", "PRIVATE", "CONFIDENTIAL", "X-SOMETHING"] {
            for update in [None, Some("PUBLIC"), Some("PRIVATE"), Some("X-OTHER")] {
                let stored = stored_tree(class);
                let itip = incoming(update);
                // An ordinary account: upstream's merge, the classification
                // read from the row's flags over the plaintext tree.
                let mut ordinary = merge(&stored, &itip);
                keep_classification(&mut ordinary, EventPrivacy::from_flags(row_flags(&stored)));
                // A key account: the drain, on the unsealed stored tree.
                let mut sealed = ZaEvent::new(
                    CalendarEvent {
                        names: vec![DavName {
                            name: "e.ics".into(),
                            parent_id: 1,
                        }],
                        ..Default::default()
                    },
                    CalendarEventContent {
                        data: CalendarEventData::new(stored.clone(), Tz::Floating, 100),
                        ..Default::default()
                    },
                );
                seal_event(&mut sealed, &keys, 9, None).unwrap();
                let mut sealed_row = CalendarEvent::default();
                sealed_row.refresh_from_content(&sealed.content, ());
                unseal_event_in_domain(&mut sealed, &domain).unwrap();
                let privacy = stored_classification(&sealed.content.data.event);
                assert_eq!(privacy, EventPrivacy::from_flags(row_flags(&stored)));
                if class == "X-SOMETHING" {
                    // The sealed row reads public: the flags are not the
                    // stored classification's source (review M1).
                    assert!(EventPrivacy::from_flags(sealed_row.flags).is_public());
                    assert!(!privacy.is_public());
                }
                let mut drained = merge(&sealed.content.data.event, &itip);
                keep_classification(&mut drained, privacy);
                assert_eq!(drained, ordinary, "{class} <- {update:?}");
                // The flags upstream derives from what each stores agree,
                // except for the recorded limit: a non-standard value stays
                // sealed, so the sealed row's flags are clear.
                let mut stored_sealed = ZaEvent::new(sealed.meta.clone(), {
                    let mut content = sealed.content.clone();
                    content.data.event = drained.clone();
                    content
                });
                seal_event(&mut stored_sealed, &keys, 9, None).unwrap();
                let mut meta = CalendarEvent::default();
                meta.refresh_from_content(&stored_sealed.content, ());
                let expected = if drained.privacy() != EventPrivacy::Public
                    && drained.to_string().contains("CLASS:X-")
                {
                    0
                } else {
                    row_flags(&ordinary)
                        & (crate::calendar::EVENT_PRIVATE | crate::calendar::EVENT_SECRET)
                };
                assert_eq!(
                    meta.flags & (crate::calendar::EVENT_PRIVATE | crate::calendar::EVENT_SECRET),
                    expected,
                    "{class} <- {update:?}"
                );
            }
        }
    }

    #[test]
    fn standard_class_stays_visible_and_derives_the_flags() {
        use crate::calendar::{EVENT_PRIVATE, EVENT_SECRET};
        use common::storage::index::SplitObject;
        for (class, private, secret) in [
            ("PUBLIC", false, false),
            ("PRIVATE", true, false),
            ("CONFIDENTIAL", false, true),
            // The recorded limit: a non-standard value is sealed, so the
            // stored tree reads as public and both flags stay clear.
            ("X-SOMETHING", false, false),
        ] {
            let (_, mut sealed) = merged_and_sealed(
                &format!(
                    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:class-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990105T090000Z\r\nDTEND:20990105T100000Z\r\nSEQUENCE:0\r\nCLASS:{class}\r\nORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
                ),
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:class-1\r\nDTSTAMP:20240102T000000Z\r\nDTSTART:20990105T100000Z\r\nDTEND:20990105T110000Z\r\nSEQUENCE:1\r\nORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
            );
            let dump = sealed.content.data.event.to_string();
            assert_eq!(
                dump.contains(&format!("CLASS:{class}")),
                class != "X-SOMETHING",
                "{class}: {dump}"
            );
            let content = std::mem::take(&mut sealed.content);
            sealed.meta.refresh_from_content(&content, ());
            assert_eq!(sealed.meta.flags & EVENT_PRIVATE != 0, private, "{class}");
            assert_eq!(sealed.meta.flags & EVENT_SECRET != 0, secret, "{class}");
        }
    }

    #[test]
    fn a_reply_below_the_events_sequence_is_stale() {
        let org = organizer();
        let current = event(Some(1));
        let current = itip_snapshot(&current, &org, false).unwrap();
        for (reply_sequence, stale) in [
            (Some(0), true),
            (None, true),
            (Some(1), false),
            (Some(2), false),
        ] {
            let reply = reply(reply_sequence);
            assert_eq!(
                is_stale_reply(&current, &itip_snapshot(&reply, &org, false).unwrap()),
                stale,
                "{reply_sequence:?}"
            );
        }
        // An event without SEQUENCE counts as 0: nothing is stale.
        let unnumbered = event(None);
        let unnumbered = itip_snapshot(&unnumbered, &org, false).unwrap();
        let reply = reply(None);
        assert!(!is_stale_reply(
            &unnumbered,
            &itip_snapshot(&reply, &org, false).unwrap()
        ));
    }

    /// A delegation REPLY as upstream builds it: the replier's entry with
    /// its delegate's (`attendee_entry_uids`).
    fn delegation_reply() -> ICalendar {
        parse(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REPLY\r\nBEGIN:VEVENT\r\nUID:stale-1\r\nDTSTAMP:20240101T000000Z\r\nSEQUENCE:0\r\nORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION;DELEGATED-FROM=\"mailto:att@example.com\":mailto:del@example.com\r\nATTENDEE;PARTSTAT=DELEGATED;DELEGATED-TO=\"mailto:del@example.com\":mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        )
    }

    #[test]
    fn sender_candidates_name_the_organizer_or_every_attendee() {
        let org = organizer();
        let request = event(Some(0));
        let snapshots = itip_snapshot(&request, &org, false).unwrap();
        assert_eq!(
            sender_candidates(&snapshots, Method::Request)
                .into_iter()
                .map(|candidate| candidate.named)
                .collect::<Vec<_>>(),
            ["org@example.com"]
        );
        assert_eq!(
            sender_candidates(&snapshots, Method::Cancel)
                .into_iter()
                .map(|candidate| candidate.named)
                .collect::<Vec<_>>(),
            ["org@example.com"]
        );
        let reply = reply(Some(0));
        let snapshots = itip_snapshot(&reply, &org, false).unwrap();
        assert_eq!(
            sender_candidates(&snapshots, Method::Reply)
                .into_iter()
                .map(|candidate| candidate.named)
                .collect::<Vec<_>>(),
            ["att@example.com"]
        );
        // Both the delegator and the delegate are candidates: which one
        // replied is decided by `changed_by` and the token, never by order.
        let delegation = delegation_reply();
        let snapshots = itip_snapshot(&delegation, &org, false).unwrap();
        let mut candidates = sender_candidates(&snapshots, Method::Reply)
            .into_iter()
            .map(|candidate| candidate.named)
            .collect::<Vec<_>>();
        candidates.sort();
        assert_eq!(candidates, ["att@example.com", "del@example.com"]);
    }

    /// A daily master at `sequence`, with an override of the 3rd at
    /// `with_override`'s sequence: an ordinary one, or (`ranged`) a
    /// `RANGE=THISANDFUTURE` one.
    fn recurring_event(sequence: i64, with_override: Option<i64>, ranged: bool) -> ICalendar {
        let range = if ranged { ";RANGE=THISANDFUTURE" } else { "" };
        let override_ = with_override.map_or(String::new(), |s| format!(
            "BEGIN:VEVENT\r\nUID:stale-1\r\nRECURRENCE-ID{range}:20990103T090000Z\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990103T100000Z\r\nDTEND:20990103T110000Z\r\nSEQUENCE:{s}\r\nORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\r\nEND:VEVENT\r\n"
        ));
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:stale-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nRRULE:FREQ=DAILY;COUNT=5\r\nSEQUENCE:{sequence}\r\nORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:att@example.com\r\nEND:VEVENT\r\n{override_}END:VCALENDAR\r\n"
        ))
    }

    /// An ordinary occurrence REPLY for the occurrence of `day` (2 to 6).
    fn occurrence_reply(sequence: i64, day: u8) -> ICalendar {
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REPLY\r\nBEGIN:VEVENT\r\nUID:stale-1\r\nRECURRENCE-ID:2099010{day}T090000Z\r\nDTSTAMP:20240101T000000Z\r\nSEQUENCE:{sequence}\r\nORGANIZER:mailto:org@example.com\r\nATTENDEE;PARTSTAT=DECLINED:mailto:att@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
        ))
    }

    #[test]
    fn an_occurrence_reply_is_judged_against_the_component_that_governs_it() {
        // A fn, not a closure: the snapshot borrows its argument.
        fn snap(ical: &ICalendar) -> ItipSnapshots<'_> {
            itip_snapshot(ical, &organizer(), false).unwrap()
        }
        // The master was rescheduled to SEQUENCE 1; the decline of an
        // occurrence at SEQUENCE 0 is stale although no override exists.
        let master_only = recurring_event(1, None, false);
        let master_only = snap(&master_only);
        let old = occurrence_reply(0, 3);
        assert!(is_stale_reply(&master_only, &snap(&old)));
        let fresh = occurrence_reply(1, 3);
        assert!(!is_stale_reply(&master_only, &snap(&fresh)));
        // A stored override keeps its own SEQUENCE: at 0 while the master
        // is at 1, a reply at 0 for that occurrence is not stale.
        let with_override = recurring_event(1, Some(0), false);
        assert!(!is_stale_reply(&snap(&with_override), &snap(&old)));
        // A THISANDFUTURE override at 1 from the 3rd governs the 3rd and
        // later occurrences while the master stays at 0: an ordinary reply
        // at 0 for the 4th is stale, one for the 2nd is not.
        let ranged = recurring_event(0, Some(1), true);
        let ranged = snap(&ranged);
        assert!(is_stale_reply(&ranged, &snap(&occurrence_reply(0, 4))));
        assert!(is_stale_reply(&ranged, &snap(&occurrence_reply(0, 3))));
        assert!(!is_stale_reply(&ranged, &snap(&occurrence_reply(0, 2))));
        assert!(!is_stale_reply(&ranged, &snap(&occurrence_reply(1, 4))));
    }

    /// An event listing `del@example.com` once per delegator in
    /// `delegators` (`DELEGATED-FROM`), each on its own ATTENDEE line.
    fn delegated_by(delegators: &[&str]) -> ICalendar {
        let lines = delegators
            .iter()
            .map(|delegator| {
                format!(
                    "ATTENDEE;PARTSTAT=NEEDS-ACTION;DELEGATED-FROM=\"mailto:{delegator}\":mailto:del@example.com\r\n"
                )
            })
            .collect::<String>();
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\nUID:stale-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSEQUENCE:0\r\nORGANIZER:mailto:org@example.com\r\n{lines}END:VEVENT\r\nEND:VCALENDAR\r\n"
        ))
    }

    /// An address listed twice in one instance, once per delegator (two
    /// different non-empty DELEGATED-FROM sets), keeps the union of them
    /// whatever order the snapshot's attendee set yields the lines in.
    #[test]
    fn a_doubled_attendee_keeps_the_union_of_its_delegators() {
        let org = organizer();
        let before = delegated_by(&["a@example.com", "b@example.com"]);
        let before = Attendance::of(&itip_snapshot(&before, &org, false).unwrap());
        for delegator in ["a@example.com", "b@example.com"] {
            let merged = delegated_by(&[delegator]);
            let merged = itip_snapshot(&merged, &org, false).unwrap();
            let del = merged
                .components
                .get(&InstanceId::Main)
                .unwrap()
                .attendees
                .iter()
                .find(|attendee| attendee.email.email == "del@example.com")
                .unwrap();
            assert!(
                !before.is_new(None, del, false),
                "the union lost delegator {delegator}"
            );
        }
    }

    /// PR review ruling P2: a DELEGATED-FROM gain makes an existing attendee
    /// new on release 2's token-authenticated path, never on the mail arm;
    /// an attendee absent before is new on both.
    #[test]
    fn mail_counts_only_attendees_absent_before() {
        fn attendee<'x>(tree: &'x ICalendar, address: &str) -> Attendee<'x> {
            itip_snapshot(tree, &organizer(), false)
                .unwrap()
                .components
                .get(&InstanceId::Main)
                .unwrap()
                .attendees
                .iter()
                .find(|attendee| attendee.email.email == address)
                .unwrap()
                .clone()
        }
        let org = organizer();
        // `del` attends without a delegator, then gains one.
        let before = parse(concat!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\n",
            "UID:stale-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\n",
            "DTEND:20990102T100000Z\r\nSEQUENCE:0\r\nORGANIZER:mailto:org@example.com\r\n",
            "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:del@example.com\r\n",
            "END:VEVENT\r\nEND:VCALENDAR\r\n"
        ));
        let before = Attendance::of(&itip_snapshot(&before, &org, false).unwrap());
        let gained = delegated_by(&["a@example.com"]);
        let del = attendee(&gained, "del@example.com");
        assert!(
            before.is_new(None, &del, false),
            "the gain on release 2's path"
        );
        assert!(!before.is_new(None, &del, true), "the gain on the mail arm");
        // `del` did not attend before (only att@example.com did).
        let absent = Attendance::of(&itip_snapshot(&event(None), &org, false).unwrap());
        assert!(absent.is_new(None, &del, false), "absent, release 2's path");
        assert!(absent.is_new(None, &del, true), "absent, the mail arm");
    }

    #[test]
    fn reasons_carry_no_address() {
        assert_eq!(
            itip_reason(&ItipError::SenderIsNotParticipant("secret@x.test".into())),
            "SenderIsNotParticipant"
        );
        assert_eq!(
            itip_reason(&ItipError::UnknownParticipant("secret@x.test".into())),
            "UnknownParticipant"
        );
        assert_eq!(
            seal_reason(&SealError::Structure("misplaced carrier")),
            "misplaced carrier"
        );
        assert_eq!(seal_reason(&SealError::Aead), "unseal failure");
        assert_eq!(DRAIN_RETRY_SECS, 30);
    }

    #[test]
    fn candidates_preserve_sent_by_assertions_and_invalid_presence() {
        let org = vec!["org@example.com".to_string()];
        for (sent_by, expected) in [
            ("MAILTO:%77riter%40example.com", "writer@example.com"),
            ("https:writer@example.com", ""),
            ("data:mailto:writer@example.com;base64,YQ==", ""),
            ("mailto:invalid", ""),
        ] {
            let tree = event(Some(0))
                .to_string()
                .replace("ORGANIZER:", &format!("ORGANIZER;SENT-BY=\"{sent_by}\":"));
            let calcard::Entry::ICalendar(tree) = calcard::Parser::new(&tree).entry() else {
                unreachable!()
            };
            let snapshots = itip_snapshot(&tree, &org, false).unwrap();
            let candidates = sender_candidates(&snapshots, Method::Request);
            assert_eq!(candidates.len(), 1);
            assert_eq!(candidates[0].named, "org@example.com");
            assert_eq!(candidates[0].sent_by.as_deref(), Some(expected));
        }
        let mut tree = recurring_event(1, Some(1), false);
        let mut assertion = 0;
        for component in &mut tree.components {
            for entry in &mut component.entries {
                if entry.name == calcard::icalendar::ICalendarProperty::Attendee {
                    entry.params.push(calcard::icalendar::ICalendarParameter {
                        name: calcard::icalendar::ICalendarParameterName::SentBy,
                        value: calcard::icalendar::ICalendarParameterValue::Uri(
                            calcard::icalendar::Uri::Location(format!(
                                "mailto:writer{assertion}@example.com"
                            )),
                        ),
                    });
                    assertion += 1;
                }
            }
        }
        let snapshots = itip_snapshot(&tree, &org, false).unwrap();
        let candidates = sender_candidates(&snapshots, Method::Reply);
        assert_eq!(
            candidates.len(),
            2,
            "same named address retains two SENT-BY assertions"
        );
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.named == "att@example.com")
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.sent_by.as_deref() == Some("writer0@example.com"))
        );
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.sent_by.as_deref() == Some("writer1@example.com"))
        );
    }

    /// Spec 10: the drain traces a fixed name per `ItipError`, never the
    /// error's payload; `SenderIsNotParticipant` and `UnknownParticipant`
    /// carry an address, which upstream's `to_string()` would trace. Every
    /// variant has its own name.
    #[test]
    fn itip_reasons_are_fixed_names_without_payload() {
        let canary = "drain-reason-canary@example.com";
        let errors = [
            ItipError::NoSchedulingInfo,
            ItipError::OtherSchedulingAgent,
            ItipError::NotOrganizer,
            ItipError::NotOrganizerNorAttendee,
            ItipError::NothingToSend,
            ItipError::MissingUid,
            ItipError::MultipleUid,
            ItipError::MultipleOrganizer,
            ItipError::MultipleObjectTypes,
            ItipError::MultipleObjectInstances,
            ItipError::CannotModifyProperty(ICalendarProperty::Summary),
            ItipError::CannotModifyInstance,
            ItipError::CannotModifyAddress,
            ItipError::OrganizerMismatch,
            ItipError::MissingMethod,
            ItipError::InvalidComponentType,
            ItipError::OutOfSequence,
            ItipError::OrganizerIsLocalAddress,
            ItipError::SenderIsNotOrganizerNorAttendee,
            ItipError::SenderIsNotOrganizer,
            ItipError::SenderIsNotParticipant(canary.to_string()),
            ItipError::UnknownParticipant(canary.to_string()),
            ItipError::UnsupportedMethod(ICalendarMethod::Publish),
            ItipError::ICalendarParseError,
            ItipError::EventNotFound,
            ItipError::EventTooLarge,
            ItipError::QuotaExceeded,
            ItipError::NoDefaultCalendar,
            ItipError::AutoAddDisabled,
            ItipError::TooManyRecipients,
        ];
        let mut names = AHashSet::new();
        for err in &errors {
            let name = itip_reason(err);
            assert!(
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic()),
                "{name:?} is not a bare variant name"
            );
            assert!(!name.contains("canary") && !name.contains('@'));
            assert!(names.insert(name), "{name} names two variants");
        }
        assert_eq!(names.len(), errors.len());
        assert_eq!(
            itip_reason(&ItipError::SenderIsNotParticipant(canary.to_string())),
            "SenderIsNotParticipant"
        );
        assert_eq!(
            itip_reason(&ItipError::UnknownParticipant(canary.to_string())),
            "UnknownParticipant"
        );
        assert!(
            ItipError::UnknownParticipant(canary.to_string())
                .to_string()
                .contains(canary),
            "the control: upstream's text carries the address"
        );
    }

    // iMIP spec 3.1 item 2: the start of `itip_ingest`, extracted. The
    // fixtures are of the `imip-*` family (spec 8.2).
    const REPLY_WITHOUT_ORGANIZER: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REPLY\r\nBEGIN:VEVENT\r\nUID:imip-fill-1\r\nDTSTAMP:20260101T000000Z\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:imip-att@remote.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    const CANCEL_WITHOUT_ORGANIZER: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:CANCEL\r\nBEGIN:VEVENT\r\nUID:imip-fill-2\r\nDTSTAMP:20260101T000000Z\r\nATTENDEE:mailto:imip-att@remote.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    const REQUEST_FROM_ORG: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REQUEST\r\nBEGIN:VEVENT\r\nUID:imip-rule-1\r\nDTSTAMP:20260101T000000Z\r\nDTSTART:20990102T090000Z\r\nDTEND:20990102T100000Z\r\nSEQUENCE:0\r\nORGANIZER:mailto:imip-org@remote.test\r\nATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:imip-key@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    const REPLY_FROM_ATT: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REPLY\r\nBEGIN:VEVENT\r\nUID:imip-rule-2\r\nDTSTAMP:20260101T000000Z\r\nSEQUENCE:0\r\nORGANIZER:mailto:imip-key@example.com\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:imip-att@remote.test\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    /// The ORGANIZER values of every component, in order.
    fn organizers(ical: &ICalendar) -> Vec<String> {
        ical.components
            .iter()
            .flat_map(|comp| comp.entries.iter())
            .filter(|entry| entry.name == ICalendarProperty::Organizer)
            .filter_map(|entry| entry.values.first().and_then(|v| v.as_text()))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn the_exchange_fill_adds_an_organizer_only_where_missing() {
        use crate::calendar::itip::{itip_fill_organizer, itip_parse_inbound};
        let mut reply = parse(REPLY_WITHOUT_ORGANIZER); // ATTENDEE imip-att, no ORGANIZER
        itip_fill_organizer(&mut reply, "imip-key@example.com");
        assert_eq!(organizers(&reply), ["mailto:imip-key@example.com"]);
        itip_fill_organizer(&mut reply, "other@example.com");
        assert_eq!(
            organizers(&reply),
            ["mailto:imip-key@example.com"],
            "idempotent"
        );
        let mut cancel = parse(CANCEL_WITHOUT_ORGANIZER);
        itip_fill_organizer(&mut cancel, "imip-key@example.com");
        assert!(organizers(&cancel).is_empty(), "REPLY and REQUEST only");
        assert!(matches!(
            itip_parse_inbound("not ical", "x@example.com"),
            Err(ItipError::ICalendarParseError)
        ));
        // The parse runs the fill, in `itip_ingest`'s order.
        let parsed = itip_parse_inbound(REPLY_WITHOUT_ORGANIZER, "imip-key@example.com")
            .unwrap_or_else(|_| panic!("the reply parses"));
        assert_eq!(organizers(&parsed), ["mailto:imip-key@example.com"]);
    }

    #[test]
    fn the_sender_rule_follows_the_method() {
        use crate::calendar::itip::itip_sender_snapshot;
        let own = vec!["imip-key@example.com".to_string()];
        let request = parse(REQUEST_FROM_ORG); // ORGANIZER imip-org, ATTENDEE imip-key
        assert!(itip_sender_snapshot(&request, &own, "imip-org@remote.test").is_ok());
        assert!(matches!(
            itip_sender_snapshot(&request, &own, "imip-att@remote.test"),
            Err(ItipError::SenderIsNotOrganizer)
        ));
        let reply = parse(REPLY_FROM_ATT); // ORGANIZER imip-key, ATTENDEE imip-att
        assert!(itip_sender_snapshot(&reply, &own, "imip-att@remote.test").is_ok());
        assert!(
            itip_sender_snapshot(&reply, &own, "imip-key@example.com").is_ok(),
            "3.2 item 12"
        );
        assert!(matches!(
            itip_sender_snapshot(&reply, &own, "imip-stranger@remote.test"),
            Err(ItipError::SenderIsNotOrganizerNorAttendee)
        ));
    }
}
