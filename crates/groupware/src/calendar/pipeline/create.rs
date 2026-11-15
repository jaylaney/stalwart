/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    ZaWritten,
    admission::ZaRoles,
    context::za_refresh_event,
    error::{ZaPipelineError, ZaRefusal},
    schedule::ZaScheduled,
    uid::{za_commit_create, za_lost_create_fence},
};
use crate::{
    calendar::{
        CalendarEvent, CalendarEventContent,
        fanout::{ZaFanOut, ZaFanOutKind, ZaSource, za_set_schedule_status},
        schedule::EventAlarms,
        seal::{ZaEvent, seal_error, seal_event_in_domain},
        sequence_floor::za_set_sequences,
        sharing::resolve::ZaCalendarContext,
    },
    scheduling::recipient::RecipientPolicy,
};
use common::{
    GroupwareResources, Server,
    auth::{AccessToken, AccountCache},
    za::{
        imip::ZaImipPending,
        lease::{ZaUidLease, ZaUidLeaseGuard},
    },
};
use store::write::{BatchBuilder, now};
use trc::AddContext;
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection},
};
use vault::session::SessionKeys;

/// What a key account's create carries from the handler's checks to its sealed commit.
pub struct ZaEventCreate {
    /// Every calendar name and the schedule tag from `za_schedule_create`.
    pub event: CalendarEvent,
    /// Data built with `CalendarEventData::new_for(.., true)`, with no preferences.
    pub content: CalendarEventContent,
    pub next_alarms: EventAlarms,
    pub scheduled: ZaScheduled,
    pub lease: ZaUidLeaseGuard,
    pub uid: String,
    /// From `za_write_contexts(.., names[0].parent_id, Acl::AddItems)`.
    pub contexts: Vec<ZaCalendarContext>,
}

/// A key account's create (invitations spec 5.1-5.4, key pinning spec
/// 5.1): the document id is assigned by a reservation-only batch before the
/// fan-out (ruling 14), which records it as a withheld entry's source; the
/// fan-out and the insert are fenced on the UID creation lease; the event is
/// sealed in its calendar's domain before upstream's builder; one bounded
/// retry follows a calendar change.
pub async fn za_create_event(
    server: &Server,
    access_token: &AccessToken,
    resources: &GroupwareResources,
    account: &AccountCache,
    keys: &SessionKeys,
    roles: &ZaRoles,
    create: ZaEventCreate,
) -> Result<ZaWritten, ZaPipelineError> {
    let ZaEventCreate {
        event,
        mut content,
        next_alarms,
        scheduled: ZaScheduled {
            messages,
            resolution,
        },
        lease,
        uid,
        mut contexts,
    } = create;
    if contexts.is_empty() {
        lease.release().await;
        return Err(ZaPipelineError::Forbidden(ZaRefusal::Context));
    }
    let account_id = account.id;
    let calendar_id = event.names[0].parent_id;
    let mut calendar_ids = Vec::new();
    for name in &event.names {
        if !calendar_ids.contains(&name.parent_id) {
            calendar_ids.push(name.parent_id);
        }
    }

    // Ruling 14: an id reserved by a committed batch is never reused.
    let mut reservation = BatchBuilder::new();
    let slot = reservation.reserve_document_id(account_id, Collection::CalendarEvent);
    let document_id = match server.commit_batch(reservation).await {
        Ok(assigned) => assigned.slot(slot),
        Err(err) => {
            lease.release().await;
            return Err(err.caused_by(trc::location!()).into());
        }
    };

    // Spec 5.3-5.4: after every check, immediately before the sealed commit;
    // the fan-out hands out the attendees' tokens. Fenced on the creation
    // lease: a creator whose lease was taken over writes no item after its
    // successor's.
    let mut pairs = None;
    // iMIP spec 5.4, decision 9: the fan-out's mail attempts wait for the
    // commit.
    let mut za_mail = ZaImipPending::default();
    if let Some(messages) = messages {
        let fan_out = match server
            .za_fan_out(
                &roles.sender,
                &roles.writer,
                &roles.owner,
                keys,
                messages,
                None,
                &content.data.event,
                ZaSource {
                    document_id,
                    uid: &uid,
                },
                ZaFanOutKind::Change,
                false,
                RecipientPolicy::new(&server.core.groupware, event.flags),
                Some(lease.lease()),
                resolution.as_ref(),
            )
            .await
        {
            Ok(fan_out) => fan_out,
            Err(err) if err.is_assertion_failure() => {
                let Err(error) = za_lost_create_fence(
                    server,
                    resources,
                    access_token,
                    lease,
                    account_id,
                    &calendar_ids,
                    &uid,
                )
                .await;
                return Err(error);
            }
            Err(err) => {
                lease.release().await;
                return Err(err.caused_by(trc::location!()).into());
            }
        };
        za_set_schedule_status(&mut content.data.event, &fan_out.statuses);
        za_set_sequences(&mut content.data.event, &fan_out.sequences);
        pairs = fan_out.pairs;
        za_mail = fan_out.mail;
    }

    let ical = content.data.event.clone();
    let schedule_tag = event.schedule_tag;
    let mut sealed = ZaEvent::new(event, content);
    if let Err(err) = seal_event_in_domain(&mut sealed, &contexts[0].domain, pairs.as_ref()) {
        lease.release().await;
        return Err(seal_error(err, account_id, document_id).into());
    }

    let timestamp = now() as i64;
    let build = |sealed: &ZaEvent,
                 contexts: &[ZaCalendarContext],
                 lease: &ZaUidLease|
     -> trc::Result<(BatchBuilder, String)> {
        let mut batch = BatchBuilder::new();
        batch.with_account_id(account_id);
        server.limit_disk_quota(&mut batch, account);
        lease.assert_in(&mut batch);
        for context in contexts {
            context.assert_in(&mut batch);
        }
        let (meta, content) = sealed.clone().into_parts();
        let etag = meta
            .insert_at(
                content,
                access_token.changed_by(),
                account_id,
                document_id,
                None,
                next_alarms.clone(),
                timestamp,
                &mut batch,
            )
            .caused_by(trc::location!())?;
        Ok((batch, etag))
    };
    let (batch, etag) = build(&sealed, &contexts, lease.lease())?;
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::commit_pause_point(account_id).await;
    #[cfg(feature = "test_mode")]
    if common::za::test_hooks::take_commit_fail(account_id) {
        lease.release().await;
        za_mail.dropped();
        return Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access test hook: sender commit failure")
            .into());
    }
    let (etag, change_id) = match server.commit_batch(batch).await {
        Ok(assigned) => {
            lease.release().await;
            (
                etag,
                assigned.last_change_id(account_id, SyncCollection::Calendar),
            )
        }
        Err(error) if error.is_assertion_failure() => {
            if let Some(fresh) = za_refresh_event(
                server,
                access_token,
                account_id,
                calendar_id,
                Acl::AddItems,
                &contexts,
                &mut sealed,
            )
            .await?
            {
                contexts = fresh;
                // If the retry build fails, drop the lease before the outer mail guard.
                let lease_guard = lease;
                let (batch, retry_etag) = build(&sealed, &contexts, lease_guard.lease())?;
                let change_id = za_mail.on_err(
                    za_commit_create(
                        server,
                        resources,
                        access_token,
                        batch,
                        lease_guard,
                        account_id,
                        &calendar_ids,
                        &uid,
                    )
                    .await,
                )?;
                (retry_etag, change_id)
            } else {
                za_mail.dropped();
                let Err(error) = za_lost_create_fence(
                    server,
                    resources,
                    access_token,
                    lease,
                    account_id,
                    &calendar_ids,
                    &uid,
                )
                .await;
                return Err(error);
            }
        }
        Err(error) => {
            lease.release().await;
            za_mail.dropped();
            return Err(error.caused_by(trc::location!()).into());
        }
    };
    server.notify_task_queue();
    // iMIP spec 5.4: reached only once the commit (or `za_commit_create`'s
    // retry) returned `Ok`; every failure returned above.
    za_mail.submit(server).await;

    Ok(ZaWritten {
        document_id,
        change_id,
        etag,
        schedule_tag,
        ical,
    })
}
