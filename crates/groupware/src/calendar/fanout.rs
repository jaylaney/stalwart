/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 5: the fan-out. Under `ItipSendStatus::SendSealed` a key
//! account's own request delivers the iTIP messages upstream's builders made
//! as sealed pending items in the scheduling inboxes of internal key
//! recipients, instead of queueing them as mail (invariant 12). It runs
//! after every check of the request and immediately before the sender's
//! commit (5.4). Each item is its own batch in the recipient's account, and
//! nothing is rolled back if the sender's commit then fails. The
//! address-to-account mapping exists only inside one call (5.2,
//! invariant 13).
//!
//! A recipient is internal when its address resolves to another local
//! account that is a key account, holds
//! `Permission::CalendarSchedulingReceive`, and whose vault record serves a
//! public key (read fresh, key pinning spec 5.1; the account cache's
//! `za_public_key` is not used). A REQUEST or CANCEL is attempted only when
//! upstream marked it as the organizer's (`from_organizer`); a REPLY only
//! to the event's ORGANIZER, and only when the replier's copy holds its
//! token. Every other recipient is not attempted. `resolve_inner` builds,
//! and so caches, the recipient's access token to read its permissions, as
//! upstream's delivery does for its recipients.
//!
//! Key pinning spec 5: before sealing to an internal recipient, the fan-out
//! compares its served key with the key the sender's pin record holds for
//! the normalized address (4.3), and refuses a different one: no item, the
//! recipient's property set to `3.8` (`ITIP_STATUS_KEY_REFUSED`) and a
//! withheld entry for the source event (`ZaSource`) unless the fan-out is a
//! deletion. It runs in four phases (5.2):
//!
//! 1. Resolve every attempted address once, reading its served key, then
//!    read the pin record (and the wrap's mark when the record is absent or
//!    unusable), keeping the refusal id of each entry a delivery consumes.
//!    An attendee's DELETE without a REPLY of upstream's declines through
//!    the fork's `za_decline_all` when its copy holds an entry for an
//!    internal ORGANIZER (5.1 item 2).
//! 2. Decide every internal recipient against the record (5.1's table) and,
//!    when a pin or an entry changed, write the record once, conditionally
//!    and before any item (`za_pin_transact`; under a legacy wrap, the
//!    batch with the rewrap, 4.4). Under a pinned wrap, or one that opens
//!    under neither purpose, an absent or unusable record fails closed:
//!    every internal recipient is refused and nothing is recorded. A read
//!    or write failure is every internal recipient's `5.1`.
//! 3. Write the items of the recipients decided "deliver", in message
//!    order. In a change, a recipient holding an entry for the source gets
//!    the full current state first (5.1, the two-item rule).
//! 4. For a change, retire the entries whose recipients' items were all
//!    written, each only under the refusal id of the state phase 2 decided
//!    on; for a deletion with a message to send, every entry of the deleted
//!    document, each under the refusal id phase 1 read. A failure is traced
//!    and undoes nothing.
//!
//! The pin-record writes carry no UID lease fence (5.5).
//!
//! Ahead of phase 1, unless the fan-out is a deletion, a clone of the
//! sender's tree and the messages are reconciled (spec 5.1,
//! `sequence_floor::za_reconcile_sequences`); the phases build from that clone,
//! and the committed values return for `za_set_sequences`.

use crate::{
    SizeWriter,
    calendar::{
        ChangedBy,
        delegation::{ZaDelegationResolution, ZaResolvedAccount},
        drain::{InstanceKey, instance_key, itip_reason},
        pinning::{za_decline_all, za_full_state_for, za_participation_replies},
        seal::{
            TOKEN_LEN, TokenPairs,
            item::{sealed_item_record, za_item_shape},
            seal_error, seal_pending_item,
        },
        sequence_floor::{ZaSequence, za_instance_keys, za_reconcile_sequences},
    },
    scheduling::{
        Email, ItipError, ItipMessage,
        inbound::itip_method,
        itip::{
            ITIP_STATUS_DELIVERED, ITIP_STATUS_DELIVERY_FAILED, ITIP_STATUS_INVALID_USER,
            ITIP_STATUS_NOT_ATTEMPTED,
        },
        recipient::RecipientPolicy,
        snapshot::itip_snapshot,
    },
};
use ahash::{AHashMap, AHashSet};
use calcard::icalendar::{
    ICalendar, ICalendarMethod, ICalendarParameter, ICalendarParameterName,
    ICalendarParameterValue, ICalendarProperty,
};
use common::{
    Server,
    auth::{
        AccountInfo, BuildAccessToken,
        vault::{ZaMark, ZaPinState, ZaPinStep, ZaPinTransact, za_normalize_address},
    },
    za::lease::ZaUidLease,
};
use registry::schema::enums::Permission;
use store::{
    rand,
    write::{BatchBuilder, now},
};
use trc::AddContext;
use types::collection::Collection;
use vault::{
    pins::{PinSource, ZaInstances, ZaPinRecord, ZaWithheld},
    session::SessionKeys,
};
use zeroize::Zeroizing;

/// One SCHEDULE-STATUS the fan-out decided (spec 5.2, 5.3, 13). No `Debug`:
/// it holds an address.
#[derive(Clone, PartialEq, Eq)]
pub struct ZaScheduleStatus {
    /// The recipient's address as the message names it.
    pub address: String,
    /// On the ORGANIZER property (a reply to the organizer) instead of the
    /// recipient's ATTENDEE properties.
    pub on_organizer: bool,
    /// `ITIP_STATUS_DELIVERED`, `ITIP_STATUS_NOT_ATTEMPTED`,
    /// `ITIP_STATUS_DELIVERY_FAILED` or `ITIP_STATUS_KEY_REFUSED`.
    pub status: &'static str,
}

/// What `za_fan_out` returns to the sender's write path.
pub struct ZaFanOutResult {
    /// The pairs the sender's event bundle keeps (`seal_event(.., pairs)`):
    /// an organizer's union restricted to current attendees, or an
    /// attendee's stored pairs unchanged. `None` when there are none.
    pub pairs: Option<TokenPairs>,
    /// For `za_set_schedule_status` on the sender's plaintext tree, before
    /// it is sealed.
    pub statuses: Vec<ZaScheduleStatus>,
    /// The reconciliation's committed values (key pinning spec 5.1), for
    /// `za_set_sequences` on the same tree, next to the statuses.
    pub sequences: Vec<ZaSequence>,
}

/// A CANCEL (organizer) or declined REPLY (attendee) that a key account's
/// DELETE built from the unsealed stored event (spec 5.5), with what the
/// fan-out needs from that event.
pub struct ZaDeleteMessage {
    /// `itip_cancel`'s messages, after upstream's per-recipient split;
    /// `None` when it builds nothing, since an attendee's DELETE enters the
    /// fan-out even then (key pinning spec 5.1 item 2).
    pub messages: Option<Vec<ItipMessage<ICalendar>>>,
    /// The stored event's token pairs.
    pub pairs: Option<TokenPairs>,
    /// The unsealed stored tree.
    pub event: ICalendar,
    /// The deleted event's document id and UID, the fan-out's source (key
    /// pinning spec 5.1).
    pub document_id: u32,
    pub uid: String,
    /// The policy `itip_cancel` split the messages under, for the delegated
    /// second run's split (upgrade ruling 93).
    pub policy: RecipientPolicy,
}

/// The sender's event a fan-out delivers for (key pinning spec 5.1): its
/// document id and UID, which a withheld entry records.
#[derive(Clone, Copy)]
pub struct ZaSource<'a> {
    pub document_id: u32,
    pub uid: &'a str,
}

/// What a fan-out delivers (key pinning spec 5.1, 5.2). A `Change` sends
/// the full current state ahead of a withheld recipient's own messages (the
/// two-item rule); a `Deletion` records no withheld entry and, when it has a
/// message to send, retires every entry of the deleted document; a
/// `Redelivery` (`contacts/pin`) sends only the messages it was given and
/// retires nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZaFanOutKind {
    Change,
    Deletion,
    Redelivery,
}

/// An in-memory scheduling payload with trusted, request-local provenance.
pub struct ZaFanOutMessage {
    pub(crate) message: ItipMessage<ICalendar>,
    pub(crate) provenance: ZaMessageProvenance,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZaMessageProvenance {
    Upstream,
    DelegatedSelf,
}

impl ZaFanOutMessage {
    pub fn upstream(message: ItipMessage<ICalendar>) -> Self {
        Self {
            message,
            provenance: ZaMessageProvenance::Upstream,
        }
    }
    pub fn recipient_count(&self) -> usize {
        self.message.to.len()
    }
    pub(crate) fn delegated_self(message: ItipMessage<ICalendar>) -> Self {
        Self {
            message,
            provenance: ZaMessageProvenance::DelegatedSelf,
        }
    }
}

/// Set the authenticated writer on only the property naming the sender.
/// Stored source trees are never passed here.
pub fn za_mark_sent_by(
    message: &mut ItipMessage<ICalendar>,
    writer: &AccountInfo,
    event_account: &AccountInfo,
) {
    let property = if message.from_organizer {
        ICalendarProperty::Organizer
    } else {
        ICalendarProperty::Attendee
    };
    let delegated = writer.account_id != event_account.account_id
        && !writer.addresses.contains(&message.from)
        && event_account.addresses.contains(&message.from);
    for component in &mut message.message.components {
        if !component.component_type.is_scheduling_object() {
            continue;
        }
        for entry in &mut component.entries {
            if entry.name == property
                && entry
                    .values
                    .first()
                    .and_then(|value| value.as_text())
                    .and_then(|value| Email::new(value, &[]))
                    .is_some_and(|email| email.email == message.from)
            {
                entry
                    .params
                    .retain(|param| param.name != ICalendarParameterName::SentBy);
                if delegated && let Some(primary) = writer.addresses.first() {
                    entry.params.push(ICalendarParameter {
                        name: ICalendarParameterName::SentBy,
                        value: ICalendarParameterValue::Uri(calcard::icalendar::Uri::Location(
                            format!("mailto:{primary}"),
                        )),
                    });
                }
            }
        }
    }
}

pub trait ZaFanOut: Sync + Send {
    /// Delivers `messages` (upstream's builders' output for `event`, the
    /// sender's plaintext tree) to internal key recipients as pending items,
    /// in the four phases of key pinning spec 5.2 (see the module doc).
    /// `sender` is the account whose own request this is and `keys` its
    /// session keys; `stored_pairs` the token pairs of the sender's stored
    /// event (`event_token_pairs`), `None` on a create; `source` the event
    /// whose change this is. A lookup, pin-record or item write failure for
    /// a recipient becomes its delivery-failed status (invitations spec 10,
    /// key pinning spec 5.4) and a refusal its `3.8`; the call fails only
    /// when `keys` are not the sender's, or when `fence` is lost. Plan 10's
    /// drain calls it for the delegation fan-out (spec 6.4) with its
    /// per-UID lease as `fence`: each item's write asserts the lease, and
    /// the first write that finds it taken over fails the call (an
    /// assertion failure), so no item is written after the fence is lost.
    /// The pin-record writes carry no fence (key pinning spec 5.5).
    /// Unless `kind` is `Deletion`, the messages and a clone of `event` are
    /// first reconciled (key pinning spec 5.1); the result's `sequences`
    /// are the clone's committed values. `policy` is the event's
    /// `RecipientPolicy`, under which a change's full state is split per
    /// recipient as upstream's messages were (upgrade ruling 91).
    #[allow(clippy::too_many_arguments)]
    fn za_fan_out(
        &self,
        sender: &AccountInfo,
        writer_account: &AccountInfo,
        event_account: &AccountInfo,
        keys: &SessionKeys,
        messages: Vec<ZaFanOutMessage>,
        stored_pairs: Option<TokenPairs>,
        event: &ICalendar,
        source: ZaSource<'_>,
        kind: ZaFanOutKind,
        policy: RecipientPolicy,
        fence: Option<&ZaUidLease>,
        resolution: Option<&ZaDelegationResolution>,
    ) -> impl Future<Output = trc::Result<ZaFanOutResult>> + Send;
}

impl ZaFanOut for Server {
    #[allow(clippy::too_many_arguments)]
    async fn za_fan_out(
        &self,
        sender: &AccountInfo,
        writer_account: &AccountInfo,
        event_account: &AccountInfo,
        keys: &SessionKeys,
        messages: Vec<ZaFanOutMessage>,
        stored_pairs: Option<TokenPairs>,
        event: &ICalendar,
        source: ZaSource<'_>,
        kind: ZaFanOutKind,
        policy: RecipientPolicy,
        fence: Option<&ZaUidLease>,
        resolution: Option<&ZaDelegationResolution>,
    ) -> trc::Result<ZaFanOutResult> {
        let sender_id = sender.account_id();
        if keys.account_id != sender_id
            || writer_account.account_id != sender_id
            || resolution.is_some_and(|resolution| {
                resolution.writer_id != sender_id || resolution.owner_id != event_account.account_id
            })
        {
            return Err(trc::SecurityEvent::Unauthorized
                .into_err()
                .details("zero-access: fan-out without the sender's session keys")
                .account_id(sender_id)
                .caused_by(trc::location!()));
        }
        let delegated = writer_account.account_id != event_account.account_id;
        let (mut messages, provenances): (Vec<_>, Vec<_>) = messages
            .into_iter()
            .map(|tagged| (tagged.message, tagged.provenance))
            .unzip();
        // Key pinning spec 5.1: the SEQUENCE reconciliation, on a clone of
        // the sender's tree; a deletion leaves its CANCEL as built.
        let (reconciled, sequences) = if kind == ZaFanOutKind::Deletion {
            (None, Vec::new())
        } else {
            let mut tree = event.clone();
            let sequences = za_reconcile_sequences(&mut tree, &mut messages, sender.addresses());
            (Some(tree), sequences)
        };
        // Reconciliation changes neither length nor order: zip the original
        // tags back onto precisely the same payload positions.
        let mut messages: Vec<_> = messages
            .into_iter()
            .zip(provenances)
            .map(|(mut message, provenance)| {
                za_mark_sent_by(&mut message, writer_account, event_account);
                ZaFanOutMessage {
                    message,
                    provenance,
                }
            })
            .collect();
        let event = reconciled.as_ref().unwrap_or(event);
        let organizer = event_organizer(event, sender.addresses());
        let is_organizer = organizer
            .as_ref()
            .is_some_and(|organizer| organizer.is_local);
        // Spec 3.3, "Maintenance": only the organizer hands out tokens; an
        // attendee's copy holds its own pair.
        let union = if is_organizer {
            token_union(stored_pairs.as_ref(), event, event_account.addresses())
        } else {
            stored_pairs.clone().unwrap_or_default()
        };

        // Phase 1: route every (message, address); resolve each attempted
        // address once, by its normalized form, with its served key; then
        // read the pin record.
        let mut recipients: AHashMap<String, Recipient> = AHashMap::new();
        let mut internal: Vec<(String, [u8; 32])> = Vec::new();
        let mut read = None;
        // Spec 5.1 item (2): an attendee's DELETE that upstream built no
        // REPLY for declines to an internal ORGANIZER for which the copy
        // holds a withheld entry, routed with the rest (the stored pair's
        // token). An entry means a usable record, which needs no mark. The
        // ORGANIZER resolved here is used by the loop below, which adds it
        // to the recipients only if the decline routes `Deliver`.
        let mut resolved = None;
        if !delegated
            && kind == ZaFanOutKind::Deletion
            && !is_organizer
            && !messages.iter().any(|message| {
                matches!(
                    itip_method(&message.message.message),
                    Ok(ICalendarMethod::Reply)
                )
            })
            && let Some(key) = organizer
                .as_ref()
                .and_then(|organizer| za_normalize_address(&organizer.email))
            && let recipient @ Recipient::Internal { .. } =
                resolve_recipient(self, sender_id, &key, None, false, None).await
        {
            match self.za_pin_read(keys).await {
                Ok(pins) => {
                    if pins.record.as_ref().is_some_and(|record| {
                        record.entry(source.document_id, source.uid, &key).is_some()
                    }) && let Some(mut decline) = za_decline_all(event, sender.addresses())
                    {
                        za_mark_sent_by(&mut decline, writer_account, event_account);
                        messages.push(ZaFanOutMessage::upstream(decline));
                        resolved = Some((key, recipient));
                        read = Some(ZaPinState { pins, mark: None });
                    }
                }
                Err(err) => {
                    trc::error!(
                        err.account_id(sender_id)
                            .details("zero-access: pin record read failed")
                            .caused_by(trc::location!())
                    );
                }
            }
        }
        let mut attempts = Vec::new();
        for tagged in &messages {
            let message = &tagged.message;
            let method = itip_method(&message.message).ok();
            let is_reply = matches!(method, Some(ICalendarMethod::Reply));
            for address in &message.to {
                let route = route(method, message, address, organizer.as_ref(), &union);
                // An address that does not sanitize is not internal.
                let key = match route {
                    Route::Deliver { .. } => za_normalize_address(address),
                    Route::NotAttempted { .. } => None,
                };
                if let Some(key) = &key
                    && !recipients.contains_key(key)
                {
                    let recipient = match resolved.take_if(|(cached, _)| cached == key) {
                        Some((_, recipient)) => recipient,
                        None => {
                            let allow_self = resolution.is_some()
                                && messages.iter().any(|candidate| {
                                    candidate.provenance == ZaMessageProvenance::DelegatedSelf
                                        && candidate.message.to.iter().any(|address| {
                                            za_normalize_address(address).as_ref() == Some(key)
                                        })
                                });
                            resolve_recipient(
                                self,
                                sender_id,
                                key,
                                resolution.and_then(|resolution| resolution.accounts.get(key)),
                                allow_self,
                                resolution.map(|_| event_account.account_id),
                            )
                            .await
                        }
                    };
                    if let Recipient::Internal { public_key, .. } = recipient {
                        internal.push((key.clone(), public_key));
                    }
                    recipients.insert(key.clone(), recipient);
                }
                attempts.push(Attempt {
                    message,
                    address,
                    is_reply,
                    route,
                    key,
                    self_allowed: resolution.is_some()
                        && tagged.provenance == ZaMessageProvenance::DelegatedSelf,
                });
            }
        }
        if read.is_none()
            && (!internal.is_empty()
                || (!delegated && kind == ZaFanOutKind::Deletion && !messages.is_empty()))
        {
            read = match self.za_pin_state(keys).await {
                Ok(state) => Some(state),
                Err(err) => {
                    trc::error!(
                        err.account_id(sender_id)
                            .details("zero-access: pin record read failed")
                            .caused_by(trc::location!())
                    );
                    None
                }
            };
        }
        // Spec 5.2 phase 4: a deletion with a message to send retires every
        // entry of the deleted document, each under the refusal id read
        // here.
        let deleted: Vec<(String, u64)> =
            if !delegated && kind == ZaFanOutKind::Deletion && !messages.is_empty() {
                read.as_ref()
                    .and_then(|state| state.pins.record.as_ref())
                    .map(|record| {
                        record
                            .entries_of(source.document_id, source.uid)
                            .map(|entry| (entry.address.clone(), entry.refusal_id))
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::pin_decide_pause_point(sender_id).await;

        // Phase 2: decide every internal recipient against the record and,
        // when a pin or an entry changed, write the record once, before any
        // item (under a legacy wrap, the batch with the rewrap).
        let mut verdicts: AHashMap<String, Verdict> = AHashMap::new();
        let mut decided = None;
        if !internal.is_empty()
            && let Some(state) = read.take()
        {
            #[cfg(feature = "test_mode")]
            let fail = common::za::test_hooks::take_pin_write_fail(sender_id);
            #[cfg(not(feature = "test_mode"))]
            let fail = false;
            let outcome = if fail {
                Err(trc::StoreEvent::UnexpectedError
                    .into_err()
                    .details("zero-access test hook: pin record write failure")
                    .caused_by(trc::location!()))
            } else {
                let now = now() as i64;
                let addresses = sender.addresses();
                self.za_pin_transact(keys, Some(state), |state| {
                    decide(
                        state, &internal, &attempts, addresses, source, kind, now, !delegated,
                    )
                })
                .await
            };
            let failure = match outcome {
                Ok(
                    ZaPinTransact::Written { state, value } | ZaPinTransact::Kept { state, value },
                ) => {
                    for ((address, _), verdict) in internal.iter().zip(value) {
                        verdicts.insert(address.clone(), verdict);
                    }
                    decided = Some(state);
                    None
                }
                // The eighth lost assertion (spec 4.2).
                Ok(ZaPinTransact::Conflict) => Some(trc::StoreEvent::AssertValueFailed.into_err()),
                Err(err) => Some(err),
            };
            if let Some(err) = failure {
                trc::error!(
                    err.account_id(sender_id)
                        .details("zero-access: pin record write failed")
                        .caused_by(trc::location!())
                );
            }
        }
        // Spec 5.4: without a decision (the read or the write failed) every
        // internal recipient is `5.1` and nothing is written for it.
        let refused = verdicts
            .values()
            .filter(|verdict| **verdict == Verdict::Refuse)
            .count();
        if refused > 0 {
            trc::event!(
                Security(trc::SecurityEvent::Unauthorized),
                AccountId = sender_id,
                Total = refused,
                Details = "zero-access: delivery refused, the served key differs from the pin",
            );
        }

        // Phase 3: the items of the recipients decided "deliver", in message
        // order; a refused recipient's property gets `3.8`. In a change, a
        // recipient holding an entry for the source gets the full current
        // state ahead of its own messages (spec 5.1, the two-item rule),
        // built from the reconciled committed tree: one REQUEST for an
        // organizer-side entry, one REPLY per local address for an
        // attendee-side one.
        let mut full_states: AHashMap<&str, Vec<ItipMessage<ICalendar>>> = AHashMap::new();
        // Upgrade ruling 135 (M10): a full state whose split failed is that
        // recipient's `5.1`; its entry stays, its own messages still go.
        let mut failed_full: AHashSet<&str> = AHashSet::new();
        if !delegated
            && kind == ZaFanOutKind::Change
            && let Some(record) = decided
                .as_ref()
                .and_then(|state| state.pins.record.as_ref())
        {
            for (key, _) in &internal {
                if let Some(Verdict::Deliver(Some(_))) = verdicts.get(key)
                    && let Some(entry) = record.entry(source.document_id, source.uid, key)
                    && let Some(full) = full_state(
                        event,
                        sender.addresses(),
                        is_organizer,
                        &attempts,
                        key,
                        entry,
                        &union,
                        policy,
                    )
                {
                    match full {
                        Ok(full) => {
                            full_states.insert(key.as_str(), full);
                        }
                        Err(err) => {
                            trc::event!(
                                Calendar(trc::CalendarEvent::ItipMessageError),
                                AccountId = sender_id,
                                DocumentId = source.document_id,
                                Reason = itip_reason(&err),
                                Details = "zero-access: full state not built",
                            );
                            failed_full.insert(key.as_str());
                        }
                    }
                }
            }
        }
        let mut statuses = Vec::new();
        let mut not_written: AHashSet<&str> = AHashSet::new();
        for attempt in &attempts {
            let status = match &attempt.route {
                Route::NotAttempted { .. } => ITIP_STATUS_NOT_ATTEMPTED,
                Route::Deliver {
                    pair, reply_token, ..
                } => match attempt.key.as_deref().map(|key| (key, recipients.get(key))) {
                    Some((
                        key,
                        Some(Recipient::Internal {
                            account_id,
                            public_key,
                        }),
                    )) if (*account_id != sender_id || attempt.self_allowed)
                        && !(resolution.is_some() && *account_id == event_account.account_id) =>
                    {
                        match verdicts.get(key) {
                            Some(Verdict::Deliver(_)) => {
                                // The full state first; a failed one (`5.1`)
                                // never stops the recipient's own messages. A
                                // full-state REPLY carries the token of its own
                                // `from`, routed as any REPLY (none: `5.3`).
                                if failed_full.remove(key) {
                                    not_written.insert(key);
                                    record_status(
                                        &mut statuses,
                                        attempt.address,
                                        attempt.route.on_organizer(),
                                        ITIP_STATUS_DELIVERY_FAILED,
                                    );
                                }
                                for mut full in full_states.remove(key).unwrap_or_default() {
                                    za_mark_sent_by(&mut full, writer_account, event_account);
                                    let full_route = if is_reply_message(&full) {
                                        Some(route(
                                            Some(&ICalendarMethod::Reply),
                                            &full,
                                            attempt.address,
                                            organizer.as_ref(),
                                            &union,
                                        ))
                                    } else {
                                        None
                                    };
                                    let carrier = match &full_route {
                                        Some(Route::NotAttempted { .. }) => None,
                                        Some(Route::Deliver {
                                            pair, reply_token, ..
                                        }) => Some((pair.as_ref(), reply_token.as_deref())),
                                        None => Some((pair.as_ref(), reply_token.as_deref())),
                                    };
                                    let status = match carrier {
                                        Some((pair, reply_token)) => write_item(
                                            self,
                                            sender_id,
                                            *account_id,
                                            public_key,
                                            &full.message,
                                            pair,
                                            reply_token,
                                            fence,
                                        )
                                        .await
                                        .caused_by(trc::location!())?,
                                        None => ITIP_STATUS_NOT_ATTEMPTED,
                                    };
                                    if status != ITIP_STATUS_DELIVERED {
                                        not_written.insert(key);
                                    }
                                    record_status(
                                        &mut statuses,
                                        attempt.address,
                                        attempt.route.on_organizer(),
                                        status,
                                    );
                                }
                                let status = write_item(
                                    self,
                                    sender_id,
                                    *account_id,
                                    public_key,
                                    &attempt.message.message,
                                    pair.as_ref(),
                                    reply_token.as_deref(),
                                    fence,
                                )
                                .await
                                .caused_by(trc::location!())?;
                                if status != ITIP_STATUS_DELIVERED {
                                    not_written.insert(key);
                                }
                                status
                            }
                            Some(Verdict::Refuse) => ITIP_STATUS_KEY_REFUSED,
                            None => ITIP_STATUS_DELIVERY_FAILED,
                        }
                    }
                    Some((_, Some(Recipient::Failed))) => ITIP_STATUS_DELIVERY_FAILED,
                    Some((_, Some(Recipient::NotInternal | Recipient::Internal { .. }) | None))
                    | None => ITIP_STATUS_NOT_ATTEMPTED,
                },
            };
            record_status(
                &mut statuses,
                attempt.address,
                attempt.route.on_organizer(),
                status,
            );
        }
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::retire_pause_point(sender_id).await;

        // Phase 4 (spec 5.2): retire each entry only under the refusal id
        // the delivery consumed. A change retires the entries whose
        // recipients' items, the full state included, were all written,
        // under the state phase 2 decided on; a deletion with a message to
        // send every entry of the deleted document, under the ids phase 1
        // read, items written or not. A failure undoes nothing: the items
        // stand and the entry stays.
        let retirement = if delegated {
            None
        } else {
            match kind {
                ZaFanOutKind::Change => decided.map(|state| {
                    let consumed: Vec<(&str, u64)> = internal
                        .iter()
                        .filter_map(|(address, _)| match verdicts.get(address) {
                            Some(Verdict::Deliver(Some(refusal_id)))
                                if !not_written.contains(address.as_str()) =>
                            {
                                Some((address.as_str(), *refusal_id))
                            }
                            _ => None,
                        })
                        .collect();
                    (Some(state), consumed)
                }),
                ZaFanOutKind::Deletion => Some((
                    decided.or(read),
                    deleted
                        .iter()
                        .map(|(address, refusal_id)| (address.as_str(), *refusal_id))
                        .collect(),
                )),
                ZaFanOutKind::Redelivery => None,
            }
        };
        if let Some((start, consumed)) = retirement
            && !consumed.is_empty()
        {
            let outcome = self
                .za_pin_transact(keys, start, |state| retire(state, &consumed, source))
                .await;
            let failure = match outcome {
                Ok(ZaPinTransact::Written { .. } | ZaPinTransact::Kept { .. }) => None,
                Ok(ZaPinTransact::Conflict) => Some(trc::StoreEvent::AssertValueFailed.into_err()),
                Err(err) => Some(err),
            };
            if let Some(err) = failure {
                trc::error!(
                    err.account_id(sender_id)
                        .details("zero-access: withheld entries not retired")
                        .caused_by(trc::location!())
                );
            }
        }

        // Spec 3.3: the organizer's bundle keeps the pairs of current
        // attendees only.
        let pairs = if is_organizer {
            Some(current_attendee_pairs(
                &union,
                event,
                event_account.addresses(),
            ))
        } else {
            stored_pairs
        };
        Ok(ZaFanOutResult {
            pairs: pairs.filter(|pairs| !pairs.is_empty()),
            statuses,
            sequences,
        })
    }
}

/// One (message, address) of a fan-out, in message order (phase 1).
struct Attempt<'m> {
    message: &'m ItipMessage<ICalendar>,
    address: &'m str,
    is_reply: bool,
    route: Route,
    /// The normalized address (key pinning spec 4.3) of a `Deliver` route;
    /// `None` otherwise, or when it does not sanitize.
    key: Option<String>,
    self_allowed: bool,
}

/// Phase 2's decision for an internal recipient (key pinning spec 5.1).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Deliver; the refusal id of the entry for (source, address) in the
    /// state decided on, which a complete delivery consumes (phase 4).
    Deliver(Option<u64>),
    /// No item; `3.8`.
    Refuse,
}

/// Phase 2 over one state: each internal recipient against the record
/// (spec 5.1's table). `Write` when a pin or an entry changed.
fn decide(
    state: &ZaPinState,
    internal: &[(String, [u8; 32])],
    attempts: &[Attempt<'_>],
    addresses: &[String],
    source: ZaSource<'_>,
    kind: ZaFanOutKind,
    now: i64,
    record_source: bool,
) -> ZaPinStep<Vec<Verdict>> {
    let Some(stored) = &state.pins.record else {
        // Absent or unusable: first use under a legacy wrap, in a fresh
        // record written with the rewrap; under a pinned wrap (or one that
        // opens under neither purpose) fail closed and record nothing (spec
        // 4.4): any write would create a fresh record and end the state.
        return if state
            .mark
            .as_ref()
            .is_some_and(|mark| mark.mark == ZaMark::Legacy)
        {
            let mut record = ZaPinRecord::new();
            for (address, served) in internal {
                record.set_pin(address, *served, PinSource::FirstUse, now);
            }
            ZaPinStep::Write(record, vec![Verdict::Deliver(None); internal.len()])
        } else {
            ZaPinStep::Keep(vec![Verdict::Refuse; internal.len()])
        };
    };
    let mut record = stored.clone();
    let mut changed = false;
    let mut verdicts = Vec::with_capacity(internal.len());
    for (address, served) in internal {
        let consumed = || {
            record_source
                .then(|| {
                    stored
                        .entry(source.document_id, source.uid, address)
                        .map(|entry| entry.refusal_id)
                })
                .flatten()
        };
        let verdict = match stored.pin(address) {
            Some(pin) if pin.public_key == *served => Verdict::Deliver(consumed()),
            Some(_) => {
                // A deletion records no entry: nothing can be redelivered
                // from a deleted document.
                if record_source && kind != ZaFanOutKind::Deletion {
                    record.refuse(
                        source.document_id,
                        source.uid,
                        address,
                        now,
                        reply_instances(attempts, address, addresses)
                            .or_else(|| cancel_instances(attempts, address, addresses))
                            .as_ref(),
                    );
                    changed = true;
                }
                Verdict::Refuse
            }
            None => {
                record.set_pin(address, *served, PinSource::FirstUse, now);
                changed = true;
                Verdict::Deliver(consumed())
            }
        };
        verdicts.push(verdict);
    }
    if changed {
        ZaPinStep::Write(record, verdicts)
    } else {
        ZaPinStep::Keep(verdicts)
    }
}

/// Phase 4 over one state: removes each consumed entry whose refusal id is
/// still the one consumed, checked on every attempt.
fn retire(state: &ZaPinState, consumed: &[(&str, u64)], source: ZaSource<'_>) -> ZaPinStep<()> {
    let Some(stored) = &state.pins.record else {
        return ZaPinStep::Keep(());
    };
    let mut record = stored.clone();
    let mut removed = false;
    for (address, refusal_id) in consumed {
        removed |= record.retire(source.document_id, source.uid, address, *refusal_id);
    }
    if removed {
        ZaPinStep::Write(record, ())
    } else {
        ZaPinStep::Keep(())
    }
}

/// The two-item rule's full current state for `key`, which holds `entry`
/// (spec 5.1 item 1), from the reconciled committed tree: an organizer's
/// full REQUEST for the address, split under `policy` as upstream's full
/// update is (the address's slice, upgrade ruling 91), followed by the
/// CANCELs of the entry's recorded instances the address no longer attends
/// (`za_full_state_for`, upgrade ruling 132), or an attendee's refused
/// instances' current participation, one REPLY per local address they are
/// attended as whose token `pairs` holds (`za_participation_replies`, as
/// `contacts/pin` builds it). `None` when it is skipped: one of the
/// address's own messages is a CANCEL carrying the master, the address's
/// own organizer REQUESTs already carry every instance it attends
/// (`requests_cover`; never while an organizer-side entry records instance
/// keys), the relationship ended (the organizer's tree names no ATTENDEE
/// with the address), or the full state yields nothing. `Some(Err)` is the
/// split's error (upgrade ruling 135, M10).
#[allow(clippy::too_many_arguments)]
fn full_state(
    event: &ICalendar,
    addresses: &[String],
    is_organizer: bool,
    attempts: &[Attempt<'_>],
    key: &str,
    entry: &ZaWithheld,
    pairs: &TokenPairs,
    policy: RecipientPolicy,
) -> Option<Result<Vec<ItipMessage<ICalendar>>, ItipError>> {
    let own = || {
        attempts
            .iter()
            .filter(move |attempt| attempt.key.as_deref() == Some(key))
    };
    // Upgrade ruling 132: own REQUESTs cannot carry a recorded removal.
    let records_removals = is_organizer && !entry.instances.all && !entry.instances.keys.is_empty();
    if own().any(|attempt| cancels_master(&attempt.message.message))
        || (!records_removals && requests_cover(event, attempts, key))
    {
        return None;
    }
    if is_organizer {
        // Never changes the outcome (`za_full_request_for` yields nothing
        // for an address the tree does not name); kept for spec 5.1's rule.
        if !scheduled_attendees(event, addresses)
            .iter()
            .any(|attendee| za_normalize_address(attendee).as_deref() == Some(key))
        {
            return None;
        }
        match za_full_state_for(event, addresses, key, &entry.instances, policy) {
            Ok(full) => (!full.is_empty()).then_some(Ok(full)),
            Err(err) => Some(Err(err)),
        }
    } else {
        // Only alongside a REPLY of the change's own to the address.
        if !own().any(|attempt| attempt.is_reply) {
            return None;
        }
        let full = za_participation_replies(event, addresses, &entry.instances, Some(pairs));
        (!full.is_empty()).then_some(Ok(full))
    }
}

/// Whether `message` is a REPLY.
fn is_reply_message(message: &ItipMessage<ICalendar>) -> bool {
    matches!(itip_method(&message.message), Ok(ICalendarMethod::Reply))
}

/// A CANCEL holding a scheduling component without RECURRENCE-ID: a master
/// CANCEL cancels every instance of the copy (spec 5.1).
fn cancels_master(message: &ICalendar) -> bool {
    matches!(itip_method(message), Ok(ICalendarMethod::Cancel))
        && message.components.iter().any(|comp| {
            comp.component_type.is_scheduling_object()
                && !comp
                    .entries
                    .iter()
                    .any(|entry| entry.name == ICalendarProperty::RecurrenceId)
        })
}

/// Whether the change's own organizer REQUESTs already carry `key`'s full
/// view (spec 5.1 item 1 as amended for 1.0, upgrade ruling 91). At 1.0
/// upstream splits its messages per recipient, so an address's copy of the
/// full update is its slice, the instances it attends, master or not. For
/// every address of an ATTENDEE of the committed tree `event` that
/// normalizes to `key` (parsed with `Email::new`, as the split parses
/// them), one REQUEST from the organizer to that address must carry every
/// scheduling instance whose ATTENDEEs name it, compared by `InstanceKey`.
/// `false` when the tree names no such address. For an attendee of the
/// master this is the 0.16 rule (a REQUEST carrying the master), since its
/// slice of a master change carries the master and each override it
/// attends.
fn requests_cover(event: &ICalendar, attempts: &[Attempt<'_>], key: &str) -> bool {
    let address_of = |value: &str| Email::new(value, &[]).map(|email| email.email);
    let mut attended: AHashMap<String, AHashSet<InstanceKey>> = AHashMap::new();
    for (comp_id, instance) in za_instance_keys(event) {
        for entry in &event.components[comp_id as usize].entries {
            if entry.name == ICalendarProperty::Attendee
                && let Some(address) = entry
                    .values
                    .first()
                    .and_then(|value| value.as_text())
                    .and_then(address_of)
                && za_normalize_address(&address).as_deref() == Some(key)
            {
                attended.entry(address).or_default().insert(instance);
            }
        }
    }
    !attended.is_empty()
        && attended.iter().all(|(address, instances)| {
            attempts.iter().any(|attempt| {
                attempt.key.as_deref() == Some(key)
                    && attempt.message.from_organizer
                    && address_of(attempt.address).as_ref() == Some(address)
                    && matches!(
                        itip_method(&attempt.message.message),
                        Ok(ICalendarMethod::Request)
                    )
                    && {
                        let carried: AHashSet<InstanceKey> =
                            za_instance_keys(&attempt.message.message)
                                .into_iter()
                                .map(|(_, instance)| instance)
                                .collect();
                        instances.is_subset(&carried)
                    }
            })
        })
}

/// The instances of the organizer's per-instance CANCELs refused for `key`
/// (spec 4.1 as amended by upgrade ruling 132): each CANCEL component with
/// a RECURRENCE-ID, by the drain's `InstanceKey` as `refused_instances`
/// reads it, the master not recorded (a master CANCEL cancels every
/// instance; the full state is skipped for it). `None` when no message to
/// `key` is an organizer CANCEL.
fn cancel_instances(
    attempts: &[Attempt<'_>],
    key: &str,
    addresses: &[String],
) -> Option<ZaInstances> {
    let mut instances: Option<ZaInstances> = None;
    for attempt in attempts.iter().filter(|attempt| {
        attempt.key.as_deref() == Some(key)
            && attempt.message.from_organizer
            && matches!(
                itip_method(&attempt.message.message),
                Ok(ICalendarMethod::Cancel)
            )
    }) {
        let mut refused = refused_instances(&attempt.message.message, addresses);
        refused.master = false;
        instances
            .get_or_insert_with(ZaInstances::default)
            .union(&refused);
    }
    instances
}

/// The instances of the REPLYs refused for `key` (spec 4.1), `None` when no
/// message to it is a REPLY (an organizer-side entry).
fn reply_instances(
    attempts: &[Attempt<'_>],
    key: &str,
    addresses: &[String],
) -> Option<ZaInstances> {
    let mut instances: Option<ZaInstances> = None;
    for attempt in attempts
        .iter()
        .filter(|attempt| attempt.is_reply && attempt.key.as_deref() == Some(key))
    {
        instances
            .get_or_insert_with(ZaInstances::default)
            .union(&refused_instances(&attempt.message.message, addresses));
    }
    instances
}

/// Writes each status as the SCHEDULE-STATUS of the matching ORGANIZER or
/// ATTENDEE properties of the sender's plaintext tree (RFC 6638 3.2.9),
/// before it is sealed: sealed with the property, shown to the sender's own
/// client. Replaces an earlier fan-out's status; upstream's `3.7` is left
/// to `itip_set_unreachable_status`.
pub fn za_set_schedule_status(ical: &mut ICalendar, statuses: &[ZaScheduleStatus]) {
    if statuses.is_empty() {
        return;
    }
    for comp in ical.components.iter_mut() {
        if !comp.component_type.is_scheduling_object() {
            continue;
        }
        for entry in comp.entries.iter_mut() {
            let on_organizer = match entry.name {
                ICalendarProperty::Organizer => true,
                ICalendarProperty::Attendee => false,
                _ => continue,
            };
            let Some(address) = entry
                .values
                .first()
                .and_then(|value| value.as_text())
                .and_then(|value| Email::new(value, &[]))
            else {
                continue;
            };
            let Some(status) = statuses.iter().find(|status| {
                status.on_organizer == on_organizer
                    && status.address.eq_ignore_ascii_case(&address.email)
            }) else {
                continue;
            };
            entry.params.retain(|param| {
                param.name != ICalendarParameterName::ScheduleStatus
                    || param.value.as_text() == Some(ITIP_STATUS_INVALID_USER)
            });
            entry.params.push(ICalendarParameter::schedule_status(
                status.status.to_string(),
            ));
        }
    }
}

#[derive(Clone, Copy)]
enum Recipient {
    Internal {
        account_id: u32,
        public_key: [u8; 32],
    },
    NotInternal,
    Failed,
}

enum Route {
    NotAttempted {
        on_organizer: bool,
    },
    Deliver {
        on_organizer: bool,
        pair: Option<TokenPairs>,
        reply_token: Option<Zeroizing<[u8; TOKEN_LEN]>>,
    },
}

impl Route {
    fn on_organizer(&self) -> bool {
        match self {
            Route::NotAttempted { on_organizer } | Route::Deliver { on_organizer, .. } => {
                *on_organizer
            }
        }
    }
}

/// Spec 5.2: whether `recipient` of `message` is attempted, and with which
/// carrier (spec 5.3 step 1).
fn route(
    method: Option<&ICalendarMethod>,
    message: &ItipMessage<ICalendar>,
    recipient: &str,
    organizer: Option<&Email>,
    union: &TokenPairs,
) -> Route {
    let to_organizer =
        organizer.is_some_and(|organizer| organizer.email.eq_ignore_ascii_case(recipient));
    match method {
        // An attendee-originated REQUEST (a delegate invitation) is not
        // attempted: delegates receive their token from the organizer's
        // drain (spec 6.4).
        Some(ICalendarMethod::Request | ICalendarMethod::Cancel) if message.from_organizer => {
            Route::Deliver {
                on_organizer: false,
                pair: union.only(recipient),
                reply_token: None,
            }
        }
        // A REPLY goes to the ORGANIZER only: a delegator's bundle holds no
        // pair to check it against (spec 5.5). Without the replier's own
        // token the organizer's drain would drop it as a forgery (3.3).
        Some(ICalendarMethod::Reply) if to_organizer => match union.get(&message.from) {
            Some(token) => Route::Deliver {
                on_organizer: true,
                pair: None,
                reply_token: Some(Zeroizing::new(*token)),
            },
            None => Route::NotAttempted { on_organizer: true },
        },
        _ => Route::NotAttempted {
            on_organizer: to_organizer,
        },
    }
}

/// The event's ORGANIZER: the first one in a scheduling component,
/// `is_local` when it is one of the sender's addresses.
pub(crate) fn event_organizer(event: &ICalendar, sender_addresses: &[String]) -> Option<Email> {
    event
        .components
        .iter()
        .filter(|comp| comp.component_type.is_scheduling_object())
        .find_map(|comp| {
            comp.entries
                .iter()
                .find(|entry| entry.name == ICalendarProperty::Organizer)
                .and_then(|entry| entry.values.first())
                .and_then(|value| value.as_text())
                .and_then(|value| Email::new(value, sender_addresses))
        })
}

/// Scheduled attendees: the ATTENDEE addresses of the scheduling components
/// other than the sender's own, sanitized and deduplicated.
fn scheduled_attendees(event: &ICalendar, sender_addresses: &[String]) -> Vec<String> {
    let mut attendees: Vec<String> = Vec::new();
    for comp in event
        .components
        .iter()
        .filter(|comp| comp.component_type.is_scheduling_object())
    {
        for entry in comp
            .entries
            .iter()
            .filter(|entry| entry.name == ICalendarProperty::Attendee)
        {
            if let Some(email) = entry
                .values
                .first()
                .and_then(|value| value.as_text())
                .and_then(|value| Email::new(value, sender_addresses))
                && !email.is_local
                && !attendees.contains(&email.email)
            {
                attendees.push(email.email);
            }
        }
    }
    attendees
}

/// Spec 3.3, "Maintenance": the stored pairs plus a fresh token for every
/// scheduled attendee without one.
fn token_union(
    stored: Option<&TokenPairs>,
    event: &ICalendar,
    sender_addresses: &[String],
) -> TokenPairs {
    let mut union = stored.cloned().unwrap_or_default();
    for address in scheduled_attendees(event, sender_addresses) {
        if union.get(&address).is_none() {
            let token = Zeroizing::new(rand::random::<[u8; TOKEN_LEN]>());
            union.insert(&address, *token);
        }
    }
    union
}

/// The union restricted to the event's current attendees.
fn current_attendee_pairs(
    union: &TokenPairs,
    event: &ICalendar,
    sender_addresses: &[String],
) -> TokenPairs {
    let mut pairs = TokenPairs::new();
    for address in scheduled_attendees(event, sender_addresses) {
        if let Some(token) = union.get(&address) {
            pairs.insert(&address, *token);
        }
    }
    pairs
}

/// Key pinning spec 5.3: the served key differs from the pin (RFC 5546 "no
/// authority"), the bare code.
pub const ITIP_STATUS_KEY_REFUSED: &str = "3.8";

/// The instances a refused REPLY carries (key pinning spec 4.1): the master
/// for a component without RECURRENCE-ID, else the drain's `instance_key`
/// from the REPLY's own snapshot, its RECURRENCE-ID resolved with the
/// REPLY's own VTIMEZONEs as `itip_snapshot` resolves it. A REPLY with no
/// snapshot (upstream builds none such) records every instance.
fn refused_instances(reply: &ICalendar, addresses: &[String]) -> ZaInstances {
    let mut instances = ZaInstances::default();
    match itip_snapshot(reply, addresses, false) {
        Ok(snapshots) => {
            for instance in snapshots.components.keys() {
                instances.add(instance_key(instance));
            }
        }
        Err(_) => {
            instances.all = true;
            instances.keys.clear();
        }
    }
    instances
}

fn status_rank(status: &str) -> u8 {
    match status {
        ITIP_STATUS_KEY_REFUSED => 3,
        ITIP_STATUS_DELIVERY_FAILED => 2,
        ITIP_STATUS_NOT_ATTEMPTED => 1,
        _ => 0,
    }
}

/// One status per (address, property); the worst outcome wins when a
/// recipient gets several messages.
fn record_status(
    statuses: &mut Vec<ZaScheduleStatus>,
    address: &str,
    on_organizer: bool,
    status: &'static str,
) {
    match statuses.iter_mut().find(|existing| {
        existing.on_organizer == on_organizer && existing.address.eq_ignore_ascii_case(address)
    }) {
        Some(existing) => {
            if status_rank(status) > status_rank(existing.status) {
                existing.status = status;
            }
        }
        None => statuses.push(ZaScheduleStatus {
            address: address.to_string(),
            on_organizer,
            status,
        }),
    }
}

/// Spec 5.2: whether `address` (normalized) is an internal key account,
/// with its served key. A lookup error, the served key's included, is that
/// recipient's delivery failure, traced without the address.
async fn resolve_recipient(
    server: &Server,
    sender_id: u32,
    address: &str,
    raw: Option<&ZaResolvedAccount>,
    allow_self: bool,
    excluded_owner: Option<u32>,
) -> Recipient {
    match resolve_inner(server, sender_id, address, raw, allow_self, excluded_owner).await {
        Ok(recipient) => recipient,
        Err(err) => {
            trc::error!(
                err.account_id(sender_id)
                    .details("zero-access: fan-out recipient lookup failed")
                    .caused_by(trc::location!())
            );
            Recipient::Failed
        }
    }
}

async fn resolve_inner(
    server: &Server,
    sender_id: u32,
    address: &str,
    raw: Option<&ZaResolvedAccount>,
    allow_self: bool,
    excluded_owner: Option<u32>,
) -> trc::Result<Recipient> {
    let account_id = match raw {
        Some(ZaResolvedAccount::Account(id)) => *id,
        Some(ZaResolvedAccount::NotFound) => return Ok(Recipient::NotInternal),
        // The producer already traced this lookup failure; reuse it unchanged.
        Some(ZaResolvedAccount::Failed) => return Ok(Recipient::Failed),
        None => {
            #[cfg(feature = "test_mode")]
            if common::za::test_hooks::take_delegation_lookup_fail_once(sender_id, address) {
                return Err(trc::StoreEvent::UnexpectedError.into_err());
            }
            let Some(id) = server.za_account_id_from_email(address).await? else {
                return Ok(Recipient::NotInternal);
            };
            id
        }
    };
    // Self-inbox authority belongs to each tagged attempt, never this cache.
    // Known owner recipients on the delegated-organizer path have the
    // owner's ordinary self-inbox exclusion before any key/permission I/O.
    // A raw Failed result above remains unknown and keeps its 5.1 status.
    if (account_id == sender_id && !allow_self) || excluded_owner == Some(account_id) {
        return Ok(Recipient::NotInternal);
    }
    // `za_is_key_account`'s lookup.
    if !server
        .try_account(account_id)
        .await?
        .is_some_and(|account| account.is_key_account())
    {
        return Ok(Recipient::NotInternal);
    }
    if !server
        .access_token(account_id)
        .await?
        .build()
        .has_permission(Permission::CalendarSchedulingReceive)
    {
        return Ok(Recipient::NotInternal);
    }
    // Key pinning spec 5.1: the served key, read fresh from the vault
    // record, never the account cache's `za_public_key`. A missing,
    // unusable or pending record serves none (not internal); a store error
    // reaches `resolve_recipient`'s trace, which names the sender only.
    let Some(public_key) = server.za_served_public_key(account_id).await? else {
        return Ok(Recipient::NotInternal);
    };
    Ok(Recipient::Internal {
        account_id,
        public_key,
    })
}

/// Spec 5.3: one pending item for one recipient, its status. A failure is
/// `5.1`, traced with the recipient's account id and no content, except a
/// lost `fence`: that assertion failure is the only `Err`, and it stops the
/// fan-out.
#[allow(clippy::too_many_arguments)]
async fn write_item(
    server: &Server,
    sender_id: u32,
    recipient_id: u32,
    public_key: &[u8; 32],
    message: &ICalendar,
    pair: Option<&TokenPairs>,
    reply_token: Option<&[u8; TOKEN_LEN]>,
    fence: Option<&ZaUidLease>,
) -> trc::Result<&'static str> {
    match write_item_inner(
        server,
        sender_id,
        recipient_id,
        public_key,
        message,
        pair,
        reply_token,
        fence,
    )
    .await
    {
        Ok(true) => Ok(ITIP_STATUS_DELIVERED),
        Ok(false) => Ok(ITIP_STATUS_DELIVERY_FAILED),
        Err(err) if fence.is_some() && err.is_assertion_failure() => Err(err
            .account_id(recipient_id)
            .details("zero-access: fan-out lost its fence")),
        Err(err) => {
            trc::error!(
                err.account_id(recipient_id)
                    .details("zero-access: fan-out item not written")
                    .caused_by(trc::location!())
            );
            Ok(ITIP_STATUS_DELIVERY_FAILED)
        }
    }
}

/// `Ok(false)` when the inbound size limit or the recipient's quota refuses
/// the item (step 5); `Ok(true)` once it is committed (steps 1-4). With a
/// `fence`, the item's batch asserts the lease (whose key is global), so a
/// write after a takeover fails with an assertion failure.
#[allow(clippy::too_many_arguments)]
async fn write_item_inner(
    server: &Server,
    sender_id: u32,
    recipient_id: u32,
    public_key: &[u8; 32],
    message: &ICalendar,
    pair: Option<&TokenPairs>,
    reply_token: Option<&[u8; TOKEN_LEN]>,
    fence: Option<&ZaUidLease>,
) -> trc::Result<bool> {
    // Step 5: the two checks upstream's ingest makes.
    let size = SizeWriter::ical(message);
    if size >= server.core.groupware.itip_inbound_max_ical_size {
        return Ok(false);
    }
    let account = server
        .account(recipient_id)
        .await
        .caused_by(trc::location!())?;
    // Only a quota limit is "over quota"; any other error reaches the
    // `write_item` trace (as upstream's `copy_message` matches it).
    match server
        .has_available_quota(account.as_ref(), size as u64)
        .await
    {
        Ok(_) => {}
        Err(err) if err.matches(trc::EventType::Limit(trc::LimitEvent::Quota)) => {
            return Ok(false);
        }
        Err(err) => return Err(err.caused_by(trc::location!())),
    }
    // Upgrade ruling 94: the shape upstream queues (its text round-trip),
    // without the split's unreferenced placeholder components.
    let mut tree = za_item_shape(message).ok_or_else(|| {
        trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access: iTIP message does not reparse")
            .caused_by(trc::location!())
    })?;
    // Steps 1-3 (plan 8): the tree sealed to the recipient's public key.
    seal_pending_item(&mut tree, recipient_id, public_key, pair, reply_token)
        .map_err(|err| seal_error(err, recipient_id, u32::MAX))?;
    // Step 4: the item, its own batch in the recipient's account. At 1.0 its
    // record is built without plaintext (`sealed_item_record`, upgrade
    // ruling 13) and its id is reserved in the batch itself.
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::fan_out_pause_point(recipient_id).await;
    let (item, content) = sealed_item_record(tree, ChangedBy::PrincipalId(sender_id), None, true);
    let mut batch = BatchBuilder::new();
    // Ahead of the insert: the assertion and the insert share the batch's
    // first commit point.
    if let Some(fence) = fence {
        fence.assert_in(&mut batch);
    }
    let document_id =
        batch.reserve_document_id(recipient_id, Collection::CalendarEventNotification);
    item.insert(
        content,
        account.changed_by(),
        recipient_id,
        document_id,
        None,
        &mut batch,
    )
    .caused_by(trc::location!())?;
    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduling::ItipSummary;
    use calcard::{Entry, Parser};

    /// An event as its organizer (org@example.com) stores it: bob listed
    /// twice in two cases, carol with a client-set 3.7, and the organizer
    /// listed as an attendee.
    const EVENT: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VEVENT\r\n",
        "UID:fo-unit\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20990102T090000Z\r\n",
        "SEQUENCE:0\r\nORGANIZER:mailto:org@example.com\r\n",
        "ATTENDEE;PARTSTAT=NEEDS-ACTION:mailto:Bob@Example.com\r\n",
        "ATTENDEE;CN=Bob again:mailto:bob@example.com\r\n",
        "ATTENDEE;SCHEDULE-STATUS=3.7:mailto:carol@example.com\r\n",
        "ATTENDEE:mailto:org@example.com\r\n",
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    fn parse(text: &str) -> ICalendar {
        match Parser::new(text).entry() {
            Entry::ICalendar(ical) => ical,
            other => panic!("{other:?}"),
        }
    }

    fn organizer_addresses() -> Vec<String> {
        vec!["org@example.com".to_string()]
    }

    fn message(from: &str, from_organizer: bool, method: &str) -> ItipMessage<ICalendar> {
        ItipMessage {
            from: from.to_string(),
            from_organizer,
            to: vec![],
            summary: ItipSummary::Invite(vec![]),
            message: parse(&format!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:{method}\r\nBEGIN:VEVENT\r\nUID:fo-unit\r\nDTSTAMP:20240101T000000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
            )),
        }
    }

    fn route_of(
        message: &ItipMessage<ICalendar>,
        recipient: &str,
        organizer: Option<&Email>,
        union: &TokenPairs,
    ) -> Route {
        route(
            itip_method(&message.message).ok(),
            message,
            recipient,
            organizer,
            union,
        )
    }

    /// (value, SCHEDULE-STATUS values) of every `property` in the tree.
    fn statuses_of(ical: &ICalendar, property: ICalendarProperty) -> Vec<(String, Vec<String>)> {
        ical.components
            .iter()
            .flat_map(|comp| comp.entries.iter())
            .filter(|entry| entry.name == property)
            .map(|entry| {
                (
                    entry.values[0].as_text().unwrap().to_string(),
                    entry
                        .params
                        .iter()
                        .filter(|param| param.name == ICalendarParameterName::ScheduleStatus)
                        .map(|param| param.value.as_text().unwrap().to_string())
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn the_union_adds_one_token_per_new_attendee_and_keeps_stored_ones() {
        let event = parse(EVENT);
        let mut stored = TokenPairs::new();
        stored.insert("carol@example.com", [3; TOKEN_LEN]);
        stored.insert("gone@example.com", [9; TOKEN_LEN]);
        let union = token_union(Some(&stored), &event, &organizer_addresses());
        assert_eq!(union.len(), 3, "carol and gone kept, bob added once");
        assert!(union.matches("carol@example.com", &[3; TOKEN_LEN]));
        assert!(union.matches("gone@example.com", &[9; TOKEN_LEN]));
        let bob = *union.get("bob@example.com").expect("bob's fresh token");
        assert!(
            union.get("org@example.com").is_none(),
            "no token for the sender"
        );
        let current = current_attendee_pairs(&union, &event, &organizer_addresses());
        assert_eq!(current.len(), 2, "the removed attendee's pair is not kept");
        assert!(current.get("gone@example.com").is_none());
        assert!(current.matches("bob@example.com", &bob));
        // The next union keeps bob's token; a union with no stored pairs
        // draws a different one.
        let again = token_union(Some(&current), &event, &organizer_addresses());
        assert!(again.matches("bob@example.com", &bob));
        let fresh = token_union(None, &event, &organizer_addresses());
        assert!(!fresh.matches("bob@example.com", &bob));
    }

    #[test]
    fn requests_and_cancels_go_out_only_from_the_organizer() {
        let event = parse(EVENT);
        let organizer = event_organizer(&event, &organizer_addresses());
        assert!(
            organizer
                .as_ref()
                .is_some_and(|organizer| organizer.is_local)
        );
        let mut union = TokenPairs::new();
        union.insert("bob@example.com", [1; TOKEN_LEN]);
        for method in ["REQUEST", "CANCEL"] {
            let ours = message("org@example.com", true, method);
            match route_of(&ours, "bob@example.com", organizer.as_ref(), &union) {
                Route::Deliver {
                    on_organizer: false,
                    pair: Some(pair),
                    reply_token: None,
                } => {
                    assert_eq!(pair.len(), 1);
                    assert!(pair.matches("bob@example.com", &[1; TOKEN_LEN]));
                }
                _ => panic!("{method} from the organizer goes out with bob's pair"),
            }
            let delegate = message("bob@example.com", false, method);
            assert!(
                matches!(
                    route_of(&delegate, "dave@example.com", organizer.as_ref(), &union),
                    Route::NotAttempted {
                        on_organizer: false
                    }
                ),
                "{method} from an attendee is not attempted"
            );
        }
        // A removed attendee with no stored pair: delivered without a pair.
        let cancel = message("org@example.com", true, "CANCEL");
        assert!(matches!(
            route_of(&cancel, "dave@example.com", organizer.as_ref(), &union),
            Route::Deliver { pair: None, .. }
        ));
    }

    #[test]
    fn a_reply_goes_to_the_organizer_only_and_needs_the_repliers_token() {
        // Bob's copy: the organizer is another account.
        let event = parse(EVENT);
        let organizer = event_organizer(&event, &["bob@example.com".to_string()]);
        assert!(
            organizer
                .as_ref()
                .is_some_and(|organizer| !organizer.is_local)
        );
        let mut own = TokenPairs::new();
        own.insert("bob@example.com", [5; TOKEN_LEN]);
        let reply = message("bob@example.com", false, "REPLY");
        match route_of(&reply, "ORG@example.com", organizer.as_ref(), &own) {
            Route::Deliver {
                on_organizer: true,
                pair: None,
                reply_token: Some(token),
            } => assert_eq!(*token, [5; TOKEN_LEN]),
            _ => panic!("the reply to the organizer carries bob's token"),
        }
        assert!(
            matches!(
                route_of(&reply, "carol@example.com", organizer.as_ref(), &own),
                Route::NotAttempted {
                    on_organizer: false
                }
            ),
            "a delegator is not attempted"
        );
        assert!(
            matches!(
                route_of(
                    &reply,
                    "org@example.com",
                    organizer.as_ref(),
                    &TokenPairs::new()
                ),
                Route::NotAttempted { on_organizer: true }
            ),
            "no token: not attempted, the status on the ORGANIZER"
        );
        let refresh = message("bob@example.com", false, "REFRESH");
        assert!(matches!(
            route_of(&refresh, "org@example.com", organizer.as_ref(), &own),
            Route::NotAttempted { on_organizer: true }
        ));
    }

    #[test]
    fn statuses_land_on_every_matching_property_and_replace_earlier_ones() {
        let mut event = parse(EVENT);
        za_set_schedule_status(
            &mut event,
            &[
                ZaScheduleStatus {
                    address: "bob@example.com".into(),
                    on_organizer: false,
                    status: ITIP_STATUS_DELIVERED,
                },
                ZaScheduleStatus {
                    address: "carol@example.com".into(),
                    on_organizer: false,
                    status: ITIP_STATUS_NOT_ATTEMPTED,
                },
                ZaScheduleStatus {
                    address: "org@example.com".into(),
                    on_organizer: true,
                    status: ITIP_STATUS_DELIVERY_FAILED,
                },
            ],
        );
        let s = |values: &[&str]| values.iter().map(|v| v.to_string()).collect::<Vec<_>>();
        assert_eq!(
            statuses_of(&event, ICalendarProperty::Attendee),
            vec![
                ("mailto:Bob@Example.com".to_string(), s(&["1.2"])),
                ("mailto:bob@example.com".to_string(), s(&["1.2"])),
                ("mailto:carol@example.com".to_string(), s(&["3.7", "5.3"])),
                ("mailto:org@example.com".to_string(), s(&[])),
            ]
        );
        assert_eq!(
            statuses_of(&event, ICalendarProperty::Organizer),
            vec![("mailto:org@example.com".to_string(), s(&["5.1"]))]
        );
        za_set_schedule_status(
            &mut event,
            &[ZaScheduleStatus {
                address: "CAROL@example.com".into(),
                on_organizer: false,
                status: ITIP_STATUS_DELIVERED,
            }],
        );
        assert_eq!(
            statuses_of(&event, ICalendarProperty::Attendee)[2].1,
            s(&["3.7", "1.2"]),
            "a fan-out status is replaced, upstream's 3.7 is kept"
        );
    }

    #[test]
    fn the_worst_status_of_a_recipient_wins() {
        let mut statuses = Vec::new();
        record_status(
            &mut statuses,
            "bob@example.com",
            false,
            ITIP_STATUS_DELIVERED,
        );
        record_status(
            &mut statuses,
            "BOB@example.com",
            false,
            ITIP_STATUS_DELIVERY_FAILED,
        );
        record_status(
            &mut statuses,
            "bob@example.com",
            false,
            ITIP_STATUS_DELIVERED,
        );
        record_status(
            &mut statuses,
            "bob@example.com",
            true,
            ITIP_STATUS_NOT_ATTEMPTED,
        );
        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].status, ITIP_STATUS_DELIVERY_FAILED);
        assert!(statuses[1].on_organizer);
        assert_eq!(statuses[1].status, ITIP_STATUS_NOT_ATTEMPTED);
    }

    /// An attendee-property status for `address`.
    fn status_of(address: &str, status: &'static str) -> ZaScheduleStatus {
        ZaScheduleStatus {
            address: address.to_string(),
            on_organizer: false,
            status,
        }
    }

    fn bob() -> Vec<String> {
        vec!["bob@example.com".to_string()]
    }

    /// bob's REPLY to org@example.com for one instance: `recurrence_id` is
    /// the RECURRENCE-ID line (none for the master), `timezone` any
    /// VTIMEZONE ahead of the VEVENT.
    fn reply(recurrence_id: Option<&str>, timezone: &str) -> ICalendar {
        parse(&format!(
            concat!(
                "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nMETHOD:REPLY\r\n",
                "{timezone}BEGIN:VEVENT\r\nUID:fo-unit\r\nDTSTAMP:20240101T000000Z\r\n",
                "{recurrence_id}ORGANIZER:mailto:org@example.com\r\n",
                "ATTENDEE;PARTSTAT=ACCEPTED:mailto:bob@example.com\r\n",
                "END:VEVENT\r\nEND:VCALENDAR\r\n"
            ),
            timezone = timezone,
            recurrence_id = recurrence_id
                .map(|line| format!("{line}\r\n"))
                .unwrap_or_default(),
        ))
    }

    #[test]
    fn a_key_refusal_outranks_every_other_status() {
        let mut statuses = Vec::new();
        for status in [
            ITIP_STATUS_DELIVERY_FAILED,
            ITIP_STATUS_KEY_REFUSED,
            ITIP_STATUS_DELIVERED,
            ITIP_STATUS_NOT_ATTEMPTED,
        ] {
            record_status(&mut statuses, "bob@example.com", false, status);
        }
        assert_eq!(statuses[0].status, "3.8");
        assert!(status_rank(ITIP_STATUS_KEY_REFUSED) > status_rank(ITIP_STATUS_DELIVERY_FAILED));
    }

    #[test]
    fn a_later_delivery_replaces_a_key_refusal() {
        // EVENT's carol line already carries SCHEDULE-STATUS=3.7.
        let mut event = parse(EVENT);
        za_set_schedule_status(
            &mut event,
            &[status_of("bob@example.com", ITIP_STATUS_KEY_REFUSED)],
        );
        za_set_schedule_status(
            &mut event,
            &[status_of("bob@example.com", ITIP_STATUS_DELIVERED)],
        );
        let attendees = statuses_of(&event, ICalendarProperty::Attendee);
        assert_eq!(attendees[1].1, ["1.2"]);
        assert_eq!(attendees[2].1, ["3.7"], "upstream's 3.7 stays");
    }

    #[test]
    fn refused_instances_are_recorded_by_instant() {
        // bob's REPLYs: (a) RECURRENCE-ID;TZID=Etc/GMT-1:20990302T110000
        // with a VTIMEZONE of one STANDARD at TZOFFSETTO:+0100; (b)
        // RECURRENCE-ID:20990302T100000Z; (c) the master only.
        // calcard resolves a VTIMEZONE by name, so the fixture uses a named fixed zone.
        let tzid = reply(
            Some("RECURRENCE-ID;TZID=Etc/GMT-1:20990302T110000"),
            concat!(
                "BEGIN:VTIMEZONE\r\nTZID:Etc/GMT-1\r\nBEGIN:STANDARD\r\n",
                "DTSTART:19700101T000000\r\nTZOFFSETFROM:+0100\r\nTZOFFSETTO:+0100\r\n",
                "END:STANDARD\r\nEND:VTIMEZONE\r\n"
            ),
        );
        let utc = reply(Some("RECURRENCE-ID:20990302T100000Z"), "");
        let master = reply(None, "");
        assert_eq!(
            refused_instances(&tzid, &bob()).keys,
            refused_instances(&utc, &bob()).keys
        );
        assert_eq!(refused_instances(&tzid, &bob()).keys.len(), 1);
        assert_eq!(
            refused_instances(&master, &bob()),
            ZaInstances {
                master: true,
                keys: vec![],
                all: false
            }
        );
    }

    #[test]
    fn sent_by_marks_only_each_components_naming_property() {
        use common::auth::AccountCache;
        use std::sync::Arc;
        let owner = AccountInfo {
            account_id: 1,
            account: Arc::new(AccountCache::default()),
            addresses: vec!["org@example.com".into(), "shared@example.com".into()],
        };
        let writer = AccountInfo {
            account_id: 2,
            account: Arc::new(AccountCache::default()),
            addresses: vec!["writer@example.com".into(), "shared@example.com".into()],
        };
        let mut request = message("org@example.com", true, "REQUEST");
        let tree = EVENT
            .replace(
                "ORGANIZER:",
                "ORGANIZER;SENT-BY=\"mailto:forged@example.com\":",
            )
            .replace(
                "ATTENDEE;PARTSTAT=NEEDS-ACTION:",
                "ATTENDEE;SENT-BY=\"mailto:unrelated@example.com\";PARTSTAT=NEEDS-ACTION:",
            );
        request.message = parse(&tree);
        request
            .message
            .components
            .push(request.message.components[1].clone());
        let unchanged = request.message.clone();
        za_mark_sent_by(&mut request, &writer, &owner);
        let names = request
            .message
            .components
            .iter()
            .flat_map(|component| &component.entries)
            .filter(|entry| entry.name == ICalendarProperty::Organizer)
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 2);
        assert!(names.iter().all(|entry| {
            entry
                .params
                .iter()
                .filter(|parameter| parameter.name == ICalendarParameterName::SentBy)
                .count()
                == 1
        }));
        assert!(names.iter().all(|entry| {
            entry
                .params
                .iter()
                .any(|parameter| parameter.value.as_text() == Some("mailto:writer@example.com"))
        }));
        for (original, marked) in unchanged.components.iter().zip(&request.message.components) {
            assert_eq!(
                original
                    .entries
                    .iter()
                    .filter(|entry| entry.name != ICalendarProperty::Organizer)
                    .collect::<Vec<_>>(),
                marked
                    .entries
                    .iter()
                    .filter(|entry| entry.name != ICalendarProperty::Organizer)
                    .collect::<Vec<_>>()
            );
        }
        for named in ["writer@example.com", "shared@example.com"] {
            let mut own = message(named, true, "CANCEL");
            own.message = parse(&tree.replace("org@example.com", named));
            za_mark_sent_by(&mut own, &writer, &owner);
            assert!(
                own.message
                    .components
                    .iter()
                    .flat_map(|component| &component.entries)
                    .filter(|entry| entry.name == ICalendarProperty::Organizer)
                    .all(|entry| entry
                        .params
                        .iter()
                        .all(|parameter| parameter.name != ICalendarParameterName::SentBy))
            );
        }
        let mut reply = message("org@example.com", false, "REPLY");
        reply.message = parse(&tree.replace(
            "ATTENDEE:mailto:org@example.com",
            "ATTENDEE;SENT-BY=\"mailto:forged@example.com\":mailto:org@example.com",
        ));
        let organizer = reply.message.components[1]
            .entries
            .iter()
            .find(|entry| entry.name == ICalendarProperty::Organizer)
            .unwrap()
            .clone();
        za_mark_sent_by(&mut reply, &writer, &owner);
        assert_eq!(
            reply.message.components[1]
                .entries
                .iter()
                .find(|entry| entry.name == ICalendarProperty::Organizer),
            Some(&organizer)
        );
        assert!(
            reply.message.components[1]
                .entries
                .iter()
                .filter(|entry| entry.name == ICalendarProperty::Attendee
                    && entry.values.first().and_then(|value| value.as_text())
                        == Some("mailto:org@example.com"))
                .all(|entry| entry.params.iter().any(
                    |parameter| parameter.value.as_text() == Some("mailto:writer@example.com")
                ))
        );
        let mut own = message("org@example.com", true, "REQUEST");
        own.message = parse(&tree);
        za_mark_sent_by(&mut own, &owner, &owner);
        assert!(
            own.message
                .components
                .iter()
                .flat_map(|component| &component.entries)
                .filter(|entry| entry.name == ICalendarProperty::Organizer)
                .all(|entry| entry
                    .params
                    .iter()
                    .all(|parameter| parameter.name != ICalendarParameterName::SentBy))
        );
    }

    /// Upgrade ruling 91's coverage rule, from 1.0's own split messages. A
    /// daily event of alice's (key1): carol (key3) on the master; override
    /// X carries `x_attendees`, override Z bob (key2).
    fn coverage_event(master: &[&str], x_attendees: &[&str], master_extra: &str) -> ICalendar {
        let attendees = |list: &[&str]| {
            list.iter()
                .map(|address| {
                    format!("ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{address}\r\n")
                })
                .collect::<String>()
        };
        let vevent = |rid: Option<&str>, summary: &str, list: &[&str]| {
            let instance = match rid {
                None => format!(
                    "DTSTART:20990301T100000Z\r\nRRULE:FREQ=DAILY;COUNT=5\r\n{master_extra}"
                ),
                Some(rid) => format!("RECURRENCE-ID:{rid}\r\nDTSTART:{rid}\r\n"),
            };
            format!(
                concat!(
                    "BEGIN:VEVENT\r\nUID:fo-cover\r\nDTSTAMP:20240101T000000Z\r\n{}",
                    "DURATION:PT1H\r\nSEQUENCE:0\r\nSUMMARY:{}\r\n",
                    "ORGANIZER:mailto:key1@example.com\r\n{}END:VEVENT\r\n"
                ),
                instance,
                summary,
                attendees(list)
            )
        };
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n{}{}{}END:VCALENDAR\r\n",
            vevent(None, "master", master),
            vevent(Some("20990302T100000Z"), "x", x_attendees),
            vevent(Some("20990304T100000Z"), "z", &["key2@example.com"]),
        ))
    }

    /// `full_state` for bob as alice's fan-out decides it after the change
    /// from `old` to `new`, built by upstream's `itip_update` (with its
    /// per-recipient split); with the messages bob is sent.
    fn bob_after(
        old: &ICalendar,
        new: ICalendar,
        instances: ZaInstances,
    ) -> (
        Option<Vec<ItipMessage<ICalendar>>>,
        Vec<ItipMessage<ICalendar>>,
    ) {
        use crate::scheduling::{
            event_update::itip_update,
            recipient::{AttendeeVisibility, RecipientPolicy},
        };
        const BOB: &str = "key2@example.com";
        let policy = RecipientPolicy {
            visibility: AttendeeVisibility::All,
            max_recipients: usize::MAX,
            max_instances: usize::MAX,
        };
        let alice = vec!["key1@example.com".to_string()];
        let mut committed = new;
        let messages = itip_update(&mut committed, old, &alice, policy).unwrap();
        let attempts: Vec<Attempt<'_>> = messages
            .iter()
            .filter(|message| message.to.iter().any(|to| to == BOB))
            .map(|message| Attempt {
                message,
                address: BOB,
                is_reply: false,
                route: Route::NotAttempted {
                    on_organizer: false,
                },
                key: Some(BOB.to_string()),
                self_allowed: false,
            })
            .collect();
        let entry = ZaWithheld {
            document_id: 1,
            uid: "fo-cover".into(),
            address: BOB.into(),
            since: 0,
            refusal_id: 1,
            instances,
        };
        let full = full_state(
            &committed,
            &alice,
            true,
            &attempts,
            BOB,
            &entry,
            &TokenPairs::default(),
            policy,
        )
        .map(|full| full.expect("no split error"));
        drop(attempts);
        let own = messages
            .into_iter()
            .filter(|message| message.to.iter().any(|to| to == BOB))
            .collect();
        (full, own)
    }

    fn instances(message: &ItipMessage<ICalendar>) -> Vec<InstanceKey> {
        let mut keys: Vec<InstanceKey> = za_instance_keys(&message.message)
            .into_iter()
            .map(|(_, instance)| instance)
            .collect();
        keys.sort_unstable();
        keys
    }

    #[test]
    fn the_full_state_is_skipped_when_own_requests_cover_the_address() {
        const X: InstanceKey = Some((4_076_128_800, false));
        const Z: InstanceKey = Some((4_076_301_600, false));
        const BOB: &str = "key2@example.com";
        const CAROL: &str = "key3@example.com";
        let method = |message: &ItipMessage<ICalendar>| itip_method(&message.message).ok().cloned();

        // Case 11 Mixed: dave added to the master, bob removed from X. bob,
        // on Z only, gets the full update's Z slice and X's CANCEL; the
        // slice covers him, so no full state.
        let old = coverage_event(&[CAROL], &[BOB, CAROL], "");
        let new = coverage_event(&[CAROL, "dave@external.test"], &[CAROL], "");
        let (full, own) = bob_after(&old, new, ZaInstances::default());
        assert!(full.is_none(), "covered by the Z slice");
        assert_eq!(
            own.iter().map(method).collect::<Vec<_>>(),
            [
                Some(ICalendarMethod::Request),
                Some(ICalendarMethod::Cancel)
            ]
        );
        assert_eq!(instances(&own[0]), [Z], "the Z-only slice");

        // Still fires: bob on X and Z, not the master; only Z changes. Z's
        // partial does not cover X: the full state is his X and Z slice,
        // without the master.
        let old = coverage_event(&[CAROL], &[BOB, CAROL], "");
        let new = coverage_event(&[CAROL], &[BOB, CAROL], "")
            .to_string()
            .replace("SUMMARY:z\r\n", "SUMMARY:z v2\r\n");
        let (full, own) = bob_after(&old, parse(&new), ZaInstances::default());
        assert_eq!(own.len(), 1);
        assert_eq!(instances(&own[0]), [Z]);
        let full = full.expect("the full state fires");
        assert_eq!(full.len(), 1);
        assert_eq!(full[0].to, vec![BOB.to_string()]);
        assert_eq!(method(&full[0]), Some(ICalendarMethod::Request));
        assert_eq!(instances(&full[0]), [X, Z], "his slice, no master");

        // A master attendee: a master change's slice carries the master and
        // his overrides (0.16's `carries_master` rule).
        let old = coverage_event(&[BOB, CAROL], &[BOB, CAROL], "");
        let new = coverage_event(&[BOB, CAROL], &[BOB, CAROL], "LOCATION:moved\r\n");
        let (full, own) = bob_after(&old, new, ZaInstances::default());
        assert!(full.is_none());
        assert_eq!(instances(&own[0]), [None, X, Z]);

        // Upgrade ruling 91, A2: an exclusion-only master edit is a CANCEL
        // with RECURRENCE-IDs at 1.0, not a REQUEST: the full state fires,
        // and carries the EXDATE.
        let old = coverage_event(&[BOB, CAROL], &[BOB, CAROL], "");
        let new = coverage_event(&[BOB, CAROL], &[BOB, CAROL], "EXDATE:20990303T100000Z\r\n");
        let (full, own) = bob_after(&old, new, ZaInstances::default());
        assert_eq!(
            own.iter().map(method).collect::<Vec<_>>(),
            [Some(ICalendarMethod::Cancel)]
        );
        assert!(!cancels_master(&own[0].message));
        let full = full.expect("the full state fires");
        assert_eq!(full.len(), 1);
        assert!(
            full[0]
                .message
                .to_string()
                .contains("EXDATE:20990303T100000Z")
        );
    }

    /// Upgrade ruling 132: a refused organizer CANCEL records its
    /// per-instance components, never the master; a REPLY-less recipient
    /// without a CANCEL records nothing.
    #[test]
    fn a_refused_cancel_records_its_instances() {
        use crate::scheduling::{
            event_update::itip_update,
            recipient::{AttendeeVisibility, RecipientPolicy},
        };
        const X: (i64, bool) = (4_076_128_800, false);
        const BOB: &str = "key2@example.com";
        const CAROL: &str = "key3@example.com";
        let policy = RecipientPolicy {
            visibility: AttendeeVisibility::All,
            max_recipients: usize::MAX,
            max_instances: usize::MAX,
        };
        let alice = vec!["key1@example.com".to_string()];
        fn attempts_for<'m>(
            messages: &'m [ItipMessage<ICalendar>],
            address: &'static str,
        ) -> Vec<Attempt<'m>> {
            messages
                .iter()
                .filter(|message| message.to.iter().any(|to| to == address))
                .map(|message| Attempt {
                    message,
                    address,
                    is_reply: false,
                    route: Route::NotAttempted {
                        on_organizer: false,
                    },
                    key: Some(address.to_string()),
                    self_allowed: false,
                })
                .collect()
        }

        // bob removed from X: his CANCEL of X is recorded.
        let old = coverage_event(&[CAROL], &[BOB, CAROL], "");
        let mut new = coverage_event(&[CAROL], &[CAROL], "");
        let messages = itip_update(&mut new, &old, &alice, policy).unwrap();
        let attempts = attempts_for(&messages, BOB);
        assert_eq!(
            cancel_instances(&attempts, BOB, &alice),
            Some(ZaInstances {
                master: false,
                keys: vec![X],
                all: false,
            })
        );
        assert!(reply_instances(&attempts, BOB, &alice).is_none());

        // bob removed from the master while on Z: the master CANCEL records
        // no master.
        let old = coverage_event(&[BOB, CAROL], &[CAROL], "");
        let mut new = coverage_event(&[CAROL], &[CAROL], "");
        let messages = itip_update(&mut new, &old, &alice, policy).unwrap();
        let attempts = attempts_for(&messages, BOB);
        assert!(
            attempts
                .iter()
                .any(|attempt| cancels_master(&attempt.message.message))
        );
        assert!(
            cancel_instances(&attempts, BOB, &alice).is_none_or(|instances| !instances.master),
            "the master is never recorded organizer-side"
        );

        // carol, sent only a REQUEST, records nothing.
        let old = coverage_event(&[CAROL], &[BOB, CAROL], "");
        let mut new = coverage_event(&[CAROL], &[CAROL], "LOCATION:moved\r\n");
        let messages = itip_update(&mut new, &old, &alice, policy).unwrap();
        let attempts = attempts_for(&messages, CAROL);
        assert!(!attempts.is_empty());
        assert!(cancel_instances(&attempts, CAROL, &alice).is_none());
    }

    /// Upgrade ruling 132: an organizer-side entry recording X, bob no
    /// longer on X. A change to Z alone: the Z partial covers bob's
    /// attended set, yet the full state fires (no coverage skip while the
    /// entry records instance keys) and is his Z slice, then the CANCEL of
    /// X. With bob on X again, no CANCEL; under `all`, today's skip.
    #[test]
    fn a_recorded_removal_is_replayed_after_the_slice() {
        const X: (i64, bool) = (4_076_128_800, false);
        const Z: InstanceKey = Some((4_076_301_600, false));
        const BOB: &str = "key2@example.com";
        const CAROL: &str = "key3@example.com";
        let method = |message: &ItipMessage<ICalendar>| itip_method(&message.message).ok().cloned();
        let recorded = ZaInstances {
            master: false,
            keys: vec![X],
            all: false,
        };
        let z_change = |x_attendees: &[&str]| {
            let old = coverage_event(&[CAROL], x_attendees, "");
            let new = coverage_event(&[CAROL], x_attendees, "")
                .to_string()
                .replace("SUMMARY:z\r\n", "SUMMARY:z v2\r\n");
            (old, parse(&new))
        };

        let (old, new) = z_change(&[CAROL]);
        let (full, own) = bob_after(&old, new.clone(), ZaInstances::default());
        assert!(full.is_none(), "without a record, the Z partial covers bob");
        assert_eq!(own.len(), 1);
        let (full, _) = bob_after(&old, new, recorded.clone());
        let full = full.expect("the full state fires");
        assert_eq!(
            full.iter().map(method).collect::<Vec<_>>(),
            [
                Some(ICalendarMethod::Request),
                Some(ICalendarMethod::Cancel)
            ]
        );
        assert_eq!(instances(&full[0]), [Z], "his slice");
        assert_eq!(instances(&full[1]), [Some(X)], "then X's CANCEL");
        assert_eq!(full[1].to, vec![BOB.to_string()]);
        assert!(full[1].from_organizer);
        let cancel = full[1].message.to_string();
        assert!(
            cancel.contains("STATUS:CANCELLED")
                && cancel.contains(&format!("ATTENDEE:mailto:{BOB}"))
                && !cancel.contains(CAROL),
            "{cancel}"
        );

        // bob on X again: no CANCEL, the slice holds X.
        let (old, new) = z_change(&[BOB, CAROL]);
        let (full, _) = bob_after(&old, new, recorded);
        let full = full.expect("no coverage skip while instances are recorded");
        assert_eq!(full.len(), 1);
        assert_eq!(instances(&full[0]), [Some(X), Z]);

        // Overflowed: today's behaviour.
        let (old, new) = z_change(&[CAROL]);
        let every = ZaInstances {
            master: false,
            keys: vec![],
            all: true,
        };
        assert!(bob_after(&old, new, every).0.is_none());
    }
}
