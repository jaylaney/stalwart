/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Sealing of one calendar collection (spec 7.2).
//!
//! The collection's user-facing name, description, colour and dead
//! properties go into one encrypted bundle carried in the owner's
//! `CalendarPreferences.name` as `"$za$" + base64(envelope) + "|" + sealed
//! bundle`, where the envelope is `policy version || wrap type || wrapped
//! DEK`. A custom timezone is sealed in place by `seal_tree` under scope
//! `calendar-tz` with the same DEK. The associated data binds the account
//! only, so copying or moving the collection needs no resealing. The record's
//! own `name` (the URL slug), sort order, flags and default alerts stay
//! visible.

use super::tree::{
    SealError, has_stray_carriers, open_archive, open_key_envelope, seal_bytes, seal_key_envelope,
    seal_tree, unseal_tree,
};
use crate::calendar::{Calendar, Timezone};
use base64::{Engine, engine::general_purpose::STANDARD};
use store::{
    Deserialize, Serialize,
    write::{AlignedBytes, Archive, Archiver},
};
use types::dead_property::DeadProperty;
use vault::{
    keys::{Secret, aad},
    session::SessionKeys,
};

/// Prefix of the owner's `CalendarPreferences.name` when sealed (spec 7.2).
pub const COLLECTION_MARKER: &str = vault::ZA_MARKER;
const TZ_SCOPE: &str = "calendar-tz";

/// The stored fields that carry content.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Debug, Clone, PartialEq)]
struct Bundle {
    name: String,
    description: Option<String>,
    color: Option<String>,
    dead_properties: DeadProperty,
}

/// The preferences entry `Calendar::preferences(account_id)` resolves to,
/// without the panic on an empty list and without `preferences_mut`'s
/// insertion of a missing entry. Used for reads only: `seal_calendar`
/// requires the owner's entry to be the only one.
fn owner_index(calendar: &Calendar, account_id: u32) -> Option<usize> {
    match calendar.preferences.len() {
        0 => None,
        1 => Some(0),
        _ => Some(
            calendar
                .preferences
                .iter()
                .position(|p| p.account_id == account_id)
                .unwrap_or(0),
        ),
    }
}

/// True if the owner's preferences name carries the collection marker.
pub fn calendar_is_sealed(calendar: &Calendar, account_id: u32) -> bool {
    owner_index(calendar, account_id)
        .is_some_and(|i| calendar.preferences[i].name.starts_with(COLLECTION_MARKER))
}

/// Seals name, description, colour, dead properties and a custom timezone
/// into the owner's preferences entry, under a fresh DEK. The bundle is
/// always written for a key account's collection, so the wrapped key is
/// present whenever any ciphertext in the collection depends on it
/// (spec 7.2).
///
/// Spec 7.2: a key account's collection holds only the owner's preferences
/// entry. Anything else is refused, so no other entry can reach the store in
/// plaintext (upstream write paths append a copy of entry 0 for an account
/// without an entry).
///
/// There is no "already sealed" check: a display name that starts with the
/// marker is client data and is sealed and restored unchanged, like a
/// client's `X-ZA-KEY` in an event. Each write path seals exactly once.
///
/// On `Err` the collection is unchanged.
pub fn seal_calendar(
    calendar: &mut Calendar,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<(), SealError> {
    if keys.account_id != account_id {
        return Err(SealError::Structure("keys of another account"));
    }
    if !matches!(calendar.preferences.as_slice(), [owner] if owner.account_id == account_id) {
        return Err(SealError::Structure("preferences other than the owner's"));
    }
    let dek = Secret::random();
    let dead_properties = std::mem::take(&mut calendar.dead_properties);
    let pref = &mut calendar.preferences[0];
    let bundle = Bundle {
        name: std::mem::take(&mut pref.name),
        description: pref.description.take(),
        color: pref.color.take(),
        dead_properties,
    };
    if let Timezone::Custom(tz) = &mut pref.time_zone {
        seal_tree(tz, &dek, account_id, TZ_SCOPE);
    }
    let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&bundle)
        .expect("rkyv serialization of in-memory bundle cannot fail");
    let envelope = seal_key_envelope(&dek, keys.ewk(), &aad("calendar-key", account_id));
    pref.name = format!(
        "{COLLECTION_MARKER}{}|{}",
        STANDARD.encode(envelope),
        seal_bytes(&dek, &aad("calendar-bundle", account_id), &plain)
    );
    Ok(())
}

/// Restores the owner's entry and the collection's dead properties in place.
///
/// A collection without the marker (for example the default calendar the
/// server created without a key) is plaintext and is left unchanged; the
/// first owner write seals it.
///
/// On `Err` the collection is partly modified and must be discarded.
pub fn unseal_calendar(
    calendar: &mut Calendar,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<(), SealError> {
    let Some(index) = owner_index(calendar, account_id) else {
        return Ok(());
    };
    let pref = &mut calendar.preferences[index];
    let Some(rest) = pref.name.strip_prefix(COLLECTION_MARKER) else {
        return Ok(());
    };
    let (envelope, sealed) = rest.split_once('|').ok_or(SealError::Format)?;
    let envelope = STANDARD.decode(envelope).map_err(|_| SealError::Decode)?;
    let dek = open_key_envelope(&envelope, keys.ewk(), &aad("calendar-key", account_id))?;
    let bundle = open_archive::<Bundle>(&dek, &aad("calendar-bundle", account_id), sealed)?;
    if let Timezone::Custom(tz) = &mut pref.time_zone {
        if has_stray_carriers(tz) {
            return Err(SealError::Structure("misplaced carrier"));
        }
        unseal_tree(tz, &dek, account_id, TZ_SCOPE)?;
    }
    pref.name = bundle.name;
    pref.description = bundle.description;
    pref.color = bundle.color;
    calendar.dead_properties = bundle.dead_properties;
    Ok(())
}

/// Spec 7.3: a read view of the collection. `version` is copied from the
/// stored archive so ETags and conditional headers stay bound to it.
///
/// Works on a deserialised copy that is dropped on any error, so the stored
/// archive is never affected. A plaintext collection yields a copy of
/// `stored`.
///
/// The view is read-only: its `version` (including the integrity hash) is
/// the stored one, but its bytes are plaintext. It must never be written
/// back, passed to `into_inner()` for a write, or used as an update's
/// "current" value (`AssertValue`); writes start from the stored archive.
pub fn unseal_calendar_archive(
    stored: &Archive<AlignedBytes>,
    keys: &SessionKeys,
    account_id: u32,
) -> Result<Archive<AlignedBytes>, SealError> {
    let mut calendar = stored
        .deserialize::<Calendar>()
        .map_err(|_| SealError::Format)?;
    if !calendar_is_sealed(&calendar, account_id) {
        return Ok(stored.clone());
    }
    unseal_calendar(&mut calendar, keys, account_id)?;
    let bytes = Archiver::new(calendar)
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
    use crate::calendar::{Calendar, CalendarPreferences, Timezone};
    use calcard::{Entry, Parser};
    use store::{Serialize, write::Archiver};
    use types::dead_property::{DeadElementTag, DeadPropertyTag};
    use vault::keys::{Secret, unwrap_key};

    const TZ: &str = "BEGIN:VCALENDAR\r\nPRODID:-//Example Corp.//CalDAV Client//EN\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:US-Eastern\r\nLAST-MODIFIED:19870101T000000Z\r\nX-LIC-LOCATION:America/New_York\r\nBEGIN:STANDARD\r\nDTSTART:19671029T020000\r\nRRULE:FREQ=YEARLY;BYDAY=-1SU;BYMONTH=10\r\nTZOFFSETFROM:-0400\r\nTZOFFSETTO:-0500\r\nTZNAME:Eastern Standard canary\r\nCOMMENT:tz comment canary\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nEND:VCALENDAR\r\n";

    fn tz() -> calcard::icalendar::ICalendar {
        match Parser::new(TZ).entry() {
            Entry::ICalendar(ical) => ical,
            _ => panic!(),
        }
    }

    fn calendar() -> Calendar {
        Calendar {
            name: "work-slug".into(),
            preferences: vec![CalendarPreferences {
                account_id: 4,
                name: "Work canary".into(),
                description: Some("desc canary".into()),
                color: Some("#ff0000".into()),
                sort_order: 3,
                flags: 1,
                time_zone: Timezone::Custom(tz()),
                ..Default::default()
            }],
            dead_properties: DeadProperty(vec![
                DeadPropertyTag::ElementStart(DeadElementTag::new("A:calendar-color".into(), None)),
                DeadPropertyTag::Text("#00ff00 canary".into()),
                DeadPropertyTag::ElementEnd,
            ]),
            ..Default::default()
        }
    }

    fn keys() -> SessionKeys {
        SessionKeys::new(4, 1, Secret::random())
    }

    fn archive(calendar: &Calendar) -> Archive<AlignedBytes> {
        let bytes = Archiver::new(calendar.clone()).serialize().unwrap();
        <Archive<AlignedBytes> as store::Deserialize>::deserialize(&bytes).unwrap()
    }

    fn custom_tz(calendar: &mut Calendar) -> &mut calcard::icalendar::ICalendar {
        match &mut calendar.preferences[0].time_zone {
            Timezone::Custom(tz) => tz,
            _ => panic!(),
        }
    }

    /// Rewrites the envelope part of a sealed name.
    fn with_envelope(sealed: &Calendar, f: impl FnOnce(&mut Vec<u8>)) -> Calendar {
        let mut out = sealed.clone();
        let name = &mut out.preferences[0].name;
        let rest = name.strip_prefix(COLLECTION_MARKER).unwrap();
        let (envelope, bundle) = rest.split_once('|').unwrap();
        let mut envelope = STANDARD.decode(envelope).unwrap();
        f(&mut envelope);
        *name = format!("{COLLECTION_MARKER}{}|{bundle}", STANDARD.encode(envelope));
        out
    }

    fn dek_of(sealed: &Calendar, keys: &SessionKeys) -> Secret {
        let rest = sealed.preferences[0]
            .name
            .strip_prefix(COLLECTION_MARKER)
            .unwrap();
        let envelope = STANDARD.decode(rest.split_once('|').unwrap().0).unwrap();
        unwrap_key(&envelope[2..], keys.ewk(), &aad("calendar-key", 4)).unwrap()
    }

    #[test]
    fn seal_unseal_round_trip_with_custom_timezone() {
        let original = calendar();
        let keys = keys();
        let mut sealed = original.clone();
        seal_calendar(&mut sealed, &keys, 4).unwrap();
        assert!(calendar_is_sealed(&sealed, 4));
        assert_eq!(sealed.name, "work-slug", "slug stays visible");
        let pref = sealed.preferences(4);
        assert!(pref.name.starts_with(COLLECTION_MARKER));
        assert!(pref.description.is_none());
        assert!(pref.color.is_none());
        assert_eq!(pref.sort_order, 3);
        assert_eq!(pref.flags, 1);
        assert!(sealed.dead_properties.0.is_empty());
        let Timezone::Custom(tz) = &pref.time_zone else {
            panic!()
        };
        assert_eq!(tz.components.len(), original_tz_len(&original));
        let dump = tz.to_string();
        assert!(
            dump.contains("TZID:US-Eastern") && dump.contains("TZOFFSETFROM:-0400"),
            "{dump}"
        );
        // Policy v1: the timezone name aliases stay visible (calcard resolves
        // by name only); every other X- and content property is sealed.
        assert!(dump.contains("X-LIC-LOCATION:America/New_York"), "{dump}");
        assert!(
            !dump.contains("canary") && !dump.contains("COMMENT") && !dump.contains("TZNAME"),
            "{dump}"
        );
        assert!(!pref.name.contains("canary"));
        assert_eq!(
            pref.time_zone.tz(),
            original.preferences(4).time_zone.tz(),
            "timezone resolution works without a key"
        );
        assert!(pref.time_zone.tz().is_some());
        let mut back = sealed.clone();
        unseal_calendar(&mut back, &keys, 4).unwrap();
        assert_eq!(back, original);
    }

    fn original_tz_len(calendar: &Calendar) -> usize {
        match &calendar.preferences[0].time_zone {
            Timezone::Custom(tz) => tz.components.len(),
            _ => panic!(),
        }
    }

    #[test]
    fn plaintext_collection_passes_through() {
        let mut plain = calendar();
        plain.preferences[0].time_zone = Timezone::Default;
        let before = plain.clone();
        unseal_calendar(&mut plain, &keys(), 4).unwrap();
        assert_eq!(plain, before);
        assert!(!calendar_is_sealed(&plain, 4));
        // The archive view of a plaintext collection is the stored archive.
        let stored = archive(&plain);
        let view = unseal_calendar_archive(&stored, &keys(), 4).unwrap();
        assert_eq!(view.version, stored.version);
        assert_eq!(view.as_bytes(), stored.as_bytes());
        // So is a collection with a plaintext custom timezone.
        let mut plain_tz = calendar();
        let before = plain_tz.clone();
        unseal_calendar(&mut plain_tz, &keys(), 4).unwrap();
        assert_eq!(plain_tz, before);
    }

    #[test]
    fn timezone_only_collection_still_carries_the_bundle() {
        let keys = keys();
        let mut c = calendar();
        c.preferences[0].description = None;
        c.preferences[0].color = None;
        c.dead_properties = DeadProperty(Vec::new());
        let original = c.clone();
        seal_calendar(&mut c, &keys, 4).unwrap();
        assert!(c.preferences(4).name.starts_with(COLLECTION_MARKER));
        unseal_calendar(&mut c, &keys, 4).unwrap();
        assert_eq!(c, original);
    }

    #[test]
    fn every_write_gets_a_new_dek() {
        let keys = keys();
        let mut a = calendar();
        let mut b = calendar();
        seal_calendar(&mut a, &keys, 4).unwrap();
        seal_calendar(&mut b, &keys, 4).unwrap();
        assert_ne!(dek_of(&a, &keys).as_bytes(), dek_of(&b, &keys).as_bytes());
    }

    #[test]
    fn sealing_with_another_accounts_keys_is_refused() {
        let mut c = calendar();
        let before = c.clone();
        assert_eq!(
            seal_calendar(&mut c, &SessionKeys::new(5, 1, Secret::random()), 4),
            Err(SealError::Structure("keys of another account"))
        );
        assert_eq!(c, before);
    }

    #[test]
    fn only_the_owners_single_entry_is_sealed() {
        let keys = keys();
        // A second entry, as upstream's `preferences_mut` appends for an
        // account without one.
        let mut two = calendar();
        let mut other = two.preferences[0].clone();
        other.account_id = 99;
        two.preferences.push(other);
        // The single entry belongs to another account.
        let mut wrong_owner = calendar();
        wrong_owner.preferences[0].account_id = 99;
        let mut none = calendar();
        none.preferences.clear();
        for mut c in [two, wrong_owner, none] {
            let before = c.clone();
            assert_eq!(
                seal_calendar(&mut c, &keys, 4),
                Err(SealError::Structure("preferences other than the owner's"))
            );
            assert_eq!(c, before);
        }
    }

    #[test]
    fn marker_in_a_display_name_is_client_data() {
        let keys = keys();
        let mut c = calendar();
        c.preferences[0].name = format!("{COLLECTION_MARKER}abc|x");
        let original = c.clone();
        seal_calendar(&mut c, &keys, 4).unwrap();
        let stored_name = &c.preferences[0].name;
        assert_ne!(stored_name, &original.preferences[0].name);
        assert!(stored_name.starts_with(COLLECTION_MARKER));
        assert!(!stored_name.contains("abc|x"));
        unseal_calendar(&mut c, &keys, 4).unwrap();
        assert_eq!(c, original);
    }

    #[test]
    fn iana_timezone_and_empty_name_round_trip() {
        let keys = keys();
        let mut iana = calendar();
        iana.preferences[0].time_zone = Timezone::IANA(42);
        let mut empty_name = calendar();
        empty_name.preferences[0].name = String::new();
        empty_name.preferences[0].time_zone = Timezone::Default;
        for original in [iana, empty_name] {
            let mut c = original.clone();
            seal_calendar(&mut c, &keys, 4).unwrap();
            assert!(calendar_is_sealed(&c, 4));
            assert!(c.preferences[0].description.is_none());
            assert_eq!(
                c.preferences[0].time_zone,
                original.preferences[0].time_zone
            );
            unseal_calendar(&mut c, &keys, 4).unwrap();
            assert_eq!(c, original);
        }
    }

    #[test]
    fn errors() {
        let keys = keys();
        let mut sealed = calendar();
        seal_calendar(&mut sealed, &keys, 4).unwrap();
        assert_eq!(
            unseal_calendar(&mut sealed.clone(), &keys, 5),
            Err(SealError::Aead)
        );
        assert_eq!(
            unseal_calendar(
                &mut sealed.clone(),
                &SessionKeys::new(4, 1, Secret::random()),
                4
            ),
            Err(SealError::Aead)
        );
        // The archive view of the sealed collection: the AAD binds the
        // account only, so a fresh archive of the same record opens.
        let stored = archive(&sealed);
        let view = unseal_calendar_archive(&stored, &keys, 4).unwrap();
        assert_eq!(view.version, stored.version);
        assert_eq!(
            view.unarchive::<Calendar>().unwrap().preferences(4).name,
            "Work canary"
        );
        assert!(!String::from_utf8_lossy(stored.as_bytes()).contains("canary"));
        assert!(unseal_calendar_archive(&stored, &keys, 5).is_err());
    }

    #[test]
    fn envelope_and_bundle_errors() {
        let keys = keys();
        let mut sealed = calendar();
        seal_calendar(&mut sealed, &keys, 4).unwrap();
        let renamed = |name: String| {
            let mut c = sealed.clone();
            c.preferences[0].name = name;
            c
        };
        let cases = [
            (with_envelope(&sealed, |e| e[0] = 2), SealError::Policy(2)),
            (with_envelope(&sealed, |e| e[1] = 2), SealError::Format),
            (
                with_envelope(&sealed, |e| {
                    e.pop();
                }),
                SealError::Aead,
            ),
            (with_envelope(&sealed, |e| e.clear()), SealError::Format),
            (
                renamed(format!("{COLLECTION_MARKER}no-separator")),
                SealError::Format,
            ),
            (
                renamed(format!("{COLLECTION_MARKER}!!|x")),
                SealError::Decode,
            ),
            (
                {
                    // Bundle from another seal of the same collection.
                    let mut other = calendar();
                    seal_calendar(&mut other, &keys, 4).unwrap();
                    let own = sealed.preferences[0].name.split_once('|').unwrap().0;
                    let theirs = other.preferences[0].name.split_once('|').unwrap().1;
                    renamed(format!("{own}|{theirs}"))
                },
                SealError::Aead,
            ),
        ];
        for (mut input, expected) in cases {
            assert_eq!(unseal_calendar(&mut input, &keys, 4), Err(expected));
        }
    }

    #[test]
    fn misplaced_carriers_are_refused() {
        let keys = keys();
        let mut sealed = calendar();
        seal_calendar(&mut sealed, &keys, 4).unwrap();

        // STANDARD's carrier moved ahead of its last visible entry.
        let mut moved = sealed.clone();
        let standard = &mut custom_tz(&mut moved).components[2].entries;
        let n = standard.len();
        standard.swap(n - 2, n - 1);
        let stored = archive(&moved);
        assert_eq!(
            unseal_calendar(&mut moved, &keys, 4),
            Err(SealError::Structure("misplaced carrier"))
        );
        assert_eq!(
            unseal_calendar_archive(&stored, &keys, 4).err(),
            Some(SealError::Structure("misplaced carrier"))
        );

        // An event-style carrier injected into the timezone tree.
        let mut injected = sealed.clone();
        let vtimezone = &mut custom_tz(&mut injected).components[1].entries;
        let at = vtimezone.len() - 1;
        vtimezone.insert(
            at,
            super::super::tree::text_entry("X-ZA-EXTRA", "AQ==".into()),
        );
        assert_eq!(
            unseal_calendar(&mut injected, &keys, 4),
            Err(SealError::Structure("misplaced carrier"))
        );
    }
}
