/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Sealing of one calendar event (spec 7.1, 7.3, 8.1).
//!
//! The tree is sealed by `seal_tree` under a fresh per-write data key (DEK).
//! The stored `display_name` and dead properties go into one encrypted
//! bundle (`X-ZA-EXTRA`), and the DEK, wrapped under the account's event
//! wrapping key, is the VCALENDAR root's last entry (`X-ZA-KEY`). Carrier
//! order on the root: `X-ZA-SEALED` (if any), `X-ZA-EXTRA` (if any),
//! `X-ZA-KEY`.

use super::{
    domain::{ZaKeyDomain, validate_event_envelope},
    token::{TOKEN_LEN, TokenPairs},
    tree::{
        EXTRA_PROP, FORMAT_V1, FORMAT_V2, KEY_PROP, SealError, decode_archive, entry_text,
        has_stray_carriers, is_carrier, open_bytes_as, seal_bytes, seal_bytes_as, seal_tree,
        text_entry, unseal_tree,
    },
};
use crate::calendar::{ArchivedCalendarEvent, CalendarEvent};
use base64::{Engine, engine::general_purpose::STANDARD};
use calcard::icalendar::{ArchivedICalendarProperty, ICalendarComponentType};
use store::{
    Deserialize, Serialize,
    write::{AlignedBytes, Archive, Archiver},
};
use types::dead_property::DeadProperty;
use vault::{
    keys::{Secret, aad},
    session::SessionKeys,
};
use zeroize::Zeroize;

/// The stored fields outside the iCalendar tree that carry content (format
/// 1, release 1).
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
struct Extra {
    display_name: Option<String>,
    dead_properties: DeadProperty,
}

/// Format 2 (invitations spec 9): `Extra`'s fields and the token pairs. No
/// `Debug`: it holds tokens.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
struct ExtraV2 {
    display_name: Option<String>,
    dead_properties: DeadProperty,
    pairs: Vec<(String, [u8; TOKEN_LEN])>,
}

/// An opened `X-ZA-EXTRA` bundle of either format.
pub(super) struct OpenedExtra {
    pub(super) display_name: Option<String>,
    pub(super) dead_properties: DeadProperty,
    /// `None` for a format 1 bundle.
    pub(super) pairs: Option<TokenPairs>,
}

/// Seals the extra fields and the pairs into one bundle, or `None` when
/// there is nothing to carry (no bundle, as in release 1). Format 2 only
/// when `pairs` holds a pair; otherwise format 1, exactly as release 1
/// writes it.
pub(super) fn seal_extra(
    dek: &Secret,
    aad: &[u8],
    display_name: Option<String>,
    dead_properties: DeadProperty,
    pairs: Option<&TokenPairs>,
) -> Option<String> {
    match pairs.filter(|pairs| !pairs.is_empty()) {
        Some(pairs) => {
            let mut extra = ExtraV2 {
                display_name,
                dead_properties,
                pairs: pairs.to_vec(),
            };
            let mut plain = rkyv::to_bytes::<rkyv::rancor::Error>(&extra)
                .expect("rkyv serialization of in-memory extra cannot fail");
            let text = seal_bytes_as(FORMAT_V2, dek, aad, &plain);
            plain.as_mut_slice().zeroize();
            extra.pairs.zeroize();
            Some(text)
        }
        None if display_name.is_some() || !dead_properties.0.is_empty() => {
            let extra = Extra {
                display_name,
                dead_properties,
            };
            let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&extra)
                .expect("rkyv serialization of in-memory extra cannot fail");
            Some(seal_bytes(dek, aad, &plain))
        }
        None => None,
    }
}

/// Opens an `X-ZA-EXTRA` bundle of either format.
pub(super) fn open_extra(dek: &Secret, aad: &[u8], text: &str) -> Result<OpenedExtra, SealError> {
    match open_bytes_as(&[FORMAT_V1, FORMAT_V2], dek, aad, text)? {
        (FORMAT_V1, plain) => {
            let extra = decode_archive::<Extra>(&plain)?;
            Ok(OpenedExtra {
                display_name: extra.display_name,
                dead_properties: extra.dead_properties,
                pairs: None,
            })
        }
        (_, plain) => {
            let extra = decode_archive::<ExtraV2>(&plain)?;
            Ok(OpenedExtra {
                display_name: extra.display_name,
                dead_properties: extra.dead_properties,
                pairs: Some(TokenPairs::from_vec(extra.pairs)),
            })
        }
    }
}

/// `za/v1|event-key|<account>|<uid>`
#[cfg(test)]
fn key_aad(account_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad("event-key", account_id);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

/// `za/v1|event-extra|<account>|<uid>`
fn extra_aad(account_id: u32, uid: &str) -> Vec<u8> {
    let mut out = aad("event-extra", account_id);
    out.push(b'|');
    out.extend_from_slice(uid.as_bytes());
    out
}

/// The first UID across components. UID is visible on every component type,
/// so this is the same before sealing and after.
fn uid_of(event: &CalendarEvent) -> String {
    event
        .data
        .event
        .uids()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// True if the VCALENDAR root's last entry is a key envelope.
/// `archived_event_is_sealed` is the same check on the stored archive.
pub(crate) fn is_sealed(event: &CalendarEvent) -> bool {
    event
        .data
        .event
        .components
        .first()
        .and_then(|root| root.entries.last())
        .is_some_and(|e| is_carrier(e, KEY_PROP))
}

/// `is_sealed` on the stored archive, for readers that never deserialize
/// it (the index builder). It must stay the same predicate as `is_sealed`
/// and `tree::is_carrier`.
pub fn archived_event_is_sealed(event: &ArchivedCalendarEvent) -> bool {
    event
        .data
        .event
        .components
        .first()
        .and_then(|root| root.entries.last())
        .is_some_and(|e| {
            matches!(&e.name, ArchivedICalendarProperty::Other(n)
                if n.as_str().eq_ignore_ascii_case(KEY_PROP))
        })
}

/// Seals an event immediately before the store write (spec 8.1). Time
/// ranges and alarms were computed on the plaintext tree by the caller.
///
/// `pairs` are the token pairs the bundle carries (invitations spec 3.3):
/// on every rewrite, the stored event's pairs (`event_token_pairs`), because
/// the unsealed view the caller edits cannot hold them; `None` for a new
/// event until the fan-out (plan 9) hands out tokens.
///
/// There is no "already sealed" check: a body whose root ends with
/// `X-ZA-KEY` is an ordinary `X-` property here and is sealed and restored
/// unchanged. Each write path seals exactly once.
pub fn seal_event(
    event: &mut CalendarEvent,
    keys: &SessionKeys,
    account_id: u32,
    pairs: Option<&TokenPairs>,
) -> Result<(), SealError> {
    if keys.account_id != account_id {
        return Err(SealError::Structure("keys of another account"));
    }
    seal_event_in_domain(event, &ZaKeyDomain::owner(keys), pairs)
}

/// Seals client plaintext exactly once. Client X-ZA properties are content;
/// the storage classifier must never run at this boundary.
pub fn seal_event_in_domain(
    event: &mut CalendarEvent,
    domain: &ZaKeyDomain,
    pairs: Option<&TokenPairs>,
) -> Result<(), SealError> {
    let root = event
        .data
        .event
        .components
        .first()
        .ok_or(SealError::Structure("empty tree"))?;
    if root.component_type != ICalendarComponentType::VCalendar {
        return Err(SealError::Structure("root is not VCALENDAR"));
    }
    check_domain_parent(event, domain)?;
    let account_id = domain.owner_id();
    let uid = uid_of(event);
    let dek = Secret::random();
    seal_tree(&mut event.data.event, &dek, account_id, &uid);

    if let Some(text) = seal_extra(
        &dek,
        &extra_aad(account_id, &uid),
        event.display_name.take(),
        std::mem::take(&mut event.dead_properties),
        pairs,
    ) {
        event.data.event.components[0]
            .entries
            .push(text_entry(EXTRA_PROP, text));
    }

    let envelope = domain.seal_event_key(&dek, &uid)?;
    event.data.event.components[0]
        .entries
        .push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
    Ok(())
}

/// Pops and opens a sealed event's key envelope and `X-ZA-EXTRA` bundle,
/// after checking that no carrier is misplaced. Returns the DEK, the UID
/// and the opened bundle. The caller checked `is_sealed`.
fn open_root(
    event: &mut CalendarEvent,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<(Secret, String, Option<OpenedExtra>), SealError> {
    if keys.account_id != account_id {
        return Err(SealError::Aead);
    }
    open_root_in_domain(event, &ZaKeyDomain::owner(keys))
}

fn open_root_in_domain(
    event: &mut CalendarEvent,
    domain: &ZaKeyDomain,
) -> Result<(Secret, String, Option<OpenedExtra>), SealError> {
    let account_id = domain.owner_id();
    let uid = uid_of(event);
    let root = &mut event.data.event.components[0];
    let key_entry = root
        .entries
        .pop()
        .ok_or(SealError::Structure("key envelope"))?;
    let envelope = STANDARD
        .decode(entry_text(&key_entry).ok_or(SealError::Format)?)
        .map_err(|_| SealError::Decode)?;
    let dek = domain.open_event_key(&envelope, &uid)?;
    let extra = root.entries.pop_if(|e| is_carrier(e, EXTRA_PROP));
    if has_stray_carriers(&event.data.event) {
        return Err(SealError::Structure("misplaced carrier"));
    }
    let extra = match extra {
        Some(carrier) => Some(open_extra(
            &dek,
            &extra_aad(account_id, &uid),
            entry_text(&carrier).ok_or(SealError::Format)?,
        )?),
        None => None,
    };
    Ok((dek, uid, extra))
}

/// Unseals an event in place.
///
/// An event whose root has no trailing `X-ZA-KEY` is plaintext (written
/// before the account held keys) and is left unchanged: reads tolerate it,
/// and the next write seals it. Token pairs are not part of the view.
///
/// On `Err` the event is partly modified and must be discarded.
pub fn unseal_event(
    event: &mut CalendarEvent,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<(), SealError> {
    if !is_sealed(event) {
        return Ok(());
    }
    let (dek, uid, extra) = open_root(event, keys, account_id)?;
    if let Some(extra) = extra {
        event.display_name = extra.display_name;
        event.dead_properties = extra.dead_properties;
    }
    unseal_tree(&mut event.data.event, &dek, account_id, &uid)
}

/// `unseal_event`, also returning the token pairs of the bundle (invitations
/// spec 3.3, 5.5): a key account's DELETE builds its CANCEL from the
/// unsealed tree and hands the stored pairs to the fan-out, with one open of
/// the envelope. `None` for a plaintext event (left unchanged), an event
/// without a bundle, and a format 1 bundle.
///
/// On `Err` the event is partly modified and must be discarded.
pub fn unseal_event_pairs(
    event: &mut CalendarEvent,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Option<TokenPairs>, SealError> {
    if !is_sealed(event) {
        return Ok(None);
    }
    let (dek, uid, extra) = open_root(event, keys, account_id)?;
    let mut pairs = None;
    if let Some(extra) = extra {
        event.display_name = extra.display_name;
        event.dead_properties = extra.dead_properties;
        pairs = extra.pairs;
    }
    unseal_tree(&mut event.data.event, &dek, account_id, &uid)?;
    Ok(pairs)
}

/// Invitations spec 3.3: the token pairs of a stored event, opening only its
/// key envelope and `X-ZA-EXTRA` bundle. `None` for a plaintext event, an
/// event without a bundle, and a format 1 bundle.
pub fn event_token_pairs(
    stored: &Archive<AlignedBytes>,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Option<TokenPairs>, SealError> {
    let mut event = stored
        .deserialize::<CalendarEvent>()
        .map_err(|_| SealError::Format)?;
    if !is_sealed(&event) {
        return Ok(None);
    }
    let (_, _, extra) = open_root(&mut event, keys, account_id)?;
    Ok(extra.and_then(|extra| extra.pairs))
}

/// Spec 7.3: a read view of the content. `version` is copied from the
/// stored archive so ETags and conditional headers stay bound to it.
///
/// Works on a deserialised copy that is dropped on any error, so the stored
/// archive is never affected. A plaintext event yields a copy of `stored`.
///
/// The view is read-only: its `version` (including the integrity hash) is
/// the stored one, but its bytes are plaintext. It must never be written
/// back, passed to `into_inner()` for a write, or used as an update's
/// "current" value (`AssertValue`); writes start from the stored archive.
pub fn unseal_event_archive(
    stored: &Archive<AlignedBytes>,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Archive<AlignedBytes>, SealError> {
    let mut event = stored
        .deserialize::<CalendarEvent>()
        .map_err(|_| SealError::Format)?;
    if !is_sealed(&event) {
        return Ok(stored.clone());
    }
    unseal_event(&mut event, keys, account_id)?;
    let bytes = Archiver::new(event)
        .serialize()
        .map_err(|_| SealError::Format)?;
    let mut view = <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes)
        .map_err(|_| SealError::Format)?;
    view.version = stored.version;
    Ok(view)
}

/// Classifies stored, deserialized input before opening. This is structural
/// validation only and must never be applied to an opened or client tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZaEventStorageKind {
    LegacyPlaintext,
    Sealed,
}

pub fn classify_event_storage(event: &CalendarEvent) -> Result<ZaEventStorageKind, SealError> {
    use super::tree::{SEALED_PROP, validate_bundle_carrier};
    let recognized = event.data.event.components.iter().any(|c| {
        c.entries.iter().any(|e| {
            [KEY_PROP, SEALED_PROP, EXTRA_PROP]
                .iter()
                .any(|name| is_carrier(e, name))
        })
    });
    if !recognized {
        return Ok(ZaEventStorageKind::LegacyPlaintext);
    }
    let mut tree = event.data.event.clone();
    let root = tree
        .components
        .first_mut()
        .filter(|c| c.component_type == ICalendarComponentType::VCalendar)
        .ok_or(SealError::Structure("root is not VCALENDAR"))?;
    let key = root
        .entries
        .pop()
        .filter(|e| is_carrier(e, KEY_PROP))
        .ok_or(SealError::Structure("key envelope"))?;
    if !key.params.is_empty() || key.values.len() != 1 {
        return Err(SealError::Format);
    }
    validate_event_envelope(
        &STANDARD
            .decode(entry_text(&key).ok_or(SealError::Format)?)
            .map_err(|_| SealError::Decode)?,
    )?;
    if let Some(extra) = root.entries.pop_if(|e| is_carrier(e, EXTRA_PROP)) {
        validate_bundle_carrier(&extra, &[FORMAT_V1, FORMAT_V2])?;
    }
    if has_stray_carriers(&tree) {
        return Err(SealError::Structure("misplaced carrier"));
    }
    for component in &tree.components {
        for entry in &component.entries {
            if !is_carrier(entry, SEALED_PROP)
                && (!super::policy::is_visible_property(&component.component_type, &entry.name)
                    || entry
                        .params
                        .iter()
                        .any(|p| !super::policy::is_visible_parameter(&p.name)))
            {
                return Err(SealError::Structure("unsealed event property"));
            }
        }
        if let Some(carrier) = component
            .entries
            .last()
            .filter(|e| is_carrier(e, SEALED_PROP))
        {
            validate_bundle_carrier(carrier, &[FORMAT_V1])?;
        }
    }
    Ok(ZaEventStorageKind::Sealed)
}

fn check_domain_parent(event: &CalendarEvent, domain: &ZaKeyDomain) -> Result<(), SealError> {
    if let Some(calendar) = domain.calendar_id() {
        if !matches!(event.names.as_slice(), [name] if name.parent_id == calendar) {
            return Err(SealError::Structure("collection key event parent"));
        }
    }
    Ok(())
}

/// Read parentage follows the stored envelope, independently of write rules.
/// A pre-conversion COPY may leave legitimate linked legacy/EWK rows in an
/// owner's conversion journal. The requested calendar must remain a parent;
/// CK rows always require their exact sole parent, including during rollback.
fn classify_domain_read(
    event: &CalendarEvent,
    domain: &ZaKeyDomain,
) -> Result<ZaEventStorageKind, SealError> {
    let storage = classify_event_storage(event)?;
    let owner_legacy = domain.is_owner()
        && (storage == ZaEventStorageKind::LegacyPlaintext || {
            // Structural classification already validated the trailing key.
            let carrier = event.data.event.components[0].entries.last().unwrap();
            let envelope = STANDARD
                .decode(entry_text(carrier).ok_or(SealError::Format)?)
                .map_err(|_| SealError::Decode)?;
            envelope[1] == super::tree::WRAP_MK
        });
    if owner_legacy {
        if domain
            .calendar_id()
            .is_some_and(|calendar| !event.names.iter().any(|name| name.parent_id == calendar))
        {
            return Err(SealError::Structure("collection key event parent"));
        }
    } else {
        check_domain_parent(event, domain)?;
    }
    Ok(storage)
}

/// Stored-input boundary. Owner conversion reads may pass through no-carrier
/// legacy rows; a domain with no owner EWK may only read CK-sealed rows.
pub fn unseal_event_in_domain(
    event: &mut CalendarEvent,
    domain: &ZaKeyDomain,
) -> Result<Option<TokenPairs>, SealError> {
    if classify_domain_read(event, domain)? == ZaEventStorageKind::LegacyPlaintext {
        return if domain.is_owner() {
            Ok(None)
        } else {
            Err(SealError::NotSealed)
        };
    }
    let (dek, uid, extra) = open_root_in_domain(event, domain)?;
    let mut pairs = None;
    if let Some(extra) = extra {
        event.display_name = extra.display_name;
        event.dead_properties = extra.dead_properties;
        pairs = extra.pairs;
    }
    unseal_tree(&mut event.data.event, &dek, domain.owner_id(), &uid)?;
    Ok(pairs)
}

pub fn event_pairs_in_domain(
    stored: &Archive<AlignedBytes>,
    domain: &ZaKeyDomain,
) -> Result<Option<TokenPairs>, SealError> {
    let mut event = stored
        .deserialize::<CalendarEvent>()
        .map_err(|_| SealError::Format)?;
    if classify_domain_read(&event, domain)? == ZaEventStorageKind::LegacyPlaintext {
        return if domain.is_owner() {
            Ok(None)
        } else {
            Err(SealError::NotSealed)
        };
    }
    let (_, _, extra) = open_root_in_domain(&mut event, domain)?;
    Ok(extra.and_then(|e| e.pairs))
}

/// Read-only response archive. Never use its bytes as an update assertion.
pub fn event_view_in_domain(
    stored: &Archive<AlignedBytes>,
    domain: &ZaKeyDomain,
) -> Result<Archive<AlignedBytes>, SealError> {
    let mut event = stored
        .deserialize::<CalendarEvent>()
        .map_err(|_| SealError::Format)?;
    unseal_event_in_domain(&mut event, domain)?;
    let bytes = Archiver::new(event)
        .serialize()
        .map_err(|_| SealError::Format)?;
    let mut view = <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes)
        .map_err(|_| SealError::Format)?;
    view.version = stored.version;
    Ok(view)
}

/// Rewraps a stored event without changing its content, token carriers or DEK.
/// Legacy plaintext is sealed in full; malformed partial carriers never are.
/// Caller updates parent metadata for a cross-domain MOVE after this call.
pub fn rewrap_event(
    event: &mut CalendarEvent,
    from: &ZaKeyDomain,
    to: &ZaKeyDomain,
) -> Result<(), SealError> {
    if from.owner_id() != to.owner_id() {
        return Err(SealError::Structure("cross-account event rewrap"));
    }
    check_domain_parent(event, from)?;
    if to.calendar_id().is_some() && event.names.len() != 1 {
        return Err(SealError::Structure("collection key event parent"));
    }
    if classify_event_storage(event)? == ZaEventStorageKind::LegacyPlaintext {
        // MOVE may still name the source calendar while sealing for the target.
        let mut changed = event.clone();
        let names = changed.names.clone();
        if let Some(calendar) = to.calendar_id() {
            if changed.names.len() != 1 {
                return Err(SealError::Structure("collection key event parent"));
            }
            changed.names[0].parent_id = calendar;
        }
        seal_event_in_domain(&mut changed, to, None)?;
        changed.names = names;
        *event = changed;
        return Ok(());
    }
    let uid = uid_of(event);
    let key = event.data.event.components[0].entries.last_mut().unwrap();
    let envelope = STANDARD
        .decode(entry_text(key).ok_or(SealError::Format)?)
        .map_err(|_| SealError::Decode)?;
    let dek = from.open_event_key(&envelope, &uid)?;
    key.values[0] =
        calcard::icalendar::ICalendarValue::Text(STANDARD.encode(to.seal_event_key(&dek, &uid)?));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::seal::tree::SEALED_PROP;
    use crate::calendar::{CalendarEvent, CalendarEventData};
    use calcard::{
        Entry, Parser,
        common::timezone::Tz,
        icalendar::{ICalendarEntry, ICalendarValue},
    };
    use store::{Serialize, write::Archiver};
    use types::dead_property::{DeadElementTag, DeadPropertyTag};
    use vault::keys::{Secret, unwrap_key};

    const ICS: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nX-WR-CALNAME:cal canary\r\nBEGIN:VEVENT\r\nUID:ev-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART;TZID=UTC:20240102T090000\r\nDTEND;TZID=UTC:20240102T100000\r\nSUMMARY:secret summary canary\r\nATTENDEE;CN=Bob:mailto:bob@example.com\r\nBEGIN:VALARM\r\nACTION:EMAIL\r\nTRIGGER:-PT5M\r\nSUMMARY:alarm canary\r\nATTENDEE:mailto:me@example.com\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    const FREEBUSY: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\nBEGIN:VFREEBUSY\r\nUID:fb-1\r\nDTSTAMP:20240101T000000Z\r\nDTSTART:20240102T000000Z\r\nDTEND:20240103T000000Z\r\nORGANIZER:mailto:fb-canary@example.com\r\nFREEBUSY;FBTYPE=BUSY:20240102T090000Z/20240102T100000Z\r\nEND:VFREEBUSY\r\nEND:VCALENDAR\r\n";

    fn event_from(text: &str) -> CalendarEvent {
        let ical = match Parser::new(text).entry() {
            Entry::ICalendar(ical) => ical,
            _ => panic!(),
        };
        let mut next = None;
        CalendarEvent {
            display_name: Some("display canary".into()),
            dead_properties: DeadProperty(vec![
                DeadPropertyTag::ElementStart(DeadElementTag::new("X:dead".into(), None)),
                DeadPropertyTag::Text("dead canary".into()),
                DeadPropertyTag::ElementEnd,
            ]),
            data: CalendarEventData::new(ical, Tz::Floating, 100, &mut next),
            size: text.len() as u32,
            ..Default::default()
        }
    }

    fn event() -> CalendarEvent {
        event_from(ICS)
    }

    fn keys() -> SessionKeys {
        SessionKeys::new(9, 1, Secret::random())
    }

    fn archive(event: &CalendarEvent) -> Archive<AlignedBytes> {
        let bytes = Archiver::new(event.clone()).serialize().unwrap();
        <Archive<AlignedBytes> as store::Deserialize>::deserialize(&bytes).unwrap()
    }

    // These regressions catch DEK/content resealing during conversion, lax stored routing,
    // and accidentally treating restored client properties as storage carriers.
    fn ck_domain(ewk: Option<Secret>, generation: u32) -> super::super::domain::ZaKeyDomain {
        super::super::domain::ZaKeyDomain::shared(
            9,
            if ewk.is_some() { 9 } else { 8 },
            ewk,
            42,
            [7; 16],
            (1..=generation)
                .map(|g| (g, Secret::from_bytes([3; 32])))
                .collect(),
            false,
        )
        .unwrap()
    }

    fn parented_event() -> CalendarEvent {
        let mut e = event();
        e.names.push(common::DavName {
            parent_id: 42,
            name: "e.ics".into(),
        });
        e
    }

    #[test]
    fn rewrap_changes_only_the_envelope() {
        let keys = keys();
        let owner = super::super::domain::ZaKeyDomain::owner(&keys);
        let ck = ck_domain(Some(keys.ewk().clone()), 1);
        let pairs = two_pairs();
        let mut e = parented_event();
        seal_event(&mut e, &keys, 9, Some(&pairs)).unwrap();
        let before = e.clone();
        rewrap_event(&mut e, &owner, &ck).unwrap();
        assert_ne!(
            root_entries(&mut e).last(),
            root_entries(&mut before.clone()).last()
        );
        let mut a = e.clone();
        let mut b = before.clone();
        root_entries(&mut a).pop();
        root_entries(&mut b).pop();
        assert_eq!(
            a, b,
            "all ciphertext, token bundle and metadata bytes stay identical"
        );
        let stored = archive(&e);
        let returned = event_pairs_in_domain(&stored, &ck).unwrap().unwrap();
        assert_eq!(returned.len(), 2);
        assert_eq!(returned.get("a@example.com"), pairs.get("a@example.com"));
        assert_eq!(returned.get("b@example.com"), pairs.get("b@example.com"));
        let view = event_view_in_domain(&stored, &ck).unwrap();
        assert_eq!(view.version, stored.version);
        assert_eq!(
            view.deserialize::<CalendarEvent>().unwrap(),
            parented_event()
        );
        rewrap_event(&mut e, &ck, &owner).unwrap();
        unseal_event(&mut e, &keys, 9).unwrap();
        assert_eq!(e, parented_event());
    }

    #[test]
    fn rewrap_preserves_the_existing_key_carrier_name() {
        let keys = keys();
        let mut e = parented_event();
        seal_event(&mut e, &keys, 9, None).unwrap();
        root_entries(&mut e).last_mut().unwrap().name =
            calcard::icalendar::ICalendarProperty::Other("x-za-key".into());
        rewrap_event(&mut e, &ZaKeyDomain::owner(&keys), &ck_domain(None, 1)).unwrap();
        assert_eq!(
            root_entries(&mut e).last().unwrap().name,
            calcard::icalendar::ICalendarProperty::Other("x-za-key".into())
        );
    }

    #[test]
    fn plaintext_converts_under_ck() {
        let mut e = parented_event();
        assert_eq!(
            classify_event_storage(&e).unwrap(),
            ZaEventStorageKind::LegacyPlaintext
        );
        let ck = ck_domain(None, 1);
        rewrap_event(&mut e, &ck, &ck).unwrap();
        assert_eq!(
            classify_event_storage(&e).unwrap(),
            ZaEventStorageKind::Sealed
        );
        let bytes = STANDARD
            .decode(entry_text(root_entries(&mut e).last().unwrap()).unwrap())
            .unwrap();
        assert_eq!(bytes.len(), 82);
        assert_eq!(&bytes[..10], &[1, 3, 0, 0, 0, 42, 0, 0, 0, 1]);
        unseal_event_in_domain(&mut e, &ck).unwrap();
        assert_eq!(e, parented_event());
    }

    #[test]
    fn event_storage_classifier_rejects_partial_carriers() {
        let keys = keys();
        let mut sealed = parented_event();
        seal_event(&mut sealed, &keys, 9, None).unwrap();
        let sealed = archive(&sealed).deserialize::<CalendarEvent>().unwrap();
        assert_eq!(
            classify_event_storage(&sealed).unwrap(),
            ZaEventStorageKind::Sealed
        );
        let mut partial = sealed.clone();
        root_entries(&mut partial).pop();
        let mut misplaced = sealed.clone();
        let key = root_entries(&mut misplaced).pop().unwrap();
        misplaced.data.event.components[1].entries.push(key);
        let mut moved = sealed.clone();
        let n = root_entries(&mut moved).len();
        root_entries(&mut moved).swap(n - 1, n - 2);
        let bad_envelope = with_envelope(&sealed, |e| {
            e.pop();
        });
        let mut bad_text = sealed.clone();
        bad_text.data.event.components[1]
            .entries
            .last_mut()
            .unwrap()
            .values = vec![ICalendarValue::Text("AQ==".into())];
        for bad in [partial, misplaced, moved, bad_envelope, bad_text] {
            let bad = archive(&bad).deserialize::<CalendarEvent>().unwrap();
            assert!(classify_event_storage(&bad).is_err());
        }
        for component in [0, 1] {
            for carrier in [KEY_PROP, EXTRA_PROP, SEALED_PROP] {
                let mut stray = parented_event();
                stray.data.event.components[component]
                    .entries
                    .push(text_entry(carrier, "AQ==".into()));
                let stray = archive(&stray).deserialize::<CalendarEvent>().unwrap();
                assert!(classify_event_storage(&stray).is_err());
            }
        }
        let fake = with_envelope(&sealed, |e| e[10] ^= 1);
        let mut fake = archive(&fake).deserialize::<CalendarEvent>().unwrap();
        assert_eq!(
            classify_event_storage(&fake).unwrap(),
            ZaEventStorageKind::Sealed
        );
        assert!(
            unseal_event_in_domain(&mut fake, &super::super::domain::ZaKeyDomain::owner(&keys))
                .is_err()
        );
    }

    #[test]
    fn owner_reads_conversion_mix_grantee_requires_ck_and_move_requires_new_parent() {
        let keys = keys();
        let account = ZaKeyDomain::owner(&keys);
        let owner = ck_domain(Some(keys.ewk().clone()), 1);
        let grantee = ck_domain(None, 1);
        let mut plain = parented_event();
        assert!(
            unseal_event_in_domain(&mut plain, &owner)
                .unwrap()
                .is_none()
        );
        assert!(unseal_event_in_domain(&mut parented_event(), &grantee).is_err());
        let mut old = parented_event();
        seal_event(&mut old, &keys, 9, None).unwrap();
        assert!(unseal_event_in_domain(&mut old.clone(), &owner).is_ok());
        assert!(unseal_event_in_domain(&mut old.clone(), &grantee).is_err());
        rewrap_event(&mut old, &account, &owner).unwrap();
        let destination = ZaKeyDomain::shared(
            9,
            8,
            None,
            43,
            [8; 16],
            std::collections::BTreeMap::from([(1, Secret::from_bytes([6; 32]))]),
            false,
        )
        .unwrap();
        let before = old.clone();
        rewrap_event(&mut old, &owner, &destination).unwrap();
        let mut before_content = before;
        let mut after_content = old.clone();
        root_entries(&mut before_content).pop();
        root_entries(&mut after_content).pop();
        assert_eq!(before_content, after_content);
        assert!(unseal_event_in_domain(&mut old.clone(), &destination).is_err());
        old.names[0].parent_id = 43;
        unseal_event_in_domain(&mut old, &destination).unwrap();
        let mut expected = parented_event();
        expected.names[0].parent_id = 43;
        assert_eq!(old, expected);
    }

    fn journal_domain(keys: &SessionKeys, reverting: bool) -> ZaKeyDomain {
        ZaKeyDomain::shared(
            9,
            9,
            Some(keys.ewk().clone()),
            42,
            [7; 16],
            std::collections::BTreeMap::from([(1, Secret::from_bytes([3; 32]))]),
            reverting,
        )
        .unwrap()
    }

    fn linked_event() -> CalendarEvent {
        let mut event = parented_event();
        event.names.push(common::DavName {
            parent_id: 51,
            name: "copy.ics".into(),
        });
        event
    }

    fn assert_journal_read(reverting: bool, ewk_wrapped: bool) {
        let keys = keys();
        let domain = journal_domain(&keys, reverting);
        let original = linked_event();
        let mut input = original.clone();
        let pairs = two_pairs();
        if ewk_wrapped {
            seal_event(&mut input, &keys, 9, Some(&pairs)).unwrap();
        }
        let stored = archive(&input);
        let mut opened = stored.deserialize::<CalendarEvent>().unwrap();
        let returned = unseal_event_in_domain(&mut opened, &domain).unwrap();
        assert_eq!(opened, original);
        let extracted = event_pairs_in_domain(&stored, &domain).unwrap();
        for actual in [returned, extracted] {
            if ewk_wrapped {
                let actual = actual.unwrap();
                assert_eq!(actual.len(), 2);
                assert_eq!(actual.get("a@example.com"), Some(&[1; TOKEN_LEN]));
                assert_eq!(actual.get("b@example.com"), Some(&[2; TOKEN_LEN]));
            } else {
                assert!(actual.is_none());
            }
        }
        let view = event_view_in_domain(&stored, &domain).unwrap();
        assert_eq!(view.version, stored.version);
        assert_eq!(view.deserialize::<CalendarEvent>().unwrap(), original);
        assert_eq!(stored.deserialize::<CalendarEvent>().unwrap(), input);
    }

    #[test]
    fn unconverted_owner_reads_linked_legacy_archive() {
        assert_journal_read(false, false);
    }
    #[test]
    fn unconverted_owner_reads_linked_ewk_archive() {
        assert_journal_read(false, true);
    }
    #[test]
    fn reverting_owner_reads_linked_legacy_archive() {
        assert_journal_read(true, false);
    }
    #[test]
    fn reverting_owner_reads_linked_ewk_archive() {
        assert_journal_read(true, true);
    }

    fn assert_domain_reads_refused(input: &CalendarEvent, domain: &ZaKeyDomain) {
        let stored = archive(input);
        assert!(
            unseal_event_in_domain(&mut stored.deserialize::<CalendarEvent>().unwrap(), domain)
                .is_err()
        );
        assert!(event_pairs_in_domain(&stored, domain).is_err());
        assert!(event_view_in_domain(&stored, domain).is_err());
    }

    #[test]
    fn journal_reads_require_requested_parent_and_grantees_require_ck() {
        let keys = keys();
        let grantee = ck_domain(None, 1);
        for reverting in [false, true] {
            let owner = journal_domain(&keys, reverting);
            for ewk_wrapped in [false, true] {
                let mut input = linked_event();
                if ewk_wrapped {
                    seal_event(&mut input, &keys, 9, Some(&two_pairs())).unwrap();
                }
                assert_domain_reads_refused(&input, &grantee);
                let mut sole = input.clone();
                sole.names.pop();
                assert_domain_reads_refused(&sole, &grantee);
                input.names[0].parent_id = 43;
                assert_domain_reads_refused(&input, &owner);
            }
        }
    }

    #[test]
    fn linked_ck_reads_and_all_linked_shared_writes_remain_refused() {
        let keys = keys();
        let account = ZaKeyDomain::owner(&keys);
        let owner = journal_domain(&keys, false);
        let grantee = ck_domain(None, 1);
        let mut ck = parented_event();
        seal_event_in_domain(&mut ck, &owner, None).unwrap();
        ck.names.push(common::DavName {
            parent_id: 51,
            name: "copy.ics".into(),
        });
        let mut ewk = linked_event();
        seal_event(&mut ewk, &keys, 9, Some(&two_pairs())).unwrap();
        for reverting in [false, true] {
            let domain = journal_domain(&keys, reverting);
            assert_domain_reads_refused(&ck, &domain);
            assert_domain_reads_refused(&ck, &grantee);
            let mut plain = linked_event();
            let before = plain.clone();
            assert!(seal_event_in_domain(&mut plain, &domain, None).is_err());
            assert_eq!(plain, before);
            for input in [linked_event(), ewk.clone(), ck.clone()] {
                let stored = archive(&input);
                for (from, to) in [(&domain, &account), (&domain, &domain), (&account, &domain)] {
                    let mut changed = stored.deserialize::<CalendarEvent>().unwrap();
                    assert!(rewrap_event(&mut changed, from, to).is_err());
                    assert_eq!(changed, input);
                }
            }
        }
    }

    #[test]
    fn classifier_rejects_plaintext_mixed_with_a_valid_key_carrier() {
        let mut sealed = parented_event();
        seal_event(&mut sealed, &keys(), 9, None).unwrap();
        let mut mixed = parented_event();
        mixed.data.event.components[0].entries.push(
            sealed.data.event.components[0]
                .entries
                .last()
                .unwrap()
                .clone(),
        );
        assert!(classify_event_storage(&mixed).is_err());
    }

    #[test]
    fn rewrap_into_shared_refuses_linked_sources() {
        let keys = keys();
        let mut sealed = parented_event();
        sealed.names.push(common::DavName {
            parent_id: 51,
            name: "copy.ics".into(),
        });
        seal_event(&mut sealed, &keys, 9, None).unwrap();
        assert!(
            rewrap_event(
                &mut sealed,
                &super::super::domain::ZaKeyDomain::owner(&keys),
                &ck_domain(None, 1)
            )
            .is_err()
        );
    }

    #[test]
    fn client_x_za_properties_round_trip_in_bundles() {
        let mut e = parented_event();
        for component in [0, 1] {
            for carrier in [KEY_PROP, EXTRA_PROP, SEALED_PROP] {
                e.data.event.components[component]
                    .entries
                    .push(text_entry(carrier, "client value".into()));
            }
        }
        let original = e.clone();
        let ck = ck_domain(None, 1);
        seal_event_in_domain(&mut e, &ck, None).unwrap();
        let mut e = archive(&e).deserialize::<CalendarEvent>().unwrap();
        assert_eq!(
            classify_event_storage(&e).unwrap(),
            ZaEventStorageKind::Sealed
        );
        unseal_event_in_domain(&mut e, &ck).unwrap();
        assert_eq!(e, original);
    }

    #[test]
    fn seal_then_unseal_restores_everything_and_keeps_precomputed_data() {
        let original = event();
        let keys = keys();
        let mut sealed = original.clone();
        seal_event(&mut sealed, &keys, 9, None).unwrap();
        assert!(sealed.display_name.is_none());
        assert!(sealed.dead_properties.0.is_empty());
        assert_eq!(sealed.data.time_ranges, original.data.time_ranges);
        assert_eq!(
            sealed.data.alarms, original.data.alarms,
            "email-alarm flag precomputed before sealing"
        );
        assert_eq!(sealed.size, original.size);
        let root = &sealed.data.event.components[0];
        let n = root.entries.len();
        assert!(is_carrier(root.entries.last().unwrap(), KEY_PROP));
        assert!(is_carrier(&root.entries[n - 2], EXTRA_PROP));
        assert!(is_carrier(&root.entries[n - 3], SEALED_PROP));
        let dump = sealed.data.event.to_string();
        for canary in ["canary", "bob@example.com", "me@example.com"] {
            assert!(!dump.contains(canary), "{dump}");
        }
        let mut back = sealed.clone();
        unseal_event(&mut back, &keys, 9).unwrap();
        assert_eq!(back, original);
    }

    fn root_entries(event: &mut CalendarEvent) -> &mut Vec<ICalendarEntry> {
        &mut event.data.event.components[0].entries
    }

    fn dek_of(event: &CalendarEvent, keys: &SessionKeys) -> Secret {
        let envelope = STANDARD
            .decode(entry_text(event.data.event.components[0].entries.last().unwrap()).unwrap())
            .unwrap();
        unwrap_key(&envelope[2..], keys.ewk(), &key_aad(9, &uid_of(event))).unwrap()
    }

    fn with_envelope(sealed: &CalendarEvent, f: impl FnOnce(&mut Vec<u8>)) -> CalendarEvent {
        let mut out = sealed.clone();
        let last = root_entries(&mut out).last_mut().unwrap();
        let mut envelope = STANDARD.decode(entry_text(last).unwrap()).unwrap();
        f(&mut envelope);
        last.values = vec![ICalendarValue::Text(STANDARD.encode(envelope))];
        out
    }

    #[test]
    fn every_write_gets_a_new_dek() {
        let keys = keys();
        let mut a = event();
        let mut b = event();
        seal_event(&mut a, &keys, 9, None).unwrap();
        seal_event(&mut b, &keys, 9, None).unwrap();
        assert_ne!(dek_of(&a, &keys).as_bytes(), dek_of(&b, &keys).as_bytes());
    }

    #[test]
    fn sealing_with_another_accounts_keys_is_refused() {
        let mut e = event();
        assert_eq!(
            seal_event(&mut e, &SessionKeys::new(10, 1, Secret::random()), 9, None),
            Err(SealError::Structure("keys of another account"))
        );
    }

    #[test]
    fn envelope_and_extra_errors() {
        let keys = keys();
        let mut sealed = event();
        seal_event(&mut sealed, &keys, 9, None).unwrap();
        let cases = [
            (with_envelope(&sealed, |e| e[0] = 2), SealError::Policy(2)),
            (with_envelope(&sealed, |e| e[1] = 2), SealError::Format),
            (
                with_envelope(&sealed, |e| {
                    e.pop();
                }),
                SealError::Aead,
            ),
            (
                {
                    let mut t = sealed.clone();
                    root_entries(&mut t).last_mut().unwrap().values =
                        vec![ICalendarValue::Text("!!not base64!!".into())];
                    t
                },
                SealError::Decode,
            ),
            (
                {
                    // EXTRA from another seal of the same event.
                    let mut other = event();
                    seal_event(&mut other, &keys, 9, None).unwrap();
                    let mut t = sealed.clone();
                    let n = root_entries(&mut t).len();
                    root_entries(&mut t)[n - 2] = root_entries(&mut other)[n - 2].clone();
                    assert!(is_carrier(&root_entries(&mut t)[n - 2], EXTRA_PROP));
                    t
                },
                SealError::Aead,
            ),
        ];
        for (mut input, expected) in cases {
            assert_eq!(unseal_event(&mut input, &keys, 9), Err(expected));
        }
    }

    #[test]
    fn misplaced_carriers_are_refused() {
        let keys = keys();
        let mut sealed = event();
        seal_event(&mut sealed, &keys, 9, None).unwrap();

        // Root order [.., SEALED, EXTRA, KEY] swapped to [.., EXTRA, SEALED, KEY].
        let mut swapped = sealed.clone();
        let n = root_entries(&mut swapped).len();
        root_entries(&mut swapped).swap(n - 3, n - 2);
        assert!(is_carrier(&root_entries(&mut swapped)[n - 2], SEALED_PROP));
        let stored = archive(&swapped);
        assert_eq!(
            unseal_event(&mut swapped, &keys, 9),
            Err(SealError::Structure("misplaced carrier"))
        );

        // EXTRA moved into the VEVENT, ahead of that component's SEALED.
        let mut moved = sealed.clone();
        let n = root_entries(&mut moved).len();
        let extra = root_entries(&mut moved).remove(n - 2);
        let vevent = &mut moved.data.event.components[1].entries;
        assert!(is_carrier(vevent.last().unwrap(), SEALED_PROP));
        let at = vevent.len() - 1;
        vevent.insert(at, extra);
        assert_eq!(
            unseal_event(&mut moved, &keys, 9),
            Err(SealError::Structure("misplaced carrier"))
        );

        // The archive path refuses it too (`swapped` itself was consumed by
        // the failed in-place unseal above, hence the copy taken before).
        assert_eq!(
            unseal_event_archive(&stored, &keys, 9).err(),
            Some(SealError::Structure("misplaced carrier"))
        );
    }

    #[test]
    fn wrong_account_and_wrong_keys_fail() {
        let keys = keys();
        let mut sealed = event();
        seal_event(&mut sealed, &keys, 9, None).unwrap();
        let mut t = sealed.clone();
        assert_eq!(unseal_event(&mut t, &keys, 10), Err(SealError::Aead));
        let mut t = sealed.clone();
        assert_eq!(
            unseal_event(&mut t, &SessionKeys::new(9, 1, Secret::random()), 9),
            Err(SealError::Aead)
        );
        let stored = archive(&sealed);
        assert!(unseal_event_archive(&stored, &keys, 10).is_err());
    }

    #[test]
    fn plaintext_event_passes_through() {
        let keys = keys();
        let plain = event();
        let mut t = plain.clone();
        assert_eq!(unseal_event(&mut t, &keys, 9), Ok(()));
        assert_eq!(t, plain);
        let stored = archive(&plain);
        let view = unseal_event_archive(&stored, &keys, 9).unwrap();
        assert_eq!(view.version, stored.version);
        assert_eq!(view.as_bytes(), stored.as_bytes());
    }

    #[test]
    fn client_key_carrier_is_an_ordinary_x_property() {
        // A body whose root ends with X-ZA-KEY (a client's, or one we sealed
        // before) is sealed like any X- property and comes back unchanged.
        let keys = keys();
        let once = {
            let mut e = event();
            seal_event(&mut e, &keys, 9, None).unwrap();
            e
        };
        let mut twice = once.clone();
        seal_event(&mut twice, &keys, 9, None).unwrap();
        let root = &twice.data.event.components[0];
        assert_eq!(
            root.entries
                .iter()
                .filter(|e| is_carrier(e, KEY_PROP))
                .count(),
            1,
            "the inner X-ZA-KEY is inside the root bundle"
        );
        let mut back = twice.clone();
        unseal_event(&mut back, &keys, 9).unwrap();
        // The extra fields of `once` were already empty, so no bundle of
        // them is written the second time and they stay empty.
        assert_eq!(back, once);
        unseal_event(&mut back, &keys, 9).unwrap();
        assert_eq!(back, event());
    }

    #[test]
    fn freebusy_only_object_keeps_its_uid_and_round_trips() {
        let keys = keys();
        let original = event_from(FREEBUSY);
        let mut sealed = original.clone();
        seal_event(&mut sealed, &keys, 9, None).unwrap();
        assert_eq!(uid_of(&sealed), "fb-1");
        assert_eq!(uid_of(&sealed), uid_of(&original));
        assert!(!sealed.data.event.to_string().contains("fb-canary"));
        let mut back = sealed.clone();
        unseal_event(&mut back, &keys, 9).unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn archive_view_keeps_stored_version() {
        let keys = keys();
        let mut sealed = event();
        seal_event(&mut sealed, &keys, 9, None).unwrap();
        let stored = archive(&sealed);
        let view = unseal_event_archive(&stored, &keys, 9).unwrap();
        assert_eq!(
            view.version, stored.version,
            "response identity comes from the stored archive"
        );
        let unarchived = view.unarchive::<CalendarEvent>().unwrap();
        assert_eq!(unarchived.display_name.as_deref(), Some("display canary"));
        assert!(
            view.deserialize::<CalendarEvent>()
                .unwrap()
                .data
                .event
                .to_string()
                .contains("secret summary canary")
        );
        assert!(!String::from_utf8_lossy(stored.as_bytes()).contains("canary"));
    }

    #[test]
    fn archived_seal_check_matches_is_sealed() {
        let plain = event();
        let stored = archive(&plain);
        assert!(!archived_event_is_sealed(
            stored.unarchive::<CalendarEvent>().unwrap()
        ));

        let mut sealed = event();
        seal_event(&mut sealed, &keys(), 9, None).unwrap();
        assert!(is_sealed(&sealed));
        let stored = archive(&sealed);
        assert!(archived_event_is_sealed(
            stored.unarchive::<CalendarEvent>().unwrap()
        ));
    }

    #[test]
    fn archived_seal_check_needs_key_carrier_last() {
        // Any case of the name and any text value count as a carrier.
        let mut last = event();
        root_entries(&mut last).push(text_entry("x-za-key", "anything".into()));
        assert!(is_sealed(&last));
        let stored = archive(&last);
        assert!(archived_event_is_sealed(
            stored.unarchive::<CalendarEvent>().unwrap()
        ));

        // A carrier followed by any other entry is not last, so not sealed.
        let mut not_last = event();
        root_entries(&mut not_last).push(text_entry(KEY_PROP, "anything".into()));
        root_entries(&mut not_last).push(text_entry("X-OTHER", "value".into()));
        assert!(!is_sealed(&not_last));
        let stored = archive(&not_last);
        assert!(!archived_event_is_sealed(
            stored.unarchive::<CalendarEvent>().unwrap()
        ));
    }

    fn two_pairs() -> TokenPairs {
        let mut pairs = TokenPairs::new();
        pairs.insert("a@example.com", [1; TOKEN_LEN]);
        pairs.insert("b@example.com", [2; TOKEN_LEN]);
        pairs
    }

    /// The format byte of the root's `X-ZA-EXTRA` bundle.
    fn extra_format(event: &CalendarEvent) -> u8 {
        let root = &event.data.event.components[0];
        let extra = &root.entries[root.entries.len() - 2];
        assert!(is_carrier(extra, EXTRA_PROP));
        STANDARD.decode(entry_text(extra).unwrap()).unwrap()[0]
    }

    fn bare_event() -> CalendarEvent {
        let mut bare = event();
        bare.display_name = None;
        bare.dead_properties = DeadProperty::default();
        bare
    }

    #[test]
    fn pairs_round_trip_through_a_format_2_bundle() {
        let keys = keys();
        let original = event();
        let mut sealed = original.clone();
        seal_event(&mut sealed, &keys, 9, Some(&two_pairs())).unwrap();
        assert_eq!(extra_format(&sealed), FORMAT_V2);
        let pairs = event_token_pairs(&archive(&sealed), &keys, 9)
            .unwrap()
            .expect("pairs");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs.get("a@example.com"), Some(&[1; TOKEN_LEN]));
        assert_eq!(pairs.get("b@example.com"), Some(&[2; TOKEN_LEN]));
        // The view restores the extra fields and the tree; pairs are not in it.
        let mut back = sealed.clone();
        unseal_event(&mut back, &keys, 9).unwrap();
        assert_eq!(back, original);
        // Pairs alone, with no display name or dead property, get a bundle.
        let mut bare = bare_event();
        seal_event(&mut bare, &keys, 9, Some(&two_pairs())).unwrap();
        assert_eq!(extra_format(&bare), FORMAT_V2);
        assert_eq!(
            event_token_pairs(&archive(&bare), &keys, 9)
                .unwrap()
                .map(|p| p.len()),
            Some(2)
        );
    }

    #[test]
    fn format_1_bundles_read_as_before_with_no_pairs() {
        let keys = keys();
        for pairs in [None, Some(TokenPairs::new())] {
            let original = event();
            let mut sealed = original.clone();
            seal_event(&mut sealed, &keys, 9, pairs.as_ref()).unwrap();
            assert_eq!(extra_format(&sealed), FORMAT_V1);
            assert!(
                event_token_pairs(&archive(&sealed), &keys, 9)
                    .unwrap()
                    .is_none()
            );
            let mut back = sealed.clone();
            unseal_event(&mut back, &keys, 9).unwrap();
            assert_eq!(back, original);
        }
        // Neither extra fields nor pairs: no bundle at all, as in release 1.
        let mut bare = bare_event();
        seal_event(&mut bare, &keys, 9, None).unwrap();
        assert!(
            !bare.data.event.components[0]
                .entries
                .iter()
                .any(|e| is_carrier(e, EXTRA_PROP))
        );
        assert!(
            event_token_pairs(&archive(&bare), &keys, 9)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_flipped_format_byte_fails_authentication() {
        let keys = keys();
        for (pairs, from, to) in [
            (Some(two_pairs()), FORMAT_V2, FORMAT_V1),
            (None, FORMAT_V1, FORMAT_V2),
        ] {
            let mut sealed = event();
            seal_event(&mut sealed, &keys, 9, pairs.as_ref()).unwrap();
            let n = root_entries(&mut sealed).len();
            let extra = &mut root_entries(&mut sealed)[n - 2];
            let mut raw = STANDARD.decode(entry_text(extra).unwrap()).unwrap();
            assert_eq!(raw[0], from);
            raw[0] = to;
            extra.values = vec![ICalendarValue::Text(STANDARD.encode(raw))];
            assert_eq!(
                event_token_pairs(&archive(&sealed), &keys, 9).err(),
                Some(SealError::Aead)
            );
            assert_eq!(unseal_event(&mut sealed, &keys, 9), Err(SealError::Aead));
        }
    }

    #[test]
    fn token_pairs_of_plaintext_moved_and_foreign_bundles() {
        let keys = keys();
        assert!(
            event_token_pairs(&archive(&event()), &keys, 9)
                .unwrap()
                .is_none(),
            "a plaintext event has no pairs"
        );
        let mut sealed = event();
        seal_event(&mut sealed, &keys, 9, Some(&two_pairs())).unwrap();
        let mut swapped = sealed.clone();
        let n = root_entries(&mut swapped).len();
        root_entries(&mut swapped).swap(n - 3, n - 2);
        assert_eq!(
            event_token_pairs(&archive(&swapped), &keys, 9).err(),
            Some(SealError::Structure("misplaced carrier"))
        );
        assert_eq!(
            event_token_pairs(
                &archive(&sealed),
                &SessionKeys::new(9, 1, Secret::random()),
                9
            )
            .err(),
            Some(SealError::Aead)
        );
    }

    #[test]
    fn unseal_with_pairs_returns_the_bundle_pairs_once() {
        let keys = keys();
        let original = event();
        let mut sealed = original.clone();
        seal_event(&mut sealed, &keys, 9, Some(&two_pairs())).unwrap();
        let mut opened = sealed.clone();
        let pairs = unseal_event_pairs(&mut opened, &keys, 9)
            .unwrap()
            .expect("pairs");
        assert_eq!(opened, original);
        assert_eq!(pairs.len(), 2);
        assert!(pairs.matches("a@example.com", &[1; TOKEN_LEN]));
        // A format 1 bundle and a plaintext event: no pairs; the plaintext
        // event is left as it is.
        let mut v1 = original.clone();
        seal_event(&mut v1, &keys, 9, None).unwrap();
        assert!(unseal_event_pairs(&mut v1, &keys, 9).unwrap().is_none());
        assert_eq!(v1, original);
        let mut plain = original.clone();
        assert!(unseal_event_pairs(&mut plain, &keys, 9).unwrap().is_none());
        assert_eq!(plain, original);
        // Keys of another session do not open it.
        assert_eq!(
            unseal_event_pairs(&mut sealed, &SessionKeys::new(9, 1, Secret::random()), 9).err(),
            Some(SealError::Aead)
        );
    }
}
