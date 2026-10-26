/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! One authoritative sharing decision and commit. HTTP owns retries and credentials.
use super::{
    boundary::{ZaBoundaryCommit, complete_boundary},
    convert::{ZaShareError, ZaShareGuards, event_ids, owner_ring, validate_scan},
    format::{
        GrantRingHeader, ZaGrant, seal_grant_ring, seal_owner_ring, za_replace_calendar_entries,
    },
    resolve::za_owned_calendar_context,
};
use crate::calendar::{
    Calendar,
    seal::{
        COLLECTION_MARKER, SealError, calendar_view_in_domain, collection::seal_personal_entry,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::Server;
use registry::{
    schema::{
        prelude::{ObjectInner, ObjectType},
        structs::Account,
    },
    types::id::ObjectId,
};
use std::collections::{BTreeMap, BTreeSet};
use store::write::{BatchBuilder, now};
use types::{
    acl::{Acl, AclGrant},
    collection::SyncCollection,
    id::Id,
};
use utils::map::bitmap::Bitmap;
use vault::{keys::Secret, session::SessionKeys};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZaShareLevel {
    View,
    Edit,
}
impl ZaShareLevel {
    pub fn bitmap(self) -> Bitmap<Acl> {
        let view = Bitmap::new().with_item(Acl::Read).with_item(Acl::ReadItems);
        match self {
            Self::View => view,
            Self::Edit => view
                .with_item(Acl::Modify)
                .with_item(Acl::AddItems)
                .with_item(Acl::ModifyItems)
                .with_item(Acl::RemoveItems),
        }
    }
    pub fn from_bitmap(bits: Bitmap<Acl>) -> Option<Self> {
        [Self::View, Self::Edit]
            .into_iter()
            .find(|level| level.bitmap() == bits)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Edit => "edit",
        }
    }
    fn code(self) -> u8 {
        match self {
            Self::View => 1,
            Self::Edit => 2,
        }
    }
    fn from_code(code: u8) -> Result<Self, SealError> {
        match code {
            1 => Ok(Self::View),
            2 => Ok(Self::Edit),
            _ => Err(SealError::Format),
        }
    }
}
pub enum ZaRevokeTarget {
    Grant(u32),
    Address(String),
}
pub enum ZaShareChange {
    Grant {
        account_id: u32,
        address: String,
        public_key: [u8; 32],
        level: ZaShareLevel,
    },
    Revoke {
        target: ZaRevokeTarget,
    },
}
pub struct ZaSharePrecheck {
    pub needs_conversion: bool,
}
pub struct ZaShareApplied {
    pub changed: bool,
    pub affected_grantees: Vec<u32>,
    pub rotated: bool,
    pub boundary_commit: Option<ZaBoundaryCommit>,
}

/// Fresh registry classification; a stale directory cache cannot keep a dead grant alive.
pub async fn za_share_key_account(server: &Server, account_id: u32) -> trc::Result<bool> {
    Ok(server.registry().get(ObjectId::new(ObjectType::Account, Id::from(account_id))).await?
        .is_some_and(|object| matches!(object.inner, ObjectInner::Account(Account::User(user))
            if user.password_credential().is_some_and(|credential| credential.secret == vault::ZA_MARKER))))
}

pub async fn za_share_precheck(
    server: &Server,
    keys: &SessionKeys,
    calendar_id: u32,
    change: &ZaShareChange,
) -> Result<ZaSharePrecheck, ZaShareError> {
    let context = za_owned_calendar_context(server, keys, calendar_id).await?;
    let ring = owner_ring(&context, keys)?;
    let needs_conversion = matches!(change, ZaShareChange::Grant { .. })
        && ring
            .as_ref()
            .is_none_or(|(_, ring)| !ring.converted || ring.reverting);
    if let ZaShareChange::Grant { account_id, .. } = change {
        if *account_id == keys.account_id {
            return Err(ZaShareError::Forbidden);
        }
        if ring
            .as_ref()
            .is_none_or(|(_, r)| !r.grants.iter().any(|g| g.account_id == *account_id))
            && ring.as_ref().map_or(0, |(_, r)| r.grants.len())
                >= server.core.groupware.max_shares_per_item
        {
            return Err(ZaShareError::TooManyShares);
        }
        // A reverting journal must finish rollback before another forward decision.
        if needs_conversion
            && ring.is_none()
            && event_ids(server, keys.account_id, calendar_id).await?.1
        {
            return Err(ZaShareError::LinkedEvents);
        }
    }
    Ok(ZaSharePrecheck { needs_conversion })
}

pub async fn za_share_apply(
    server: &Server,
    keys: &SessionKeys,
    calendar_id: u32,
    change: ZaShareChange,
    guards: &ZaShareGuards,
) -> Result<ZaShareApplied, ZaShareError> {
    if guards.owner_id != keys.account_id || guards.verified_generation != keys.generation {
        return Err(ZaShareError::Forbidden);
    }
    let context = za_owned_calendar_context(server, keys, calendar_id).await?;
    let mut calendar = context.stored.deserialize::<Calendar>()?;
    let mut applied = ZaShareApplied {
        changed: false,
        affected_grantees: Vec::new(),
        rotated: false,
        boundary_commit: None,
    };
    let Some((_, _)) = owner_ring(&context, keys)? else {
        return if matches!(change, ZaShareChange::Revoke { .. }) {
            Ok(applied)
        } else {
            Err(ZaShareError::NotReady)
        };
    };
    if context.boundary.is_some_and(|b| b.pending) {
        // This log read follows the archive that observed the pending flags.
        let last = server
            .store()
            .get_last_change_id(keys.account_id, SyncCollection::Calendar.into())
            .await?
            .unwrap_or_default();
        complete_boundary(&mut calendar, &context.domain, last)?;
        applied.changed = true;
        applied
            .affected_grantees
            .extend(calendar.acls.iter().map(|g| g.account_id));
    }
    let owner_index = calendar
        .preferences
        .iter()
        .position(|p| p.account_id == keys.account_id)
        .ok_or(SealError::Format)?;
    let mut parts = calendar.preferences[owner_index]
        .name
        .strip_prefix(COLLECTION_MARKER)
        .ok_or(SealError::NotSealed)?
        .split('|')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let (mut header, mut ring) = super::format::open_owner_ring(
        &STANDARD.decode(&parts[0]).map_err(|_| SealError::Decode)?,
        keys.account_id,
        calendar_id,
        keys.ewk(),
    )?;
    if (!ring.converted || ring.reverting)
        && (!ring.grants.is_empty() || !calendar.acls.is_empty() || calendar.preferences.len() != 1)
    {
        return Err(SealError::Structure("conversion journal grants").into());
    }
    if ring.reverting && matches!(change, ZaShareChange::Grant { .. }) {
        return Err(ZaShareError::NotReady);
    }
    // Authenticate ledger/ACL correspondence before using any identity or recorded key.
    if ring.grants.len() != calendar.acls.len()
        || ring.grants.iter().any(|g| {
            !calendar.acls.iter().any(|a| {
                a.account_id == g.account_id
                    && ZaShareLevel::from_code(g.level)
                        .is_ok_and(|level| level.bitmap() == a.grants)
            })
        })
    {
        return Err(SealError::Structure("grant ledger correspondence").into());
    }
    // Enforce the current recorded ledger bound before any successful sweep.
    // A prior HTTP precheck may have observed an older calendar archive.
    if let ZaShareChange::Grant { account_id, .. } = &change
        && !ring
            .grants
            .iter()
            .any(|grant| grant.account_id == *account_id)
        && ring.grants.len() >= server.core.groupware.max_shares_per_item
    {
        return Err(ZaShareError::TooManyShares);
    }
    let revoke = if let ZaShareChange::Revoke { target } = &change {
        match target {
            ZaRevokeTarget::Grant(id) => Some(*id),
            ZaRevokeTarget::Address(address) => {
                let mut candidates = ring
                    .grants
                    .iter()
                    .filter(|g| g.address == *address)
                    .map(|g| g.account_id)
                    .collect::<BTreeSet<_>>();
                if let Some(id) = server
                    .za_account_id_from_email(address)
                    .await?
                    .filter(|id| ring.grants.iter().any(|g| g.account_id == *id))
                {
                    candidates.insert(id);
                }
                if candidates.len() > 1 {
                    return Err(ZaShareError::AmbiguousAddress);
                }
                candidates.first().copied()
            }
        }
    } else {
        None
    };
    let mut removed = Vec::new();
    for grant in &ring.grants {
        if Some(grant.account_id) == revoke
            || !za_share_key_account(server, grant.account_id).await?
        {
            removed.push(grant.account_id);
        }
    }
    if !removed.is_empty() {
        ring.grants.retain(|g| !removed.contains(&g.account_id));
        calendar
            .preferences
            .retain(|p| !removed.contains(&p.account_id));
        applied.affected_grantees.extend_from_slice(&removed);
        applied.changed = true;
        applied.rotated = true;
    }
    let mut new_entry = None;
    let mut new_grant = None;
    if let ZaShareChange::Grant {
        account_id,
        address,
        public_key,
        level,
    } = change
    {
        if account_id == keys.account_id || guards.pin_assertion.is_none() {
            return Err(ZaShareError::Forbidden);
        }
        if !ring.converted {
            // First publication requires this fresh calendar's complete valid scan
            // and archive fence. An authenticated converted ring skips phase 2.
            validate_scan(server, &context, true).await?;
            ring.converted = true;
            applied.changed = true;
        }
        if let Some(grant) = ring.grants.iter_mut().find(|g| g.account_id == account_id) {
            if grant.public_key != public_key {
                grant.public_key = public_key;
                grant.granted_at = now() as i64;
                applied.rotated = true;
                applied.changed = true;
                applied.affected_grantees.push(account_id);
            }
            if grant.level != level.code() {
                grant.level = level.code();
                applied.changed = true;
                applied.affected_grantees.push(account_id);
            }
        } else {
            let view = calendar_view_in_domain(&context.stored, &context.domain, keys.account_id)?
                .deserialize::<Calendar>()?;
            let mut entry = view
                .preferences
                .iter()
                .find(|p| p.account_id == keys.account_id)
                .ok_or(SealError::Format)?
                .clone();
            entry.account_id = account_id;
            entry.flags = 0;
            new_entry = Some(entry);
            ring.grants.push(ZaGrant {
                account_id,
                address,
                public_key,
                level: level.code(),
                granted_at: now() as i64,
                change_id: 0,
            });
            new_grant = Some(account_id);
            applied.changed = true;
            applied.affected_grantees.push(account_id);
        }
    }
    if !applied.changed {
        return Ok(applied);
    }
    if applied.rotated {
        header.generations.push(header.next_generation()?);
        ring.append_key(&Secret::random());
        applied
            .affected_grantees
            .extend(ring.grants.iter().map(|g| g.account_id));
    }
    let opened = header
        .generations
        .iter()
        .copied()
        .zip(ring.keys.iter().copied().map(Secret::from_bytes))
        .collect::<BTreeMap<_, _>>();
    // Bundle envelopes remain unchanged across rotation; only a new entry is sealed.
    for grant in &ring.grants {
        if applied.rotated || Some(grant.account_id) == new_grant {
            let encoded = seal_grant_ring(
                &GrantRingHeader {
                    calendar_id,
                    domain_id: header.domain_id,
                    grantee_id: grant.account_id,
                    grant_change_id: grant.change_id,
                    rename_change_id: header.rename_change_id,
                    boundary_pending: grant.change_id == 0,
                    generations: header.generations.clone(),
                },
                &opened,
                keys.account_id,
                &grant.public_key,
            )?;
            if Some(grant.account_id) == new_grant {
                // The new generation is relevant only when this grant also swept a dead principal.
                let domain = crate::calendar::seal::ZaKeyDomain::shared(
                    keys.account_id,
                    keys.account_id,
                    Some(keys.ewk().clone()),
                    calendar_id,
                    header.domain_id,
                    opened.iter().map(|(g, k)| (*g, k.clone())).collect(),
                    false,
                )?;
                calendar.preferences.push(seal_personal_entry(
                    new_entry.as_ref().ok_or(SealError::Format)?,
                    &domain,
                    &encoded,
                )?);
            } else {
                let entry = calendar
                    .preferences
                    .iter_mut()
                    .find(|p| p.account_id == grant.account_id)
                    .ok_or(SealError::Format)?;
                let rest = entry
                    .name
                    .strip_prefix(COLLECTION_MARKER)
                    .ok_or(SealError::Format)?
                    .split_once('|')
                    .ok_or(SealError::Format)?
                    .1;
                entry.name = format!("{COLLECTION_MARKER}{}|{rest}", STANDARD.encode(encoded));
            }
        }
    }
    // Removal cannot alter the owner index because the owner is never a grantee.
    parts[0] = STANDARD.encode(seal_owner_ring(
        &header,
        &ring,
        keys.account_id,
        keys.ewk(),
    )?);
    calendar
        .preferences
        .iter_mut()
        .find(|p| p.account_id == keys.account_id)
        .ok_or(SealError::Format)?
        .name = format!("{COLLECTION_MARKER}{}", parts.join("|"));
    let acls = ring
        .grants
        .iter()
        .map(|g| {
            Ok(AclGrant {
                account_id: g.account_id,
                grants: ZaShareLevel::from_code(g.level)?.bitmap(),
            })
        })
        .collect::<Result<Vec<_>, SealError>>()?;
    let entries = calendar.preferences.clone();
    za_replace_calendar_entries(&mut calendar, keys.account_id, calendar_id, entries, acls)?;
    let changed_by = server
        .account_info(keys.account_id)
        .await?
        .account_tenant_ids();
    let mut batch = BatchBuilder::new();
    context.assert_in(&mut batch);
    guards.assert_in(&mut batch);
    calendar.update(
        changed_by,
        context.stored.to_unarchived::<Calendar>()?,
        keys.account_id,
        calendar_id,
        &mut batch,
    )?;
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::share_commit_pause_point(keys.account_id).await;
    let committed = server.commit_batch(batch).await?;
    server.notify_task_queue();
    if new_grant.is_some() {
        applied.boundary_commit = Some(ZaBoundaryCommit {
            change_id: committed.last_change_id(keys.account_id)?,
        });
    }
    applied.affected_grantees.sort_unstable();
    applied.affected_grantees.dedup();
    for grantee in &applied.affected_grantees {
        trc::event!(
            Store(trc::StoreEvent::DataWrite),
            AccountId = keys.account_id,
            DocumentId = calendar_id,
            Id = *grantee,
            Type = ring
                .grants
                .iter()
                .find(|g| g.account_id == *grantee)
                .map_or(0, |g| g.level as u64),
            Total = removed.len(),
            Details = "zero-access: calendar share entry committed"
        );
    }
    trc::event!(
        Store(trc::StoreEvent::DataWrite),
        AccountId = keys.account_id,
        DocumentId = calendar_id,
        Total = applied.affected_grantees.len(),
        Details = "zero-access: calendar share committed"
    );
    Ok(applied)
}
