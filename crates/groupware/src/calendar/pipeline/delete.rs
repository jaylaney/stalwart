/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    ZaDeleted,
    admission::ZaRoles,
    context::{za_mutation_context, za_write_contexts},
    error::{ZaPipelineError, ZaRefusal},
};
use crate::{
    DestroyArchive,
    calendar::{
        Calendar, CalendarEvent, EVENT_HAS_ALARMS,
        delegation::{
            ZaSelfOperation, za_combine_delegated, za_resolve_delegation, za_self_cancel,
        },
        fanout::{ZaDeleteMessage, ZaFanOut, ZaFanOutKind, ZaFanOutMessage, ZaSource},
        schedule::AlarmTasks,
        sharing::resolve::{ZaCalendarContext, ZaCalendarError, calendars_changed},
    },
};
use common::{
    GroupwareResources, Server,
    auth::AccessToken,
    cache::invalidate::CacheInvalidationBuilder,
    ipc::CacheInvalidation,
    storage::{dav::canonical_calcard_segment, metadata::ContainerCleanup},
    za::imip::ZaImipPending,
};
use std::future::{Future, Ready};
use store::write::{Archive, ArchiveBytes, BatchBuilder, now};
use trc::AddContext;
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection},
    field::PrincipalField,
};
use utils::map::bitmap::Bitmap;
use vault::session::SessionKeys;

pub struct ZaCalendarDeleteTarget {
    /// `None` for an ordinary account.
    pub context: Option<ZaCalendarContext>,
    /// The repaired stored calendar; the caller's ETag precondition reads it.
    pub stored: Archive<ArchiveBytes>,
    pub deleted_grantees: Vec<u32>,
}

/// Resolve the fresh exact-principal grant before validating the deletion's ETag.
pub async fn za_calendar_delete_target(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    stored: Archive<ArchiveBytes>,
) -> Result<ZaCalendarDeleteTarget, ZaPipelineError> {
    let context = za_mutation_context(
        server,
        token,
        owner_id,
        calendar_id,
        Bitmap::new()
            .with_item(Acl::Delete)
            .with_item(Acl::RemoveItems),
    )
    .await
    .map_err(|error| ZaPipelineError::from_calendar(error, owner_id, calendar_id))?;
    let stored = match &context {
        Some(context) => context.stored.clone(),
        None => stored,
    };
    let mut deleted_grantees = Vec::new();
    if context
        .as_ref()
        .is_some_and(|context| context.domain.calendar_id().is_some())
    {
        let calendar = stored
            .to_unarchived::<Calendar>()
            .caused_by(trc::location!())?;
        deleted_grantees.extend(
            calendar
                .inner
                .acls
                .iter()
                .map(|grant| grant.account_id.to_native())
                .filter(|grantee| *grantee != owner_id),
        );
    }
    Ok(ZaCalendarDeleteTarget {
        context,
        stored,
        deleted_grantees,
    })
}

/// Delete one name of a key account's event. The content input is the stored archive.
#[allow(clippy::too_many_arguments)]
pub async fn za_delete_event(
    server: &Server,
    token: &AccessToken,
    _resources: &GroupwareResources,
    keys: &SessionKeys,
    roles: &ZaRoles,
    owner_id: u32,
    document_id: u32,
    calendar_id: u32,
    event: Archive<ArchiveBytes>,
    content: Option<Archive<ArchiveBytes>>,
    href: Option<String>,
    send_itip: bool,
) -> Result<ZaDeleted, ZaPipelineError> {
    let context = za_mutation_context(
        server,
        token,
        owner_id,
        calendar_id,
        Bitmap::new().with_item(Acl::RemoveItems),
    )
    .await
    .map_err(|error| ZaPipelineError::from_calendar(error, owner_id, calendar_id))?
    .ok_or_else(|| {
        // Upgrade ruling 176 (B-M2): a key account's delete needs its key domain.
        ZaPipelineError::from_calendar(ZaCalendarError::Forbidden, owner_id, calendar_id)
    })?;
    let mut batch = BatchBuilder::new();
    context.assert_in(&mut batch);
    let cutoff = now() as i64;
    let retry_event = event.clone();
    let retry_href = href.clone();
    let message = DestroyArchive(
        event
            .to_unarchived::<CalendarEvent>()
            .caused_by(trc::location!())?,
    )
    .delete_za_at(
        server,
        &roles.sender,
        owner_id,
        document_id,
        calendar_id,
        content,
        href,
        send_itip,
        Some(&context.domain),
        cutoff,
        &mut batch,
    )
    .await
    .caused_by(trc::location!())?;

    let rebuild = |error: trc::Error| async move {
        let fresh = za_mutation_context(
            server,
            token,
            owner_id,
            calendar_id,
            Bitmap::new().with_item(Acl::RemoveItems),
        )
        .await
        .map_err(|error| ZaPipelineError::from_calendar(error, owner_id, calendar_id))?;
        if !calendars_changed(std::slice::from_ref(&context), fresh.as_slice()) {
            return Err(error.caused_by(trc::location!()).into());
        }
        let mut batch = BatchBuilder::new();
        if let Some(context) = &fresh {
            context.assert_in(&mut batch);
        }
        DestroyArchive(
            retry_event
                .to_unarchived::<CalendarEvent>()
                .caused_by(trc::location!())?,
        )
        .delete_za_at(
            server,
            &roles.sender,
            owner_id,
            document_id,
            calendar_id,
            None,
            retry_href,
            false,
            fresh.as_ref().map(|context| &context.domain),
            cutoff,
            &mut batch,
        )
        .await
        .caused_by(trc::location!())?;
        Ok(batch)
    };

    za_commit_delete(
        server,
        keys,
        roles,
        owner_id,
        batch,
        message,
        Some(rebuild),
        Vec::new(),
    )
    .await
}

/// Destroy a key account's event and every one of its names in one batch.
#[allow(clippy::too_many_arguments)]
pub async fn za_destroy_event(
    server: &Server,
    token: &AccessToken,
    resources: &GroupwareResources,
    keys: &SessionKeys,
    roles: &ZaRoles,
    owner_id: u32,
    document_id: u32,
    event: Archive<ArchiveBytes>,
    send_itip: bool,
) -> Result<ZaDeleted, ZaPipelineError> {
    let row = event
        .deserialize::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let addressed = row
        .names
        .first()
        .ok_or(ZaPipelineError::Forbidden(ZaRefusal::Context))?
        .parent_id;
    let contexts =
        za_write_contexts(server, token, owner_id, &row, addressed, Acl::RemoveItems).await?;
    if contexts.is_empty() {
        return Err(ZaPipelineError::Forbidden(ZaRefusal::Context));
    }
    let mut batch = BatchBuilder::new();
    for context in &contexts {
        context.assert_in(&mut batch);
    }
    let cutoff = now() as i64;
    let retry_event = event.clone();
    let message = enqueue_destroy_event(
        server,
        resources,
        roles,
        owner_id,
        document_id,
        &event,
        &contexts,
        send_itip,
        cutoff,
        &mut batch,
    )
    .await?;

    let rebuild = |error: trc::Error| async move {
        let fresh =
            za_write_contexts(server, token, owner_id, &row, addressed, Acl::RemoveItems).await?;
        if !calendars_changed(&contexts, &fresh) {
            return Err(error.caused_by(trc::location!()).into());
        }
        if fresh.is_empty() {
            return Err(ZaPipelineError::Forbidden(ZaRefusal::Context));
        }
        let mut batch = BatchBuilder::new();
        for context in &fresh {
            context.assert_in(&mut batch);
        }
        enqueue_destroy_event(
            server,
            resources,
            roles,
            owner_id,
            document_id,
            &retry_event,
            &fresh,
            false,
            cutoff,
            &mut batch,
        )
        .await?;
        Ok(batch)
    };

    za_commit_delete(
        server,
        keys,
        roles,
        owner_id,
        batch,
        message,
        Some(rebuild),
        Vec::new(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn enqueue_destroy_event(
    server: &Server,
    resources: &GroupwareResources,
    roles: &ZaRoles,
    owner_id: u32,
    document_id: u32,
    event: &Archive<ArchiveBytes>,
    contexts: &[ZaCalendarContext],
    send_itip: bool,
    cutoff: i64,
    batch: &mut BatchBuilder,
) -> Result<Option<ZaDeleteMessage>, ZaPipelineError> {
    let event = event
        .to_unarchived::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let names = &event.inner.names;
    let message = DestroyArchive(event)
        .remove_all_za_at(
            server,
            &roles.sender,
            owner_id,
            document_id,
            None,
            &mut AlarmTasks::None,
            send_itip,
            &ContainerCleanup::empty(Collection::CalendarEvent),
            Some(&contexts[0].domain),
            cutoff,
            batch,
        )
        .await
        .caused_by(trc::location!())?;
    for name in names.iter() {
        let parent_id = name.parent_id.to_native();
        if resources.container_resource_path_by_id(parent_id).is_none() {
            continue;
        }
        // Use this attempt's asserted calendar name, including after a rename.
        let parent = contexts
            .iter()
            .find(|context| context.calendar_id == parent_id)
            .expect("destroy must resolve every stored parent");
        let calendar = parent
            .stored
            .unarchive::<Calendar>()
            .caused_by(trc::location!())?;
        let calendar = canonical_calcard_segment(calendar.name.as_str());
        let name = canonical_calcard_segment(name.name.as_str());
        batch.log_calendar_vanished(
            parent_id,
            resources.format_item(&format!("{calendar}/{name}")),
        );
    }
    // Finalization writes the whole batch once, including alarm and container cleanup.
    Ok(message)
}

/// Delete a key account's calendar and its events, including scheduling when enabled.
#[allow(clippy::too_many_arguments)]
pub async fn za_delete_calendar(
    server: &Server,
    token: &AccessToken,
    resources: &GroupwareResources,
    keys: &SessionKeys,
    roles: &ZaRoles,
    owner_id: u32,
    calendar_id: u32,
    target: ZaCalendarDeleteTarget,
    children: Vec<u32>,
    cleanup: &ContainerCleanup,
    href: Option<String>,
    send_itip: bool,
) -> Result<ZaDeleted, ZaPipelineError> {
    let mut batch = BatchBuilder::new();
    // PR #90 review C4617611521: the calendar and every event are deleted in
    // one transaction, or nothing is (the per-event commit points become
    // no-ops).
    batch.za_one_transaction();
    let messages = DestroyArchive(
        target
            .stored
            .to_unarchived::<Calendar>()
            .caused_by(trc::location!())?,
    )
    .delete_with_events_za(
        server,
        token,
        &roles.sender,
        owner_id,
        calendar_id,
        children,
        &resources.event_ids_with_flags(EVENT_HAS_ALARMS),
        cleanup,
        href,
        send_itip,
        Some(keys),
        &mut batch,
    )
    .await
    .caused_by(trc::location!())?;

    // Reset default calendar id
    batch
        .with_account_id(owner_id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .clear_if_equals(PrincipalField::DefaultCalendarId, calendar_id);
    // A transaction past the store's chunk size is refused before the
    // fan-out, so no cancellation goes out for a delete that cannot commit.
    if batch.za_exceeds_one_chunk() {
        return Err(ZaPipelineError::CalendarTooLarge);
    }

    za_commit_delete(
        server,
        keys,
        roles,
        owner_id,
        batch,
        messages,
        None::<fn(trc::Error) -> Ready<_>>,
        target.deleted_grantees,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn za_commit_delete<F>(
    server: &Server,
    keys: &SessionKeys,
    roles: &ZaRoles,
    owner_id: u32,
    batch: BatchBuilder,
    messages: impl IntoIterator<Item = ZaDeleteMessage>,
    rebuild: Option<impl FnOnce(trc::Error) -> F>,
    deleted_grantees: Vec<u32>,
) -> Result<ZaDeleted, ZaPipelineError>
where
    F: Future<Output = Result<BatchBuilder, ZaPipelineError>>,
{
    // iMIP spec 5.4, decision 9: each fan-out's mail attempts wait for
    // the deletion's one commit.
    let mut za_mail = ZaImipPending::default();
    // Spec 5.4: the items first, immediately before the commit.
    for message in messages {
        let resolution = za_resolve_delegation(
            server,
            &roles.sender,
            &roles.writer,
            &roles.owner,
            ZaSelfOperation::Delete,
            Some(&message.event),
            None,
        )
        .await
        .caused_by(trc::location!())?;
        let first = message.messages.unwrap_or_default();
        let messages = if let Some(resolution) = &resolution {
            let second = za_self_cancel(
                &message.event,
                &roles.writer,
                &roles.owner,
                resolution,
                message.policy,
            )
            .ok()
            .flatten();
            za_combine_delegated(first, second, resolution)
        } else {
            first.into_iter().map(ZaFanOutMessage::upstream).collect()
        };
        za_mail.extend(
            server
                .za_fan_out(
                    &roles.sender,
                    &roles.writer,
                    &roles.owner,
                    keys,
                    messages,
                    message.pairs,
                    &message.event,
                    ZaSource {
                        document_id: message.document_id,
                        uid: &message.uid,
                    },
                    ZaFanOutKind::Deletion,
                    false,
                    message.policy,
                    None,
                    resolution.as_ref(),
                )
                .await
                .caused_by(trc::location!())?
                .mail,
        );
    }

    #[cfg(feature = "test_mode")]
    if rebuild.is_some() {
        common::za::test_hooks::delete_commit_pause_point(owner_id).await;
    }
    #[cfg(feature = "test_mode")]
    if common::za::test_hooks::take_delete_commit_fail(owner_id) {
        za_mail.dropped();
        return Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access test hook: delete commit failure")
            .into());
    }
    // iMIP spec 5.4: every error return of the commit drops the mail
    // attempts, traced, before returning.
    let committed: Result<_, ZaPipelineError> = async {
        let error = match server.commit_batch(batch).await {
            Ok(ids) => return Ok(ids),
            Err(error) => error,
        };
        let Some(rebuild) = rebuild.filter(|_| error.is_assertion_failure()) else {
            return Err(error.caused_by(trc::location!()).into());
        };
        // A calendar change (a grant, a rotation or a conversion) lost
        // the assertion: one bounded retry in the fresh domain, with
        // scheduling off (its items already went out).
        let batch = rebuild(error).await?;
        Ok(server
            .commit_batch(batch)
            .await
            .caused_by(trc::location!())?)
    }
    .await;
    let ids = match committed {
        Ok(ids) => ids,
        Err(error) => {
            za_mail.dropped();
            return Err(error);
        }
    };
    // The committed deletion retires every grant. Evict the grantees'
    // resident collection-key sessions and authorization only after that
    // commit succeeds, using the same channel as share mutations.
    let invalidated = if !deleted_grantees.is_empty() {
        let mut invalidation = CacheInvalidationBuilder::default();
        for grantee in deleted_grantees {
            invalidation.invalidate(CacheInvalidation::AccessToken(grantee));
        }
        server.invalidate_caches(invalidation).await
    } else {
        Ok(())
    };
    if invalidated.is_ok() {
        server.notify_task_queue();
    }
    // iMIP spec 5.4, section 7: the deletion (or its retry) committed, so
    // its mail goes whether or not the invalidation failed; the
    // invalidation's error is returned after.
    za_mail.submit(server).await;
    invalidated?;

    Ok(ZaDeleted {
        // A stale DAV path can resolve to an event whose calendar name was
        // already removed. The assertion-only commit emits no change ID.
        change_id: ids
            .change_id(owner_id, SyncCollection::Calendar)
            .unwrap_or_default(),
    })
}
