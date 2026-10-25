/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Sharing 3.2-3.3 wire formats. Header parsing is structural, never an
//! authorization decision. Only opening the owner ring authenticates its header.

use crate::calendar::seal::policy::POLICY_VERSION;
use crate::calendar::seal::{
    COLLECTION_MARKER, PAD, SealError, decode_archive, open_padded, seal_padded,
};
use crate::calendar::{Calendar, CalendarPreferences};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::auth::vault::za_normalize_address;
use std::collections::{BTreeMap, HashSet};
use types::acl::{Acl, AclGrant};
use utils::map::bitmap::Bitmap;
use vault::keys::{NONCE_LEN, Secret, TAG_LEN, WRAP_LEN, aad, pk_unwrap, pk_wrap};
use zeroize::{Zeroize, Zeroizing};

pub const WRAP_CK: u8 = 0x03;
pub const WRAP_OWNER_RING: u8 = 0x04;
pub const WRAP_GRANT_RING: u8 = 0x05;
const OWNER_FIXED: usize = 33;
const GRANT_FIXED: usize = 45;
const GRANT_RECORD: usize = 4 + 32 + WRAP_LEN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerRingHeader {
    pub calendar_id: u32,
    pub domain_id: [u8; 16],
    pub rename_change_id: u64,
    pub boundary_pending: bool,
    pub generations: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRingHeader {
    pub calendar_id: u32,
    pub domain_id: [u8; 16],
    pub grantee_id: u32,
    pub grant_change_id: u64,
    pub rename_change_id: u64,
    pub boundary_pending: bool,
    pub generations: Vec<u32>,
}

/// New encrypted payloads only; no existing stored layout changes.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaOwnerRing {
    pub version: u8,
    pub keys: Vec<[u8; 32]>,
    pub converted: bool,
    pub reverting: bool,
    pub grants: Vec<ZaGrant>,
}

#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize, Clone, PartialEq, Eq)]
pub struct ZaGrant {
    pub account_id: u32,
    pub address: String,
    pub public_key: [u8; 32],
    pub level: u8,
    pub granted_at: i64,
    pub change_id: u64,
}

impl std::fmt::Debug for ZaOwnerRing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ZaOwnerRing(..)")
    }
}

impl std::fmt::Debug for ZaGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ZaGrant(..)")
    }
}

impl Zeroize for ZaOwnerRing {
    fn zeroize(&mut self) {
        self.version.zeroize();
        self.keys.zeroize();
        self.converted.zeroize();
        self.reverting.zeroize();
        self.grants.iter_mut().for_each(Zeroize::zeroize);
        self.grants.clear();
    }
}

impl Drop for ZaOwnerRing {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl ZaOwnerRing {
    /// Append without reallocating storage that already contains collection keys.
    pub(crate) fn append_key(&mut self, key: &Secret) {
        append_owner_key(
            &mut self.keys,
            key,
            #[cfg(test)]
            |_| {},
        );
    }
}

fn append_owner_key(
    keys: &mut Vec<[u8; 32]>,
    key: &Secret,
    #[cfg(test)] before_release: impl FnOnce(&[[u8; 32]]),
) {
    // Allocate before copying any keys. Neither write can exceed this capacity;
    // Zeroizing owns the replacement until it belongs to the ring, including unwind.
    let mut replacement = Zeroizing::new(Vec::with_capacity(keys.len() + 1));
    replacement.extend_from_slice(keys);
    replacement.push(*key.as_bytes());
    // Keep initialized storage observable until the wipe is complete. Wipe spare
    // capacity too, then swap without allocation. The retired Vec remains under
    // Zeroizing ownership until release; the ring owns the new keys from the swap.
    keys.iter_mut().for_each(Zeroize::zeroize);
    keys.spare_capacity_mut().zeroize();
    #[cfg(test)]
    before_release(keys);
    std::mem::swap(keys, &mut replacement);
}

impl Zeroize for ZaGrant {
    fn zeroize(&mut self) {
        self.account_id.zeroize();
        self.address.zeroize();
        self.public_key.zeroize();
        self.level.zeroize();
        self.granted_at.zeroize();
        self.change_id.zeroize();
    }
}

impl Drop for ZaGrant {
    fn drop(&mut self) {
        self.zeroize();
    }
}

fn validate_generations(generations: &[u32]) -> Result<(), SealError> {
    if generations.is_empty()
        || generations.len() > u16::MAX as usize
        || generations[0] == 0
        || generations.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(SealError::Structure("ring generations"));
    }
    Ok(())
}

impl OwnerRingHeader {
    /// A stored maximum generation remains readable, but cannot be rotated.
    pub fn next_generation(&self) -> Result<u32, SealError> {
        validate_generations(&self.generations)?;
        self.generations
            .last()
            .and_then(|last| last.checked_add(1))
            .ok_or(SealError::Structure("generation exhausted"))
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], SealError> {
        let end = self.offset.checked_add(N).ok_or(SealError::Format)?;
        let bytes = self.bytes.get(self.offset..end).ok_or(SealError::Format)?;
        self.offset = end;
        bytes.try_into().map_err(|_| SealError::Format)
    }

    fn u32(&mut self) -> Result<u32, SealError> {
        Ok(u32::from_be_bytes(self.take()?))
    }
    fn u64(&mut self) -> Result<u64, SealError> {
        Ok(u64::from_be_bytes(self.take()?))
    }
    fn count(&mut self) -> Result<usize, SealError> {
        Ok(u16::from_be_bytes(self.take()?) as usize)
    }
    fn flag(&mut self) -> Result<bool, SealError> {
        match self.take()? {
            [0] => Ok(false),
            [1] => Ok(true),
            _ => Err(SealError::Format),
        }
    }
}

fn reader(bytes: &[u8], wrap: u8) -> Result<Reader<'_>, SealError> {
    match bytes {
        [POLICY_VERSION, kind, ..] if *kind == wrap => Ok(Reader { bytes, offset: 2 }),
        [version, ..] if *version != POLICY_VERSION => Err(SealError::Policy(*version)),
        _ => Err(SealError::Format),
    }
}

fn padded_shape(len: usize) -> bool {
    len >= NONCE_LEN + TAG_LEN + PAD && (len - NONCE_LEN - TAG_LEN).is_multiple_of(PAD)
}

/// Parses the visible header and checks the encrypted payload's outer shape.
/// A whole padded block removed from a larger payload is detected by opening it.
pub fn parse_owner_header(bytes: &[u8]) -> Result<OwnerRingHeader, SealError> {
    let mut reader = reader(bytes, WRAP_OWNER_RING)?;
    let calendar_id = reader.u32()?;
    let domain_id = reader.take()?;
    let rename_change_id = reader.u64()?;
    let boundary_pending = reader.flag()?;
    let count = reader.count()?;
    let end = OWNER_FIXED + count * 4;
    if !bytes
        .get(end..)
        .is_some_and(|payload| padded_shape(payload.len()))
    {
        return Err(SealError::Format);
    }
    let mut generations = Vec::with_capacity(count);
    for _ in 0..count {
        generations.push(reader.u32()?);
    }
    validate_generations(&generations)?;
    Ok(OwnerRingHeader {
        calendar_id,
        domain_id,
        rename_change_id,
        boundary_pending,
        generations,
    })
}

pub fn parse_grant_header(bytes: &[u8]) -> Result<GrantRingHeader, SealError> {
    let mut reader = reader(bytes, WRAP_GRANT_RING)?;
    let calendar_id = reader.u32()?;
    let domain_id = reader.take()?;
    let grantee_id = reader.u32()?;
    let grant_change_id = reader.u64()?;
    let rename_change_id = reader.u64()?;
    let boundary_pending = reader.flag()?;
    let count = reader.count()?;
    if bytes.len() != GRANT_FIXED + count * GRANT_RECORD {
        return Err(SealError::Format);
    }
    let mut generations = Vec::with_capacity(count);
    for _ in 0..count {
        generations.push(reader.u32()?);
        reader.take::<32>()?;
        reader.take::<WRAP_LEN>()?;
    }
    validate_generations(&generations)?;
    Ok(GrantRingHeader {
        calendar_id,
        domain_id,
        grantee_id,
        grant_change_id,
        rename_change_id,
        boundary_pending,
        generations,
    })
}

/// Rewrite only unauthenticated boundary metadata after validating the entire ring.
/// Identity, generations, ephemeral public keys and wrapped records stay byte-identical.
pub(crate) fn rewrite_grant_boundary(
    bytes: &[u8],
    grant_change_id: u64,
    rename_change_id: u64,
    pending: bool,
) -> Result<Vec<u8>, SealError> {
    parse_grant_header(bytes)?;
    let mut changed = bytes.to_vec();
    changed[26..34].copy_from_slice(&grant_change_id.to_be_bytes());
    changed[34..42].copy_from_slice(&rename_change_id.to_be_bytes());
    changed[42] = u8::from(pending);
    Ok(changed)
}

fn encode_owner_header(header: &OwnerRingHeader) -> Result<Vec<u8>, SealError> {
    validate_generations(&header.generations)?;
    let mut bytes = Vec::with_capacity(OWNER_FIXED + header.generations.len() * 4);
    bytes.extend_from_slice(&[POLICY_VERSION, WRAP_OWNER_RING]);
    bytes.extend_from_slice(&header.calendar_id.to_be_bytes());
    bytes.extend_from_slice(&header.domain_id);
    bytes.extend_from_slice(&header.rename_change_id.to_be_bytes());
    bytes.push(u8::from(header.boundary_pending));
    bytes.extend_from_slice(&(header.generations.len() as u16).to_be_bytes());
    for generation in &header.generations {
        bytes.extend_from_slice(&generation.to_be_bytes());
    }
    Ok(bytes)
}

fn collection_aad(purpose: &str, owner: u32, calendar: u32, domain: &[u8; 16]) -> Vec<u8> {
    let mut bytes = aad(purpose, owner);
    bytes.push(b'|');
    bytes.extend_from_slice(&calendar.to_be_bytes());
    bytes.push(b'|');
    bytes.extend_from_slice(domain);
    bytes
}

fn owner_aad(owner: u32, header: &OwnerRingHeader, encoded: &[u8]) -> Vec<u8> {
    let mut bytes = collection_aad("ck-owner", owner, header.calendar_id, &header.domain_id);
    bytes.push(b'|');
    bytes.extend_from_slice(encoded);
    bytes
}

fn grant_aad(owner: u32, header: &GrantRingHeader, generation: u32) -> Vec<u8> {
    let mut bytes = collection_aad("ck-grant", owner, header.calendar_id, &header.domain_id);
    bytes.push(b'|');
    bytes.extend_from_slice(&header.grantee_id.to_be_bytes());
    bytes.push(b'|');
    bytes.extend_from_slice(&generation.to_be_bytes());
    bytes
}

fn validate_owner_ring(
    header: &OwnerRingHeader,
    ring: &ZaOwnerRing,
    owner: u32,
) -> Result<(), SealError> {
    if ring.version != POLICY_VERSION {
        return Err(SealError::Policy(ring.version));
    }
    if ring.keys.len() != header.generations.len() {
        return Err(SealError::Structure("ring key count"));
    }
    let mut accounts = HashSet::with_capacity(ring.grants.len());
    for grant in &ring.grants {
        let normalized = za_normalize_address(&grant.address).map(Zeroizing::new);
        if grant.account_id == owner
            || !accounts.insert(grant.account_id)
            || !matches!(grant.level, 1 | 2)
            || normalized.as_deref().map(String::as_str) != Some(grant.address.as_str())
        {
            return Err(SealError::Structure("grant ledger"));
        }
    }
    Ok(())
}

/// Borrows fixed storage: writes cannot grow or relocate it. Wiping the whole
/// slice on Drop covers successful serialization, overflow and every error exit.
struct OwnerRingWriter<'a> {
    bytes: &'a mut [u8],
    position: usize,
}

impl Drop for OwnerRingWriter<'_> {
    fn drop(&mut self) {
        self.bytes.zeroize();
    }
}

impl rkyv::ser::Positional for OwnerRingWriter<'_> {
    fn pos(&self) -> usize {
        self.position
    }
}

impl rkyv::ser::Writer<rkyv::rancor::Error> for OwnerRingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> Result<(), rkyv::rancor::Error> {
        use rkyv::rancor::Source;
        let end = self
            .position
            .checked_add(bytes.len())
            .ok_or_else(|| rkyv::rancor::Error::new(SealError::Format))?;
        let target = self
            .bytes
            .get_mut(self.position..end)
            .ok_or_else(|| rkyv::rancor::Error::new(SealError::Format))?;
        target.copy_from_slice(bytes);
        self.position = end;
        Ok(())
    }
}

/// Bound the exact payload's archived arrays, strings and alignment gaps.
/// Inline strings only reduce the used length. A future serializer/layout change
/// that exceeds this bound fails in the fixed writer instead of reallocating.
fn owner_archive_capacity(ring: &ZaOwnerRing) -> Result<usize, SealError> {
    use std::mem::{align_of, size_of};
    let sizes = [
        size_of::<ArchivedZaOwnerRing>(),
        align_of::<ArchivedZaOwnerRing>() - 1,
        ring.keys
            .len()
            .checked_mul(size_of::<rkyv::Archived<[u8; 32]>>())
            .ok_or(SealError::Format)?,
        align_of::<rkyv::Archived<[u8; 32]>>() - 1,
        ring.grants
            .len()
            .checked_mul(size_of::<ArchivedZaGrant>())
            .ok_or(SealError::Format)?,
        align_of::<ArchivedZaGrant>() - 1,
    ];
    sizes
        .into_iter()
        .chain(ring.grants.iter().map(|grant| grant.address.len()))
        .try_fold(0usize, |size, part| size.checked_add(part))
        .filter(|size| *size <= u32::MAX as usize)
        .ok_or(SealError::Format)
}

pub fn seal_owner_ring(
    header: &OwnerRingHeader,
    ring: &ZaOwnerRing,
    owner: u32,
    ewk: &Secret,
) -> Result<Vec<u8>, SealError> {
    let mut bytes = encode_owner_header(header)?;
    validate_owner_ring(header, ring, owner)?;
    let mut plain = Zeroizing::new(vec![0; owner_archive_capacity(ring)?]);
    let mut writer = OwnerRingWriter {
        bytes: &mut plain,
        position: 0,
    };
    // These payloads' rkyv allocator scratch contains only unit resolvers and
    // string/vector positions. All CK and ledger bytes go to the fixed writer.
    rkyv::api::high::to_bytes_in::<_, rkyv::rancor::Error>(ring, &mut writer)
        .map_err(|_| SealError::Format)?;
    let sealed = seal_padded(
        ewk,
        &owner_aad(owner, header, &bytes),
        &writer.bytes[..writer.position],
    );
    bytes.extend_from_slice(&sealed);
    Ok(bytes)
}

pub fn open_owner_ring(
    bytes: &[u8],
    owner: u32,
    calendar: u32,
    ewk: &Secret,
) -> Result<(OwnerRingHeader, ZaOwnerRing), SealError> {
    let header = parse_owner_header(bytes)?;
    if header.calendar_id != calendar {
        return Err(SealError::Structure("ring calendar"));
    }
    let offset = OWNER_FIXED + header.generations.len() * 4;
    let plain = open_padded(
        ewk,
        &owner_aad(owner, &header, &bytes[..offset]),
        &bytes[offset..],
    )?;
    let ring = decode_archive::<ZaOwnerRing>(&plain)?;
    validate_owner_ring(&header, &ring, owner)?;
    Ok((header, ring))
}

pub fn seal_grant_ring(
    header: &GrantRingHeader,
    keys: &BTreeMap<u32, Secret>,
    owner: u32,
    public_key: &[u8; 32],
) -> Result<Vec<u8>, SealError> {
    validate_generations(&header.generations)?;
    if !keys.keys().copied().eq(header.generations.iter().copied()) {
        return Err(SealError::Structure("grant key generations"));
    }
    let mut bytes = Vec::with_capacity(GRANT_FIXED + keys.len() * GRANT_RECORD);
    bytes.extend_from_slice(&[POLICY_VERSION, WRAP_GRANT_RING]);
    bytes.extend_from_slice(&header.calendar_id.to_be_bytes());
    bytes.extend_from_slice(&header.domain_id);
    bytes.extend_from_slice(&header.grantee_id.to_be_bytes());
    bytes.extend_from_slice(&header.grant_change_id.to_be_bytes());
    bytes.extend_from_slice(&header.rename_change_id.to_be_bytes());
    bytes.push(u8::from(header.boundary_pending));
    bytes.extend_from_slice(&(keys.len() as u16).to_be_bytes());
    for (generation, key) in keys {
        let (ephemeral, wrapped) = pk_wrap(key, public_key, &grant_aad(owner, header, *generation))
            .map_err(|_| SealError::Structure("recipient public key"))?;
        bytes.extend_from_slice(&generation.to_be_bytes());
        bytes.extend_from_slice(&ephemeral);
        bytes.extend_from_slice(&wrapped);
    }
    Ok(bytes)
}

pub fn open_grant_ring(
    bytes: &[u8],
    owner: u32,
    calendar: u32,
    principal: u32,
    domain: [u8; 16],
    private_key: &Secret,
) -> Result<(GrantRingHeader, BTreeMap<u32, Secret>), SealError> {
    let header = parse_grant_header(bytes)?;
    if header.calendar_id != calendar
        || header.grantee_id != principal
        || header.domain_id != domain
    {
        return Err(SealError::Structure("grant identity"));
    }
    let mut keys = BTreeMap::new();
    let mut reader = Reader {
        bytes,
        offset: GRANT_FIXED,
    };
    for generation in &header.generations {
        let record_generation = reader.u32()?;
        if record_generation != *generation {
            return Err(SealError::Format);
        }
        let ephemeral = reader.take()?;
        let wrapped = reader.take::<WRAP_LEN>()?;
        let key = pk_unwrap(
            &ephemeral,
            &wrapped,
            private_key,
            &grant_aad(owner, &header, *generation),
        )
        .map_err(|_| SealError::Aead)?;
        keys.insert(*generation, key);
    }
    Ok((header, keys))
}

fn sealed_bundle(text: &str) -> Result<(), SealError> {
    let bytes = STANDARD.decode(text).map_err(|_| SealError::Decode)?;
    if bytes.first() != Some(&1) || !padded_shape(bytes.len().saturating_sub(1)) {
        return Err(SealError::Format);
    }
    Ok(())
}

fn bundle_envelope(text: &str, calendar: u32, generations: &[u32]) -> Result<(), SealError> {
    let bytes = STANDARD.decode(text).map_err(|_| SealError::Decode)?;
    let mut reader = reader(&bytes, WRAP_CK)?;
    if bytes.len() != 10 + WRAP_LEN
        || reader.u32()? != calendar
        || !generations.contains(&reader.u32()?)
    {
        return Err(SealError::Format);
    }
    Ok(())
}

fn sealed_timezone(entry: &CalendarPreferences) -> Result<(), SealError> {
    use crate::calendar::seal::{
        policy::{is_visible_parameter, is_visible_property},
        tree::{SEALED_PROP, is_carrier},
    };
    use calcard::icalendar::ICalendarValue;
    if let crate::calendar::Timezone::Custom(tree) = &entry.time_zone {
        for component in &tree.components {
            for (index, property) in component.entries.iter().enumerate() {
                if is_carrier(property, SEALED_PROP) {
                    if index + 1 != component.entries.len() || !property.params.is_empty() {
                        return Err(SealError::Structure("timezone carrier"));
                    }
                    let [ICalendarValue::Text(text)] = property.values.as_slice() else {
                        return Err(SealError::Structure("timezone carrier"));
                    };
                    sealed_bundle(text)?;
                } else if !is_visible_property(&component.component_type, &property.name)
                    || property
                        .params
                        .iter()
                        .any(|parameter| !is_visible_parameter(&parameter.name))
                {
                    return Err(SealError::Structure("unsealed timezone property"));
                }
            }
        }
    }
    Ok(())
}

/// Assembles prepared sealed entries, with no key access, authorization or store write.
/// The caller verifies the decrypted ledger before calling this, then uses Calendar::update.
// Task 1 introduces this crate-private writer; conversion and mutations consume it in Tasks 6-7.
pub(crate) fn za_replace_calendar_entries(
    calendar: &mut Calendar,
    owner_id: u32,
    calendar_id: u32,
    entries: Vec<CalendarPreferences>,
    acls: Vec<AclGrant>,
) -> Result<(), SealError> {
    if entries.is_empty() || !calendar.dead_properties.0.is_empty() {
        return Err(SealError::Structure("calendar entries"));
    }
    let mut ids = HashSet::with_capacity(entries.len());
    let mut parts = Vec::with_capacity(entries.len());
    for entry in &entries {
        if !ids.insert(entry.account_id) || entry.description.is_some() || entry.color.is_some() {
            return Err(SealError::Structure("calendar preferences"));
        }
        sealed_timezone(entry)?;
        parts.push(
            entry
                .name
                .strip_prefix(COLLECTION_MARKER)
                .ok_or(SealError::NotSealed)?
                .split('|')
                .collect::<Vec<_>>(),
        );
    }
    let owner_index = entries
        .iter()
        .position(|entry| entry.account_id == owner_id)
        .ok_or(SealError::Structure("owner entry"))?;
    let owner_parts = &parts[owner_index];
    let owner_ring = STANDARD
        .decode(owner_parts[0])
        .map_err(|_| SealError::Decode)?;
    if owner_parts.len() == 2 {
        if entries.len() != 1
            || !acls.is_empty()
            || owner_ring.len() != 2 + WRAP_LEN
            || owner_ring[..2] != [POLICY_VERSION, 1]
        {
            return Err(SealError::Format);
        }
        sealed_bundle(owner_parts[1])?;
    } else {
        if owner_parts.len() != 5 {
            return Err(SealError::Format);
        }
        let header = parse_owner_header(&owner_ring)?;
        if header.calendar_id != calendar_id {
            return Err(SealError::Structure("ring calendar"));
        }
        bundle_envelope(owner_parts[1], calendar_id, &header.generations)?;
        sealed_bundle(owner_parts[2])?;
        bundle_envelope(owner_parts[3], calendar_id, &header.generations)?;
        sealed_bundle(owner_parts[4])?;
        let view = Bitmap::new().with_item(Acl::Read).with_item(Acl::ReadItems);
        let edit = view
            .with_item(Acl::Modify)
            .with_item(Acl::AddItems)
            .with_item(Acl::ModifyItems)
            .with_item(Acl::RemoveItems);
        let mut acl_ids = HashSet::with_capacity(acls.len());
        for acl in &acls {
            if acl.account_id == owner_id
                || !acl_ids.insert(acl.account_id)
                || !ids.contains(&acl.account_id)
                || (acl.grants != view && acl.grants != edit)
            {
                return Err(SealError::Structure("calendar ACLs"));
            }
        }
        if acl_ids.len() + 1 != ids.len() {
            return Err(SealError::Structure("calendar ACLs"));
        }
        for (index, entry) in entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.account_id != owner_id)
        {
            let parts = &parts[index];
            if parts.len() != 3 {
                return Err(SealError::Format);
            }
            let grant =
                parse_grant_header(&STANDARD.decode(parts[0]).map_err(|_| SealError::Decode)?)?;
            if grant.calendar_id != calendar_id
                || grant.grantee_id != entry.account_id
                || grant.domain_id != header.domain_id
                || grant.generations != header.generations
                || grant.rename_change_id != header.rename_change_id
                || (header.boundary_pending && !grant.boundary_pending)
            {
                return Err(SealError::Structure("grant header correspondence"));
            }
            bundle_envelope(parts[1], calendar_id, &header.generations)?;
            sealed_bundle(parts[2])?;
        }
    }
    calendar.preferences = entries;
    calendar.acls = acls;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault::keys::{Secret, generate_keypair};

    fn header() -> OwnerRingHeader {
        OwnerRingHeader {
            calendar_id: 23,
            domain_id: [0x41; 16],
            rename_change_id: 19,
            boundary_pending: false,
            generations: vec![1, 2],
        }
    }

    fn ring() -> ZaOwnerRing {
        ZaOwnerRing {
            version: 1,
            keys: vec![[0xAB; 32], [0xCD; 32]],
            converted: true,
            reverting: false,
            grants: vec![ZaGrant {
                account_id: 31,
                address: "alice@example.org".into(),
                public_key: [0xEF; 32],
                level: 1,
                granted_at: 1700000000,
                change_id: 17,
            }],
        }
    }

    fn grant_header() -> GrantRingHeader {
        GrantRingHeader {
            calendar_id: 23,
            domain_id: [0x41; 16],
            grantee_id: 31,
            grant_change_id: 17,
            rename_change_id: 19,
            boundary_pending: false,
            generations: vec![1, 2],
        }
    }

    #[test]
    fn owner_key_growth_wipes_decoded_storage_before_release() {
        let ewk = Secret::random();
        let sealed = seal_owner_ring(&header(), &ring(), 7, &ewk).unwrap();
        let (mut header, mut decoded) = open_owner_ring(&sealed, 7, 23, &ewk).unwrap();
        assert_eq!(decoded.keys.capacity(), decoded.keys.len());
        let old_pointer = decoded.keys.as_ptr();
        let mut observed = false;
        append_owner_key(
            &mut decoded.keys,
            &Secret::from_bytes([0x12; 32]),
            |retired| {
                observed = true;
                assert_eq!(retired.as_ptr(), old_pointer);
                assert_eq!(retired.len(), 2);
                assert!(
                    retired.iter().flatten().all(|byte| *byte == 0),
                    "retired live key storage must be wiped before release"
                );
            },
        );
        assert!(observed, "the retired allocation was inspected while owned");
        assert!(decoded.keys.as_ptr() != old_pointer);
        assert!(decoded.keys == [[0xAB; 32], [0xCD; 32], [0x12; 32]]);
        header.generations.push(header.next_generation().unwrap());
        assert_eq!(header.generations, [1, 2, 3]);
        let mut expected = ring();
        expected.keys = vec![[0xAB; 32], [0xCD; 32], [0x12; 32]];
        assert_eq!(decoded, expected);

        // Synthetic keys only: the established rkyv serializer is the compatibility oracle.
        let mut expected_archive = rkyv::to_bytes::<rkyv::rancor::Error>(&expected).unwrap();
        let sealed = seal_owner_ring(&header, &decoded, 7, &ewk).unwrap();
        let encoded_header = encode_owner_header(&header).unwrap();
        let plain = open_padded(
            &ewk,
            &owner_aad(7, &header, &encoded_header),
            &sealed[encoded_header.len()..],
        )
        .unwrap();
        assert!(plain.as_slice() == expected_archive.as_slice());
        expected_archive.as_mut_slice().zeroize();
        assert_eq!(
            open_owner_ring(&sealed, 7, 23, &ewk).unwrap(),
            (header, expected)
        );
    }

    #[test]
    fn owner_key_growth_wipes_spare_storage_before_unwind() {
        let mut owner = ring();
        // Allocate while empty; never reserve or grow a populated secret buffer.
        let mut spare = Zeroizing::new(Vec::with_capacity(8));
        spare.extend_from_slice(&owner.keys);
        std::mem::swap(&mut owner.keys, &mut spare);
        assert!(owner.keys.capacity() > owner.keys.len());
        let old_pointer = owner.keys.as_ptr();
        let mut observed = false;
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            append_owner_key(
                &mut owner.keys,
                &Secret::from_bytes([0x12; 32]),
                |retired| {
                    observed = true;
                    assert_eq!(retired.as_ptr(), old_pointer);
                    assert_eq!(retired.len(), 2);
                    assert!(
                        retired.iter().flatten().all(|byte| *byte == 0),
                        "retired live key storage must be wiped before unwind"
                    );
                    panic!("controlled pre-release unwind");
                },
            );
        }));
        assert!(unwound.is_err());
        assert!(observed);
        // The original allocation still belongs to the ring after the observer unwinds.
        assert_eq!(owner.keys.as_ptr(), old_pointer);
        assert_eq!(owner.keys.len(), 2);
        assert!(owner.keys.iter().flatten().all(|byte| *byte == 0));
    }

    #[test]
    fn owner_key_growth_preserves_keys_with_spare_capacity() {
        let mut owner = ring();
        let mut spare = Zeroizing::new(Vec::with_capacity(8));
        spare.extend_from_slice(&owner.keys);
        std::mem::swap(&mut owner.keys, &mut spare);
        assert!(owner.keys.capacity() > owner.keys.len());
        owner.append_key(&Secret::from_bytes([0x12; 32]));
        assert!(owner.keys == [[0xAB; 32], [0xCD; 32], [0x12; 32]]);
    }

    #[test]
    fn ring_round_trip_and_identity_binding() {
        let ewk = Secret::random();
        let header = header();
        let ring = ring();
        let bytes = seal_owner_ring(&header, &ring, 7, &ewk).unwrap();
        let (opened_header, opened) = open_owner_ring(&bytes, 7, 23, &ewk).unwrap();
        assert_eq!(opened_header, header);
        assert_eq!(opened, ring);
        assert!(open_owner_ring(&bytes, 8, 23, &ewk).is_err());
        assert!(open_owner_ring(&bytes, 7, 24, &ewk).is_err());
        assert!(open_owner_ring(&bytes, 7, 23, &Secret::random()).is_err());
        // Each visible header field is authenticated, including metadata and each generation.
        for index in [2, 6, 22, 30, 33, 37] {
            let mut changed = bytes.clone();
            changed[index] ^= 1;
            assert!(
                open_owner_ring(&changed, 7, 23, &ewk).is_err(),
                "offset {index}"
            );
        }
        assert_eq!(format!("{ring:?}"), "ZaOwnerRing(..)");
        assert_eq!(format!("{:?}", ring.grants[0]), "ZaGrant(..)");
        let header = grant_header();
        let (public, private) = generate_keypair();
        let key = Secret::from_bytes([0xAB; 32]);
        let keys = BTreeMap::from([(1, key.clone()), (2, Secret::from_bytes([0xCD; 32]))]);
        let bytes = seal_grant_ring(&header, &keys, 7, &public).unwrap();
        let (opened_header, opened_keys) =
            open_grant_ring(&bytes, 7, 23, 31, [0x41; 16], &private).unwrap();
        assert_eq!(opened_header, header);
        assert_eq!(opened_keys[&1].as_bytes(), key.as_bytes());
        for (owner, calendar, principal, domain) in [
            (8, 23, 31, [0x41; 16]),
            (7, 24, 31, [0x41; 16]),
            (7, 23, 32, [0x41; 16]),
            (7, 23, 31, [0x42; 16]),
        ] {
            assert!(open_grant_ring(&bytes, owner, calendar, principal, domain, &private).is_err());
        }
        assert!(open_grant_ring(&bytes, 7, 23, 31, [0x41; 16], &Secret::random()).is_err());
        for index in [2, 6, 22, 47, 155] {
            let mut changed = bytes.clone();
            changed[index] ^= 1;
            assert!(open_grant_ring(&changed, 7, 23, 31, [0x41; 16], &private).is_err());
        }
        // A header cannot silently drop or add a wrapped generation.
        assert!(seal_grant_ring(&header, &BTreeMap::from([(1, key)]), 7, &public).is_err());
    }

    #[test]
    fn ring_headers_reject_malformed_input() {
        let ewk = Secret::random();
        let owner = seal_owner_ring(&header(), &ring(), 7, &ewk).unwrap();
        let (public, private) = generate_keypair();
        let grant = seal_grant_ring(
            &grant_header(),
            &BTreeMap::from([(1, Secret::random()), (2, Secret::random())]),
            7,
            &public,
        )
        .unwrap();
        assert_eq!(parse_owner_header(&owner).unwrap(), header());
        assert_eq!(parse_grant_header(&grant).unwrap(), grant_header());
        for end in 0..owner.len() {
            assert!(
                parse_owner_header(&owner[..end]).is_err(),
                "owner truncation {end}"
            );
        }
        for end in 0..grant.len() {
            assert!(
                parse_grant_header(&grant[..end]).is_err(),
                "grant truncation {end}"
            );
        }
        for (bytes, owner_ring, flag, count, generation, next) in [
            (owner, true, 30, 31, 33, 37),
            (grant, false, 42, 43, 45, 153),
        ] {
            for (offset, value) in [(0, 2), (1, 99), (flag, 2), (count, 255), (count + 1, 0)] {
                let mut changed = bytes.clone();
                changed[offset] = value;
                assert!(if owner_ring {
                    parse_owner_header(&changed).is_err()
                } else {
                    parse_grant_header(&changed).is_err()
                });
            }
            for bad in [0u32, 1] {
                let mut changed = bytes.clone();
                changed[next..next + 4].copy_from_slice(&bad.to_be_bytes());
                assert!(if owner_ring {
                    parse_owner_header(&changed).is_err()
                } else {
                    parse_grant_header(&changed).is_err()
                });
            }
            let mut changed = bytes.clone();
            changed[generation..generation + 4].copy_from_slice(&3u32.to_be_bytes());
            assert!(if owner_ring {
                parse_owner_header(&changed).is_err()
            } else {
                parse_grant_header(&changed).is_err()
            });
            let mut appended = bytes;
            appended.push(0);
            assert!(if owner_ring {
                parse_owner_header(&appended).is_err()
            } else {
                parse_grant_header(&appended).is_err()
            });
        }
        let mut bad = ring();
        bad.keys.pop();
        assert!(seal_owner_ring(&header(), &bad, 7, &ewk).is_err());
        let mut bad = ring();
        bad.version = 2;
        assert!(seal_owner_ring(&header(), &bad, 7, &ewk).is_err());
        for bad_generations in [vec![], vec![0], vec![2, 1], vec![1, 1], vec![1; 65536]] {
            let mut bad = header();
            bad.generations = bad_generations;
            assert!(seal_owner_ring(&bad, &ring(), 7, &ewk).is_err());
        }
        // Construct an authenticated payload that disagrees with its header.
        let mut bad = ring();
        bad.keys.pop();
        let raw_header = encode_owner_header(&header()).unwrap();
        let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&bad).unwrap();
        let mut bytes = raw_header.clone();
        bytes.extend_from_slice(&seal_padded(
            &ewk,
            &owner_aad(7, &header(), &raw_header),
            &plain,
        ));
        assert!(open_owner_ring(&bytes, 7, 23, &ewk).is_err());
        let _ = private;
    }
    #[test]
    fn owner_ring_secret_material_zeroizes() {
        let mut owner = ring();
        owner.zeroize();
        assert!(owner.keys.is_empty());
        assert!(owner.grants.is_empty());
        assert_eq!(owner.version, 0);
        assert!(!owner.converted);
        assert!(!owner.reverting);
        let mut grant = ring().grants[0].clone();
        grant.zeroize();
        assert!(grant.address.is_empty());
        assert_eq!(grant.public_key, [0; 32]);
        assert_eq!(grant.account_id, 0);
        assert_eq!(grant.level, 0);
        assert_eq!(grant.granted_at, 0);
        assert_eq!(grant.change_id, 0);
    }

    #[test]
    fn next_generation_refuses_exhaustion() {
        assert_eq!(header().next_generation().unwrap(), 3);
        let mut last = header();
        last.generations = vec![u32::MAX];
        assert!(last.next_generation().is_err());
        let owner = {
            let mut owner = ring();
            owner.keys = vec![[1; 32]];
            owner
        };
        let ewk = Secret::random();
        let bytes = seal_owner_ring(&last, &owner, 7, &ewk).unwrap();
        assert_eq!(
            open_owner_ring(&bytes, 7, 23, &ewk).unwrap().0.generations,
            vec![u32::MAX]
        );
    }

    fn bundle() -> String {
        // Independently shaped format-1 padded bundle; assembly does not authenticate it.
        STANDARD.encode([&[1u8][..], &[0u8; 296][..]].concat())
    }

    fn envelope(generation: u32) -> String {
        let mut bytes = vec![1, 3];
        bytes.extend_from_slice(&23u32.to_be_bytes());
        bytes.extend_from_slice(&generation.to_be_bytes());
        bytes.extend_from_slice(&[0; 72]);
        STANDARD.encode(bytes)
    }

    fn prepared_entries() -> Vec<CalendarPreferences> {
        let ewk = Secret::random();
        let (public, _) = generate_keypair();
        let owner = seal_owner_ring(&header(), &ring(), 7, &ewk).unwrap();
        let grant = seal_grant_ring(
            &grant_header(),
            &BTreeMap::from([(1, Secret::random()), (2, Secret::random())]),
            7,
            &public,
        )
        .unwrap();
        vec![
            CalendarPreferences {
                account_id: 7,
                name: format!(
                    "$za${}|{}|{}|{}|{}",
                    STANDARD.encode(owner),
                    envelope(2),
                    bundle(),
                    envelope(2),
                    bundle()
                ),
                ..Default::default()
            },
            CalendarPreferences {
                account_id: 31,
                name: format!(
                    "$za${}|{}|{}",
                    STANDARD.encode(grant),
                    envelope(2),
                    bundle()
                ),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn prepared_entries_require_shapes_identities_and_matching_acls() {
        let entries = prepared_entries();
        let acls = vec![AclGrant {
            account_id: 31,
            grants: Bitmap::new().with_item(Acl::Read).with_item(Acl::ReadItems),
        }];
        let mut calendar = Calendar::default();
        za_replace_calendar_entries(&mut calendar, 7, 23, entries.clone(), acls.clone()).unwrap();
        assert_eq!(calendar.preferences, entries);
        assert_eq!(calendar.acls, acls);
        let before = calendar.clone();
        let mut cases = vec![
            (vec![], acls.clone()),
            (entries.clone(), vec![]),
            (entries.clone(), [acls.clone(), acls.clone()].concat()),
        ];
        for (index, name) in [
            (
                0,
                entries[0]
                    .name
                    .split('|')
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
            (1, format!("{}|{}", entries[1].name, bundle())),
            (1, "$za$AQ==|AQ==|AQ==".into()),
        ] {
            let mut changed = entries.clone();
            changed[index].name = name;
            cases.push((changed, acls.clone()));
        }
        let mut wrong = entries.clone();
        wrong[1].account_id = 32;
        cases.push((wrong, acls.clone()));
        let mut duplicate = entries.clone();
        duplicate.push(entries[1].clone());
        cases.push((duplicate, acls.clone()));
        let mut visible = entries.clone();
        visible[1].description = Some("secret".into());
        cases.push((visible, acls.clone()));
        let mut wrong_acl = acls.clone();
        wrong_acl[0].grants.insert(Acl::Share);
        cases.push((entries.clone(), wrong_acl));
        for (entries, acls) in cases {
            assert!(za_replace_calendar_entries(&mut calendar, 7, 23, entries, acls).is_err());
            assert_eq!(calendar, before, "failed validation is atomic");
        }
        assert!(za_replace_calendar_entries(&mut calendar, 7, 24, entries, acls).is_err());
        let mut unshared = Calendar {
            preferences: vec![CalendarPreferences {
                account_id: 7,
                name: "plain".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let session = vault::session::SessionKeys::new(7, 1, Secret::random());
        crate::calendar::seal::seal_calendar(&mut unshared, &session, 7).unwrap();
        za_replace_calendar_entries(&mut calendar, 7, 23, unshared.preferences.clone(), vec![])
            .unwrap();
        assert_eq!(calendar.preferences, unshared.preferences);
    }

    #[test]
    fn rings_interoperate_with_literal_aad_vectors() {
        let ewk = Secret::random();
        let header = header();
        let owner = seal_owner_ring(&header, &ring(), 7, &ewk).unwrap();
        let raw_header =
            b"\x01\x04\0\0\0\x17AAAAAAAAAAAAAAAA\0\0\0\0\0\0\0\x13\0\0\x02\0\0\0\x01\0\0\0\x02";
        assert_eq!(&owner[..41], raw_header);
        let mut literal_owner_aad =
            b"za/v1|ck-owner|\0\0\0\x07|\0\0\0\x17|AAAAAAAAAAAAAAAA|".to_vec();
        literal_owner_aad.extend_from_slice(raw_header);
        let opened = open_padded(&ewk, &literal_owner_aad, &owner[41..]).unwrap();
        assert_eq!(decode_archive::<ZaOwnerRing>(&opened).unwrap(), ring());
        let (public, private) = generate_keypair();
        let key = Secret::random();
        let mut header = grant_header();
        header.generations = vec![1];
        let grant =
            seal_grant_ring(&header, &BTreeMap::from([(1, key.clone())]), 7, &public).unwrap();
        let literal_grant_aad =
            b"za/v1|ck-grant|\0\0\0\x07|\0\0\0\x17|AAAAAAAAAAAAAAAA|\0\0\0\x1f|\0\0\0\x01";
        let ephemeral: [u8; 32] = grant[49..81].try_into().unwrap();
        assert_eq!(
            pk_unwrap(&ephemeral, &grant[81..], &private, literal_grant_aad)
                .unwrap()
                .as_bytes(),
            key.as_bytes()
        );
    }

    #[test]
    fn authenticated_invalid_payloads_and_ledgers_are_refused() {
        let ewk = Secret::random();
        let header = header();
        let mut invalid = vec![];
        let mut bad = ring();
        bad.version = 2;
        invalid.push(bad);
        let mut bad = ring();
        bad.keys.pop();
        invalid.push(bad);
        let mut bad = ring();
        bad.grants.push(bad.grants[0].clone());
        invalid.push(bad);
        let mut bad = ring();
        bad.grants[0].account_id = 7;
        invalid.push(bad);
        let mut bad = ring();
        bad.grants[0].level = 3;
        invalid.push(bad);
        let mut bad = ring();
        bad.grants[0].address = "Alice@EXAMPLE.ORG".into();
        invalid.push(bad);
        for ring in invalid {
            assert!(seal_owner_ring(&header, &ring, 7, &ewk).is_err());
            let plain = rkyv::to_bytes::<rkyv::rancor::Error>(&ring).unwrap();
            let mut bytes = encode_owner_header(&header).unwrap();
            let sealed = seal_padded(&ewk, &owner_aad(7, &header, &bytes), &plain);
            bytes.extend_from_slice(&sealed);
            assert!(open_owner_ring(&bytes, 7, 23, &ewk).is_err());
        }
        let mut bytes = encode_owner_header(&header).unwrap();
        let sealed = seal_padded(&ewk, &owner_aad(7, &header, &bytes), &[0xff; 11]);
        bytes.extend_from_slice(&sealed);
        assert!(open_owner_ring(&bytes, 7, 23, &ewk).is_err());
        let mut large = ring();
        for account_id in 32..40 {
            let mut grant = large.grants[0].clone();
            grant.account_id = account_id;
            large.grants.push(grant);
        }
        let bytes = seal_owner_ring(&header, &large, 7, &ewk).unwrap();
        for end in 0..bytes.len() {
            assert!(
                open_owner_ring(&bytes[..end], 7, 23, &ewk).is_err(),
                "truncation {end}"
            );
        }
    }

    #[test]
    fn prepared_custom_timezones_refuse_plaintext_and_misplaced_carriers() {
        use calcard::{Entry, Parser};
        let Entry::ICalendar(tz) = Parser::new("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:Etc/Test\r\nCOMMENT:timezone secret\r\nEND:VTIMEZONE\r\nEND:VCALENDAR\r\n").entry() else { panic!() };
        let mut entries = prepared_entries();
        entries[0].time_zone = crate::calendar::Timezone::Custom(tz.clone());
        let acls = vec![AclGrant {
            account_id: 31,
            grants: Bitmap::new().with_item(Acl::Read).with_item(Acl::ReadItems),
        }];
        let mut calendar = Calendar::default();
        assert!(
            za_replace_calendar_entries(&mut calendar, 7, 23, entries.clone(), acls.clone())
                .is_err()
        );
        let mut sealed_tz = tz;
        crate::calendar::seal::tree::seal_tree(&mut sealed_tz, &Secret::random(), 7, "calendar-tz");
        entries[0].time_zone = crate::calendar::Timezone::Custom(sealed_tz.clone());
        za_replace_calendar_entries(&mut calendar, 7, 23, entries.clone(), acls.clone()).unwrap();
        let component = sealed_tz
            .components
            .iter_mut()
            .find(|c| {
                c.entries
                    .iter()
                    .any(|e| crate::calendar::seal::tree::is_carrier(e, "X-ZA-SEALED"))
            })
            .unwrap();
        component
            .entries
            .push(crate::calendar::seal::tree::text_entry(
                "X-ZA-EXTRA",
                bundle(),
            ));
        entries[0].time_zone = crate::calendar::Timezone::Custom(sealed_tz);
        assert!(za_replace_calendar_entries(&mut calendar, 7, 23, entries, acls).is_err());
    }

    #[test]
    fn owner_ring_writer_is_fixed_and_wipes_after_success() {
        let ring = ring();
        let mut storage = vec![0x5A; 4096];
        let pointer = storage.as_ptr();
        {
            let mut writer = OwnerRingWriter {
                bytes: &mut storage,
                position: 0,
            };
            rkyv::api::high::to_bytes_in::<_, rkyv::rancor::Error>(&ring, &mut writer).unwrap();
            assert_eq!(
                writer.bytes.as_ptr(),
                pointer,
                "serialization cannot relocate its buffer"
            );
            assert_eq!(
                writer.bytes.len(),
                4096,
                "serialization cannot grow its buffer"
            );
            assert!(
                writer.bytes[..writer.position]
                    .iter()
                    .any(|byte| *byte != 0)
            );
            assert_eq!(
                decode_archive::<ZaOwnerRing>(&writer.bytes[..writer.position]).unwrap(),
                ring
            );
        }
        assert!(
            storage.iter().all(|byte| *byte == 0),
            "the live backing storage must be wiped before release"
        );
    }

    #[test]
    fn owner_ring_writer_refuses_growth_and_wipes_after_error() {
        let ring = ring();
        // Both keys fit, then the first ledger address exceeds the remaining space.
        let mut storage = [0; 64];
        let pointer = storage.as_ptr();
        {
            let mut writer = OwnerRingWriter {
                bytes: &mut storage,
                position: 0,
            };
            assert!(
                rkyv::api::high::to_bytes_in::<_, rkyv::rancor::Error>(&ring, &mut writer).is_err()
            );
            assert_eq!(writer.bytes.as_ptr(), pointer);
            assert_eq!(
                writer.position, 64,
                "the key bytes were written before overflow"
            );
            assert!(writer.bytes.iter().any(|byte| *byte != 0));
        }
        assert!(
            storage.iter().all(|byte| *byte == 0),
            "an error must also wipe the live backing storage"
        );
    }

    #[test]
    fn owner_ring_large_ledger_preserves_archive_and_wire_bytes() {
        let mut ring = ring();
        ring.grants.clear();
        for account_id in 31..159 {
            ring.grants.push(ZaGrant {
                account_id,
                address: format!("member{account_id}{}@example.org", "a".repeat(40)),
                public_key: [0xEF; 32],
                level: 1,
                granted_at: 1700000000,
                change_id: 17,
            });
        }
        let ewk = Secret::random();
        let header = header();
        let sealed = seal_owner_ring(&header, &ring, 7, &ewk).unwrap();
        let encoded_header = encode_owner_header(&header).unwrap();
        let plain = open_padded(
            &ewk,
            &owner_aad(7, &header, &encoded_header),
            &sealed[encoded_header.len()..],
        )
        .unwrap();
        // The old writer is used only as an interoperability oracle for fixed,
        // synthetic test key bytes; it is never used with production keys.
        let mut old_archive = rkyv::to_bytes::<rkyv::rancor::Error>(&ring).unwrap();
        assert!(
            old_archive.len() > 8192,
            "exercises repeated growth in the old serializer"
        );
        assert_eq!(
            plain.as_slice(),
            old_archive.as_slice(),
            "the rkyv payload remains identical"
        );
        old_archive.as_mut_slice().zeroize();
        assert_eq!(
            open_owner_ring(&sealed, 7, 23, &ewk).unwrap(),
            (header, ring)
        );
    }
}
