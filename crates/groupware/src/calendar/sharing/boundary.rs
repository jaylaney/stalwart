/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Explicit owner-session boundary writes. Resolution itself is always read-only.
use super::{
    format::{
        open_owner_ring, parse_grant_header, rewrite_grant_boundary, seal_owner_ring,
        za_replace_calendar_entries,
    },
    resolve::{
        ZaCalendarContext, ZaCalendarError, resolve_owner_stored, za_owned_calendar_context,
    },
};
use crate::calendar::{
    Calendar,
    seal::{COLLECTION_MARKER, SealError, ZaKeyDomain},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::Server;
use store::write::{AlignedBytes, Archive, BatchBuilder};
use types::collection::SyncCollection;
use vault::session::SessionKeys;

pub struct ZaBoundaryCommit {
    pub change_id: u64,
}

/// Mark the owner's slug change and all grant headers in the rename's transaction.
pub fn mark_boundary_pending(
    calendar: &mut Calendar,
    domain: &ZaKeyDomain,
) -> Result<(), SealError> {
    rewrite(calendar, domain, None)
}

pub fn complete_boundary(
    calendar: &mut Calendar,
    domain: &ZaKeyDomain,
    change_id: u64,
) -> Result<(), SealError> {
    rewrite(calendar, domain, Some(change_id))
}

fn rewrite(
    calendar: &mut Calendar,
    domain: &ZaKeyDomain,
    completion: Option<u64>,
) -> Result<(), SealError> {
    let ewk = domain.ewk()?;
    let calendar_id = domain.calendar_id().ok_or(SealError::NotSealed)?;
    let owner_id = domain.owner_id();
    // Validate the complete entry set before touching either visible metadata or sealed AAD.
    za_replace_calendar_entries(
        calendar,
        owner_id,
        calendar_id,
        calendar.preferences.clone(),
        calendar.acls.clone(),
    )?;
    let mut entries = calendar.preferences.clone();
    let owner_index = entries
        .iter()
        .position(|e| e.account_id == owner_id)
        .ok_or(SealError::Format)?;
    let mut owner_parts = entries[owner_index]
        .name
        .strip_prefix(COLLECTION_MARKER)
        .ok_or(SealError::NotSealed)?
        .split('|')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (mut owner_header, mut ring) = open_owner_ring(
        &STANDARD
            .decode(&owner_parts[0])
            .map_err(|_| SealError::Decode)?,
        owner_id,
        calendar_id,
        ewk,
    )?;
    if Some(&owner_header.domain_id) != domain.domain_id() {
        return Err(SealError::Structure("ring domain"));
    }
    let rename_pending = owner_header.boundary_pending;
    for entry in entries.iter_mut().filter(|e| e.account_id != owner_id) {
        let mut parts = entry
            .name
            .strip_prefix(COLLECTION_MARKER)
            .ok_or(SealError::NotSealed)?
            .split('|')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let bytes = STANDARD.decode(&parts[0]).map_err(|_| SealError::Decode)?;
        let mut header = parse_grant_header(&bytes)?;
        if let Some(id) = completion {
            if header.boundary_pending && header.grant_change_id == 0 {
                header.grant_change_id = id;
                let grant = ring
                    .grants
                    .iter_mut()
                    .find(|g| g.account_id == entry.account_id)
                    .ok_or(SealError::Structure("grant ledger"))?;
                if grant.change_id != 0 {
                    return Err(SealError::Structure("grant boundary ledger"));
                }
                grant.change_id = id;
            }
            if rename_pending {
                header.rename_change_id = id;
            }
            header.boundary_pending = false;
        } else {
            header.boundary_pending = true;
        }
        parts[0] = STANDARD.encode(rewrite_grant_boundary(
            &bytes,
            header.grant_change_id,
            header.rename_change_id,
            header.boundary_pending,
        )?);
        entry.name = format!("{COLLECTION_MARKER}{}", parts.join("|"));
    }
    if let Some(id) = completion {
        if rename_pending {
            owner_header.rename_change_id = id;
        }
        owner_header.boundary_pending = false;
    } else {
        owner_header.boundary_pending = true;
    }
    owner_parts[0] = STANDARD.encode(seal_owner_ring(&owner_header, &ring, owner_id, ewk)?);
    entries[owner_index].name = format!("{COLLECTION_MARKER}{}", owner_parts.join("|"));
    za_replace_calendar_entries(
        calendar,
        owner_id,
        calendar_id,
        entries,
        calendar.acls.clone(),
    )
}

/// Complete using an own-commit id only for that exact archive. On its first CAS loss,
/// reread the archive *then* the account log and make one conservative fallback attempt.
pub async fn za_complete_boundary(
    server: &Server,
    keys: &SessionKeys,
    calendar_id: u32,
    own_commit: Option<ZaBoundaryCommit>,
) -> Result<Archive<AlignedBytes>, ZaCalendarError> {
    let attempts = if own_commit.is_some() { 2 } else { 1 };
    for attempt in 0..attempts {
        let context = za_owned_calendar_context(server, keys, calendar_id).await?;
        if !context.boundary.is_some_and(|b| b.pending) {
            return Ok(context.stored);
        }
        let id = if attempt == 0
            && own_commit
                .as_ref()
                .is_some_and(|own| context.stored.version.change_id() == Some(own.change_id))
        {
            own_commit.as_ref().unwrap().change_id
        } else {
            server
                .store()
                .get_last_change_id(keys.account_id, SyncCollection::Calendar.into())
                .await?
                .unwrap_or_default()
        };
        let mut calendar = context.stored.deserialize::<Calendar>()?;
        complete_boundary(&mut calendar, &context.domain, id)?;
        let account = server.account_info(keys.account_id).await?;
        let mut batch = BatchBuilder::new();
        calendar.update(
            account.account_tenant_ids(),
            context.stored.to_unarchived::<Calendar>()?,
            keys.account_id,
            calendar_id,
            &mut batch,
        )?;
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::share_boundary_pause_point(keys.account_id).await;
        match server.commit_batch(batch).await {
            Ok(_) => {
                return Ok(za_owned_calendar_context(server, keys, calendar_id)
                    .await?
                    .stored);
            }
            Err(error) if error.is_assertion_failure() => {
                if attempt + 1 == attempts {
                    return Ok(context.stored);
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!()
}

/// Explicit adapter for already authorized contexts. This never manufactures another
/// owner's keys, and resolves the repair-returned archive before a caller uses its fence.
pub async fn za_repair_owned_context(
    server: &Server,
    keys: &SessionKeys,
    context: ZaCalendarContext,
) -> Result<ZaCalendarContext, ZaCalendarError> {
    if keys.account_id == context.owner_id
        && context.personal_id == context.owner_id
        && context.boundary.is_some_and(|b| b.pending)
    {
        let stored = za_complete_boundary(server, keys, context.calendar_id, None).await?;
        resolve_owner_stored(stored, keys, context.calendar_id)
    } else {
        Ok(context)
    }
}

pub async fn za_repair_owned_contexts(
    server: &Server,
    keys: &SessionKeys,
    contexts: Vec<ZaCalendarContext>,
) -> Result<Vec<ZaCalendarContext>, ZaCalendarError> {
    let mut repaired = Vec::with_capacity(contexts.len());
    for context in contexts {
        repaired.push(za_repair_owned_context(server, keys, context).await?);
    }
    Ok(repaired)
}
