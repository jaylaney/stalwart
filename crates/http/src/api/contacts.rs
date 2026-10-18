/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Key pinning spec 6: the four contacts endpoints under `/api/vault/`
//! (`contacts`, `contacts/lookup`, `contacts/pin`, `contacts/forget`).
//!
//! Each verifies the primary password afresh (`za_verify_primary`) and runs
//! the self-check (spec 3.3, the full check, repair included) on the
//! request's session keys before anything else; a private key that does not
//! open is the `vault record missing` 409. Addresses are normalized by
//! `za_normalize_address` (spec 4.3) as given, so a `mailto:` or
//! display-name form is a 400. The pin record is read fresh on every call
//! and written only through `za_pin_transact` (spec 4.2). Responses go
//! through `json` and `conflict`, untraced and `no-store`; no 400 detail
//! echoes its input, and traces carry ids and fixed strings only.
//!
//! `contacts/pin` pins the served key as `Confirmed`, drains the account
//! (`za_drain` with the request's keys), then redelivers up to
//! `REDELIVER_BATCH` withheld entries for the address as the full current
//! state (spec 6.4) and retires the delivered and dropped ones under the
//! refusal id it consumed.

use crate::api::vault::{
    CredentialsRequest, ZaVerified, bad_request, conflict, deserialize_some, json, ok,
    za_verify_primary,
};
use ::vault::{
    keys::{display_fingerprint, fingerprint_bytes, parse_fingerprint},
    pins::{PinSource, ZaPin, ZaPinRecord, ZaWithheld},
};
use common::{
    Server,
    auth::{
        AccountInfo,
        vault::{ZaMark, ZaPinState, ZaPinStep, ZaPinTransact, ZaSelfCheck, za_normalize_address},
    },
    za::lease::{ZaUidLease, ZaUidLeaseGuard},
};
use groupware::{
    calendar::{
        CalendarEvent,
        drain::{PassOutcome, ZaDrain},
        fanout::{
            ITIP_STATUS_KEY_REFUSED, ZaFanOut, ZaFanOutKind, ZaScheduleStatus, ZaSource,
            za_set_schedule_status,
        },
        itip::ItipSendStatus,
        pinning::{za_full_request_for, za_participation_reply, za_reply_from},
        seal::{TokenPairs, seal_error, seal_event, unseal_event_pairs},
    },
    scheduling::{
        itip::{ITIP_STATUS_DELIVERED, ITIP_STATUS_NOT_ATTEMPTED},
        snapshot::itip_snapshot,
    },
};
use http_proto::{HttpResponse, HttpSessionData};
use std::collections::BTreeMap;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, BatchBuilder, now},
};
use trc::AddContext;
use types::collection::Collection;

/// Withheld entries one `contacts/pin` call redelivers at most; the rest
/// are `remaining` (spec 6.4). A constant, not a setting.
pub const REDELIVER_BATCH: usize = 32;

#[derive(serde::Deserialize)]
pub(super) struct LookupRequest {
    username: String,
    password: String,
    #[serde(default)]
    totp: Option<String>,
    address: String,
}

#[derive(serde::Deserialize)]
pub(super) struct PinRequest {
    username: String,
    password: String,
    #[serde(default)]
    totp: Option<String>,
    address: String,
    fingerprint: String,
}

#[derive(serde::Deserialize)]
pub(super) struct ForgetRequest {
    username: String,
    password: String,
    #[serde(default)]
    totp: Option<String>,
    /// Required: absent `None`, `null` `Some(None)` (start over), an
    /// address `Some(Some(address))` (remove its pin).
    #[serde(default, deserialize_with = "deserialize_some")]
    address: Option<Option<String>>,
}

#[derive(serde::Serialize)]
struct ContactsResponse {
    fingerprint: String,
    stored_key_matches: bool,
    key_repaired_at: Option<i64>,
    record_missing: bool,
    pins: Vec<ContactResponse>,
}

#[derive(serde::Serialize)]
struct ContactResponse {
    address: String,
    fingerprint: Option<String>,
    pinned_at: Option<i64>,
    source: Option<&'static str>,
    served_fingerprint: Option<String>,
    withheld: usize,
}

#[derive(serde::Serialize)]
struct LookupResponse {
    address: String,
    fingerprint: String,
}

#[derive(serde::Serialize, Default)]
struct PinResponse {
    ok: bool,
    delivered: usize,
    failed: usize,
    dropped: usize,
    remaining: usize,
}

/// Spec 6.2: the account's own fingerprint, the self-check's findings and
/// one object per pin and per unpinned address with withheld entries,
/// sorted by address. The listing creates no record.
pub(super) async fn za_contacts(
    server: &Server,
    session: &HttpSessionData,
    request: CredentialsRequest,
) -> trc::Result<HttpResponse> {
    let (verified, check) = match za_contacts_verify(
        server,
        session,
        &request.username,
        &request.password,
        request.totp,
    )
    .await?
    {
        Ok(verified) => verified,
        Err(response) => return Ok(response),
    };
    // The mark first: a first-use batch (record plus rewrap) committing
    // between the reads then shows as a legacy mark or a usable record,
    // never as a pinned mark over a missing record (`record_missing`).
    let mark = server.za_mark_read(&verified.keys).await?.mark;
    let pins = server.za_pin_read(&verified.keys).await?;
    let mut listed = Vec::new();
    let mut key_repaired_at = None;
    if let Some(record) = &pins.record {
        key_repaired_at = record.key_repaired_at;
        let mut contacts: BTreeMap<&str, (Option<&ZaPin>, usize)> = BTreeMap::new();
        for pin in &record.pins {
            contacts.entry(pin.address.as_str()).or_default().0 = Some(pin);
        }
        for entry in &record.withheld {
            contacts.entry(entry.address.as_str()).or_default().1 += 1;
        }
        for (address, (pin, withheld)) in contacts {
            let served = za_resolve(server, address).await?;
            listed.push(ContactResponse {
                address: address.to_string(),
                fingerprint: pin.map(|pin| display_fingerprint(&pin.public_key)),
                pinned_at: pin.map(|pin| pin.pinned_at),
                source: pin.map(|pin| match pin.source {
                    PinSource::FirstUse => "first-use",
                    PinSource::Confirmed => "confirmed",
                }),
                served_fingerprint: served.map(|served| display_fingerprint(&served)),
                withheld,
            });
        }
    }
    Ok(json(ContactsResponse {
        fingerprint: display_fingerprint(&check.public_key),
        stored_key_matches: check.stored_key_matches,
        key_repaired_at,
        record_missing: mark == ZaMark::Pinned && !pins.is_usable(),
        pins: listed,
    }))
}

/// Spec 6.3: the normalized address and the fingerprint of its served key.
pub(super) async fn za_contacts_lookup(
    server: &Server,
    session: &HttpSessionData,
    request: LookupRequest,
) -> trc::Result<HttpResponse> {
    let address = za_address(&request.address)?;
    if let Err(response) = za_contacts_verify(
        server,
        session,
        &request.username,
        &request.password,
        request.totp,
    )
    .await?
    {
        return Ok(response);
    }
    // Not counted as a failed authentication (spec 6.3).
    Ok(match za_resolve(server, &address).await? {
        Some(served) => json(LookupResponse {
            fingerprint: display_fingerprint(&served),
            address,
        }),
        None => conflict("not a key account"),
    })
}

/// Spec 6.4: pins the served key as `Confirmed` when its fingerprint is the
/// given one, drains the account, then redelivers the address's withheld
/// entries. Redelivery failures never fail the pin.
pub(super) async fn za_contacts_pin(
    server: &Server,
    session: &HttpSessionData,
    request: PinRequest,
) -> trc::Result<HttpResponse> {
    let Some(given) = parse_fingerprint(&request.fingerprint) else {
        return Err(bad_request("invalid fingerprint"));
    };
    let address = za_address(&request.address)?;
    let (verified, _) = match za_contacts_verify(
        server,
        session,
        &request.username,
        &request.password,
        request.totp,
    )
    .await?
    {
        Ok(verified) => verified,
        Err(response) => return Ok(response),
    };
    let Some(served) = za_resolve(server, &address).await? else {
        return Ok(conflict("not a key account"));
    };
    if fingerprint_bytes(&served) != given {
        return Ok(conflict("fingerprint mismatch"));
    }
    let keys = &verified.keys;
    // Written first: a fresh record when the read found none or an
    // unusable one; under a legacy wrap the batch with the rewrap (4.4).
    let now = now() as i64;
    if let ZaPinTransact::Conflict = server
        .za_pin_transact(keys, None, |state| {
            let mut record = state.pins.record.clone().unwrap_or_else(ZaPinRecord::new);
            record.set_pin(&address, served, PinSource::Confirmed, now);
            ZaPinStep::Write(record, ())
        })
        .await?
    {
        return Ok(conflict("conflict"));
    }
    // The drain first, as DAV paths do: its merges may re-arm or retire
    // entries, and a REPLY is built only from a copy no pending item is
    // ahead of.
    let outcome = loop {
        let outcome = server.za_drain(keys).await;
        if outcome != PassOutcome::More {
            break outcome;
        }
    };
    let incomplete = matches!(outcome, PassOutcome::LeftPending | PassOutcome::Suspended);
    Ok(json(
        za_redeliver(server, &verified, &address, incomplete).await,
    ))
}

/// Spec 6.5: removes a pin, or starts over with an empty record. Under a
/// legacy wrap a usable record is written as the heal batch; with no usable
/// record only null under the pinned wrap writes (`ZaPinRecord::new()`,
/// asserting the bytes read), so the fail-closed state ends there and
/// nowhere else.
pub(super) async fn za_contacts_forget(
    server: &Server,
    session: &HttpSessionData,
    request: ForgetRequest,
) -> trc::Result<HttpResponse> {
    let Some(address) = request.address else {
        return Err(bad_request(
            "address is required (an address, or null to forget every pin).",
        ));
    };
    let address = address.as_deref().map(za_address).transpose()?;
    let (verified, _) = match za_contacts_verify(
        server,
        session,
        &request.username,
        &request.password,
        request.totp,
    )
    .await?
    {
        Ok(verified) => verified,
        Err(response) => return Ok(response),
    };
    let keys = &verified.keys;
    // The mark, read once after the self-check; a later attempt's state
    // carries a fresh one whenever the record is not usable.
    let mark = server.za_mark_read(keys).await?;
    let read_mark = mark.mark;
    let start = ZaPinState {
        pins: server.za_pin_read(keys).await?,
        mark: Some(mark),
    };
    let outcome = server
        .za_pin_transact(keys, Some(start), |state| {
            let mark = state.mark.as_ref().map_or(read_mark, |mark| mark.mark);
            match (&state.pins.record, &address) {
                (Some(stored), Some(address)) => {
                    let mut record = stored.clone();
                    if record.remove_pin(address) || mark == ZaMark::Legacy {
                        ZaPinStep::Write(record, ())
                    } else {
                        ZaPinStep::Keep(())
                    }
                }
                (Some(stored), None) => ZaPinStep::Write(
                    ZaPinRecord {
                        key_repaired_at: stored.key_repaired_at,
                        ..ZaPinRecord::new()
                    },
                    (),
                ),
                (None, None) if mark == ZaMark::Pinned => ZaPinStep::Write(ZaPinRecord::new(), ()),
                (None, _) => ZaPinStep::Keep(()),
            }
        })
        .await?;
    Ok(match outcome {
        ZaPinTransact::Conflict => conflict("conflict"),
        ZaPinTransact::Written { .. } | ZaPinTransact::Kept { .. } => ok(),
    })
}

/// Spec 6.1: fresh verification, then the full self-check on the request's
/// session keys (`once: false`; `heal: false`, the endpoints heal only by
/// writing the record). A recorded unwrap failure is the `vault record
/// missing` 409. Keys a cached session recorded as opened can see the wrap
/// stop opening: the check then errs with the key still recorded, so the
/// mark is read once more and a wrap that opens under neither purpose is
/// the same 409; any other error propagates.
async fn za_contacts_verify(
    server: &Server,
    session: &HttpSessionData,
    username: &str,
    password: &str,
    totp: Option<String>,
) -> trc::Result<Result<(ZaVerified, ZaSelfCheck), HttpResponse>> {
    let verified = match za_verify_primary(server, session, username, password, totp).await? {
        Ok(verified) => verified,
        Err(response) => return Ok(Err(response)),
    };
    match server.za_self_check(&verified.keys, false, false).await {
        Ok(check) => Ok(Ok((verified, check))),
        Err(err) => match verified.keys.private_key() {
            Some(Err(_)) => Ok(Err(conflict("vault record missing"))),
            Some(Ok(_)) if server.za_mark_read(&verified.keys).await?.mark == ZaMark::Neither => {
                Ok(Err(conflict("vault record missing")))
            }
            _ => Err(err),
        },
    }
}

/// Spec 4.3: the address as given, normalized; 400 without echoing it.
fn za_address(address: &str) -> trc::Result<String> {
    za_normalize_address(address).ok_or_else(|| bad_request("invalid address"))
}

/// Spec 6.3: the served key of a local key account, as the fan-out
/// resolves a recipient (spec 5.1) but without the receive permission.
/// `None` for unknown addresses, plain accounts and accounts pending setup.
async fn za_resolve(server: &Server, address: &str) -> trc::Result<Option<[u8; 32]>> {
    let Some(account_id) = server.za_account_id_from_email(address).await? else {
        return Ok(None);
    };
    if !server.za_is_key_account(account_id).await? {
        return Ok(None);
    }
    server.za_served_public_key(account_id).await
}

/// An entry's outcome (spec 6.4's table).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Delivered,
    Failed,
    Dropped,
    Remaining,
}

/// Which side of the event the account is on, from its stored tree.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Organizer,
    Attendee,
}

/// Spec 6.4 steps 1-5 over the address's entries, read fresh after the
/// drain. Errors are counted, never returned: the pin stands.
async fn za_redeliver(
    server: &Server,
    verified: &ZaVerified,
    address: &str,
    incomplete: bool,
) -> PinResponse {
    let account_id = verified.account_id;
    let mut response = PinResponse {
        ok: true,
        ..Default::default()
    };
    let record = match server.za_pin_read(&verified.keys).await {
        Ok(pins) => pins.record,
        Err(err) => {
            trc::error!(
                err.account_id(account_id)
                    .details("zero-access: pin record read failed")
                    .caused_by(trc::location!())
            );
            return response;
        }
    };
    let Some(record) = record else {
        return response;
    };
    let entries: Vec<&ZaWithheld> = record
        .withheld
        .iter()
        .filter(|entry| entry.address == address)
        .collect();
    response.remaining += entries.len().saturating_sub(REDELIVER_BATCH);
    let account_info = match server.account_info(account_id).await {
        Ok(account_info) => Some(account_info),
        Err(err) => {
            trc::error!(
                err.account_id(account_id)
                    .details("zero-access: withheld entry not redelivered")
                    .caused_by(trc::location!())
            );
            None
        }
    };
    // The entries a delivery or a drop consumed, with their outcome.
    let mut consumed: Vec<(&ZaWithheld, Outcome)> = Vec::new();
    for entry in entries.into_iter().take(REDELIVER_BATCH) {
        let outcome = match &account_info {
            Some(account_info) => {
                match za_redeliver_entry(server, verified, account_info, entry, incomplete).await {
                    Ok(outcome) => outcome,
                    Err(err) => {
                        trc::error!(
                            err.account_id(account_id)
                                .document_id(entry.document_id)
                                .details("zero-access: withheld entry not redelivered")
                                .caused_by(trc::location!())
                        );
                        Outcome::Failed
                    }
                }
            }
            None => Outcome::Failed,
        };
        match outcome {
            Outcome::Delivered | Outcome::Dropped => consumed.push((entry, outcome)),
            Outcome::Failed => response.failed += 1,
            Outcome::Remaining => response.remaining += 1,
        }
    }
    #[cfg(feature = "test_mode")]
    common::za::redeliver_cleanup_pause_point(account_id).await;

    // Step 5: one conditional write retiring each consumed entry whose
    // stored refusal id is still the one read after the drain, checked on
    // every attempt; an entry re-armed meanwhile stays and is `remaining`.
    let mut rearmed = vec![false; consumed.len()];
    if !consumed.is_empty() {
        let retired = server
            .za_pin_transact(&verified.keys, None, |state| {
                za_retire(state, address, &consumed)
            })
            .await;
        // A failed retirement leaves the entries withheld and undoes no
        // delivery (spec 5.2).
        let failure = match retired {
            Ok(ZaPinTransact::Written { value, .. } | ZaPinTransact::Kept { value, .. }) => {
                rearmed = value;
                None
            }
            Ok(ZaPinTransact::Conflict) => Some(trc::StoreEvent::AssertValueFailed.into_err()),
            Err(err) => Some(err),
        };
        if let Some(err) = failure {
            trc::error!(
                err.account_id(account_id)
                    .details("zero-access: withheld entries not retired")
                    .caused_by(trc::location!())
            );
        }
    }
    for ((_, outcome), rearmed) in consumed.iter().zip(rearmed) {
        match (outcome, rearmed) {
            (_, true) => response.remaining += 1,
            (Outcome::Delivered, false) => response.delivered += 1,
            (_, false) => response.dropped += 1,
        }
    }
    response
}

/// Step 5 over one state: removes each consumed entry whose refusal id is
/// unchanged; the flags name the entries a newer refusal re-armed. An entry
/// already gone keeps its outcome.
fn za_retire(
    state: &ZaPinState,
    address: &str,
    consumed: &[(&ZaWithheld, Outcome)],
) -> ZaPinStep<Vec<bool>> {
    let Some(stored) = &state.pins.record else {
        return ZaPinStep::Keep(vec![false; consumed.len()]);
    };
    let mut record = stored.clone();
    let mut removed = false;
    let rearmed = consumed
        .iter()
        .map(
            |(entry, _)| match stored.entry(entry.document_id, &entry.uid, address) {
                Some(current) if current.refusal_id == entry.refusal_id => {
                    removed |=
                        record.retire(entry.document_id, &entry.uid, address, entry.refusal_id);
                    false
                }
                Some(_) => true,
                None => false,
            },
        )
        .collect();
    if removed {
        ZaPinStep::Write(record, rearmed)
    } else {
        ZaPinStep::Keep(rearmed)
    }
}

/// Spec 6.4 steps 1-4 for one entry. An `Err` is a failure the caller
/// traces and counts as `failed`; the lease, if held, is released or
/// abandoned first.
async fn za_redeliver_entry(
    server: &Server,
    verified: &ZaVerified,
    account_info: &AccountInfo,
    entry: &ZaWithheld,
    incomplete: bool,
) -> trc::Result<Outcome> {
    let keys = &verified.keys;
    let account_id = verified.account_id;
    let document_id = entry.document_id;
    let addresses = account_info.addresses();

    // Step 1: the source document, unsealed; dropped when it is gone,
    // carries another UID, no longer names the address or fails the
    // `SendSealed` preconditions.
    let Some(archive) = za_event_archive(server, account_id, document_id).await? else {
        return Ok(Outcome::Dropped);
    };
    let mut event = archive
        .deserialize::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let pairs = unseal_event_pairs(&mut event, keys, account_id)
        .map_err(|err| seal_error(err, account_id, document_id))?;
    if event.data.event.uids().next() != Some(entry.uid.as_str()) {
        return Ok(Outcome::Dropped);
    }
    let Some(side) = za_side(&event, addresses, &entry.address) else {
        return Ok(Outcome::Dropped);
    };
    if !ItipSendStatus::resolve(
        server,
        &verified.access_token,
        account_info,
        event.data.event_range_end(),
        true,
    )
    .is_send_sealed()
    {
        return Ok(Outcome::Dropped);
    }

    // Step 2: the full current state for this one recipient.
    let tree = &event.data.event;
    let (message, lease) = match side {
        Side::Organizer => {
            let Some(message) = za_full_request_for(tree, addresses, &entry.address) else {
                return Ok(Outcome::Dropped);
            };
            // The organizer's UID lease, as the delegating merge holds it.
            let Some(lease) = ZaUidLease::acquire(server.store(), account_id, &entry.uid)
                .await
                .caused_by(trc::location!())?
            else {
                return Ok(Outcome::Failed);
            };
            (
                message,
                Some(ZaUidLeaseGuard::new(server.store().clone(), lease)),
            )
        }
        // The stale check would reject a REPLY behind a pending item.
        Side::Attendee if incomplete => return Ok(Outcome::Remaining),
        Side::Attendee => {
            let Some(message) = za_reply_from(tree, addresses, &entry.instances)
                .and_then(|from| za_participation_reply(tree, addresses, &entry.instances, &from))
            else {
                return Ok(Outcome::Dropped);
            };
            (message, None)
        }
    };

    // Step 3: the fan-out sends only this message and retires nothing.
    let fan_out = match server
        .za_fan_out(
            account_info,
            keys,
            vec![message],
            pairs,
            tree,
            ZaSource {
                document_id,
                uid: &entry.uid,
            },
            ZaFanOutKind::Redelivery,
            lease.as_ref().map(|lease| lease.lease()),
        )
        .await
    {
        Ok(fan_out) => fan_out,
        Err(err) => {
            if let Some(lease) = lease {
                if err.is_assertion_failure() {
                    // The key holds a successor's value: nothing to release.
                    lease.abandon();
                } else {
                    lease.release().await;
                }
            }
            return Err(err);
        }
    };
    let on_organizer = side == Side::Attendee;
    let status = fan_out
        .statuses
        .iter()
        .find(|status| {
            status.on_organizer == on_organizer
                && za_normalize_address(&status.address).as_deref() == Some(entry.address.as_str())
        })
        .map(|status| status.status);
    // The address's status: `1.2` goes on; `3.8` is a newer refusal, which
    // the fan-out recorded on the entry; `5.3` is no internal key account
    // any more; `5.1` (or no status) is a failure.
    let outcome = match status {
        Some(ITIP_STATUS_DELIVERED) => None,
        Some(ITIP_STATUS_KEY_REFUSED) => Some(Outcome::Remaining),
        Some(ITIP_STATUS_NOT_ATTEMPTED) => Some(Outcome::Dropped),
        _ => Some(Outcome::Failed),
    };
    let result = match outcome {
        Some(outcome) => Ok(outcome),
        // Step 4: the status, resealed under the assertion on the stored
        // archive; the schedule tag is unchanged.
        None => {
            za_set_schedule_status(&mut event.data.event, &fan_out.statuses);
            match za_reseal(
                server,
                verified,
                account_info,
                &archive,
                event,
                fan_out.pairs.as_ref(),
                document_id,
            )
            .await
            {
                Ok(()) => Ok(Outcome::Delivered),
                Err(err) if err.is_assertion_failure() => {
                    za_reseal_again(server, verified, account_info, entry, &fan_out.statuses)
                        .await
                        .map(|()| Outcome::Remaining)
                }
                Err(err) => Err(err),
            }
        }
    };
    if let Some(lease) = lease {
        lease.release().await;
    }
    result
}

/// The account's stored event, as written.
async fn za_event_archive(
    server: &Server,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Option<Archive<AlignedBytes>>> {
    server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEvent,
            document_id,
        ))
        .await
        .caused_by(trc::location!())
}

/// Whether the account organizes the event or attends it, when the tree
/// still names `address` as the other party: an ATTENDEE when the ORGANIZER
/// is one of `addresses`, else the ORGANIZER.
fn za_side(event: &CalendarEvent, addresses: &[String], address: &str) -> Option<Side> {
    let snapshots = itip_snapshot(&event.data.event, addresses, false).ok()?;
    let names = |email: &str| za_normalize_address(email).as_deref() == Some(address);
    if snapshots.organizer.email.is_local {
        snapshots
            .components
            .values()
            .any(|instance| {
                instance
                    .attendees
                    .iter()
                    .any(|attendee| names(&attendee.email.email))
            })
            .then_some(Side::Organizer)
    } else {
        names(&snapshots.organizer.email.email).then_some(Side::Attendee)
    }
}

/// Step 4's write: `event` (unsealed, its status set) sealed with `pairs`
/// and committed as an update asserting `archive`, the stored bytes it was
/// read from.
async fn za_reseal(
    server: &Server,
    verified: &ZaVerified,
    account_info: &AccountInfo,
    archive: &Archive<AlignedBytes>,
    mut event: CalendarEvent,
    pairs: Option<&TokenPairs>,
    document_id: u32,
) -> trc::Result<()> {
    let account_id = verified.account_id;
    seal_event(&mut event, &verified.keys, account_id, pairs)
        .map_err(|err| seal_error(err, account_id, document_id))?;
    let current = archive
        .to_unarchived::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let mut batch = BatchBuilder::new();
    event
        .update(
            account_info.account_tenant_ids(),
            current,
            account_id,
            document_id,
            &mut batch,
        )
        .caused_by(trc::location!())?;
    server
        .commit_batch(batch)
        .await
        .map(|_| ())
        .caused_by(trc::location!())
}

/// Step 4 after a lost archive assertion: once more from the fresh archive
/// (its own token pairs, never those read at step 1), applying the status
/// only while the fresh pin record still holds the consumed refusal id; a
/// newer refusal, a gone document or a second loss writes nothing.
async fn za_reseal_again(
    server: &Server,
    verified: &ZaVerified,
    account_info: &AccountInfo,
    entry: &ZaWithheld,
    statuses: &[ZaScheduleStatus],
) -> trc::Result<()> {
    let account_id = verified.account_id;
    let document_id = entry.document_id;
    let Some(archive) = za_event_archive(server, account_id, document_id).await? else {
        return Ok(());
    };
    let mut event = archive
        .deserialize::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let pairs = unseal_event_pairs(&mut event, &verified.keys, account_id)
        .map_err(|err| seal_error(err, account_id, document_id))?;
    let unchanged = server
        .za_pin_read(&verified.keys)
        .await?
        .record
        .as_ref()
        .and_then(|record| record.entry(document_id, &entry.uid, &entry.address))
        .is_some_and(|current| current.refusal_id == entry.refusal_id);
    if !unchanged {
        return Ok(());
    }
    za_set_schedule_status(&mut event.data.event, statuses);
    match za_reseal(
        server,
        verified,
        account_info,
        &archive,
        event,
        pairs.as_ref(),
        document_id,
    )
    .await
    {
        Err(err) if err.is_assertion_failure() => Ok(()),
        result => result,
    }
}
