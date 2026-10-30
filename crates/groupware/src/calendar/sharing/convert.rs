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
    Calendar, CalendarEvent, CalendarEventContent, CalendarPreferences,
    seal::{
        COLLECTION_MARKER, SealError, ZaContainerOf, ZaEvent, ZaEventStorageKind, ZaKeyDomain,
        calendar_view_in_domain, classify_event_storage,
        collection::{seal_personal_entry, seal_shared_properties},
        container_is_ck,
        event::za_event_uid,
        open_container, rewrap_container, rewrap_event, seal_calendar, unseal_event_in_domain,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use common::{
    Server,
    auth::ChangedBy,
    storage::metadata::{ContainerChange, MetadataLog},
};
use registry::{schema::prelude::ObjectType, types::EnumImpl};
use std::collections::BTreeMap;
use store::write::assert::AssertValue;
use store::{
    ValueKey,
    write::{
        Archive, ArchiveBytes, ArchiveVersion, BatchBuilder, PendingId, RegistryClass, ValueClass,
        metadata::MetadataBuf,
    },
};
use types::{
    collection::Collection,
    field::{CalendarEventField, Field, PrincipalField},
    id::Id,
    metadata::{EncodedMetadata, MetadataEdit},
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
    TooManyShares,
    AmbiguousAddress,
    NotReady,
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
            Self::TooManyShares => f.write_str("TooManyShares"),
            Self::AmbiguousAddress => f.write_str("AmbiguousAddress"),
            Self::NotReady => f.write_str("NotReady"),
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
    let mut wrote = false;
    za_convert_calendar_tracked(server, keys, calendar_id, guards, &mut wrote).await
}

/// HTTP retains this monotonic witness across resumes. Ring-only writes count as
/// changes too; observed progress or a failed assertion never establishes attribution.
pub async fn za_convert_calendar_tracked(
    server: &Server,
    keys: &SessionKeys,
    calendar_id: u32,
    guards: &ZaShareGuards,
    wrote: &mut bool,
) -> Result<ZaConversionCounts, ZaShareError> {
    if guards.owner_id != keys.account_id || guards.verified_generation != keys.generation {
        return Err(ZaShareError::Forbidden);
    }
    let changed_by = server.account_info(keys.account_id).await?.changed_by();
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
                let (calendar, domain) = initialize(&context, keys)?;
                // Ruling 29: the calendar's dead-property container moves to
                // the collection key in the ring's own commit.
                let container =
                    calendar_container(server, &context, &context.domain, &domain).await?;
                write_calendar(
                    server,
                    keys,
                    guards,
                    changed_by,
                    &context,
                    calendar,
                    container,
                    WriteClass::Init,
                    counts,
                )
                .await?;
                *wrote = true;
                conversion_cut(keys.account_id).await?;
            }
            Some((_header, ring)) if ring.reverting => {
                rollback(
                    server,
                    keys,
                    guards,
                    changed_by,
                    &context,
                    &mut counts,
                    wrote,
                )
                .await?;
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
                        None,
                        WriteClass::Revert,
                        counts,
                    )
                    .await?;
                    *wrote = true;
                    continue;
                }
                for (index, ids) in ids.chunks(100).enumerate() {
                    let chunk = read_chunk(server, keys.account_id, ids).await?;
                    if index > 0 {
                        conversion_cut(keys.account_id).await?;
                    }
                    for stored in &chunk {
                        let meta = stored.meta.deserialize::<CalendarEvent>()?;
                        if !meta.names.iter().any(|n| n.parent_id == calendar_id) {
                            continue;
                        }
                        if !matches!(meta.names.as_slice(), [name] if name.parent_id == calendar_id)
                        {
                            // A legitimate new link is impossible after phase 1;
                            // still refuse rather than writing a linked CK row.
                            return Err(SealError::Structure("collection key event parent").into());
                        }
                        let content = stored.content.deserialize::<CalendarEventContent>()?;
                        let kind = classify_event_storage(&content)?;
                        let mut event = ZaEvent::new(meta, content);
                        let uid = za_event_uid(&event.content);
                        // Ruling 29: the event's container moves with it, in
                        // the same commit chunk (identity rule: the
                        // container is the event's, by account, collection
                        // and document id).
                        let container = rewrapped_container(
                            stored.container.as_ref(),
                            &context.domain,
                            &context.domain,
                            ZaContainerOf::Event(&uid),
                            true,
                        )?;
                        if is_ck(&event.content, kind)? {
                            unseal_event_in_domain(&mut event, &context.domain)?;
                            if let Some(container) = container {
                                // A converted event whose container is not
                                // under the collection key yet.
                                write_container(
                                    server,
                                    guards,
                                    &context,
                                    stored.id,
                                    container,
                                    WriteClass::Rewrap,
                                    counts,
                                )
                                .await?;
                                *wrote = true;
                            }
                            continue;
                        }
                        rewrap_event(&mut event, &context.domain, &context.domain)?;
                        write_event(
                            server,
                            guards,
                            changed_by,
                            &context,
                            stored,
                            event,
                            container,
                            WriteClass::Rewrap,
                            counts,
                        )
                        .await?;
                        *wrote = true;
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

pub(super) fn owner_ring(
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

fn initialize(
    context: &ZaCalendarContext,
    keys: &SessionKeys,
) -> Result<(Calendar, ZaKeyDomain), ZaShareError> {
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
    let (envelope, bundle) = seal_shared_properties(&domain)?;
    let mut owner = seal_personal_entry(&calendar.preferences[0], &domain, &encoded)?;
    owner.name.push_str(&format!("|{envelope}|{bundle}"));
    za_replace_calendar_entries(
        &mut calendar,
        keys.account_id,
        context.calendar_id,
        vec![owner],
        Vec::new(),
    )?;
    Ok((calendar, domain))
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
    changed_by: ChangedBy,
    context: &ZaCalendarContext,
    counts: &mut ZaConversionCounts,
    wrote: &mut bool,
) -> Result<(), ZaShareError> {
    let owner = ZaKeyDomain::owner(keys);
    let (ids, _) = event_ids(server, keys.account_id, context.calendar_id).await?;
    for (index, ids) in ids.chunks(100).enumerate() {
        let chunk = read_chunk(server, keys.account_id, ids).await?;
        if index > 0 {
            conversion_cut(keys.account_id).await?;
        }
        for stored in &chunk {
            let meta = stored.meta.deserialize::<CalendarEvent>()?;
            if !meta
                .names
                .iter()
                .any(|n| n.parent_id == context.calendar_id)
            {
                continue;
            }
            let content = stored.content.deserialize::<CalendarEventContent>()?;
            let kind = classify_event_storage(&content)?;
            let uid = za_event_uid(&content);
            let container = rewrapped_container(
                stored.container.as_ref(),
                &context.domain,
                &owner,
                ZaContainerOf::Event(&uid),
                false,
            )?;
            if !is_ck(&content, kind)? {
                if let Some(container) = container {
                    write_container(
                        server,
                        guards,
                        context,
                        stored.id,
                        container,
                        WriteClass::Rollback,
                        *counts,
                    )
                    .await?;
                    *wrote = true;
                }
                continue;
            }
            let mut event = ZaEvent::new(meta, content);
            rewrap_event(&mut event, &context.domain, &owner)?;
            write_event(
                server,
                guards,
                changed_by,
                context,
                stored,
                event,
                container,
                WriteClass::Rollback,
                *counts,
            )
            .await?;
            *wrote = true;
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
    let container = calendar_container(server, &fresh, &fresh.domain, &owner).await?;
    write_calendar(
        server,
        keys,
        guards,
        changed_by,
        &fresh,
        calendar,
        container,
        WriteClass::Restore,
        *counts,
    )
    .await?;
    *wrote = true;
    Ok(())
}

/// The complete parentage walk retains only document ids, never every payload.
pub(super) async fn event_ids(
    server: &Server,
    owner: u32,
    calendar: u32,
) -> Result<(Vec<u32>, bool), ZaShareError> {
    let mut ids = Vec::new();
    let mut linked = false;
    server
        .archives(
            owner,
            Collection::CalendarEvent,
            Field::ARCHIVE,
            &(),
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
                Ok(true)
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
) -> Result<Vec<StoredEvent>, ZaShareError> {
    let mut chunk = Vec::with_capacity(ids.len());
    for &id in ids {
        if let Some(stored) = read_event(server, owner, id).await? {
            chunk.push(stored);
        }
    }
    Ok(chunk)
}

/// A stored event at 1.0: its metadata row, its content and its
/// dead-property container, read together.
struct StoredEvent {
    id: u32,
    meta: Archive<ArchiveBytes>,
    content: Archive<ArchiveBytes>,
    container: Option<MetadataBuf>,
}

/// The rewrapped form of a stored container, or `None` when there is none
/// or it is already in the target class (`to_ck`).
fn rewrapped_container(
    stored: Option<&MetadataBuf>,
    from: &ZaKeyDomain,
    to: &ZaKeyDomain,
    of: ZaContainerOf<'_>,
    to_ck: bool,
) -> Result<Option<(MetadataBuf, EncodedMetadata)>, SealError> {
    let Some(stored) = stored else {
        return Ok(None);
    };
    let bytes = stored.view().as_bytes();
    if container_is_ck(bytes) == Some(to_ck) {
        return Ok(None);
    }
    rewrap_container(bytes, from, to, of).map(|next| Some((stored.clone(), next)))
}

/// The calendar's own container, rewrapped from `from` to `to`.
async fn calendar_container(
    server: &Server,
    context: &ZaCalendarContext,
    from: &ZaKeyDomain,
    to: &ZaKeyDomain,
) -> Result<Option<(MetadataBuf, EncodedMetadata)>, ZaShareError> {
    let stored = server
        .metadata_container(context.owner_id, Collection::Calendar, context.calendar_id)
        .await?;
    Ok(rewrapped_container(
        stored.as_ref(),
        from,
        to,
        ZaContainerOf::Calendar(context.calendar_id),
        to.calendar_id().is_some() && !to.is_reverting(),
    )?)
}

/// Adds a container's conditional rewrite (upstream's own write: the
/// stored container's hash is asserted) to `batch`.
async fn add_container_write(
    server: &Server,
    owner_id: u32,
    collection: Collection,
    document_id: u32,
    (previous, next): (MetadataBuf, EncodedMetadata),
    batch: &mut BatchBuilder,
) -> Result<(), ZaShareError> {
    let owner = server.account(owner_id).await?;
    if let Some(change) = ContainerChange::new(Some(&previous), Some(next), MetadataEdit::Write) {
        change
            .into_write(owner.as_ref(), collection, MetadataLog::None)
            .build(PendingId::Assigned(document_id), batch)?;
    }
    Ok(())
}

/// Stream final validation, retaining only the current archive and any fixed
/// typed error. Store read errors retain precedence over an incomplete scan.
pub(super) async fn validate_scan(
    server: &Server,
    context: &ZaCalendarContext,
    forward: bool,
) -> Result<(), ZaShareError> {
    let (ids, _) = event_ids(server, context.owner_id, context.calendar_id).await?;
    let mut failure = None;
    for id in ids {
        // Store read errors keep precedence over a failed check.
        let Some(stored) = read_event(server, context.owner_id, id).await? else {
            continue;
        };
        if failure.is_some() {
            continue;
        }
        let checked = (|| -> Result<(), ZaShareError> {
            let meta = stored.meta.deserialize::<CalendarEvent>()?;
            if !meta
                .names
                .iter()
                .any(|n| n.parent_id == context.calendar_id)
            {
                return Ok(());
            }
            let content = stored.content.deserialize::<CalendarEventContent>()?;
            let kind = classify_event_storage(&content)?;
            let container = stored
                .container
                .as_ref()
                .map(|container| container_is_ck(container.view().as_bytes()));
            if forward {
                if !is_ck(&content, kind)? || container.is_some_and(|ck| ck != Some(true)) {
                    return Err(SealError::NotSealed.into());
                }
                let uid = za_event_uid(&content);
                let mut event = ZaEvent::new(meta, content);
                unseal_event_in_domain(&mut event, &context.domain)?;
                if let Some(container) = &stored.container {
                    open_container(
                        container.view().as_bytes(),
                        &context.domain,
                        ZaContainerOf::Event(&uid),
                    )?;
                }
            } else if is_ck(&content, kind)? || container == Some(Some(true)) {
                return Err(SealError::Structure("rollback incomplete").into());
            }
            Ok(())
        })();
        if let Err(error) = checked {
            failure = Some(error);
        }
    }
    failure.map_or(Ok(()), Err)
}
fn is_ck(content: &CalendarEventContent, kind: ZaEventStorageKind) -> Result<bool, SealError> {
    if kind == ZaEventStorageKind::LegacyPlaintext {
        return Ok(false);
    }
    let carrier = content.data.event.components[0]
        .entries
        .last()
        .ok_or(SealError::Format)?;
    let calcard::icalendar::ICalendarValue::Text(encoded) = &carrier.values[0] else {
        return Err(SealError::Format);
    };
    Ok(STANDARD.decode(encoded).map_err(|_| SealError::Decode)?[1] == super::format::WRAP_CK)
}
async fn read_event(server: &Server, owner: u32, id: u32) -> trc::Result<Option<StoredEvent>> {
    let (meta, content) = server
        .archive_and_field(
            owner,
            Collection::CalendarEvent,
            id,
            CalendarEventField::Content.field(),
        )
        .await?;
    let (Some(meta), Some(content)) = (meta, content) else {
        return Ok(None);
    };
    let container = server
        .metadata_container(owner, Collection::CalendarEvent, id)
        .await?;
    Ok(Some(StoredEvent {
        id,
        meta,
        content,
        container,
    }))
}

async fn read_event_version(
    server: &Server,
    owner: u32,
    id: u32,
) -> trc::Result<Option<ArchiveVersion>> {
    server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(owner, Collection::CalendarEvent, id))
        .await
        .map(|archive| archive.map(|archive| archive.version))
}

#[allow(clippy::too_many_arguments)]
async fn write_calendar(
    server: &Server,
    keys: &SessionKeys,
    guards: &ZaShareGuards,
    changed_by: ChangedBy,
    context: &ZaCalendarContext,
    calendar: Calendar,
    container: Option<(MetadataBuf, EncodedMetadata)>,
    class: WriteClass,
    counts: ZaConversionCounts,
) -> Result<(), ZaShareError> {
    let mut batch = BatchBuilder::new();
    context.assert_in(&mut batch);
    guards.assert_in(&mut batch);
    if let Some(container) = container {
        add_container_write(
            server,
            context.owner_id,
            Collection::Calendar,
            context.calendar_id,
            container,
            &mut batch,
        )
        .await?;
    }
    calendar.update(
        changed_by,
        context.stored.to_unarchived::<Calendar>()?,
        keys.account_id,
        context.calendar_id,
        &mut batch,
    )?;
    commit(server, context, batch, class, &[], counts).await
}
#[allow(clippy::too_many_arguments)]
async fn write_event(
    server: &Server,
    guards: &ZaShareGuards,
    changed_by: ChangedBy,
    context: &ZaCalendarContext,
    stored: &StoredEvent,
    event: ZaEvent,
    container: Option<(MetadataBuf, EncodedMetadata)>,
    class: WriteClass,
    counts: ZaConversionCounts,
) -> Result<(), ZaShareError> {
    let mut batch = BatchBuilder::new();
    context.assert_in(&mut batch);
    guards.assert_in(&mut batch);
    // The container's write precedes the event's, whose commit point closes
    // the chunk: both land in one transaction.
    if let Some(container) = container {
        add_container_write(
            server,
            context.owner_id,
            Collection::CalendarEvent,
            stored.id,
            container,
            &mut batch,
        )
        .await?;
    }
    let (meta, content) = event.into_parts();
    let modified = meta.modified;
    meta.update_full_at(
        content,
        changed_by,
        stored.meta.to_unarchived::<CalendarEvent>()?,
        stored.content.unarchive::<CalendarEventContent>()?,
        context.owner_id,
        stored.id,
        None,
        modified,
        &mut batch,
    )?;
    commit(
        server,
        context,
        batch,
        class,
        &[(stored.id, Some(stored.meta.version))],
        counts,
    )
    .await
}

/// A container-only rewrap of a converted (or rolled back) event.
async fn write_container(
    server: &Server,
    guards: &ZaShareGuards,
    context: &ZaCalendarContext,
    id: u32,
    container: (MetadataBuf, EncodedMetadata),
    class: WriteClass,
    counts: ZaConversionCounts,
) -> Result<(), ZaShareError> {
    let mut batch = BatchBuilder::new();
    context.assert_in(&mut batch);
    guards.assert_in(&mut batch);
    add_container_write(
        server,
        context.owner_id,
        Collection::CalendarEvent,
        id,
        container,
        &mut batch,
    )
    .await?;
    commit(server, context, batch, class, &[], counts).await
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
            let after: Option<Archive<ArchiveBytes>> = server
                .store()
                .get_value(ValueKey::archive(
                    context.owner_id,
                    Collection::Calendar,
                    context.calendar_id,
                ))
                .await?;
            let mut progress_changed = after.map(|a| a.version) != Some(context.stored.version);
            for (id, before) in affected {
                progress_changed |=
                    read_event_version(server, context.owner_id, *id).await? != *before;
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
