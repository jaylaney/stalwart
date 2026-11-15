/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    ZaScheduling,
    admission::ZaRoles,
    error::{ZaPipelineError, za_itip_fails_write},
};
use crate::{
    calendar::{
        CalendarEvent, CalendarEventContent,
        delegation::{
            ZaDelegationResolution, ZaSelfMessages, ZaSelfOperation, za_combine_delegated,
            za_finalize, za_resolve_delegation, za_self_create, za_self_update,
        },
        fanout::ZaFanOutMessage,
        itip::ItipSendStatus,
    },
    scheduling::{
        ItipError, event_create::itip_create, event_update::itip_update,
        itip::itip_set_unreachable_status, recipient::RecipientPolicy,
    },
};
use calcard::icalendar::ICalendar;
use common::{Server, auth::AccessToken};
use compact_str::ToCompactString;
use trc::AddContext;

pub struct ZaScheduled {
    pub messages: Option<Vec<ZaFanOutMessage>>,
    pub resolution: Option<ZaDelegationResolution>,
}

/// Invitations spec 5.1 (release 2) and delegated scheduling (release 4): a
/// key account's scheduling (`SendSealed`). Upstream's builders run on the
/// plaintext tree, with the writer's self-delegated run beside them; the
/// messages go to the fan-out, never to the task queue. `None` when nothing
/// was built. `old` is `Some` for an update.
#[allow(clippy::too_many_arguments)]
pub fn za_schedule(
    server: &Server,
    roles: &ZaRoles,
    resolution: Option<&ZaDelegationResolution>,
    old: Option<&ICalendar>,
    ical: &mut ICalendar,
    policy: RecipientPolicy,
    schedule_tag: &mut Option<u32>,
    owner_id: u32,
    document_id: Option<u32>,
) -> Result<Option<Vec<ZaFanOutMessage>>, ZaPipelineError> {
    // The second run sees the pre-finalize trees.
    let second = match resolution {
        Some(resolution) => za_second_run(match old {
            Some(old) => za_self_update(old, ical, &roles.writer, &roles.owner, resolution, policy),
            None => za_self_create(ical, &roles.writer, &roles.owner, resolution, policy),
        })?,
        None => None,
    };
    let result = match old {
        Some(old) => itip_update(ical, old, roles.sender.addresses(), policy),
        None => itip_create(ical, roles.sender.addresses(), policy),
    };
    let first_built = result.is_ok();
    let first_nothing = matches!(&result, Err(ItipError::NothingToSend));
    let first = match result {
        Ok(messages) => Some(messages),
        Err(err) => {
            if za_itip_fails_write(&err) {
                return Err(ZaPipelineError::Scheduling(err));
            }
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageError),
                AccountId = owner_id,
                DocumentId = document_id,
                Reason = err.to_compact_string(),
            );
            // Preserve the original error-arm bump exactly once.
            if old.is_some()
                && let Some(schedule_tag) = schedule_tag
            {
                *schedule_tag += 1;
            }
            None
        }
    };
    if !first_built && second.is_none() {
        return Ok(None);
    }
    if first_nothing
        && let Some(second) = &second
        && second.finalize()
    {
        za_finalize(ical, second.increments());
    }
    if old.is_some() {
        let is_organizer = second.is_some()
            || first
                .as_ref()
                .is_some_and(|messages| messages.iter().any(|message| message.from_organizer));
        if is_organizer {
            if first_built {
                if let Some(schedule_tag) = schedule_tag {
                    *schedule_tag += 1;
                } else {
                    *schedule_tag = Some(1);
                }
            } else if schedule_tag.is_none() {
                *schedule_tag = Some(1);
            }
        }
    } else {
        *schedule_tag = Some(1);
    }
    let first = first.unwrap_or_default();
    let messages = match resolution {
        Some(resolution) => za_combine_delegated(first, second, resolution),
        None => first.into_iter().map(ZaFanOutMessage::upstream).collect(),
    };
    if messages
        .iter()
        .map(ZaFanOutMessage::recipient_count)
        .sum::<usize>()
        >= server.core.groupware.itip_outbound_max_recipients
    {
        return Err(ZaPipelineError::TooManyRecipients);
    }
    Ok(Some(messages))
}

/// PR review ruling 197: the delegated second run's outcome. A failed
/// precondition (the per-recipient split's `TooManyRecipients` among them,
/// since upgrade ruling 93) fails the PUT with the first run's 412, before
/// any fan-out or persistence; `NothingToSend` and the other errors without
/// one stay a run that built nothing, as before.
pub fn za_second_run(
    result: Result<Option<ZaSelfMessages>, ItipError>,
) -> Result<Option<ZaSelfMessages>, ZaPipelineError> {
    match result {
        Ok(second) => Ok(second),
        Err(err) if za_itip_fails_write(&err) => Err(ZaPipelineError::Scheduling(err)),
        Err(_) => Ok(None),
    }
}

/// Resolves create scheduling and updates the tree's statuses and schedule tag.
pub async fn za_schedule_create(
    server: &Server,
    token: &AccessToken,
    roles: &ZaRoles,
    schedule: ZaScheduling,
    event: &mut CalendarEvent,
    content: &mut CalendarEventContent,
    owner_id: u32,
) -> Result<ZaScheduled, ZaPipelineError> {
    let status = match schedule {
        ZaScheduling::Off => ItipSendStatus::NotRequested,
        ZaScheduling::On | ZaScheduling::Required => ItipSendStatus::resolve(
            server,
            token,
            &roles.sender,
            content.data.event_range_end(),
            true,
        ),
    };
    if status.is_send_sealed() {
        let resolution = za_resolve_delegation(
            server,
            &roles.sender,
            &roles.writer,
            &roles.owner,
            ZaSelfOperation::Create,
            None,
            Some(&content.data.event),
        )
        .await
        .caused_by(trc::location!())?;
        let messages = za_schedule(
            server,
            roles,
            resolution.as_ref(),
            None,
            &mut content.data.event,
            RecipientPolicy::new(&server.core.groupware, event.flags),
            &mut event.schedule_tag,
            owner_id,
            None,
        )?;
        itip_set_unreachable_status(&mut content.data.event, roles.sender.addresses());
        Ok(ZaScheduled {
            messages,
            resolution,
        })
    } else {
        if let Some(reason) = status.reason() {
            if schedule == ZaScheduling::Required && status.is_denied() {
                return Err(ZaPipelineError::SchedulingDenied(reason));
            }
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageError),
                AccountId = owner_id,
                Reason = reason,
            );
        }
        Ok(ZaScheduled {
            messages: None,
            resolution: None,
        })
    }
}

/// Resolves update scheduling. An update of user data alone has no old tree.
#[allow(clippy::too_many_arguments)]
pub async fn za_schedule_update(
    server: &Server,
    token: &AccessToken,
    roles: &ZaRoles,
    schedule: ZaScheduling,
    old: Option<&ICalendar>,
    new_event: &mut CalendarEvent,
    new_content: &mut CalendarEventContent,
    owner_id: u32,
    document_id: u32,
) -> Result<ZaScheduled, ZaPipelineError> {
    let status = match (schedule, old) {
        (ZaScheduling::Off, _) | (_, None) => ItipSendStatus::NotRequested,
        (ZaScheduling::On | ZaScheduling::Required, Some(_)) => ItipSendStatus::resolve(
            server,
            token,
            &roles.sender,
            new_content.data.event_range_end(),
            true,
        ),
    };
    if status.is_send_sealed() {
        // Invitations spec 5.1: upstream's builders on the plaintext
        // tree; the messages go to the fan-out.
        let is_update = new_event.schedule_tag.is_some() && old.is_some();
        let resolution = za_resolve_delegation(
            server,
            &roles.sender,
            &roles.writer,
            &roles.owner,
            if is_update {
                ZaSelfOperation::Update
            } else {
                ZaSelfOperation::Create
            },
            old,
            Some(&new_content.data.event),
        )
        .await
        .caused_by(trc::location!())?;
        let messages = za_schedule(
            server,
            roles,
            resolution.as_ref(),
            old.filter(|_| is_update),
            &mut new_content.data.event,
            RecipientPolicy::new(&server.core.groupware, new_event.flags),
            &mut new_event.schedule_tag,
            owner_id,
            Some(document_id),
        )?;
        itip_set_unreachable_status(&mut new_content.data.event, roles.sender.addresses());
        Ok(ZaScheduled {
            messages,
            resolution,
        })
    } else {
        if let Some(reason) = status.reason() {
            if schedule == ZaScheduling::Required && status.is_denied() {
                return Err(ZaPipelineError::SchedulingDenied(reason));
            }
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageError),
                AccountId = owner_id,
                DocumentId = document_id,
                Reason = reason,
            );
        }
        Ok(ZaScheduled {
            messages: None,
            resolution: None,
        })
    }
}
