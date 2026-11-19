/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    ZaDeleted,
    admission::ZaRoles,
    context::{za_mutation_context, za_write_contexts},
    error::{ZaBusy, ZaPipelineError, ZaRefusal},
};
use crate::{
    DestroyArchive,
    cache::GroupwareCache,
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
use std::{
    future::{Future, Ready},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use store::{
    Deserialize, ValueKey,
    write::{Archive, ArchiveBytes, BatchBuilder, MergeResult, ValueClass, now},
};
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
        None,
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
        None,
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
        // Use this attempt's asserted calendar name, including after a rename.
        // PR #90 review C4617611584: the href is logged from the asserted
        // parent, whether or not the request's snapshot knows the calendar.
        let parent = contexts
            .iter()
            .find(|context| context.calendar_id == parent_id)
            .ok_or(ZaPipelineError::Forbidden(ZaRefusal::Context))?;
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
    cleanup: &ContainerCleanup,
    href: Option<String>,
    send_itip: bool,
) -> Result<ZaDeleted, ZaPipelineError> {
    let mut batch = BatchBuilder::new();
    // PR #90 second review C4633314503: the calendar's members come from the
    // snapshot's stored records, not from the path index, which shows one
    // document per href.
    let children = za_calendar_members(resources, calendar_id);
    // PR #90 review RA-2: FoundationDB's transaction limits and stale reads,
    // a read replica's lagging reads and the ephemeral store's missing
    // rollback do not give the one transaction and the change-id fence their
    // contract; those stores keep the earlier, chunked delete without them.
    // The fenced branch adds the one transaction, the unwritten-child
    // refusal, the size refusal (with its reserve) and the fence; on every
    // store the members come from the stored records and every name under
    // the calendar is removed, which are part of what the delete writes.
    let fenced = !(server.store().is_foundationdb()
        || server.store().is_read_replica()
        || server.store().is_ephemeral());
    #[cfg(feature = "test_mode")]
    let fenced = fenced && !common::za::test_hooks::calendar_delete_unfenced(owner_id);
    // PR #90 review C4617611521: the calendar and every event are deleted in
    // one transaction, or nothing is (the per-event commit points become
    // no-ops).
    let fence = fenced.then(|| {
        batch.za_one_transaction();
        ZaChildFence::new(token, calendar_id, &children)
    });
    let built = DestroyArchive(
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
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::calendar_delete_built_pause_point(owner_id).await;

    if let Some(fence) = &fence {
        // PR #90 second review C4633314478: a listed child the build wrote
        // nothing for is not asserted, so it could name the calendar again at
        // the commit; refused before the size check and any cancellation.
        if built.unwritten > 0 {
            return Err(ZaPipelineError::Busy(ZaBusy::CalendarChanged));
        }
        // The default calendar id is reset by the fence's merge operation
        // (`ZaChildFence::assert_in`), in the same transaction.
        // A cap on the size of the one transaction, by the builder's own
        // count (which leaves out assertions and the change-log record),
        // checked before the fan-out so a refusal sends no cancellation.
        // PR #90 second review C4633314510: the reserve is what the fence's
        // merge operation, added after this check, counts.
        let reserve = ValueClass::from(PrincipalField::DefaultCalendarId).key_len_hint();
        if batch.za_exceeds_one_chunk(reserve) {
            return Err(ZaPipelineError::CalendarTooLarge);
        }
        // PR #90 review C4617611506: a stale child list is refused before any
        // cancellation is delivered.
        fence.current(server, owner_id).await?;
    } else {
        // Reset default calendar id
        batch
            .with_account_id(owner_id)
            .with_collection(Collection::Principal)
            .with_document(0)
            .clear_if_equals(PrincipalField::DefaultCalendarId, calendar_id);
    }

    za_commit_delete(
        server,
        keys,
        roles,
        owner_id,
        batch,
        built.messages,
        None::<fn(trc::Error) -> Ready<_>>,
        target.deleted_grantees,
        fence,
    )
    .await
}

/// PR #90 second review C4633314503: every event document with a name under
/// `calendar_id`, read from the snapshot's resource records, sorted. The path
/// index keeps one document per href, so two documents at one href show there
/// as one.
pub fn za_calendar_members(resources: &GroupwareResources, calendar_id: u32) -> Vec<u32> {
    let mut members = resources
        .resources
        .iter_run(false)
        .filter(|resource| {
            resource
                .child_names()
                .iter()
                .any(|name| name.parent_id == calendar_id)
        })
        .map(|resource| resource.document_id())
        .collect::<Vec<_>>();
    members.sort_unstable();
    members.dedup();
    members
}

/// PR #90 review C4617611506: a calendar delete's child list, fenced against
/// the account's Calendar change counter. A create or a placement into the
/// calendar asserts only the calendar archive, which the delete leaves as it
/// was, and writes nothing the delete reads; the counter is the one key both
/// write. `current` compares the list with a snapshot no older than the
/// counter; `assert_in` adds the merge operation that fails the delete's
/// commit unless the change id assigned to it is the next one, so nothing
/// was committed to the account's calendars in between. It relies on the
/// delete being one transaction (`za_one_transaction`) and on the store
/// allocating a batch's change ids before its merge functions run, and
/// aborting the transaction on their error (`za_change_id_fence_tests`).
struct ZaChildFence<'x> {
    token: &'x AccessToken,
    calendar_id: u32,
    children: Vec<u32>,
    lost: Arc<AtomicBool>,
}

impl<'x> ZaChildFence<'x> {
    fn new(token: &'x AccessToken, calendar_id: u32, children: &[u32]) -> Self {
        let mut children = children.to_vec();
        children.sort_unstable();
        children.dedup();
        ZaChildFence {
            token,
            calendar_id,
            children,
            lost: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The counter's value once the child list is checked against a snapshot
    /// that holds every change up to it; `Busy` when the lists differ.
    async fn current(&self, server: &Server, owner_id: u32) -> Result<u64, ZaPipelineError> {
        let counter = server
            .store()
            .get_counter(ValueKey {
                account_id: owner_id,
                collection: 0,
                document_id: 0,
                class: ValueClass::ChangeId(SyncCollection::Calendar.into()),
            })
            .await
            .caused_by(trc::location!())?;
        let tip =
            u64::try_from(counter).map_err(|_| ZaPipelineError::Busy(ZaBusy::CalendarChanged))?;
        let resources = server
            .fetch_groupware_resources_at(
                self.token.account_id(),
                owner_id,
                SyncCollection::Calendar,
                tip,
            )
            .await
            .caused_by(trc::location!())?;
        if za_calendar_members(&resources, self.calendar_id) != self.children {
            return Err(ZaPipelineError::Busy(ZaBusy::CalendarChanged));
        }
        Ok(tip)
    }

    /// The merge operation on the account's default calendar id: it fails
    /// the commit unless the batch's Calendar change id is `tip + 1`, and
    /// otherwise clears the id when it names the deleted calendar.
    fn assert_in(&self, owner_id: u32, tip: u64, batch: &mut BatchBuilder) {
        let (calendar_id, lost) = (self.calendar_id, self.lost.clone());
        let expected = tip + 1;
        batch
            .with_account_id(owner_id)
            .with_collection(Collection::Principal)
            .with_document(0)
            .merge_fnc(PrincipalField::DefaultCalendarId, move |ids, bytes| {
                if ids.change_id(owner_id, SyncCollection::Calendar) != Some(expected) {
                    lost.store(true, Ordering::Relaxed);
                    return Err(trc::StoreEvent::AssertValueFailed.into_err());
                }
                match bytes {
                    Some(bytes) if u32::deserialize(bytes)? == calendar_id => {
                        Ok(MergeResult::Delete)
                    }
                    _ => Ok(MergeResult::Skip),
                }
            });
    }

    fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }
}

#[allow(clippy::too_many_arguments)]
async fn za_commit_delete<F>(
    server: &Server,
    keys: &SessionKeys,
    roles: &ZaRoles,
    owner_id: u32,
    mut batch: BatchBuilder,
    messages: impl IntoIterator<Item = ZaDeleteMessage>,
    rebuild: Option<impl FnOnce(trc::Error) -> F>,
    deleted_grantees: Vec<u32>,
    fence: Option<ZaChildFence<'_>>,
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
    // PR #90 review C4617611506: the calendar delete's child list once more,
    // immediately before the commit, and its change-id fence.
    if let Some(fence) = &fence {
        match fence.current(server, owner_id).await {
            Ok(tip) => fence.assert_in(owner_id, tip, &mut batch),
            Err(error) => {
                za_mail.dropped();
                return Err(error);
            }
        }
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::calendar_delete_fence_pause_point(owner_id).await;
    }
    // iMIP spec 5.4: every error return of the commit drops the mail
    // attempts, traced, before returning.
    let committed: Result<_, ZaPipelineError> = async {
        let error = match server.commit_batch(batch).await {
            Ok(ids) => return Ok(ids),
            Err(_) if fence.as_ref().is_some_and(ZaChildFence::is_lost) => {
                return Err(ZaPipelineError::Busy(ZaBusy::CalendarChanged));
            }
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
