/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    format::{
        OwnerRingHeader, ZaOwnerRing, open_owner_ring, seal_owner_ring, za_replace_calendar_entries,
    },
    resolve::{ZaCalendarContext, ZaCalendarError, za_owned_calendar_context},
};
use crate::calendar::{
    Calendar, CalendarEvent, CalendarPreferences,
    seal::{
        COLLECTION_MARKER, SealError, ZaEventStorageKind, ZaKeyDomain, calendar_view_in_domain,
        classify_event_storage,
        collection::{seal_personal_entry, seal_shared_properties},
        rewrap_event, seal_calendar, unseal_event_in_domain,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{Server, auth::AccountTenantIds};
use registry::{schema::prelude::ObjectType, types::EnumImpl};
use std::collections::BTreeMap;
use store::write::assert::AssertValue;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, ArchiveVersion, BatchBuilder, RegistryClass, ValueClass},
};
use types::{
    collection::Collection,
    field::{Field, PrincipalField},
    id::Id,
};
use vault::keys::Secret;
use vault::session::SessionKeys;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ZaConversionCounts {
    pub rewrapped: usize,
    pub sealed: usize,
}

pub struct ZaShareGuards {
    pub owner_id: u32,
    pub verified_generation: u64,
    pub vault_cas: u64,
    pub pin_assertion: Option<AssertValue>,
}

pub enum ZaShareError {
    LinkedEvents,
    Forbidden,
    Seal(crate::calendar::seal::SealError),
    Store(trc::Error),
    ConversionConflict {
        committed: ZaConversionCounts,
        progress_changed: bool,
    },
}

impl std::fmt::Debug for ZaShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LinkedEvents => f.write_str("LinkedEvents"),
            Self::Forbidden => f.write_str("Forbidden"),
            Self::Seal(_) => f.write_str("Seal(..)"),
            Self::Store(_) => f.write_str("Store(..)"),
            Self::ConversionConflict {
                committed,
                progress_changed,
            } => f
                .debug_struct("ConversionConflict")
                .field("committed", committed)
                .field("progress_changed", progress_changed)
                .finish(),
        }
    }
}

impl From<SealError> for ZaShareError {
    fn from(error: SealError) -> Self {
        Self::Seal(error)
    }
}
impl From<trc::Error> for ZaShareError {
    fn from(error: trc::Error) -> Self {
        Self::Store(error)
    }
}
impl From<ZaCalendarError> for ZaShareError {
    fn from(error: ZaCalendarError) -> Self {
        match error {
            ZaCalendarError::Forbidden => Self::Forbidden,
            ZaCalendarError::LinkedEvent => Self::LinkedEvents,
            ZaCalendarError::Seal(error) => Self::Seal(error),
            ZaCalendarError::Store(error) => Self::Store(error),
        }
    }
}

impl ZaShareGuards {
    /// Append every externally verified fence before any indexed write.
    pub fn assert_in(&self, batch: &mut BatchBuilder) {
        batch
            .with_account_id(self.owner_id)
            .with_collection(Collection::Principal)
            .with_document(0)
            .assert_value(
                PrincipalField::ZeroAccessVault,
                AssertValue::Hash(self.vault_cas),
            );
        if let Some(pin) = self.pin_assertion {
            batch.assert_value(PrincipalField::ZeroAccessPins, pin);
        }
        batch.assert_value(
            ValueClass::Registry(RegistryClass::Item {
                object_id: ObjectType::Account.to_id(),
                item_id: Id::from(self.owner_id).id(),
            }),
            AssertValue::Some,
        );
    }
}

#[derive(Clone, Copy)]
enum WriteClass {
    Init,
    Rewrap,
    Revert,
    Rollback,
    Restore,
}

/// Conversion owns no HTTP attempt budget. Every opaque assertion loss goes back
/// to the coordinator, even when the calendar/event reread also observes progress.
pub async fn za_convert_calendar(
    server: &Server,
    keys: &SessionKeys,
    calendar_id: u32,
    guards: &ZaShareGuards,
) -> Result<ZaConversionCounts, ZaShareError> {
    if guards.owner_id != keys.account_id || guards.verified_generation != keys.generation {
        return Err(ZaShareError::Forbidden);
    }
    let changed_by = server
        .account_info(keys.account_id)
        .await?
        .account_tenant_ids();
    let mut counts = ZaConversionCounts::default();
    let mut linked = false;
    loop {
        let context = za_owned_calendar_context(server, keys, calendar_id).await?;
        let ring = owner_ring(&context, keys)?;
        if let Some((_, journal)) = &ring
            && (!journal.converted || journal.reverting)
        {
            let calendar = context.stored.deserialize::<Calendar>()?;
            if !journal.grants.is_empty()
                || !calendar.acls.is_empty()
                || calendar.preferences.len() != 1
            {
                return Err(SealError::Structure("conversion journal grants").into());
            }
        }
        match ring {
            None => {
                if linked || event_ids(server, keys.account_id, calendar_id).await?.1 {
                    return Err(ZaShareError::LinkedEvents);
                }
                #[cfg(feature = "test_mode")]
                common::za::test_hooks::share_precheck_pause_point(keys.account_id).await;
                let calendar = initialize(&context, keys)?;
                write_calendar(
                    server,
                    keys,
                    guards,
                    changed_by,
                    &context,
                    calendar,
                    WriteClass::Init,
                    counts,
                )
                .await?;
                conversion_cut(keys.account_id).await?;
            }
            Some((_header, ring)) if ring.reverting => {
                rollback(server, keys, guards, changed_by, &context, &mut counts).await?;
                if linked {
                    return Err(ZaShareError::LinkedEvents);
                }
            }
            Some((_header, ring)) if ring.converted => return Ok(counts),
            Some((_header, _ring)) => {
                // This complete post-phase-1 walk closes the unfenced precheck
                // window before even the first EWK envelope is changed.
                let (ids, has_link) = event_ids(server, keys.account_id, calendar_id).await?;
                if has_link {
                    linked = true;
                    let calendar = mark_reverting(&context, keys)?;
                    write_calendar(
                        server,
                        keys,
                        guards,
                        changed_by,
                        &context,
                        calendar,
                        WriteClass::Revert,
                        counts,
                    )
                    .await?;
                    continue;
                }
                for (index, ids) in ids.chunks(100).enumerate() {
                    let chunk = read_chunk(server, keys.account_id, ids).await?;
                    if index > 0 {
                        conversion_cut(keys.account_id).await?;
                    }
                    for (id, stored) in &chunk {
                        let mut event = stored.deserialize::<CalendarEvent>()?;
                        if !event.names.iter().any(|n| n.parent_id == calendar_id) {
                            continue;
                        }
                        if !matches!(event.names.as_slice(), [name] if name.parent_id == calendar_id)
                        {
                            // A legitimate new link is impossible after phase 1;
                            // still refuse rather than writing a linked CK row.
                            return Err(SealError::Structure("collection key event parent").into());
                        }
                        let kind = classify_event_storage(&event)?;
                        if is_ck(&event, kind)? {
                            unseal_event_in_domain(&mut event, &context.domain)?;
                            continue;
                        }
                        rewrap_event(&mut event, &context.domain, &context.domain)?;
                        write_event(
                            server,
                            guards,
                            changed_by,
                            &context,
                            *id,
                            &stored,
                            event,
                            WriteClass::Rewrap,
                            counts,
                        )
                        .await?;
                        match kind {
                            ZaEventStorageKind::LegacyPlaintext => counts.sealed += 1,
                            ZaEventStorageKind::Sealed => counts.rewrapped += 1,
                        }
                    }
                }
                // No grant-ready result until every current row has sole parent
                // and an authenticated envelope in this ring's domain.
                validate_scan(server, &context, true).await?;
                return Ok(counts);
            }
        }
    }
}

fn owner_ring(
    context: &ZaCalendarContext,
    keys: &SessionKeys,
) -> Result<Option<(OwnerRingHeader, ZaOwnerRing)>, ZaShareError> {
    if context.domain.calendar_id().is_none() {
        return Ok(None);
    }
    let calendar = context.stored.deserialize::<Calendar>()?;
    let entry = calendar
        .preferences
        .iter()
        .find(|p| p.account_id == keys.account_id)
        .ok_or(SealError::Format)?;
    let encoded = entry
        .name
        .strip_prefix(COLLECTION_MARKER)
        .ok_or(SealError::Format)?
        .split('|')
        .next()
        .ok_or(SealError::Format)?;
    Ok(Some(open_owner_ring(
        &STANDARD.decode(encoded).map_err(|_| SealError::Decode)?,
        keys.account_id,
        context.calendar_id,
        keys.ewk(),
    )?))
}

fn initialize(context: &ZaCalendarContext, keys: &SessionKeys) -> Result<Calendar, ZaShareError> {
    let mut calendar = calendar_view_in_domain(&context.stored, &context.domain, keys.account_id)?
        .deserialize::<Calendar>()?;
    if calendar.preferences.is_empty() {
        calendar.preferences.push(CalendarPreferences {
            account_id: keys.account_id,
            name: "default".into(),
            ..Default::default()
        });
    }
    let mut domain_id = [0; 16];
    domain_id.copy_from_slice(&Secret::random().as_bytes()[..16]);
    let ck = Secret::random();
    let header = OwnerRingHeader {
        calendar_id: context.calendar_id,
        domain_id,
        rename_change_id: 0,
        boundary_pending: false,
        generations: vec![1],
    };
    let ring = ZaOwnerRing {
        version: 1,
        keys: vec![*ck.as_bytes()],
        converted: false,
        reverting: false,
        grants: Vec::new(),
    };
    let encoded = seal_owner_ring(&header, &ring, keys.account_id, keys.ewk())?;
    let domain = ZaKeyDomain::shared(
        keys.account_id,
        keys.account_id,
        Some(keys.ewk().clone()),
        context.calendar_id,
        domain_id,
        BTreeMap::from([(1, ck)]),
        false,
    )?;
    let (envelope, bundle) = seal_shared_properties(&calendar.dead_properties, &domain)?;
    let mut owner = seal_personal_entry(&calendar.preferences[0], &domain, &encoded)?;
    owner.name.push_str(&format!("|{envelope}|{bundle}"));
    calendar.dead_properties = Default::default();
    za_replace_calendar_entries(
        &mut calendar,
        keys.account_id,
        context.calendar_id,
        vec![owner],
        Vec::new(),
    )?;
    Ok(calendar)
}

fn mark_reverting(
    context: &ZaCalendarContext,
    keys: &SessionKeys,
) -> Result<Calendar, ZaShareError> {
    let (header, mut ring) = owner_ring(context, keys)?.ok_or(SealError::NotSealed)?;
    ring.reverting = true;
    let encoded = seal_owner_ring(&header, &ring, keys.account_id, keys.ewk())?;
    let mut calendar = context.stored.deserialize::<Calendar>()?;
    let mut entries = calendar.preferences.clone();
    let owner = entries
        .iter_mut()
        .find(|p| p.account_id == keys.account_id)
        .ok_or(SealError::Format)?;
    let parts = owner
        .name
        .strip_prefix(COLLECTION_MARKER)
        .ok_or(SealError::Format)?
        .split('|')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    owner.name = format!(
        "{COLLECTION_MARKER}{}|{}",
        STANDARD.encode(encoded),
        parts[1..].join("|")
    );
    let acls = calendar.acls.clone();
    za_replace_calendar_entries(
        &mut calendar,
        keys.account_id,
        context.calendar_id,
        entries,
        acls,
    )?;
    Ok(calendar)
}

async fn rollback(
    server: &Server,
    keys: &SessionKeys,
    guards: &ZaShareGuards,
    changed_by: AccountTenantIds,
    context: &ZaCalendarContext,
    counts: &mut ZaConversionCounts,
) -> Result<(), ZaShareError> {
    let owner = ZaKeyDomain::owner(keys);
    let (ids, _) = event_ids(server, keys.account_id, context.calendar_id).await?;
    for (index, ids) in ids.chunks(100).enumerate() {
        let chunk = read_chunk(server, keys.account_id, ids).await?;
        if index > 0 {
            conversion_cut(keys.account_id).await?;
        }
        for (id, stored) in &chunk {
            let mut event = stored.deserialize::<CalendarEvent>()?;
            if !event
                .names
                .iter()
                .any(|n| n.parent_id == context.calendar_id)
            {
                continue;
            }
            let kind = classify_event_storage(&event)?;
            if !is_ck(&event, kind)? {
                continue;
            }
            rewrap_event(&mut event, &context.domain, &owner)?;
            write_event(
                server,
                guards,
                changed_by,
                context,
                *id,
                &stored,
                event,
                WriteClass::Rollback,
                *counts,
            )
            .await?;
            counts.rewrapped += 1;
        }
    }
    // Keep the ring until no current event can need any of its keys.
    let fresh = za_owned_calendar_context(server, keys, context.calendar_id).await?;
    if !fresh.domain.is_reverting() {
        return Err(SealError::Structure("conversion journal changed").into());
    }
    validate_scan(server, &fresh, false).await?;
    let mut calendar = calendar_view_in_domain(&fresh.stored, &fresh.domain, keys.account_id)?
        .deserialize::<Calendar>()?;
    seal_calendar(&mut calendar, keys, keys.account_id)?;
    let entries = calendar.preferences.clone();
    za_replace_calendar_entries(
        &mut calendar,
        keys.account_id,
        context.calendar_id,
        entries,
        Vec::new(),
    )?;
    write_calendar(
        server,
        keys,
        guards,
        changed_by,
        &fresh,
        calendar,
        WriteClass::Restore,
        *counts,
    )
    .await
}

/// The complete parentage walk retains only document ids, never every payload.
async fn event_ids(
    server: &Server,
    owner: u32,
    calendar: u32,
) -> Result<(Vec<u32>, bool), ZaShareError> {
    let mut ids = Vec::new();
    let mut linked = false;
    server
        .all_archives(
            owner,
            Collection::CalendarEvent,
            Field::ARCHIVE.into(),
            |id, stored| {
                let event = stored.unarchive::<CalendarEvent>()?;
                if event
                    .names
                    .iter()
                    .any(|n| n.parent_id.to_native() == calendar)
                {
                    linked |= event.names.len() != 1;
                    ids.push(id);
                }
                Ok(())
            },
        )
        .await?;
    Ok((ids, linked))
}

/// At most one logical group's snapshots survive across writes/a pause. Reading
/// the next group before the between-group seam also makes genuine archive CAS
/// races observable without holding the entire calendar in memory.
async fn read_chunk(
    server: &Server,
    owner: u32,
    ids: &[u32],
) -> Result<Vec<(u32, Archive<AlignedBytes>)>, ZaShareError> {
    let mut chunk = Vec::with_capacity(ids.len());
    for &id in ids {
        if let Some(stored) = read_event(server, owner, id).await? {
            chunk.push((id, stored));
        }
    }
    Ok(chunk)
}

/// Stream final validation, retaining only the current archive and any fixed
/// typed error. Store read errors retain precedence over an incomplete scan.
async fn validate_scan(
    server: &Server,
    context: &ZaCalendarContext,
    forward: bool,
) -> Result<(), ZaShareError> {
    let mut failure = None;
    server
        .all_archives(
            context.owner_id,
            Collection::CalendarEvent,
            Field::ARCHIVE.into(),
            |_, stored| {
                let event = stored.unarchive::<CalendarEvent>()?;
                if failure.is_none()
                    && event
                        .names
                        .iter()
                        .any(|n| n.parent_id.to_native() == context.calendar_id)
                {
                    let checked = (|| -> Result<(), ZaShareError> {
                        let mut event = stored.deserialize::<CalendarEvent>()?;
                        let kind = classify_event_storage(&event)?;
                        if forward {
                            if !is_ck(&event, kind)? {
                                return Err(SealError::NotSealed.into());
                            }
                            unseal_event_in_domain(&mut event, &context.domain)?;
                        } else if is_ck(&event, kind)? {
                            return Err(SealError::Structure("rollback incomplete").into());
                        }
                        Ok(())
                    })();
                    if let Err(error) = checked {
                        failure = Some(error);
                    }
                }
                Ok(())
            },
        )
        .await?;
    failure.map_or(Ok(()), Err)
}
fn is_ck(event: &CalendarEvent, kind: ZaEventStorageKind) -> Result<bool, SealError> {
    if kind == ZaEventStorageKind::LegacyPlaintext {
        return Ok(false);
    }
    let carrier = event.data.event.components[0]
        .entries
        .last()
        .ok_or(SealError::Format)?;
    let calcard::icalendar::ICalendarValue::Text(encoded) = &carrier.values[0] else {
        return Err(SealError::Format);
    };
    Ok(STANDARD.decode(encoded).map_err(|_| SealError::Decode)?[1] == super::format::WRAP_CK)
}
async fn read_event(
    server: &Server,
    owner: u32,
    id: u32,
) -> trc::Result<Option<Archive<AlignedBytes>>> {
    server
        .store()
        .get_value(ValueKey::archive(owner, Collection::CalendarEvent, id))
        .await
}

async fn write_calendar(
    server: &Server,
    keys: &SessionKeys,
    guards: &ZaShareGuards,
    changed_by: AccountTenantIds,
    context: &ZaCalendarContext,
    calendar: Calendar,
    class: WriteClass,
    counts: ZaConversionCounts,
) -> Result<(), ZaShareError> {
    let mut batch = BatchBuilder::new();
    context.assert_in(&mut batch);
    guards.assert_in(&mut batch);
    calendar.update(
        changed_by,
        context.stored.to_unarchived::<Calendar>()?,
        keys.account_id,
        context.calendar_id,
        &mut batch,
    )?;
    commit(server, context, batch, class, &[], counts).await
}
async fn write_event(
    server: &Server,
    guards: &ZaShareGuards,
    changed_by: AccountTenantIds,
    context: &ZaCalendarContext,
    id: u32,
    stored: &Archive<AlignedBytes>,
    event: CalendarEvent,
    class: WriteClass,
    counts: ZaConversionCounts,
) -> Result<(), ZaShareError> {
    let mut batch = BatchBuilder::new();
    context.assert_in(&mut batch);
    guards.assert_in(&mut batch);
    let modified = event.modified;
    event.update_at(
        changed_by,
        stored.to_unarchived::<CalendarEvent>()?,
        context.owner_id,
        id,
        modified,
        &mut batch,
    )?;
    commit(
        server,
        context,
        batch,
        class,
        &[(id, Some(stored.version))],
        counts,
    )
    .await
}
async fn commit(
    server: &Server,
    context: &ZaCalendarContext,
    batch: BatchBuilder,
    class: WriteClass,
    affected: &[(u32, Option<ArchiveVersion>)],
    counts: ZaConversionCounts,
) -> Result<(), ZaShareError> {
    #[cfg(feature = "test_mode")]
    let injected = {
        use common::za::test_hooks::*;
        match class {
            WriteClass::Init => take_share_init_assert_fail(context.owner_id, context.calendar_id),
            WriteClass::Rewrap => {
                take_share_rewrap_assert_fail(context.owner_id, context.calendar_id)
            }
            WriteClass::Revert => {
                take_share_revert_assert_fail(context.owner_id, context.calendar_id)
            }
            WriteClass::Rollback => {
                take_share_rollback_assert_fail(context.owner_id, context.calendar_id)
            }
            WriteClass::Restore => {
                take_share_restore_assert_fail(context.owner_id, context.calendar_id)
            }
        }
    };
    #[cfg(not(feature = "test_mode"))]
    let injected = {
        let _ = class;
        false
    };
    let result = if injected {
        Err(trc::StoreEvent::AssertValueFailed.into_err())
    } else {
        server.commit_batch(batch).await.map(|_| ())
    };
    match result {
        Ok(()) => {
            server.notify_task_queue();
            Ok(())
        }
        Err(error) if error.is_assertion_failure() => {
            let after: Option<Archive<AlignedBytes>> = server
                .store()
                .get_value(ValueKey::archive(
                    context.owner_id,
                    Collection::Calendar,
                    context.calendar_id,
                ))
                .await?;
            let mut progress_changed = after.map(|a| a.version) != Some(context.stored.version);
            for (id, before) in affected {
                progress_changed |= read_event(server, context.owner_id, *id)
                    .await?
                    .map(|a| a.version)
                    != *before;
            }
            Err(ZaShareError::ConversionConflict {
                committed: counts,
                progress_changed,
            })
        }
        Err(error) => Err(error.into()),
    }
}
async fn conversion_cut(owner: u32) -> Result<(), ZaShareError> {
    #[cfg(feature = "test_mode")]
    {
        common::za::test_hooks::share_rewrap_pause_point(owner).await;
        if common::za::test_hooks::take_share_rewrap_fail_once(owner) {
            return Err(trc::StoreEvent::UnexpectedError
                .into_err()
                .details("zero-access conversion interrupted")
                .into());
        }
    }
    #[cfg(not(feature = "test_mode"))]
    let _ = owner;
    Ok(())
}
