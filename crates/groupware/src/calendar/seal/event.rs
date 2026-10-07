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
    policy::POLICY_VERSION,
    tree::{
        EXTRA_PROP, KEY_PROP, SealError, entry_text, is_carrier, open_archive, seal_bytes,
        seal_tree, text_entry, unseal_tree,
    },
};
use crate::calendar::CalendarEvent;
use base64::{Engine, engine::general_purpose::STANDARD};
use calcard::icalendar::ICalendarComponentType;
use store::{
    Deserialize, Serialize,
    write::{AlignedBytes, Archive, Archiver},
};
use types::dead_property::DeadProperty;
use vault::{
    keys::{Secret, aad, unwrap_key, wrap_key},
    session::SessionKeys,
};

/// Wrap type `mk` (spec 7.1). `0x02` is reserved for `pk`.
const WRAP_MK: u8 = 1;

/// The stored fields outside the iCalendar tree that carry content.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
struct Extra {
    display_name: Option<String>,
    dead_properties: DeadProperty,
}

/// `za/v1|event-key|<account>|<uid>`
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
fn is_sealed(event: &CalendarEvent) -> bool {
    event
        .data
        .event
        .components
        .first()
        .and_then(|root| root.entries.last())
        .is_some_and(|e| is_carrier(e, KEY_PROP))
}

/// Seals an event immediately before the store write (spec 8.1). Time
/// ranges and alarms were computed on the plaintext tree by the caller.
///
/// There is no "already sealed" check: a body whose root ends with
/// `X-ZA-KEY` is an ordinary `X-` property here and is sealed and restored
/// unchanged. Each write path seals exactly once.
pub fn seal_event(
    event: &mut CalendarEvent,
    keys: &SessionKeys,
    account_id: u32,
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
    let uid = uid_of(event);
    let dek = Secret::random();
    seal_tree(&mut event.data.event, &dek, account_id, &uid);

    if event.display_name.is_some() || !event.dead_properties.0.is_empty() {
        let extra = Extra {
            display_name: event.display_name.take(),
            dead_properties: std::mem::take(&mut event.dead_properties),
        };
        let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&extra)
            .expect("rkyv serialization of in-memory extra cannot fail");
        event.data.event.components[0].entries.push(text_entry(
            EXTRA_PROP,
            seal_bytes(&dek, &extra_aad(account_id, &uid), &plain),
        ));
    }

    let mut envelope = vec![POLICY_VERSION, WRAP_MK];
    envelope.extend_from_slice(&wrap_key(&dek, keys.ewk(), &key_aad(account_id, &uid)));
    event.data.event.components[0]
        .entries
        .push(text_entry(KEY_PROP, STANDARD.encode(envelope)));
    Ok(())
}

/// Unseals an event in place.
///
/// An event whose root has no trailing `X-ZA-KEY` is plaintext (written
/// before the account held keys) and is left unchanged: reads tolerate it,
/// and the next write seals it.
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
    let uid = uid_of(event);
    let root = &mut event.data.event.components[0];
    let key_entry = root
        .entries
        .pop()
        .ok_or(SealError::Structure("key envelope"))?;
    let envelope = STANDARD
        .decode(entry_text(&key_entry).ok_or(SealError::Format)?)
        .map_err(|_| SealError::Decode)?;
    let dek = match envelope.as_slice() {
        [POLICY_VERSION, WRAP_MK, wrapped @ ..] => {
            unwrap_key(wrapped, keys.ewk(), &key_aad(account_id, &uid))
                .map_err(|_| SealError::Aead)?
        }
        [version, ..] if *version != POLICY_VERSION => return Err(SealError::Policy(*version)),
        _ => return Err(SealError::Format),
    };
    if let Some(carrier) = root.entries.pop_if(|e| is_carrier(e, EXTRA_PROP)) {
        let extra = open_archive::<Extra>(
            &dek,
            &extra_aad(account_id, &uid),
            entry_text(&carrier).ok_or(SealError::Format)?,
        )?;
        event.display_name = extra.display_name;
        event.dead_properties = extra.dead_properties;
    }
    unseal_tree(&mut event.data.event, &dek, account_id, &uid)
}

/// Spec 7.3: a read view of the content. `version` is copied from the
/// stored archive so ETags and conditional headers stay bound to it.
///
/// Works on a deserialised copy that is dropped on any error, so the stored
/// archive is never affected. A plaintext event yields a copy of `stored`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{CalendarEvent, CalendarEventData, seal::tree::SEALED_PROP};
    use calcard::{Entry, Parser, common::timezone::Tz};
    use store::{Serialize, write::Archiver};
    use types::dead_property::{DeadElementTag, DeadPropertyTag};
    use vault::keys::Secret;

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

    #[test]
    fn seal_then_unseal_restores_everything_and_keeps_precomputed_data() {
        let original = event();
        let keys = keys();
        let mut sealed = original.clone();
        seal_event(&mut sealed, &keys, 9).unwrap();
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

    #[test]
    fn every_write_gets_a_new_dek() {
        let keys = keys();
        let mut a = event();
        let mut b = event();
        seal_event(&mut a, &keys, 9).unwrap();
        seal_event(&mut b, &keys, 9).unwrap();
        assert_ne!(
            a.data.event.components[0].entries.last(),
            b.data.event.components[0].entries.last()
        );
    }

    #[test]
    fn wrong_account_and_wrong_keys_fail() {
        let keys = keys();
        let mut sealed = event();
        seal_event(&mut sealed, &keys, 9).unwrap();
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
            seal_event(&mut e, &keys, 9).unwrap();
            e
        };
        let mut twice = once.clone();
        seal_event(&mut twice, &keys, 9).unwrap();
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
        seal_event(&mut sealed, &keys, 9).unwrap();
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
        seal_event(&mut sealed, &keys, 9).unwrap();
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
}
