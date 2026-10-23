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

use super::domain::ZaKeyDomain;
use super::tree::{
    SealError, has_stray_carriers, open_archive, open_key_envelope, seal_bytes, seal_key_envelope,
    seal_tree, unseal_tree,
};
use crate::calendar::{Calendar, CalendarPreferences, Timezone};
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
    seal_calendar_in_owner(calendar, &ZaKeyDomain::owner(keys))
}

fn seal_calendar_in_owner(calendar: &mut Calendar, domain: &ZaKeyDomain) -> Result<(), SealError> {
    let account_id = domain.owner_id();
    let ewk = domain.ewk()?;
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
    let envelope = seal_key_envelope(&dek, ewk, &aad("calendar-key", account_id));
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
    unseal_calendar_with_key(calendar, keys.ewk(), account_id)
}

fn unseal_calendar_with_key(
    calendar: &mut Calendar,
    ewk: &Secret,
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
    let dek = open_key_envelope(&envelope, ewk, &aad("calendar-key", account_id))?;
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

/// Prepare a personal entry for explicit conversion/grant assembly. This
/// does not add an entry or authorize a grant; the sharing writer owns that.
pub(crate) fn seal_personal_entry(
    entry: &CalendarPreferences,
    domain: &ZaKeyDomain,
    ring: &[u8],
) -> Result<CalendarPreferences, SealError> {
    let principal = entry.account_id.to_be_bytes();
    let dek = Secret::random();
    let bundle = Bundle {
        name: entry.name.clone(),
        description: entry.description.clone(),
        color: None,
        dead_properties: DeadProperty::default(),
    };
    let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&bundle).map_err(|_| SealError::Format)?;
    let envelope = domain.seal_ck(&dek, "ck-bundle", Some(&principal))?;
    let sealed = seal_bytes(
        &dek,
        &domain.bundle_aad("ck-calendar-bundle", Some(entry.account_id))?,
        &plain,
    );
    let mut changed = entry.clone();
    changed.name = format!(
        "{COLLECTION_MARKER}{}|{}|{sealed}",
        STANDARD.encode(ring),
        STANDARD.encode(envelope)
    );
    changed.description = None;
    changed.color = None;
    if let Timezone::Custom(tree) = &mut changed.time_zone {
        seal_tree(tree, &dek, domain.owner_id(), TZ_SCOPE);
    }
    Ok(changed)
}

/// Prepare the calendar-wide dead-property bundle for explicit assembly or
/// a property write. A new bundle always has a fresh independently wrapped DEK.
pub(crate) fn seal_shared_properties(
    properties: &DeadProperty,
    domain: &ZaKeyDomain,
) -> Result<(String, String), SealError> {
    let dek = Secret::random();
    let bundle = Bundle {
        name: String::new(),
        description: None,
        color: None,
        dead_properties: properties.clone(),
    };
    let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&bundle).map_err(|_| SealError::Format)?;
    Ok((
        STANDARD.encode(domain.seal_ck(&dek, "ck-shared-dek", None)?),
        seal_bytes(&dek, &domain.aad("ck-shared-bundle")?, &plain),
    ))
}

fn entry_parts(entry: &CalendarPreferences) -> Result<Vec<&str>, SealError> {
    Ok(entry
        .name
        .strip_prefix(COLLECTION_MARKER)
        .ok_or(SealError::NotSealed)?
        .split('|')
        .collect())
}

/// Validate the sealed shapes without opening any foreign personal material.
/// The domain itself was resolved from the freshly opened ring by the caller.
fn shared_parts<'a>(
    calendar: &'a Calendar,
    domain: &ZaKeyDomain,
) -> Result<Vec<Vec<&'a str>>, SealError> {
    use crate::calendar::sharing::format::{parse_owner_header, za_replace_calendar_entries};
    let calendar_id = domain.calendar_id().ok_or(SealError::Format)?;
    let owner = calendar
        .preferences
        .iter()
        .find(|p| p.account_id == domain.owner_id())
        .ok_or(SealError::Structure("owner entry"))?;
    let parts = entry_parts(owner)?;
    if parts.len() != 5 {
        return Err(SealError::Format);
    }
    let header = parse_owner_header(&STANDARD.decode(parts[0]).map_err(|_| SealError::Decode)?)?;
    if header.calendar_id != calendar_id
        || Some(&header.domain_id) != domain.domain_id()
        || !domain.matches_generations(&header.generations)
    {
        return Err(SealError::Structure("ring domain"));
    }
    let mut checked = calendar.clone();
    za_replace_calendar_entries(
        &mut checked,
        domain.owner_id(),
        calendar_id,
        calendar.preferences.clone(),
        calendar.acls.clone(),
    )?;
    calendar.preferences.iter().map(entry_parts).collect()
}

fn open_personal(
    calendar: &mut Calendar,
    index: usize,
    parts: &[&str],
    domain: &ZaKeyDomain,
) -> Result<(), SealError> {
    let entry = &mut calendar.preferences[index];
    let envelope = STANDARD.decode(parts[1]).map_err(|_| SealError::Decode)?;
    let dek = domain.open_ck(
        &envelope,
        "ck-bundle",
        Some(&entry.account_id.to_be_bytes()),
    )?;
    let bundle = open_archive::<Bundle>(
        &dek,
        &domain.bundle_aad("ck-calendar-bundle", Some(entry.account_id))?,
        parts[2],
    )?;
    if bundle.color.is_some() || !bundle.dead_properties.0.is_empty() {
        return Err(SealError::Format);
    }
    if let Timezone::Custom(tree) = &mut entry.time_zone {
        unseal_tree(tree, &dek, domain.owner_id(), TZ_SCOPE)?;
    }
    entry.name = bundle.name;
    entry.description = bundle.description;
    entry.color = None;
    Ok(())
}

fn open_shared(parts: &[&str], domain: &ZaKeyDomain) -> Result<DeadProperty, SealError> {
    let envelope = STANDARD.decode(parts[3]).map_err(|_| SealError::Decode)?;
    let dek = domain.open_ck(&envelope, "ck-shared-dek", None)?;
    let bundle = open_archive::<Bundle>(&dek, &domain.aad("ck-shared-bundle")?, parts[4])?;
    if !bundle.name.is_empty() || bundle.description.is_some() || bundle.color.is_some() {
        return Err(SealError::Format);
    }
    Ok(bundle.dead_properties)
}

fn calendar_in_domain(
    stored: &Calendar,
    domain: &ZaKeyDomain,
    personal_id: u32,
) -> Result<Calendar, SealError> {
    if personal_id != domain.principal_id() {
        return Err(SealError::Structure("personal entry principal"));
    }
    let mut view = stored.clone();
    if domain.calendar_id().is_none() {
        if personal_id != domain.owner_id() {
            return Err(SealError::Structure("personal entry"));
        }
        unseal_calendar_with_key(&mut view, domain.ewk()?, domain.owner_id())?;
        return Ok(view);
    }
    let parts = shared_parts(stored, domain)?;
    let personal = stored
        .preferences
        .iter()
        .position(|p| p.account_id == personal_id)
        .ok_or(SealError::Structure("personal entry"))?;
    let owner = stored
        .preferences
        .iter()
        .position(|p| p.account_id == domain.owner_id())
        .unwrap();
    view.dead_properties = open_shared(&parts[owner], domain)?;
    open_personal(&mut view, personal, &parts[personal], domain)?;
    Ok(view)
}

/// Opens only the requested principal's personal bundle and shared properties.
/// This archive is a response view; never use it as an update's current value.
pub fn calendar_view_in_domain(
    stored: &Archive<AlignedBytes>,
    domain: &ZaKeyDomain,
    personal_id: u32,
) -> Result<Archive<AlignedBytes>, SealError> {
    let calendar = stored
        .deserialize::<Calendar>()
        .map_err(|_| SealError::Format)?;
    let calendar = calendar_in_domain(&calendar, domain, personal_id)?;
    let bytes = Archiver::new(calendar)
        .serialize()
        .map_err(|_| SealError::Format)?;
    let mut view = <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes)
        .map_err(|_| SealError::Format)?;
    view.version = stored.version;
    Ok(view)
}

/// Property-write codec. The entry id set and rings are immutable here;
/// grants/revokes/conversion use the explicit sharing assembly path instead.
pub fn seal_calendar_changes(
    stored: &Calendar,
    edited: &Calendar,
    domain: &ZaKeyDomain,
    personal_id: u32,
) -> Result<Calendar, SealError> {
    use std::collections::BTreeSet;
    let ids = |calendar: &Calendar| {
        calendar
            .preferences
            .iter()
            .map(|p| p.account_id)
            .collect::<BTreeSet<_>>()
    };
    let stored_ids = ids(stored);
    let edited_ids = ids(edited);
    if stored_ids != edited_ids
        || stored_ids.len() != stored.preferences.len()
        || edited_ids.len() != edited.preferences.len()
        || stored.acls != edited.acls
    {
        return Err(SealError::Structure("calendar entry id set or ACLs"));
    }
    let opened = calendar_in_domain(stored, domain, personal_id)?;
    let mut changed = edited.clone();
    if domain.calendar_id().is_none() {
        seal_calendar_in_owner(&mut changed, domain)?;
        return Ok(changed);
    }
    let parts = shared_parts(stored, domain)?;
    let personal = stored
        .preferences
        .iter()
        .position(|p| p.account_id == personal_id)
        .ok_or(SealError::Structure("personal entry"))?;
    let owner = stored
        .preferences
        .iter()
        .position(|p| p.account_id == domain.owner_id())
        .unwrap();
    // Keep stored order and all unopened entries verbatim. An edited view cannot
    // smuggle a foreign personal write through this codec.
    for entry in &edited.preferences {
        if entry.account_id != personal_id
            && stored
                .preferences
                .iter()
                .find(|p| p.account_id == entry.account_id)
                != Some(entry)
        {
            return Err(SealError::Structure("foreign personal change"));
        }
    }
    changed.preferences = stored.preferences.clone();
    let edited_personal = edited
        .preferences
        .iter()
        .find(|p| p.account_id == personal_id)
        .unwrap();
    let before = &opened.preferences[personal];
    if edited_personal.name != before.name
        || edited_personal.description != before.description
        || edited_personal.time_zone != before.time_zone
    {
        let ring = STANDARD
            .decode(parts[personal][0])
            .map_err(|_| SealError::Decode)?;
        changed.preferences[personal] = seal_personal_entry(edited_personal, domain, &ring)?;
        if personal == owner {
            changed.preferences[personal]
                .name
                .push_str(&format!("|{}|{}", parts[owner][3], parts[owner][4]));
        }
    } else {
        // Visible preferences need no resealing of a personal bundle.
        let target = &mut changed.preferences[personal];
        target.sort_order = edited_personal.sort_order;
        target.flags = edited_personal.flags;
        target.default_alerts = edited_personal.default_alerts.clone();
    }
    if edited_personal.color.is_some() {
        return Err(SealError::Structure("personal color"));
    }
    if edited.dead_properties != opened.dead_properties {
        let (envelope, bundle) = seal_shared_properties(&edited.dead_properties, domain)?;
        let own = &mut changed.preferences[owner];
        let personal_parts = entry_parts(own)?;
        own.name = format!(
            "{}|{}|{}|{envelope}|{bundle}",
            COLLECTION_MARKER.to_owned() + personal_parts[0],
            personal_parts[1],
            personal_parts[2]
        );
    }
    changed.dead_properties = DeadProperty::default();
    // Validate exactly the same assembly contract as explicit share mutations.
    shared_parts(&changed, domain)?;
    Ok(changed)
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

    fn shared_fixture() -> (
        Calendar,
        super::super::domain::ZaKeyDomain,
        super::super::domain::ZaKeyDomain,
    ) {
        use crate::calendar::sharing::format::*;
        use std::collections::BTreeMap;
        let ck = Secret::from_bytes([3; 32]);
        let ewk = Secret::random();
        let owner = super::super::domain::ZaKeyDomain::shared(
            4,
            4,
            Some(ewk.clone()),
            42,
            [7; 16],
            BTreeMap::from([(1, ck.clone())]),
            false,
        )
        .unwrap();
        let grantee = super::super::domain::ZaKeyDomain::shared(
            4,
            8,
            None,
            42,
            [7; 16],
            BTreeMap::from([(1, ck.clone())]),
            false,
        )
        .unwrap();
        let header = OwnerRingHeader {
            calendar_id: 42,
            domain_id: [7; 16],
            rename_change_id: 0,
            boundary_pending: false,
            generations: vec![1],
        };
        let (public, _) = vault::keys::generate_keypair();
        let ring = seal_owner_ring(
            &header,
            &ZaOwnerRing {
                version: 1,
                keys: vec![[3; 32]],
                converted: true,
                reverting: false,
                grants: vec![ZaGrant {
                    account_id: 8,
                    address: "grantee@example.org".into(),
                    public_key: public,
                    level: 1,
                    granted_at: 0,
                    change_id: 0,
                }],
            },
            4,
            &ewk,
        )
        .unwrap();
        let mut c = calendar();
        let mut own = seal_personal_entry(&c.preferences[0], &owner, &ring).unwrap();
        let (envelope, bundle) = seal_shared_properties(&c.dead_properties, &owner).unwrap();
        own.name.push_str(&format!("|{envelope}|{bundle}"));
        let grant_header = GrantRingHeader {
            calendar_id: 42,
            domain_id: [7; 16],
            grantee_id: 8,
            grant_change_id: 0,
            rename_change_id: 0,
            boundary_pending: false,
            generations: vec![1],
        };
        let grant = seal_grant_ring(&grant_header, &BTreeMap::from([(1, ck)]), 4, &public).unwrap();
        let mut personal = c.preferences[0].clone();
        personal.account_id = 8;
        personal.name = "Grantee name".into();
        personal.flags = 0;
        personal.color = None;
        let personal = seal_personal_entry(&personal, &grantee, &grant).unwrap();
        c.preferences = vec![own, personal];
        c.dead_properties = DeadProperty::default();
        c.acls = vec![types::acl::AclGrant {
            account_id: 8,
            grants: utils::map::bitmap::Bitmap::new()
                .with_item(types::acl::Acl::Read)
                .with_item(types::acl::Acl::ReadItems),
        }];
        (c, owner, grantee)
    }

    #[test]
    fn shared_bundles_interoperate_with_literal_content_aad() {
        let (c, _, grantee) = shared_fixture();
        let own = entry_parts(&c.preferences[0]).unwrap();
        let personal = entry_parts(&c.preferences[1]).unwrap();
        let ck = Secret::from_bytes([3; 32]);
        let envelope = STANDARD.decode(personal[1]).unwrap();
        let dek=unwrap_key(&envelope[10..],&ck,b"za/v1|ck-bundle|\0\0\0\x04|\0\0\0\x2a|\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07|\0\0\0\x01|\0\0\0\x08").unwrap();
        let bundle=open_archive::<Bundle>(&dek,b"za/v1|ck-calendar-bundle|\0\0\0\x04|\0\0\0\x2a|\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07|\0\0\0\x08",personal[2]).unwrap();
        assert_eq!(bundle.name, "Grantee name");
        assert!(bundle.dead_properties.0.is_empty());
        let envelope = STANDARD.decode(own[3]).unwrap();
        let dek=unwrap_key(&envelope[10..],&ck,b"za/v1|ck-shared-dek|\0\0\0\x04|\0\0\0\x2a|\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07|\0\0\0\x01").unwrap();
        let bundle=open_archive::<Bundle>(&dek,b"za/v1|ck-shared-bundle|\0\0\0\x04|\0\0\0\x2a|\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07\x07",own[4]).unwrap();
        assert_eq!(bundle.dead_properties, calendar().dead_properties);
        let view = calendar_view_in_domain(&archive(&c), &grantee, 8)
            .unwrap()
            .deserialize::<Calendar>()
            .unwrap();
        assert_eq!(
            view.preferences[1].time_zone,
            calendar().preferences[0].time_zone
        );
    }

    #[test]
    fn grantee_domain_refuses_a_foreign_personal_id() {
        let (c, _, grantee) = shared_fixture();
        assert!(calendar_view_in_domain(&archive(&c), &grantee, 4).is_err());
        assert!(seal_calendar_changes(&c, &c, &grantee, 4).is_err());
    }

    #[test]
    fn shared_collection_uses_two_three_and_five_part_forms() {
        let mut legacy = calendar();
        let keys = keys();
        seal_calendar(&mut legacy, &keys, 4).unwrap();
        assert_eq!(legacy.preferences[0].name.split('|').count(), 2);
        let (c, owner, grantee) = shared_fixture();
        assert_eq!(c.preferences[0].name.split('|').count(), 5);
        assert_eq!(c.preferences[1].name.split('|').count(), 3);
        let stored = archive(&c);
        let view = calendar_view_in_domain(&stored, &owner, 4).unwrap();
        assert_eq!(view.version, stored.version);
        let opened = view.deserialize::<Calendar>().unwrap();
        assert_eq!(opened.preferences[0].name, "Work canary");
        assert!(opened.preferences[0].color.is_none());
        assert_eq!(opened.dead_properties, calendar().dead_properties);
        assert_eq!(opened.preferences[1], c.preferences[1]);
        let opened = calendar_view_in_domain(&stored, &grantee, 8)
            .unwrap()
            .deserialize::<Calendar>()
            .unwrap();
        assert_eq!(opened.preferences[1].name, "Grantee name");
        assert_eq!(opened.preferences[0], c.preferences[0]);
        for index in [0, 1] {
            let mut bad = c.clone();
            bad.preferences[index].name.push_str("|extra");
            assert!(calendar_view_in_domain(&archive(&bad), &grantee, 8).is_err());
        }
    }

    #[test]
    fn grantee_view_never_opens_foreign_personal_material() {
        let (mut c, _, grantee) = shared_fixture();
        // Structurally valid but unauthentic owner personal envelope, bundle and timezone.
        let mut parts = c.preferences[0]
            .name
            .split('|')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut envelope = STANDARD.decode(&parts[1]).unwrap();
        envelope[20] ^= 1;
        parts[1] = STANDARD.encode(envelope);
        let mut bundle = STANDARD.decode(&parts[2]).unwrap();
        bundle[30] ^= 1;
        parts[2] = STANDARD.encode(bundle);
        c.preferences[0].name = parts.join("|");
        let tz = custom_tz(&mut c);
        let carrier = tz.components[2].entries.last_mut().unwrap();
        let mut bytes = STANDARD
            .decode(super::super::tree::entry_text(carrier).unwrap())
            .unwrap();
        bytes[30] ^= 1;
        carrier.values = vec![calcard::icalendar::ICalendarValue::Text(
            STANDARD.encode(bytes),
        )];
        let view = calendar_view_in_domain(&archive(&c), &grantee, 8)
            .unwrap()
            .deserialize::<Calendar>()
            .unwrap();
        assert_eq!(view.preferences[0], c.preferences[0]);
        assert_eq!(view.preferences[1].name, "Grantee name");
    }

    #[test]
    fn shared_property_write_preserves_owner_parts() {
        let (stored, owner, grantee) = shared_fixture();
        let mut edited = calendar_view_in_domain(&archive(&stored), &grantee, 8)
            .unwrap()
            .deserialize::<Calendar>()
            .unwrap();
        edited.preferences[1].name = "Changed".into();
        edited.dead_properties = DeadProperty::default();
        let changed = seal_calendar_changes(&stored, &edited, &grantee, 8).unwrap();
        let before = stored.preferences[0].name.split('|').collect::<Vec<_>>();
        let after = changed.preferences[0].name.split('|').collect::<Vec<_>>();
        assert_eq!(&before[..3], &after[..3]);
        assert_eq!(
            stored.preferences[0].time_zone,
            changed.preferences[0].time_zone
        );
        assert_ne!(before[3], after[3]);
        assert_ne!(before[4], after[4]);
        let old_shared = grantee
            .open_ck(&STANDARD.decode(before[3]).unwrap(), "ck-shared-dek", None)
            .unwrap();
        let new_shared = grantee
            .open_ck(&STANDARD.decode(after[3]).unwrap(), "ck-shared-dek", None)
            .unwrap();
        assert_ne!(
            old_shared.as_bytes(),
            new_shared.as_bytes(),
            "rewriting shared properties draws a fresh DEK"
        );
        let old_parts = entry_parts(&stored.preferences[1]).unwrap();
        let new_parts = entry_parts(&changed.preferences[1]).unwrap();
        let old_personal = grantee
            .open_ck(
                &STANDARD.decode(old_parts[1]).unwrap(),
                "ck-bundle",
                Some(&8u32.to_be_bytes()),
            )
            .unwrap();
        let new_personal = grantee
            .open_ck(
                &STANDARD.decode(new_parts[1]).unwrap(),
                "ck-bundle",
                Some(&8u32.to_be_bytes()),
            )
            .unwrap();
        assert_ne!(
            old_personal.as_bytes(),
            new_personal.as_bytes(),
            "rewriting personal properties draws a fresh DEK"
        );
        assert_ne!(
            stored.preferences[1].time_zone,
            changed.preferences[1].time_zone
        );

        assert_eq!(
            stored.preferences[1].name.split('|').next(),
            changed.preferences[1].name.split('|').next()
        );
        let opened = calendar_view_in_domain(&archive(&changed), &grantee, 8)
            .unwrap()
            .deserialize::<Calendar>()
            .unwrap();
        assert_eq!(opened.preferences[1].name, "Changed");
        assert!(opened.dead_properties.0.is_empty());
        let mut injected = edited.clone();
        injected.preferences_mut(99);
        assert!(seal_calendar_changes(&stored, &injected, &grantee, 8).is_err());
        let mut removed = edited.clone();
        removed.preferences.remove(0);
        assert!(seal_calendar_changes(&stored, &removed, &grantee, 8).is_err());
        let unchanged = seal_calendar_changes(
            &stored,
            &calendar_view_in_domain(&archive(&stored), &owner, 4)
                .unwrap()
                .deserialize::<Calendar>()
                .unwrap(),
            &owner,
            4,
        )
        .unwrap();
        assert_eq!(
            unchanged, stored,
            "no edited bundles means no new ciphertext"
        );
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
