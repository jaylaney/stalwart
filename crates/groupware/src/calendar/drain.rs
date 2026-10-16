/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6: the drain. A key account's own DAV request, which
//! holds the account's session keys, opens the pending items of the
//! account's scheduling inbox before the request's own work (6.1), so no
//! background code needs a key. A pass (6.2-6.3) takes the session's lock,
//! reads the inbox change id (the snapshot), enumerates the items whose
//! pending bit is set in document id order, unwraps the private key once
//! (3.2), and for each item: opens the `pk` wrap (the only place one is
//! opened, invariant 11), checks the sender (6.4 step 2), finds the event
//! by UID, imports under the creation lease or merges under the event's
//! archive assertion, adopts or keeps token pairs (3.3), authenticates a
//! REPLY by its token and rejects a stale one, fans out the organizer's
//! REQUEST to delegates the merge added, and rewrites the item resealed
//! under the master key with the pending bit cleared (6.4 step 5). Each
//! item is one store write holding the event write and the item rewrite
//! (its batch stays far below `BatchBuilder::is_large_batch`, the only
//! point at which a commit point splits a batch).
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
//! defers the next pass by `DRAIN_RETRY_SECS`.
//!
//! Lease order: a drain holds its session lock and then may take a per-UID
//! lease (an import, or a REPLY merge that fans out to delegates); the PUT
//! path takes the lease without any drain lock, so there is no cycle.
//! Both lease holders re-read the item once the lease is theirs and skip
//! it, writing nothing, when another session finished it in the meantime.

use crate::{
    DestroyArchive,
    cache::GroupwareCache,
    calendar::{
        CalendarEvent, CalendarEventData, CalendarEventNotification, ChangedBy,
        EVENT_NOTIFICATION_IS_CHANGE,
        fanout::{ZaFanOut, za_set_schedule_status},
        seal::{
            SealError, TOKEN_LEN, TokenPairs, archived_item_is_pending, open_pending_item,
            seal_drained_item, seal_error, seal_event, unseal_event_pairs,
        },
        storage::ItipAutoExpunge,
    },
    scheduling::{
        InstanceId, ItipError, ItipSnapshots, ical_size,
        inbound::{
            MergeAction, MergeResult, itip_import_message, itip_merge_changes, itip_method,
            itip_process_message,
        },
        organizer::organizer_request_full,
        snapshot::itip_snapshot,
    },
};
use ahash::AHashSet;
use calcard::{
    common::timezone::Tz,
    icalendar::{ICalendar, ICalendarMethod},
};
use common::{
    DavName, Server,
    auth::AccountInfo,
    storage::index::ObjectIndexBuilder,
    za::lease::{ZaUidLease, ZaUidLeaseGuard},
};
use std::{future::Future, sync::Arc};
use store::{
    ValueKey, rand,
    write::{AlignedBytes, Archive, BatchBuilder, now},
};
use trc::AddContext;
use types::{
    collection::{Collection, SyncCollection},
    field::CalendarEventField,
};
use vault::session::SessionKeys;
use zeroize::Zeroizing;

/// Spec 6.2: a pass that left an item pending is not retried before this
/// many seconds have passed. A constant with a test-mode override, not a
/// setting (spec 13).
pub const DRAIN_RETRY_SECS: u64 = 30;

pub trait ZaDrain: Sync + Send {
    /// Spec 6.1: drains the inbox of the account whose session keys these
    /// are, before the request's own work. Never fails the request
    /// (invariant 14): errors are traced here.
    fn za_drain(&self, keys: &Arc<SessionKeys>) -> impl Future<Output = ()> + Send;
}

impl ZaDrain for Server {
    async fn za_drain(&self, keys: &Arc<SessionKeys>) {
        #[cfg(feature = "test_mode")]
        if common::za::test_hooks::drain_skipped(keys.account_id) {
            return;
        }
        if let Err(err) = drain_pass(self, keys).await {
            trc::error!(
                err.account_id(keys.account_id)
                    .details("zero-access: drain pass failed")
                    .caused_by(trc::location!())
            );
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
/// deferral is in force or while the cursor is at the inbox's change id.
async fn drain_pass(server: &Server, keys: &Arc<SessionKeys>) -> trc::Result<()> {
    let account_id = keys.account_id;
    let state = keys.drain();
    let _pass = state.lock().await;
    let now = now();
    if state.retry_after() > now {
        return Ok(());
    }
    // The snapshot: the inbox change id observed before enumeration, from
    // the same since-query upstream runs for a request's own collection.
    let snapshot = server
        .fetch_dav_resources(
            account_id,
            account_id,
            SyncCollection::CalendarEventNotification,
        )
        .await
        .caused_by(trc::location!())?
        .highest_change_id;
    if state.cursor().is_some_and(|cursor| cursor >= snapshot) {
        return Ok(());
    }
    let mut pending = Vec::new();
    for document_id in server
        .itip_ids(account_id)
        .await
        .caused_by(trc::location!())?
    {
        if let Some(stored) = read_item(server, account_id, document_id).await?
            && archived_item_is_pending(
                stored
                    .unarchive::<CalendarEventNotification>()
                    .caused_by(trc::location!())?,
            )
        {
            pending.push(document_id);
        }
    }
    if pending.is_empty() {
        state.complete(snapshot);
        return Ok(());
    }
    match drain_items(server, keys, pending).await {
        Ok(true) => {
            state.complete(snapshot);
            Ok(())
        }
        Ok(false) => {
            state.defer_until(now + drain_retry_secs());
            Ok(())
        }
        Err(err) => {
            state.defer_until(now + drain_retry_secs());
            Err(err)
        }
    }
}

async fn read_item(
    server: &Server,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Option<Archive<AlignedBytes>>> {
    server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEventNotification,
            document_id,
        ))
        .await
        .caused_by(trc::location!())
}

/// Spec 6.4: the private key once, then each item in document id order.
/// `Ok(true)` when every item is done; `Ok(false)` when one stays pending
/// without an error (a busy UID lease): the pass stops at that item and the
/// items after it stay pending, so later items for the same UID keep their
/// order; `Err` on a transient error, which aborts the pass (6.5).
async fn drain_items(
    server: &Server,
    keys: &Arc<SessionKeys>,
    pending: Vec<u32>,
) -> trc::Result<bool> {
    let account_id = keys.account_id;
    let private_key = match server.za_private_key(keys).await {
        Ok(key) => key,
        // A recorded failure (the wrap does not open, the record is missing
        // or unusable) was traced once for the session and the drain is
        // skipped (spec 10); a store error is not recorded, so it is traced
        // by the caller and the next pass retries it.
        Err(err) => {
            return if keys.private_key().is_some() {
                Ok(false)
            } else {
                Err(err)
            };
        }
    };
    let account_info = server
        .account_info(account_id)
        .await
        .caused_by(trc::location!())?;
    for document_id in pending {
        if !drain_item(server, &account_info, keys, private_key, document_id).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// One pending item: what the rewrite, the deletion and the commit need.
struct ItemCtx<'a> {
    server: &'a Server,
    account_info: &'a AccountInfo,
    keys: &'a SessionKeys,
    /// The stored archive as read; every write asserts it.
    stored: Archive<AlignedBytes>,
    document_id: u32,
    changed_by: ChangedBy,
    created: i64,
    size: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Request,
    Cancel,
    Reply,
}

/// What a merge applies.
enum Incoming {
    /// The recipient's pair from the item (spec 3.3, "Attendee side").
    Request(Option<TokenPairs>),
    Cancel,
    /// The replier's token (spec 3.3, "Verification").
    Reply(Zeroizing<[u8; TOKEN_LEN]>),
}

/// Spec 6.4 steps 1-5 for one item. `Ok(true)`: done (rewritten or
/// deleted); `Ok(false)`: left pending (the lease was busy).
async fn drain_item(
    server: &Server,
    account_info: &AccountInfo,
    keys: &SessionKeys,
    private_key: &vault::keys::Secret,
    document_id: u32,
) -> trc::Result<bool> {
    let account_id = keys.account_id;
    // Re-read (spec 6.3, "The pass"): an item that is gone, or that another
    // session drained, is skipped.
    let Some(stored) = read_item(server, account_id, document_id).await? else {
        return Ok(true);
    };
    let archived = stored
        .unarchive::<CalendarEventNotification>()
        .caused_by(trc::location!())?;
    if !archived_item_is_pending(archived) {
        return Ok(true);
    }
    let mut item = stored
        .deserialize::<CalendarEventNotification>()
        .caused_by(trc::location!())?;
    let ctx = ItemCtx {
        server,
        account_info,
        keys,
        stored,
        document_id,
        changed_by: item.changed_by.clone(),
        created: item.created,
        size: item.size,
    };
    // Step 1: open. A failure is permanent (spec 6.5): the private key
    // never changes, so nothing will ever read this item.
    let carriers = match open_pending_item(&mut item.event, account_id, private_key) {
        Ok(carriers) => carriers,
        Err(err) => return ctx.delete_undecodable(seal_reason(&err)).await,
    };
    let tree = item.event;
    // An unknown or missing METHOD cannot come from the fan-out: permanent.
    let method = match itip_method(&tree) {
        Ok(ICalendarMethod::Request) => Method::Request,
        Ok(ICalendarMethod::Cancel) => Method::Cancel,
        Ok(ICalendarMethod::Reply) => Method::Reply,
        Ok(_) => return ctx.delete_undecodable("UnsupportedMethod").await,
        Err(_) => return ctx.delete_undecodable("MissingMethod").await,
    };
    let (uid, candidates) = match itip_snapshot(&tree, account_info.addresses(), false) {
        Ok(snapshots) => (
            snapshots.uid.to_string(),
            sender_candidates(&snapshots, method),
        ),
        Err(err) => return ctx.reject(tree, None, &err).await,
    };
    // Step 2: the sending address must resolve to the principal in
    // `changed_by`, the check upstream's ingest makes against a mail's
    // sender. A REQUEST or CANCEL names one candidate, its ORGANIZER; a
    // REPLY names every ATTENDEE it carries, since upstream's attendee
    // builder lists the replier together with its delegates and
    // delegators, so the sender's addresses are the candidates that
    // resolve to `changed_by`. The lookup is the quiet one (spec 10).
    let mut sender_addresses = Vec::new();
    if let ChangedBy::PrincipalId(sender_id) = &ctx.changed_by {
        for address in candidates {
            if server
                .za_account_id_from_email(&address)
                .await
                .caused_by(trc::location!())?
                == Some(*sender_id)
            {
                sender_addresses.push(address);
            }
        }
    }
    if sender_addresses.is_empty() {
        return ctx
            .reject(tree, None, &ItipError::SenderIsNotOrganizerNorAttendee)
            .await;
    }
    // Step 3, by METHOD. The UID lookup is upstream's (the fresh index);
    // the lowest document id is the copy that merges, as `itip_ingest`
    // takes the first hit.
    let existing = find_event(server, account_id, &uid).await?;
    match (method, existing) {
        (Method::Request, None) => import(&ctx, tree, carriers.pair, &uid, sender_addresses).await,
        (Method::Request, Some(event_id)) => {
            merge(
                &ctx,
                event_id,
                tree,
                Incoming::Request(carriers.pair),
                sender_addresses,
            )
            .await
        }
        (Method::Cancel, Some(event_id)) => {
            merge(&ctx, event_id, tree, Incoming::Cancel, sender_addresses).await
        }
        (Method::Reply, Some(event_id)) => match carriers.reply_token {
            Some(token) => {
                merge(
                    &ctx,
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
    }
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
fn sender_candidates(snapshots: &ItipSnapshots<'_>, method: Method) -> Vec<String> {
    match method {
        Method::Reply => {
            let mut seen = AHashSet::new();
            snapshots
                .components
                .values()
                .flat_map(|instance| instance.attendees.iter())
                .map(|attendee| attendee.email.email.clone())
                .filter(|address| seen.insert(address.to_ascii_lowercase()))
                .collect()
        }
        Method::Request | Method::Cancel => vec![snapshots.organizer.email.email.clone()],
    }
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
    if let Some(exact) = event.components.get(instance_id) {
        return Some(exact.sequence.unwrap_or(0));
    }
    if let InstanceId::Recurrence(occurrence) = instance_id
        && let Some((_, ranged)) = event
            .components
            .iter()
            .filter_map(|(id, component)| match id {
                InstanceId::Recurrence(range)
                    if range.this_and_future && range.date <= occurrence.date =>
                {
                    Some((range.date, component))
                }
                _ => None,
            })
            .max_by_key(|(date, _)| *date)
    {
        return Some(ranged.sequence.unwrap_or(0));
    }
    event
        .components
        .get(&InstanceId::Main)
        .map(|master| master.sequence.unwrap_or(0))
}

/// Spec 6.4, REQUEST with no event: upstream's import (`itip_ingest`) with
/// the commit replaced, under the creation lease (6.3). A local sender
/// always adds, so `itip_auto_add` does not apply.
async fn import(
    ctx: &ItemCtx<'_>,
    tree: ICalendar,
    pair: Option<TokenPairs>,
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
        return merge(
            ctx,
            event_id,
            tree,
            Incoming::Request(pair),
            sender_addresses,
        )
        .await;
    }
    let mut event_tree = tree.clone();
    if let Err(err) = itip_import_message(&mut event_tree) {
        lease.release().await;
        return ctx.reject(tree, None, &err).await;
    }
    // Upstream's two checks: quota for the plaintext size (the item's, as
    // the fan-out computed it) and the default calendar. Only a quota
    // limit is the iTIP-level rejection; a failed counter read is a store
    // error that keeps the item pending (spec 6.5), as the fan-out
    // classifies it.
    if let Err(err) = server
        .has_available_quota(
            server
                .account(account_id)
                .await
                .caused_by(trc::location!())?
                .as_ref(),
            ctx.size as u64,
        )
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
        .get_or_create_default_calendar(account_id, account_id)
        .await
        .caused_by(trc::location!())?
    else {
        lease.release().await;
        return ctx.reject(tree, None, &ItipError::NoDefaultCalendar).await;
    };
    let mut next_email_alarm = None;
    let now = now() as i64;
    let mut event = CalendarEvent {
        names: vec![DavName {
            name: format!("{}_{}.ics", now, rand::random::<u64>()),
            parent_id,
        }],
        data: CalendarEventData::new_for(
            event_tree,
            Tz::Floating,
            server.core.groupware.max_ical_instances,
            &mut next_email_alarm,
            true,
        ),
        size: ctx.size,
        schedule_tag: Some(1),
        ..Default::default()
    };
    // Spec 3.3, "Attendee side": the copy takes the pair the item carried.
    seal_event(&mut event, ctx.keys, account_id, pair.as_ref())
        .map_err(|err| seal_error(err, account_id, u32::MAX))?;
    let document_id = server
        .store()
        .assign_document_ids(account_id, Collection::CalendarEvent, 1)
        .await
        .caused_by(trc::location!())?;
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::drain_lease_pause_point(account_id).await;
    let mut batch = BatchBuilder::new();
    // Spec 6.3 step 3: fenced on the lease, with the item rewrite ahead of
    // the insert (one write; see the module doc).
    lease.lease().assert_in(&mut batch);
    ctx.rewrite_in(&mut batch, tree, Some(document_id), 0)?;
    event
        .insert(
            ctx.account_info.account_tenant_ids(),
            account_id,
            document_id,
            next_email_alarm,
            &mut batch,
        )
        .caused_by(trc::location!())?;
    ctx.commit(batch, Some(lease)).await
}

enum Step {
    Reject(ItipError),
    Merge(Vec<MergeAction>),
    Nothing,
}

/// An instance's identity across two snapshots of one event: `None` for
/// the master, the RECURRENCE-ID's date and range for an override (never
/// its entry id, which a merge may change).
type InstanceKey = Option<(i64, bool)>;

fn instance_key(instance: &InstanceId) -> InstanceKey {
    match instance {
        InstanceId::Main => None,
        InstanceId::Recurrence(recurrence) => Some((recurrence.date, recurrence.this_and_future)),
    }
}

/// An event's attendance before a merge, per instance (spec 6.4,
/// "Delegates"), as lowercased addresses.
struct Attendance {
    instances: AHashSet<InstanceKey>,
    attendees: AHashSet<(InstanceKey, String)>,
}

impl Attendance {
    fn of(snapshots: &ItipSnapshots<'_>) -> Self {
        let mut attendance = Attendance {
            instances: AHashSet::new(),
            attendees: AHashSet::new(),
        };
        for (instance, snapshot) in &snapshots.components {
            let instance = instance_key(instance);
            attendance.instances.insert(instance);
            for attendee in &snapshot.attendees {
                attendance
                    .attendees
                    .insert((instance, attendee.email.email.to_ascii_lowercase()));
            }
        }
        attendance
    }

    /// Whether `address` attended `instance` before the merge. An override
    /// the merge created inherits the master's attendance, so a
    /// delegator's own per-occurrence delegation does not make the
    /// delegator new.
    fn attended(&self, instance: InstanceKey, address: &str) -> bool {
        let attended =
            |instance: InstanceKey| self.attendees.contains(&(instance, address.to_string()));
        attended(instance)
            || (instance.is_some() && !self.instances.contains(&instance) && attended(None))
    }
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
    let Some(archive) = server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEvent,
            event_id,
        ))
        .await
        .caused_by(trc::location!())?
    else {
        return ctx.reject(tree, None, &ItipError::EventNotFound).await;
    };
    let event_ = archive
        .to_unarchived::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let mut event = event_
        .deserialize::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let mut pairs = unseal_event_pairs(&mut event, ctx.keys, account_id)
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
    };
    let (step, is_organizer_update, before, event_uid) = {
        let snapshots = match itip_snapshot(&event.data.event, addresses, false) {
            Ok(snapshots) => snapshots,
            Err(err) => return ctx.reject(tree, Some(event_id), &err).await,
        };
        let itip_snapshots = match itip_snapshot(&tree, addresses, false) {
            Ok(snapshots) => snapshots,
            Err(err) => return ctx.reject(tree, Some(event_id), &err).await,
        };
        let is_organizer_update = !itip_snapshots.organizer.email.is_local;
        let event_uid = snapshots.uid.to_string();
        // The attendance before the merge, per instance: an attendee the
        // merge adds to an instance is a delegate (spec 6.4, "Delegates").
        let before = Attendance::of(&snapshots);
        let step = if matches!(incoming, Incoming::Reply(_))
            && is_stale_reply(&snapshots, &itip_snapshots)
        {
            Step::Reject(ItipError::OutOfSequence)
        } else {
            match itip_process_message(
                &event.data.event,
                snapshots,
                &tree,
                itip_snapshots,
                sender_address.clone(),
            ) {
                Ok(MergeResult::Actions(changes)) => Step::Merge(changes),
                Ok(MergeResult::None) => Step::Nothing,
                // Only REFRESH yields a message; it cannot come from the
                // fan-out (spec 6.4 step 3).
                Ok(MergeResult::Message(_)) => {
                    Step::Reject(ItipError::UnsupportedMethod(ICalendarMethod::Refresh))
                }
                Err(err) => Step::Reject(err),
            }
        };
        (step, is_organizer_update, before, event_uid)
    };
    let changes = match step {
        Step::Reject(err) => return ctx.reject(tree, Some(event_id), &err).await,
        Step::Nothing => return ctx.rewrite_only(tree, Some(event_id), 0).await,
        Step::Merge(changes) => changes,
    };
    itip_merge_changes(&mut event.data.event, changes);
    event.size = ical_size(&event.data.event) as u32;
    if event.size > server.core.groupware.max_ical_size as u32 {
        return ctx
            .reject(tree, Some(event_id), &ItipError::EventTooLarge)
            .await;
    }
    let extra_bytes = (event.size as u64).saturating_sub(event_.inner.size.to_native() as u64);
    if extra_bytes > 0
        && let Err(err) = server
            .has_available_quota(
                server
                    .account(account_id)
                    .await
                    .caused_by(trc::location!())?
                    .as_ref(),
                extra_bytes,
            )
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
        // Delegates the merge added get their token from the organizer,
        // whose session this is, through the same fan-out a PUT uses:
        // upstream's organizer request builder on the merged tree,
        // restricted to the new attendees (spec 6.4, "Delegates").
        Incoming::Reply(_) => {
            let new_attendees = match itip_snapshot(&event.data.event, addresses, false) {
                Ok(after) => {
                    // Compared per instance, so a delegate who already
                    // attends another instance is new to this one; the
                    // set dedups the addresses for delivery.
                    let mut new_attendees: AHashSet<String> = AHashSet::new();
                    for (instance, snapshot) in &after.components {
                        let instance = instance_key(instance);
                        for attendee in &snapshot.attendees {
                            let address = attendee.email.email.to_ascii_lowercase();
                            if !before.attended(instance, &address) {
                                new_attendees.insert(address);
                            }
                        }
                    }
                    if new_attendees.is_empty() {
                        Ok(None)
                    } else {
                        organizer_request_full(&event.data.event, &after, None, false)
                            .map(|messages| Some((new_attendees, messages)))
                    }
                }
                Err(err) => Err(err),
            };
            match new_attendees {
                Ok(Some((new_attendees, mut messages))) => {
                    for message in &mut messages {
                        message
                            .to
                            .retain(|to| new_attendees.contains(&to.to_ascii_lowercase()));
                    }
                    messages.retain(|message| !message.to.is_empty());
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
                            ctx.keys,
                            messages,
                            pairs.take(),
                            &event.data.event,
                            reply_lease.as_ref().map(|lease| lease.lease()),
                        )
                        .await
                    {
                        Ok(fan_out) => fan_out,
                        // The key holds the successor's value: nothing to
                        // release. The same outcome as a lost commit fence.
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
                    za_set_schedule_status(&mut event.data.event, &fan_out.statuses);
                    pairs = fan_out.pairs;
                }
                Ok(None) => {}
                // The reply still applies; the delegate's invitation is
                // not built. Traced as upstream traces a builder error.
                Err(err) => {
                    trc::event!(
                        Calendar(trc::CalendarEvent::ItipMessageError),
                        AccountId = account_id,
                        DocumentId = ctx.document_id,
                        Reason = itip_reason(&err),
                        Details = "zero-access: delegate invitation not built",
                    );
                }
            }
        }
    }
    let now = now() as i64;
    let prev_email_alarm = event_.inner.data.next_alarm(now, Tz::Floating);
    let mut next_email_alarm = None;
    event.data = CalendarEventData::new_for(
        event.data.event,
        Tz::Floating,
        server.core.groupware.max_ical_instances,
        &mut next_email_alarm,
        true,
    );
    if is_organizer_update {
        match &mut event.schedule_tag {
            Some(schedule_tag) => *schedule_tag += 1,
            None => event.schedule_tag = Some(1),
        }
    }
    seal_event(&mut event, ctx.keys, account_id, pairs.as_ref())
        .map_err(|err| seal_error(err, account_id, event_id))?;
    let mut batch = BatchBuilder::new();
    // A delegating REPLY is fenced on its lease first, as `import` is.
    if let Some(lease) = &reply_lease {
        lease.lease().assert_in(&mut batch);
    }
    // The item rewrite first, ahead of the event update (one write; see
    // the module doc).
    ctx.rewrite_in(
        &mut batch,
        tree,
        Some(event_id),
        EVENT_NOTIFICATION_IS_CHANGE,
    )?;
    event
        .update(
            ctx.account_info.account_tenant_ids(),
            event_,
            account_id,
            event_id,
            &mut batch,
        )
        .caused_by(trc::location!())?;
    if prev_email_alarm != next_email_alarm {
        if let Some(prev_alarm) = prev_email_alarm {
            prev_alarm.delete_task(&mut batch);
        }
        if let Some(next_alarm) = next_email_alarm {
            next_alarm.write_task(&mut batch);
        }
    }
    ctx.commit(batch, reply_lease).await
}

impl ItemCtx<'_> {
    /// The re-read under a lease: whether the item is still the archive
    /// this pass read, pending. Another session that finished it (or a
    /// deletion) while this one waited for the lease leaves nothing to do.
    async fn still_pending(&self) -> trc::Result<bool> {
        let Some(current) = read_item(self.server, self.keys.account_id, self.document_id).await?
        else {
            return Ok(false);
        };
        Ok(current.as_bytes() == self.stored.as_bytes()
            && current.version == self.stored.version
            && archived_item_is_pending(
                current
                    .unarchive::<CalendarEventNotification>()
                    .caused_by(trc::location!())?,
            ))
    }

    /// Spec 6.4 step 5: the item resealed under the master key, bit
    /// cleared, no carrier, `event_id` and the change flag as upstream's
    /// ingest sets them, `created` kept. No commit point of its own: the
    /// callers put it ahead of the event write, or end the batch
    /// themselves.
    fn rewrite_in(
        &self,
        batch: &mut BatchBuilder,
        mut tree: ICalendar,
        event_id: Option<u32>,
        flags: u16,
    ) -> trc::Result<()> {
        let account_id = self.keys.account_id;
        seal_drained_item(&mut tree, self.keys, account_id)
            .map_err(|err| seal_error(err, account_id, self.document_id))?;
        let item = CalendarEventNotification {
            event: tree,
            event_id,
            changed_by: self.changed_by.clone(),
            flags,
            size: self.size,
            created: self.created,
            modified: now() as i64,
        };
        batch
            .with_account_id(account_id)
            .with_collection(Collection::CalendarEventNotification)
            .with_document(self.document_id)
            .custom(
                ObjectIndexBuilder::new()
                    .with_current(
                        self.stored
                            .to_unarchived::<CalendarEventNotification>()
                            .caused_by(trc::location!())?,
                    )
                    .with_changes(item)
                    .with_changed_by(self.account_info.account_tenant_ids()),
            )
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
        self.rewrite_in(&mut batch, tree, event_id, flags)?;
        batch.commit_point();
        self.commit(batch, None).await
    }

    /// Spec 6.4 step 4: an iTIP-level rejection leaves the calendar
    /// unchanged; the item is kept as history. The reason is a fixed name.
    async fn reject(
        &self,
        tree: ICalendar,
        event_id: Option<u32>,
        err: &ItipError,
    ) -> trc::Result<bool> {
        trc::event!(
            Calendar(trc::CalendarEvent::ItipMessageError),
            AccountId = self.keys.account_id,
            DocumentId = self.document_id,
            Reason = itip_reason(err),
            Details = "zero-access: inbox item kept as history",
        );
        self.rewrite_only(tree, event_id, 0).await
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
            self.account_info.account_tenant_ids(),
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
        if common::za::test_hooks::take_drain_fail(self.keys.account_id) {
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
}

/// Only a quota limit is "over quota", as `fanout::write_item_inner`
/// classifies it; any other error of the quota check is a store error.
fn is_over_quota(err: &trc::Error) -> bool {
    err.matches(trc::EventType::Limit(trc::LimitEvent::Quota))
        || err.matches(trc::EventType::Limit(trc::LimitEvent::TenantQuota))
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
fn itip_reason(err: &ItipError) -> &'static str {
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
        ItipError::SenderIsNotParticipant(_) => "SenderIsNotParticipant",
        ItipError::UnknownParticipant(_) => "UnknownParticipant",
        ItipError::UnsupportedMethod(_) => "UnsupportedMethod",
        ItipError::ICalendarParseError => "ICalendarParseError",
        ItipError::EventNotFound => "EventNotFound",
        ItipError::EventTooLarge => "EventTooLarge",
        ItipError::QuotaExceeded => "QuotaExceeded",
        ItipError::NoDefaultCalendar => "NoDefaultCalendar",
        ItipError::AutoAddDisabled => "AutoAddDisabled",
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
            sender_candidates(&snapshots, Method::Request),
            ["org@example.com"]
        );
        assert_eq!(
            sender_candidates(&snapshots, Method::Cancel),
            ["org@example.com"]
        );
        let reply = reply(Some(0));
        let snapshots = itip_snapshot(&reply, &org, false).unwrap();
        assert_eq!(
            sender_candidates(&snapshots, Method::Reply),
            ["att@example.com"]
        );
        // Both the delegator and the delegate are candidates: which one
        // replied is decided by `changed_by` and the token, never by order.
        let delegation = delegation_reply();
        let snapshots = itip_snapshot(&delegation, &org, false).unwrap();
        let mut candidates = sender_candidates(&snapshots, Method::Reply);
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
}
