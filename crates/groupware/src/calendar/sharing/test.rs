/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Disposable fixture preparation only: never writes to storage.

use super::format::{open_owner_ring, za_replace_calendar_entries};
use crate::calendar::{
    Calendar,
    seal::{
        SealError, ZaKeyDomain,
        collection::{seal_personal_entry, seal_shared_properties},
    },
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{LazyLock, Mutex, PoisonError},
};
use vault::{keys::Secret, session::SessionKeys};

/// Prepare sealed entries from an authenticated owner's ring and already prepared
/// grantee rings. The integration harness owns persistence and fixture cleanup.
pub fn prepare_shared_calendar(
    keys: &SessionKeys,
    calendar_id: u32,
    mut calendar: Calendar,
    owner_ring: &[u8],
    grant_rings: &BTreeMap<u32, Vec<u8>>,
) -> Result<Calendar, SealError> {
    let (header, ring) = open_owner_ring(owner_ring, keys.account_id, calendar_id, keys.ewk())?;
    let collection_keys = header
        .generations
        .iter()
        .copied()
        .zip(ring.keys.iter().copied().map(Secret::from_bytes))
        .collect();
    let domain = ZaKeyDomain::shared(
        keys.account_id,
        keys.account_id,
        Some(keys.ewk().clone()),
        calendar_id,
        header.domain_id,
        collection_keys,
        ring.reverting,
    )?;
    let (envelope, bundle) = seal_shared_properties(&calendar.dead_properties, &domain)?;
    let entries = calendar
        .preferences
        .iter()
        .map(|entry| {
            let encoded_ring = if entry.account_id == keys.account_id {
                owner_ring
            } else {
                grant_rings
                    .get(&entry.account_id)
                    .ok_or(SealError::Structure("fixture grant ring"))?
                    .as_slice()
            };
            let mut sealed = seal_personal_entry(entry, &domain, encoded_ring)?;
            if entry.account_id == keys.account_id {
                sealed.name.push_str(&format!("|{envelope}|{bundle}"));
            }
            Ok(sealed)
        })
        .collect::<Result<Vec<_>, SealError>>()?;
    calendar.dead_properties = Default::default();
    let acls = calendar.acls.clone();
    za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls)?;
    Ok(calendar)
}

static READS: LazyLock<Mutex<HashMap<(u32, u32), u64>>> = LazyLock::new(Default::default);

pub(super) fn record_read(owner_id: u32, calendar_id: u32) {
    *READS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry((owner_id, calendar_id))
        .or_default() += 1;
}

/// Counts actual resolver calendar archive reads for a dedicated fixture.
pub fn calendar_reads(owner_id: u32, calendar_id: u32) -> u64 {
    READS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&(owner_id, calendar_id))
        .copied()
        .unwrap_or(0)
}
