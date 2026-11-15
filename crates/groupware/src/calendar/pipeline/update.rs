/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    ZaWritten,
    admission::ZaRoles,
    context::{za_refresh_event, za_write_contexts},
    error::{ZaPipelineError, ZaRefusal},
    placement::{ZaPlacementContexts, ZaPlacementPlan},
    schedule::ZaScheduled,
};
use crate::{
    calendar::{
        CalendarEvent, CalendarEventContent,
        fanout::{ZaFanOut, ZaFanOutKind, ZaSource, za_set_schedule_status},
        schedule::{AlarmTasks, EventAlarmScheduler, EventAlarms},
        seal::{
            ZaEvent, ZaEventView, event_pairs_in_domain, event_view_in_domain, seal_error,
            seal_event_in_domain,
        },
        sequence_floor::za_set_sequences,
        sharing::resolve::ZaCalendarContext,
    },
    scheduling::recipient::RecipientPolicy,
};
use common::{
    GroupwareResources, Server,
    auth::{AccessToken, AccountCache},
    za::imip::ZaImipPending,
};
use store::write::{Archive, ArchiveBytes, BatchBuilder, now};
use trc::AddContext;
use types::{acl::Acl, collection::SyncCollection};
use vault::session::SessionKeys;

pub struct ZaUpdateTarget {
    /// Every parent, resolved with `Acl::ModifyItems`.
    pub contexts: Vec<ZaCalendarContext>,
    /// Opened in the first context's domain.
    pub view: ZaEventView,
}

/// Resolve the write contexts and open the event for the handler's checks.
pub async fn za_update_target(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    document_id: u32,
    addressed: u32,
    event: &CalendarEvent,
    content: &Archive<ArchiveBytes>,
) -> Result<ZaUpdateTarget, ZaPipelineError> {
    let contexts =
        za_write_contexts(server, token, owner_id, event, addressed, Acl::ModifyItems).await?;
    let context = contexts
        .first()
        .ok_or(ZaPipelineError::Forbidden(ZaRefusal::Context))?;
    let view = event_view_in_domain(event, content, &context.domain)
        .map_err(|err| seal_error(err, owner_id, document_id))?;
    Ok(ZaUpdateTarget { contexts, view })
}

pub struct ZaEventUpdate<'x> {
    pub document_id: u32,
    /// The calendar a retry resolves from.
    pub addressed: u32,
    pub stored_event: &'x Archive<ArchiveBytes>,
    pub stored_content: &'x Archive<ArchiveBytes>,
    /// The names after the update and the schedule tag from `za_schedule_update`.
    pub new_event: CalendarEvent,
    /// Data built with `CalendarEventData::new_for(.., true)` after the sequence floor.
    pub new_content: CalendarEventContent,
    /// The stored display name from the view (ruling 15).
    pub display_name: Option<String>,
    pub prev_alarms: EventAlarms,
    pub next_alarms: EventAlarms,
    pub scheduled: ZaScheduled,
    pub contexts: ZaPlacementContexts,
    /// A placement change is handled by Task 11. CalDAV PUT passes `None`.
    pub placement: Option<ZaPlacementPlan>,
}

/// Fan out, seal and commit an update using the stored archives as its current values.
pub async fn za_commit_update(
    server: &Server,
    access_token: &AccessToken,
    _resources: &GroupwareResources,
    account: &AccountCache,
    keys: &SessionKeys,
    roles: &ZaRoles,
    update: ZaEventUpdate<'_>,
) -> Result<ZaWritten, ZaPipelineError> {
    if update.placement.is_some() {
        return Err(ZaPipelineError::Store(
            trc::StoreEvent::UnexpectedError
                .into_err()
                .details("zero-access: placement change not implemented")
                .caused_by(trc::location!()),
        ));
    }

    let ZaEventUpdate {
        document_id,
        addressed,
        stored_event,
        stored_content,
        mut new_event,
        mut new_content,
        display_name,
        prev_alarms,
        next_alarms,
        scheduled,
        mut contexts,
        placement: _,
    } = update;
    let account_id = account.id;
    let event = stored_event
        .to_unarchived::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let content = stored_content
        .to_unarchived::<CalendarEventContent>()
        .caused_by(trc::location!())?;

    // Invitations spec 3.3: every rewrite carries the stored pairs.
    let mut pairs = event_pairs_in_domain(
        &event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?,
        stored_content,
        contexts.from_domain(),
    )
    .map_err(|err| seal_error(err, account_id, document_id))?;
    // Spec 5.3-5.4: after every check of the request and
    // immediately before the sealed commit, the items go out; the
    // fan-out decides the pairs the bundle keeps. Its mail
    // attempts wait for the commit (iMIP spec 5.4, decision 9).
    let mut za_mail = ZaImipPending::default();
    if let Some(messages) = scheduled.messages {
        // The stored UID, which the new one was validated equal to.
        let fan_out = server
            .za_fan_out(
                &roles.sender,
                &roles.writer,
                &roles.owner,
                keys,
                messages,
                pairs,
                &new_content.data.event,
                ZaSource {
                    document_id,
                    uid: event.inner.uid.as_str(),
                },
                ZaFanOutKind::Change,
                false,
                RecipientPolicy::new(&server.core.groupware, new_event.flags),
                None,
                scheduled.resolution.as_ref(),
            )
            .await
            .caused_by(trc::location!())?;
        za_set_schedule_status(&mut new_content.data.event, &fan_out.statuses);
        za_set_sequences(&mut new_content.data.event, &fan_out.sequences);
        pairs = fan_out.pairs;
        za_mail = fan_out.mail;
    }
    // Ruling 15: the name stays in the sealed bundle.
    new_event.display_name = display_name;
    let schedule_tag = new_event.schedule_tag;
    let ical = new_content.data.event.clone();
    let mut sealed = ZaEvent::new(new_event, new_content);
    seal_event_in_domain(&mut sealed, contexts.to_domain(), pairs.as_ref())
        .map_err(|err| seal_error(err, account_id, document_id))?;
    // The same event and alarm intent is rebuilt once, without
    // scheduling, after a calendar change.
    let timestamp = now() as i64;
    for attempt in 0..=1 {
        let mut batch = BatchBuilder::new();
        batch.with_account_id(account_id);
        server.limit_disk_quota(&mut batch, account);
        contexts.assert_in(&mut batch);
        let (meta, sealed_content) = sealed.clone().into_parts();
        let etag = meta
            .update_full_at(
                sealed_content,
                access_token.changed_by(),
                event.clone(),
                content.inner,
                account_id,
                document_id,
                None,
                timestamp,
                &mut batch,
            )
            .caused_by(trc::location!())?;
        server
            .replace_event_alarms(
                account_id,
                document_id,
                prev_alarms.clone(),
                next_alarms.clone(),
                &mut AlarmTasks::None,
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::update_commit_pause_point(account_id).await;
        #[cfg(feature = "test_mode")]
        if attempt == 0 && common::za::test_hooks::take_update_commit_fail(account_id) {
            za_mail.dropped();
            return Err(trc::StoreEvent::UnexpectedError
                .into_err()
                .details("zero-access test hook: update commit failure")
                .into());
        }
        match server.commit_batch(batch).await {
            Ok(assigned) => {
                server.notify_task_queue();
                // iMIP spec 5.4: mail only once the commit
                // (or the bounded retry's) returned `Ok`.
                za_mail.submit(server).await;
                return Ok(ZaWritten {
                    document_id,
                    change_id: assigned.last_change_id(account_id, SyncCollection::Calendar),
                    etag,
                    schedule_tag,
                    ical,
                });
            }
            Err(error) if error.is_assertion_failure() && attempt == 0 => {
                if let Some(fresh) = za_refresh_event(
                    server,
                    access_token,
                    account_id,
                    addressed,
                    Acl::ModifyItems,
                    &contexts.parents,
                    &mut sealed,
                )
                .await?
                {
                    contexts = ZaPlacementContexts::unchanged(fresh);
                } else {
                    za_mail.dropped();
                    return Err(error.caused_by(trc::location!()).into());
                }
            }
            Err(error) => {
                za_mail.dropped();
                return Err(error.caused_by(trc::location!()).into());
            }
        }
    }
    unreachable!("bounded event retry always returns");
}
