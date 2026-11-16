/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 spec 6.5: `CalendarEvent/set` for a key account. Upstream's
//! private validation runs in upstream's order (D1, D17); the write is the
//! pipeline's create under a UID lease, one commit per object (D3), with
//! scheduling as `sendSchedulingMessages` says (D4, D18) and default alerts
//! applied by value (D11). A store error answers that object and every later
//! one (D28).
//!
//! Until Tasks 31 to 33 land, updates and destroys are answered `forbidden`
//! with `ZA_INTERIM` and never reach upstream's path (I §7.3).

use super::{
    DEFAULTED_FLAGS, EventWrite, PatchTarget, add_calendar_forbidden, assert_participants_limit,
    clamp_created, generate_uid, stamp_created, stamp_updated, too_large_event, too_many_calendars,
    too_many_events, update_calendar_event,
};
use crate::{
    api::{
        metadata::{
            MetadataPatches, MetadataType, MetadataWriter, ObjectMetadata, ZA_METADATA_REFUSED,
        },
        za_calendar::{
            ZA_CALENDAR_ATTACHMENTS, ZA_CALENDAR_DRAFTS, ZA_CALENDAR_PARTICIPANT_FLAG,
            za_set_incomplete,
        },
    },
    blob::embedded::EmbeddedExport,
    calendar_event::{
        EventMainComponent, EventValue, JSCalendarEntries, duplicate_uid, is_origin,
        privacy::{UidPrivacyConflicts, assert_privacy_allowed, uid_privacy_conflict},
        server_set::{BlobProperties, ImportedValues, ServerSetValues, TrackedValues},
        user::user_data_error,
    },
    changes::state::JmapCacheState,
};
use calcard::{
    common::timezone::Tz,
    jscalendar::{JSCALENDAR_VERSION, JSCalendar, JSCalendarProperty},
};
use common::{
    DavName, GroupwareResources, Server,
    auth::{AccessToken, AccountCache, AccountInfo},
    storage::quota::{DiskQuotaBudget, ObjectQuotaUsage},
    za::calendar::za_calendar_keyless_refusal,
};
use groupware::{
    SizeWriter,
    cache::GroupwareCache,
    calendar::{
        CalendarEvent, CalendarEventContent, CalendarEventData,
        alerts::{DefaultAlerts, DefaultAlertsResolver},
        identity::{CalendarAddresses, ParticipantIdentityAddresses},
        index::ICalendarObjectUid,
        pipeline::{
            ZaScheduling,
            admission::{ZaRoles, za_roles, za_writer_keys},
            context::za_write_contexts,
            create::{ZaEventCreate, za_create_event},
            error::ZaPipelineError,
            schedule::{ZaScheduled, za_schedule_create},
            uid::za_take_uid_lease,
        },
        privacy::EventViewer,
        rights::EventAcl,
        schedule::{EventAlarmScheduler, EventAlarmUsers, EventAlarms},
        sharing::resolve::ZaCalendarContext,
        user::ICalendarUserData,
        za_alerts::za_apply_default_alerts_by_value,
    },
    scheduling::itip::itip_assign_organizer,
};
use jmap_proto::{
    error::set::SetError,
    method::set::{SetRequest, SetResponse},
    object::calendar_event,
    request::capability::CapabilityIds,
    types::state::State,
};
use jmap_tools::{Key, Value};
use registry::schema::enums::StorageQuota;
use std::sync::Arc;
use store::{roaring::RoaringBitmap, write::now};
use trc::{AddContext, EventType, LimitEvent, SecurityEvent};
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection},
    id::Id,
};
use vault::session::SessionKeys;

/// I §7.3: the answer to an update or a destroy until Tasks 31 to 33.
pub(super) const ZA_INTERIM: &str = "This operation is not available for this account yet.";

/// Upstream's text for a lost assertion (`calendar_event/set.rs:1611`).
const ZA_MODIFIED: &str = "Another process modified this calendar event, please try again.";

type Answer<T> = trc::Result<Result<T, SetError<JSCalendarProperty<Id>>>>;

/// What spec 6.5 takes out of the event's own patch.
pub(super) struct ZaRulings {
    pub use_default_alerts: bool,
}

struct ZaCreated {
    document_id: u32,
    change_id: u64,
    server_set: ServerSetValues,
}

/// The state every object of one call shares: the head's values, read once.
struct ZaEventWriter<'x> {
    server: &'x Server,
    access_token: &'x AccessToken,
    account_id: u32,
    cache: Arc<GroupwareResources>,
    account_info: AccountInfo,
    identities: CalendarAddresses,
    uid_privacy_conflicts: UidPrivacyConflicts,
    can_add_calendars: Option<RoaringBitmap>,
    quota: ObjectQuotaUsage,
    schedule: ZaScheduling,
    default_alerts: DefaultAlertsResolver,
    disk_quota: DiskQuotaBudget,
    metadata: MetadataWriter,
    keys: Arc<SessionKeys>,
    roles: ZaRoles,
    account: Arc<AccountCache>,
}

pub(super) async fn za_calendar_event_set(
    server: &Server,
    mut request: SetRequest<'_, calendar_event::CalendarEvent>,
    access_token: &AccessToken,
    using: CapabilityIds,
) -> trc::Result<SetResponse<calendar_event::CalendarEvent>> {
    // Head, once per call (upstream's `calendar_event/set.rs:190-285`).
    let account_id = request.account_id.document_id();
    let metadata = ObjectMetadata::new(server, access_token, using, MetadataType::CalendarEvent);
    let sampled = metadata.viewer_change_id(server, account_id).await?;
    let cache = server
        .fetch_groupware_resources(
            access_token.account_id(),
            account_id,
            SyncCollection::Calendar,
        )
        .await?;
    let account_info = server
        .account_info(account_id)
        .await
        .caused_by(trc::location!())?;
    let mut response = SetResponse::from_request(&request, server.core.jmap.set_max_objects)?
        .with_state(sampled.assert_state(cache.get_state(false), &request.if_in_state)?);
    let will_destroy = response.collect_will_destroy(request.unwrap_destroy());
    let is_owner = access_token.is_member(account_id);
    let identities = if is_owner {
        CalendarAddresses::default()
    } else {
        server
            .account_identity_addresses(access_token.account_id())
            .await
            .caused_by(trc::location!())?
    };
    let uid_privacy_conflicts = UidPrivacyConflicts::new(
        &cache,
        request.create.as_ref(),
        request.update.as_ref(),
        &will_destroy,
    );

    // Obtain calendarIds
    let is_shared = access_token.is_shared(account_id);
    // The remove rights are read for destroys, which Task 33 serves.
    let (can_add_calendars, _can_delete_calendars) = if is_shared {
        (
            cache
                .shared_containers(access_token, [Acl::AddItems], true)
                .into(),
            cache
                .shared_containers(access_token, [Acl::RemoveItems], true)
                .into(),
        )
    } else {
        (None, None)
    };

    // Obtain quota
    let quota = if request.has_creates() {
        let account = server
            .account(account_id)
            .await
            .caused_by(trc::location!())?;
        server.object_quota_usage(&account, StorageQuota::MaxCalendarEvents, || {
            cache.resources.count(false)
        })
    } else {
        ObjectQuotaUsage::unlimited()
    };

    // D4, D18: true requires scheduling; false or absent turns it off.
    let schedule = if request.arguments.send_scheduling_messages.unwrap_or(false) {
        ZaScheduling::Required
    } else {
        ZaScheduling::Off
    };
    let default_alerts = DefaultAlertsResolver::with_resources(account_id, cache.clone());
    let disk_quota = if request.has_creates() || request.has_updates() {
        server
            .disk_quota_budget(account_info.account())
            .await
            .caused_by(trc::location!())?
    } else {
        DiskQuotaBudget::unlimited()
    };
    let metadata = MetadataWriter::new(metadata, account_id);

    // Spec 6.1: the writer's own keys, its roles and the owner's account.
    let keys = match za_writer_keys(server, access_token, account_id).await {
        Ok(Some(keys)) => keys,
        // Spec 8 (ruling R28-2): a store error before the first object fails the method.
        Err(ZaPipelineError::Store(error)) => return Err(error),
        Ok(None) | Err(_) => return Err(za_calendar_keyless_refusal().account_id(account_id)),
    };
    let roles = za_roles(server, access_token, account_id)
        .await
        .map_err(|error| match error {
            ZaPipelineError::Store(error) => error,
            _ => za_calendar_keyless_refusal().account_id(account_id),
        })?;
    let account = server
        .account(account_id)
        .await
        .caused_by(trc::location!())?;

    let mut writer = ZaEventWriter {
        server,
        access_token,
        account_id,
        cache,
        account_info,
        identities,
        uid_privacy_conflicts,
        can_add_calendars,
        quota,
        schedule,
        default_alerts,
        disk_quota,
        metadata,
        keys,
        roles,
        account,
    };
    // D28: after a store failure no later object is attempted.
    let mut failed = false;
    let mut created = 0;

    // Process creates
    for (id, object) in request.unwrap_create() {
        if failed {
            response.not_created.append(id, za_set_incomplete());
            continue;
        }
        match writer.create(&id, object, created).await {
            Ok(Ok(written)) => {
                created += 1;
                response.created(id.clone(), written.document_id);
                response.add_created_properties(&id, written.server_set);
                // D3: the change id of the last commit.
                response.new_state = State::Exact(written.change_id).into();
            }
            Ok(Err(err)) => response.not_created.append(id, err),
            Err(err) => {
                za_trace_set_failure(account_id, &err);
                failed = true;
                response.not_created.append(id, za_set_incomplete());
            }
        }
    }

    // I §7.3: updates and destroys wait for Tasks 31 to 33.
    for (id, _) in request.unwrap_update() {
        response.not_updated.append(
            id,
            if failed {
                za_set_incomplete()
            } else {
                SetError::forbidden().with_description(ZA_INTERIM)
            },
        );
    }
    for id in will_destroy {
        response.not_destroyed.append(
            id,
            if failed {
                za_set_incomplete()
            } else {
                SetError::forbidden().with_description(ZA_INTERIM)
            },
        );
    }

    Ok(response)
}

impl ZaEventWriter<'_> {
    /// One create, in upstream's order (`calendar_event/set.rs:288-305`,
    /// `:1632-2069`), written by the pipeline.
    async fn create(
        &mut self,
        id: &str,
        mut object: EventValue<'_>,
        created: usize,
    ) -> Answer<ZaCreated> {
        let server = self.server;
        let access_token = self.access_token;
        let account_id = self.account_id;
        let personal_id = access_token.personal_id(account_id, Collection::Calendar);
        let is_member = access_token.is_member(account_id);

        if !self.quota.has_room(created) {
            return Ok(Err(too_many_events()));
        }
        if self.uid_privacy_conflicts.creates.contains(id) {
            return Ok(Err(uid_privacy_conflict()));
        }
        match self
            .metadata
            .extract(MetadataPatches::for_create(), &mut object)
        {
            Ok(None) => {}
            Ok(Some(_)) => {
                // Upgrade ruling 66: object metadata is refused where it is extracted.
                return Ok(Err(
                    SetError::forbidden().with_description(ZA_METADATA_REFUSED)
                ));
            }
            Err(err) => return Ok(Err(err)),
        }

        // Process changes
        let client_values = TrackedValues::from_patch(&object);
        let has_version = matches!(
            object.as_object_and_get(&Key::Property(JSCalendarProperty::Version)),
            Some(Value::Str(_))
        );
        let omitted_flags = DEFAULTED_FLAGS
            .map(|property| object.as_object_and_get(&Key::Property(property)).is_none());
        // Spec 6.5: before upstream's patcher, which stores `useDefaultAlerts`
        // as per-user data (D10).
        let rulings = match za_take_rulings(&mut object, true) {
            Ok(rulings) => rulings,
            Err(err) => return Ok(Err(err)),
        };
        let mut event = CalendarEvent::default();
        let mut content = CalendarEventContent::default();
        let mut js_calendar_group = JSCalendar::new_event();
        if let Err(err) = update_calendar_event(
            &self.cache,
            personal_id,
            PatchTarget {
                write: EventWrite::Create,
                id: None,
                is_origin: None,
            },
            object,
            &mut event,
            &mut content,
            &mut js_calendar_group,
        ) {
            return Ok(Err(err));
        }
        if event.names.len() > server.core.groupware.max_calendars_per_event {
            return Ok(Err(too_many_calendars(
                server.core.groupware.max_calendars_per_event,
            )));
        }

        // Verify that the calendar ids valid
        if let Some(name) = event
            .names
            .iter()
            .find(|name| !self.cache.has_container_id(&name.parent_id))
        {
            return Ok(Err(SetError::invalid_properties()
                .with_property(JSCalendarProperty::CalendarIds)
                .with_description(format!(
                    "calendarId {} does not exist.",
                    Id::from(name.parent_id)
                ))));
        }
        if let Some(ids) = &self.can_add_calendars
            && let Some(name) = event.names.iter().find(|name| {
                !ids.contains(name.parent_id)
                    && !EventAcl::for_calendar(&self.cache, access_token, name.parent_id)
                        .may_manage_own_items()
            })
        {
            return Ok(Err(add_calendar_forbidden(name.parent_id)));
        }
        let dates = TrackedValues::from_dates(&js_calendar_group);

        // Convert JSCalendar to iCalendar
        let mut ical = match server
            .export_icalendar(access_token, js_calendar_group)
            .await?
        {
            Ok(ical) => ical,
            Err(err) => return Ok(Err(err)),
        };
        if let Err(err) = assert_privacy_allowed(&ical, EventViewer::new(is_member)) {
            return Ok(Err(err));
        }

        // Generate a UID when the client omitted one
        if ical.object_uid().is_none() {
            let uid = generate_uid();
            for component in &mut ical.components {
                if component.component_type.is_event_or_todo() {
                    component.add_uid(&uid);
                }
            }
        }

        // D11: the writer's default alerts, by value, over the event's final calendars.
        let mut calendar_ids: Vec<u32> = Vec::with_capacity(event.names.len());
        for calendar_id in event.names.iter().map(DavName::parent_id) {
            if !calendar_ids.contains(&calendar_id) {
                calendar_ids.push(calendar_id);
            }
        }
        if rulings.use_default_alerts {
            za_apply_default_alerts_by_value(
                server,
                &mut self.default_alerts,
                account_id,
                personal_id,
                &calendar_ids,
                &mut ical,
            )
            .await
            .caused_by(trc::location!())?;
        }

        // D10: the per-user split is not run; the content holds no preferences.
        if let Err(err) = ical.validate_user_data() {
            return Ok(Err(user_data_error(err)));
        }

        // Assign an organizer when the event has participants but none was provided
        let is_organizer_assigned = self
            .account_info
            .addresses()
            .first()
            .is_some_and(|organizer_address| itip_assign_organizer(&mut ical, organizer_address));
        if let Some(ids) = &self.can_add_calendars
            && let Some(name) = event
                .names
                .iter()
                .find(|name| !ids.contains(name.parent_id))
            && !self.identities.event_ownership(&ical).may_write_own()
        {
            return Ok(Err(add_calendar_forbidden(name.parent_id)));
        }
        let now = now() as i64;
        stamp_created(&mut ical, now);
        if is_origin(&ical, self.account_info.addresses()) {
            stamp_updated(&mut ical, now);
            clamp_created(&mut ical, now);
        }

        if let Err(err) =
            assert_participants_limit(&ical, server.core.groupware.max_ical_attendees_per_instance)
        {
            return Ok(Err(err));
        }

        // Check size and quota
        let size = SizeWriter::ical(&ical);
        if size > server.core.groupware.max_ical_size {
            return Ok(Err(too_large_event(
                size,
                server.core.groupware.max_ical_size,
            )));
        }
        if self.disk_quota.check(size as u64).is_err() {
            return Ok(Err(SetError::over_quota()));
        }

        // Invitations spec 5.1, release 6 spec 3.3: the UID lease, held from
        // here through the commit, checks the UID in every named calendar.
        let Some(uid) = ical.object_uid().map(str::to_string) else {
            // Unreachable: the conversion gives every event or task a UID above.
            return Ok(Err(
                SetError::invalid_properties().with_property(JSCalendarProperty::Uid)
            ));
        };
        let lease = match za_take_uid_lease(
            server,
            &self.cache,
            access_token,
            account_id,
            &calendar_ids,
            &uid,
        )
        .await
        {
            Ok(lease) => lease,
            Err(err) => return za_event_set_error(err, Some(&uid)).map(Err),
        };

        // Build event
        content.data = CalendarEventData::new_for(
            ical,
            Tz::Floating,
            server.core.groupware.max_ical_instances,
            &DefaultAlerts::disabled(),
            true,
        );
        let (contexts, next_alarms, scheduled) = match self
            .prepare(&mut event, &mut content, &calendar_ids, now)
            .await
        {
            Ok(Ok(prepared)) => prepared,
            Ok(Err(err)) => {
                lease.release().await;
                return za_event_set_error(err, Some(&uid)).map(Err);
            }
            Err(err) => {
                lease.release().await;
                return Err(err);
            }
        };
        let written = match za_create_event(
            server,
            access_token,
            &self.cache,
            &self.account,
            &self.keys,
            &self.roles,
            ZaEventCreate {
                event,
                content,
                next_alarms,
                scheduled,
                lease,
                uid: uid.clone(),
                contexts,
            },
        )
        .await
        {
            Ok(written) => written,
            Err(err) => return za_event_set_error(err, Some(&uid)).map(Err),
        };
        self.disk_quota.reserve(size as u64);

        // Upstream's server-set (`calendar_event/set.rs:1960-2003`), from the
        // committed tree and the real document id.
        let mut server_set = dates
            .with_component(written.ical.main_component())
            .with_origin(is_origin(&written.ical, self.account_info.addresses()))
            .server_set(&client_values, None);
        if !has_version {
            server_set.push((
                JSCalendarProperty::Version,
                Value::Str(JSCALENDAR_VERSION.into()),
            ));
        }
        // Spec 6.5, invariant 58: the four flags and `useDefaultAlerts` stay false.
        server_set.extend(DEFAULTED_FLAGS.into_iter().zip(omitted_flags).filter_map(
            |(property, is_omitted)| is_omitted.then_some((property, Value::Bool(false))),
        ));
        // Spec 6.11: a blob id was refused above, so no blob property is imported.
        let blob_properties = BlobProperties::default();
        let imported = ImportedValues {
            participants: is_organizer_assigned,
            blob_properties: &blob_properties,
            every_blob_property: false,
            visible_to: None,
        };
        if imported.is_requested() {
            server_set.extend(imported.values(&written.ical, account_id, written.document_id));
        }

        Ok(Ok(ZaCreated {
            document_id: written.document_id,
            change_id: written.change_id,
            server_set,
        }))
    }

    /// Steps the lease covers before the pipeline's create: the contexts of
    /// every named calendar, upstream's next alarms and the scheduling.
    async fn prepare(
        &mut self,
        event: &mut CalendarEvent,
        content: &mut CalendarEventContent,
        calendar_ids: &[u32],
        now: i64,
    ) -> trc::Result<Result<(Vec<ZaCalendarContext>, EventAlarms, ZaScheduled), ZaPipelineError>>
    {
        let server = self.server;
        let account_id = self.account_id;
        let contexts = match za_write_contexts(
            server,
            self.access_token,
            account_id,
            event,
            calendar_ids[0],
            Acl::AddItems,
        )
        .await
        {
            Ok(contexts) => contexts,
            Err(err) => return Ok(Err(err)),
        };
        let next_alarms = server
            .next_event_alarms(
                account_id,
                &EventAlarmUsers::new(account_id, &*content)?.with_event_flags(event.flags),
                &content.data,
                calendar_ids,
                now,
                &mut self.default_alerts,
            )
            .await
            .caused_by(trc::location!())?;
        match za_schedule_create(
            server,
            self.access_token,
            &self.roles,
            self.schedule,
            event,
            content,
            account_id,
        )
        .await
        {
            Ok(scheduled) => Ok(Ok((contexts, next_alarms, scheduled))),
            Err(err) => Ok(Err(err)),
        }
    }
}

/// Spec 6.5. With `base` (the event's own patch): refuses `isDraft: true` and
/// a true `mayInviteSelf`, `mayInviteOthers` or `hideAttendees`, and takes a
/// boolean or null `useDefaultAlerts` out of the object. For every patch:
/// refuses every link with a `blobId`, found as upstream finds them.
pub(super) fn za_take_rulings(
    object: &mut EventValue<'_>,
    base: bool,
) -> Result<ZaRulings, SetError<JSCalendarProperty<Id>>> {
    if !BlobProperties::from_patch(object, |_| true).is_empty() {
        return Err(SetError::invalid_properties()
            .with_property(JSCalendarProperty::Links)
            .with_description(ZA_CALENDAR_ATTACHMENTS));
    }
    let mut rulings = ZaRulings {
        use_default_alerts: false,
    };
    if !base {
        return Ok(rulings);
    }
    let Some(entries) = object.as_object_mut() else {
        return Ok(rulings);
    };
    for (key, value) in entries.iter() {
        match (key, value) {
            (Key::Property(JSCalendarProperty::IsDraft), Value::Bool(true)) => {
                return Err(SetError::invalid_properties()
                    .with_property(JSCalendarProperty::IsDraft)
                    .with_description(ZA_CALENDAR_DRAFTS));
            }
            (
                Key::Property(
                    property @ (JSCalendarProperty::MayInviteSelf
                    | JSCalendarProperty::MayInviteOthers
                    | JSCalendarProperty::HideAttendees),
                ),
                Value::Bool(true),
            ) => {
                return Err(SetError::invalid_properties()
                    .with_property(property.clone())
                    .with_description(ZA_CALENDAR_PARTICIPANT_FLAG));
            }
            _ => {}
        }
    }
    entries.as_mut_vec().retain(|(key, value)| {
        if matches!(key, Key::Property(JSCalendarProperty::UseDefaultAlerts))
            && matches!(value, Value::Bool(_) | Value::Null)
        {
            rulings.use_default_alerts |= matches!(value, Value::Bool(true));
            false
        } else {
            true
        }
    });
    Ok(rulings)
}

/// Spec 8: the pipeline's errors as `CalendarEvent/set` answers them (I §2.2).
/// `Err` is a store failure, which the writer answers under D28.
pub(super) fn za_event_set_error(
    error: ZaPipelineError,
    uid: Option<&str>,
) -> Result<SetError<JSCalendarProperty<Id>>, trc::Error> {
    match error {
        ZaPipelineError::Forbidden(_) | ZaPipelineError::LinkedEvent => Ok(SetError::forbidden()),
        ZaPipelineError::NotFound => Ok(SetError::not_found()),
        ZaPipelineError::Busy(_) => Ok(SetError::forbidden().with_description(ZA_MODIFIED)),
        ZaPipelineError::UidConflict { .. } => Ok(match uid {
            Some(uid) => duplicate_uid(uid),
            None => SetError::invalid_properties().with_property(JSCalendarProperty::Uid),
        }),
        ZaPipelineError::TooManyRecipients => Ok(SetError::invalid_properties()
            .with_property(JSCalendarProperty::Participants)
            .with_description(concat!(
                "The number of scheduling message recipients ",
                "exceeds the maximum allowed."
            ))),
        ZaPipelineError::Scheduling(err) => Ok(SetError::invalid_properties()
            .with_property(JSCalendarProperty::Participants)
            .with_description(err.to_string())),
        ZaPipelineError::SchedulingDenied(reason) => {
            Ok(SetError::forbidden().with_description(reason))
        }
        ZaPipelineError::Store(error) => {
            if error.is_assertion_failure() {
                return Ok(SetError::forbidden().with_description(ZA_MODIFIED));
            }
            match error.event_type() {
                EventType::Limit(LimitEvent::Quota) => Ok(SetError::over_quota()),
                EventType::Security(SecurityEvent::Unauthorized) => Ok(SetError::forbidden()),
                _ => Err(error),
            }
        }
    }
}

/// Spec 8, D28 (I §7.5): a store failure is traced once as a fresh event of
/// its kind, with the account id and a fixed reason (a create has no document
/// id yet). The original error is not traced: its context may carry stored
/// bytes.
fn za_trace_set_failure(account_id: u32, error: &trc::Error) {
    trc::error!(
        error
            .event_type()
            .into_err()
            .account_id(account_id)
            .details("zero-access: calendar event write failed")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar_event::{
        EventValue,
        server_set::{BlobProperties, EmbeddedBlobId},
    };
    use calcard::jscalendar::JSCalendarProperty;
    use groupware::{
        calendar::pipeline::error::{ZaBusy, ZaPipelineError, ZaRefusal},
        scheduling::ItipError,
    };
    use jmap_proto::error::set::SetError;
    use jmap_tools::Key;
    use serde_json::{Value, json};
    use trc::{LimitEvent, SecurityEvent, StoreEvent};
    use types::{
        blob::{BlobClass, BlobId},
        blob_hash::BlobHash,
        collection::Collection,
        id::Id,
    };

    const FLAGS: [(&str, JSCalendarProperty<Id>); 4] = [
        ("isDraft", JSCalendarProperty::IsDraft),
        ("mayInviteSelf", JSCalendarProperty::MayInviteSelf),
        ("mayInviteOthers", JSCalendarProperty::MayInviteOthers),
        ("hideAttendees", JSCalendarProperty::HideAttendees),
    ];

    fn object(json: &str) -> EventValue<'_> {
        EventValue::parse_json(json).expect("a JSCalendar object")
    }

    fn has(object: &EventValue<'_>, property: JSCalendarProperty<Id>) -> bool {
        object
            .as_object()
            .is_some_and(|object| object.contains_key(&Key::Property(property)))
    }

    fn passes(
        label: &str,
        result: Result<ZaRulings, SetError<JSCalendarProperty<Id>>>,
    ) -> ZaRulings {
        match result {
            Ok(rulings) => rulings,
            Err(err) => panic!(
                "{label}: refused with {}",
                serde_json::to_value(err).unwrap()
            ),
        }
    }

    fn refused(label: &str, result: Result<ZaRulings, SetError<JSCalendarProperty<Id>>>) -> Value {
        match result {
            Ok(_) => panic!("{label}: passed"),
            Err(err) => serde_json::to_value(err).unwrap(),
        }
    }

    #[test]
    fn rulings_cases() {
        // The four flags: `true` is refused on the event's own patch, `false`
        // and `null` stay for upstream, and an occurrence patch is upstream's.
        for (name, property) in FLAGS {
            let text = if name == "isDraft" {
                "Drafts are not available for zero-access accounts."
            } else {
                "This property is not available for zero-access accounts."
            };
            let json = format!(r#"{{"{name}": true}}"#);
            let mut value = object(&json);
            assert_eq!(
                refused(name, za_take_rulings(&mut value, true)),
                json!({ "type": "invalidProperties", "properties": [name], "description": text }),
                "{name}"
            );

            for literal in ["false", "null"] {
                let json = format!(r#"{{"{name}": {literal}}}"#);
                let mut value = object(&json);
                let rulings = passes(name, za_take_rulings(&mut value, true));
                assert!(!rulings.use_default_alerts, "{name}: {literal}");
                assert!(has(&value, property.clone()), "{name}: {literal} stays");
            }

            let json = format!(r#"{{"{name}": true}}"#);
            let mut value = object(&json);
            passes(name, za_take_rulings(&mut value, false));
            assert!(has(&value, property.clone()), "{name}: an occurrence patch");
        }

        // `useDefaultAlerts`: taken out of the event's own patch when a
        // boolean or null; any other value is left for upstream's refusal.
        for (literal, expected) in [("true", true), ("false", false), ("null", false)] {
            let json = format!(r#"{{"useDefaultAlerts": {literal}, "title": "t"}}"#);
            let mut value = object(&json);
            let rulings = passes(literal, za_take_rulings(&mut value, true));
            assert_eq!(rulings.use_default_alerts, expected, "{literal}");
            assert!(
                !has(&value, JSCalendarProperty::UseDefaultAlerts),
                "{literal}: taken out"
            );
            assert!(has(&value, JSCalendarProperty::Title), "{literal}");
        }
        let mut value = object(r#"{"useDefaultAlerts": "yes"}"#);
        let rulings = passes("yes", za_take_rulings(&mut value, true));
        assert!(!rulings.use_default_alerts);
        assert!(has(&value, JSCalendarProperty::UseDefaultAlerts));
        let mut value = object(r#"{"useDefaultAlerts": true}"#);
        let rulings = passes("occurrence", za_take_rulings(&mut value, false));
        assert!(!rulings.use_default_alerts);
        assert!(has(&value, JSCalendarProperty::UseDefaultAlerts));

        // A link with a `blobId`, on every patch.
        let attachments = json!({
            "type": "invalidProperties",
            "properties": ["links"],
            "description": "Event attachments are not available for zero-access accounts.",
        });
        let upload = BlobId::new(
            BlobHash::default(),
            BlobClass::Reserved {
                account_id: 1,
                expires: u64::MAX,
            },
        );
        let embedded = BlobId::new(
            BlobHash::default(),
            BlobClass::Embedded {
                account_id: 1,
                collection: u8::from(Collection::CalendarEvent),
                document_id: 2,
            },
        );
        assert!(embedded.is_embedded_in(1, 2));
        for (label, blob_id) in [("upload", &upload), ("embedded", &embedded)] {
            let json = format!(
                r#"{{"links": {{"l1": {{"@type": "Link", "blobId": "{blob_id}", "rel": "enclosure"}}}}}}"#
            );
            for base in [true, false] {
                let mut value = object(&json);
                assert!(
                    !BlobProperties::from_patch(&value, |_| true).is_empty(),
                    "{label}: the blob id parses"
                );
                assert_eq!(
                    refused(label, za_take_rulings(&mut value, base)),
                    attachments,
                    "{label}, base {base}"
                );
            }
        }
        // Updating event 2 of account 1, upstream's own update would not count
        // a blob embedded in that event as a change; the ruling still refuses it.
        let json = format!(
            r#"{{"links": {{"l1": {{"@type": "Link", "blobId": "{embedded}", "rel": "enclosure"}}}}}}"#
        );
        let mut value = object(&json);
        assert!(
            BlobProperties::from_patch(&value, |blob_id| !blob_id.is_embedded_in(1, 2)).is_empty()
        );
        assert_eq!(
            refused("own embedded", za_take_rulings(&mut value, false)),
            attachments
        );
    }

    fn answer(label: &str, error: ZaPipelineError, uid: Option<&str>) -> Value {
        match za_event_set_error(error, uid) {
            Ok(err) => serde_json::to_value(err).unwrap(),
            Err(_) => panic!("{label}: a store failure, not a set error"),
        }
    }

    #[test]
    fn event_set_error_table() {
        let forbidden = json!({ "type": "forbidden" });
        let modified = json!({
            "type": "forbidden",
            "description": "Another process modified this calendar event, please try again.",
        });
        let scheduling = ItipError::MultipleOrganizer;
        let scheduling_text = scheduling.to_string();
        for (label, error, expected) in [
            (
                "Forbidden(Context)",
                ZaPipelineError::Forbidden(ZaRefusal::Context),
                forbidden.clone(),
            ),
            (
                "Forbidden(LinkedMove)",
                ZaPipelineError::Forbidden(ZaRefusal::LinkedMove),
                forbidden.clone(),
            ),
            (
                "LinkedEvent",
                ZaPipelineError::LinkedEvent,
                forbidden.clone(),
            ),
            (
                "NotFound",
                ZaPipelineError::NotFound,
                json!({ "type": "notFound" }),
            ),
            (
                "Busy(LeaseBusy)",
                ZaPipelineError::Busy(ZaBusy::LeaseBusy),
                modified.clone(),
            ),
            (
                "Busy(CreateLost)",
                ZaPipelineError::Busy(ZaBusy::CreateLost),
                modified.clone(),
            ),
            (
                "Store(AssertValueFailed)",
                ZaPipelineError::Store(StoreEvent::AssertValueFailed.into_err()),
                modified.clone(),
            ),
            (
                "UidConflict",
                ZaPipelineError::UidConflict { href: None },
                json!({
                    "type": "invalidProperties",
                    "properties": ["uid"],
                    "description": "An event with UID u1 already exists.",
                }),
            ),
            (
                "TooManyRecipients",
                ZaPipelineError::TooManyRecipients,
                json!({
                    "type": "invalidProperties",
                    "properties": ["participants"],
                    "description": "The number of scheduling message recipients exceeds the maximum allowed.",
                }),
            ),
            (
                "Scheduling",
                ZaPipelineError::Scheduling(scheduling),
                json!({
                    "type": "invalidProperties",
                    "properties": ["participants"],
                    "description": scheduling_text,
                }),
            ),
            (
                "SchedulingDenied",
                ZaPipelineError::SchedulingDenied(
                    "This account is not allowed to send calendar scheduling messages.",
                ),
                json!({
                    "type": "forbidden",
                    "description": "This account is not allowed to send calendar scheduling messages.",
                }),
            ),
            (
                "Store(Limit(Quota))",
                ZaPipelineError::Store(LimitEvent::Quota.into_err()),
                json!({ "type": "overQuota", "description": "Account quota exceeded." }),
            ),
            (
                "Store(Security(Unauthorized))",
                ZaPipelineError::Store(SecurityEvent::Unauthorized.into_err()),
                forbidden.clone(),
            ),
        ] {
            assert_eq!(answer(label, error, Some("u1")), expected, "{label}");
        }

        // Without the UID the conflict carries no description.
        assert_eq!(
            answer(
                "UidConflict without uid",
                ZaPipelineError::UidConflict { href: None },
                None
            ),
            json!({ "type": "invalidProperties", "properties": ["uid"] })
        );

        // Anything else is a store failure, which the writer answers (D28).
        assert!(
            za_event_set_error(
                ZaPipelineError::Store(StoreEvent::UnexpectedError.into_err()),
                Some("u1")
            )
            .is_err(),
            "Store(UnexpectedError) is a store failure"
        );
    }
}
