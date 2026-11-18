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
//! Updates are grouped as upstream groups them and written by the pipeline's
//! update commit with the stored token pairs, display name and SEQUENCE floor
//! (spec 5.4). A change of an event's calendars is a placement plan the
//! pipeline resolves and commits with the update (spec 5.2).
//!
//! A destroy is the pipeline's delete of the event and every one of its
//! names, scheduled through CalDAV's delete condition (D18), one commit per
//! event; one occurrence's destroy is upstream's conversion to an update
//! (spec 6.5). No destroy reaches upstream's path (invariant 57).

use super::{
    ComponentSequences, DEFAULTED_FLAGS, EVENT_METADATA_FLAGS, EventOp, EventOps, EventUpdate,
    EventUpdates, EventWrite, InstanceAction, InstanceOp, InstancePlan, PatchTarget,
    add_calendar_forbidden, assert_participants_limit, clamp_created, generate_uid,
    instance_imported_values, stamp_created, stamp_updated, too_large_event, too_many_calendars,
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
        CalendarSyntheticId, EventMainComponent, EventValue, JSCalendarEntries,
        detach_iana_timezones, duplicate_uid, is_origin,
        privacy::{
            UidPrivacyConflicts, assert_privacy_access, assert_privacy_allowed,
            uid_privacy_conflict,
        },
        server_set::{
            BlobProperties, EmbeddedBlobId, ImportedValues, ServerSetValues, TrackedValues,
        },
        user::user_data_error,
        validate::InstanceMetadata,
        za_read::{ZaEventReader, za_event_state},
    },
};
use calcard::{
    common::timezone::Tz,
    icalendar::ICalendar,
    jscalendar::{JSCALENDAR_VERSION, JSCalendar, JSCalendarProperty},
};
use common::{
    ArchivedDavName, DavName, GroupwareResources, Server,
    auth::{AccessToken, AccountCache, AccountInfo},
    storage::quota::{DiskQuotaBudget, ObjectQuotaUsage},
    za::calendar::za_calendar_keyless_refusal,
};
use groupware::{
    SizeWriter,
    cache::GroupwareCache,
    calendar::{
        ArchivedCalendarEvent, CalendarEvent, CalendarEventContent, CalendarEventData,
        alerts::{DefaultAlerts, DefaultAlertsResolver},
        identity::{CalendarAddresses, ParticipantIdentityAddresses},
        index::ICalendarObjectUid,
        pipeline::{
            ZaScheduling,
            admission::{ZaRoles, za_roles, za_writer_keys},
            context::za_write_contexts,
            create::{ZaEventCreate, za_create_event},
            delete::za_destroy_event,
            error::ZaPipelineError,
            placement::{ZaPlacementContexts, ZaPlacementPlan, za_placement_contexts},
            schedule::{ZaScheduled, za_delete_send_itip, za_schedule_create, za_schedule_update},
            uid::za_take_uid_lease,
            update::{ZaEventUpdate, za_commit_update},
        },
        privacy::{EventPrivacy, EventViewer, ICalendarPrivacy},
        rights::{EventAcl, EventChanges},
        schedule::{EventAlarmScheduler, EventAlarmUsers, EventAlarms},
        sequence::{ICalendarSequence, SCHEDULING_EVENT_FLAGS},
        sequence_floor::za_sequence_floor,
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
    request::{MaybeInvalid, capability::CapabilityIds},
    types::state::State,
};
use jmap_tools::{Key, Value};
use registry::schema::enums::StorageQuota;
use std::sync::Arc;
use store::{
    ValueKey,
    roaring::RoaringBitmap,
    write::{Archive, ArchiveBytes, now, serialize::rkyv_deserialize},
};
use trc::{AddContext, EventType, LimitEvent, SecurityEvent};
use types::{
    acl::Acl,
    blob::BlobId,
    collection::{Collection, SyncCollection},
    field::CalendarEventField,
    id::Id,
};
use vault::session::SessionKeys;

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
    can_delete_calendars: Option<RoaringBitmap>,
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
        .with_state(sampled.assert_state(
            // PR #90 review C4617611565: a grantee's state is the snapshot's
            // highest change id, as `/get`, `/query` and `/changes` answer it.
            za_event_state(true, access_token, account_id, &cache),
            &request.if_in_state,
        )?);
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
    let (can_add_calendars, can_delete_calendars) = if is_shared {
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
    // I §7.6: the reader sits in the head, so only a head error fails the
    // method; the branch admitted a key account only.
    let mut reader = ZaEventReader::new(server, access_token, account_id)
        .await?
        .ok_or_else(|| {
            trc::StoreEvent::UnexpectedError
                .into_err()
                .account_id(account_id)
                .details("zero-access: calendar event writer without a key account")
        })?;

    let mut writer = ZaEventWriter {
        server,
        access_token,
        account_id,
        cache,
        account_info,
        identities,
        uid_privacy_conflicts,
        can_add_calendars,
        can_delete_calendars,
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
                za_trace_set_failure(account_id, None, &err);
                failed = true;
                response.not_created.append(id, za_set_incomplete());
            }
        }
    }

    // D28: after a store failure no update or destroy is attempted.
    if failed {
        for (id, _) in request.unwrap_update() {
            response.not_updated.append(id, za_set_incomplete());
        }
        for id in will_destroy {
            response.not_destroyed.append(id, za_set_incomplete());
        }
        return Ok(response);
    }

    // Group updates and instance removals by event (upstream's
    // `calendar_event/set.rs:334-431`). The metadata preload (`:427`) and the
    // moved UID index (`:432-448`) are not taken.
    let has_synthetic_ids = will_destroy.iter().any(|id| id.is_synthetic())
        || request.update.as_ref().is_some_and(|update| {
            update
                .iter()
                .any(|(id, _)| matches!(id, MaybeInvalid::Value(id) if id.is_synthetic()))
        });
    let is_destroyed_event = |document_id: u32| {
        will_destroy
            .iter()
            .any(|id| !id.is_synthetic() && id.document_id() == document_id)
    };
    let will_be_destroyed = |id: Id| {
        will_destroy.iter().any(|destroy_id| {
            *destroy_id == id
                || (!destroy_id.is_synthetic() && destroy_id.document_id() == id.document_id())
        })
    };
    let mut updates =
        EventUpdates::with_capacity(request.update.as_ref().map_or(0, |update| update.len()));
    for (id, mut object) in request.unwrap_update() {
        let id = match id {
            MaybeInvalid::Value(id) => id,
            invalid => {
                response.not_updated.append(invalid, SetError::not_found());
                continue;
            }
        };
        if will_be_destroyed(id) {
            response.not_updated.append(id, SetError::will_destroy());
            continue;
        }
        match writer
            .metadata
            .extract(MetadataPatches::for_update(), &mut object)
        {
            Ok(None) => {}
            Ok(Some(_)) => {
                // Upgrade ruling 66: object metadata is refused where it is extracted.
                response.not_updated.append(
                    id,
                    SetError::forbidden().with_description(ZA_METADATA_REFUSED),
                );
                continue;
            }
            Err(err) => {
                response.not_updated.append(id, err);
                continue;
            }
        }
        let op = match id.recurrence_key() {
            Some(recurrence_key) => EventOp::Instance(InstanceOp {
                id,
                recurrence_key,
                target: None,
                action: InstanceAction::Update {
                    patch: object,
                    server_set: None,
                },
            }),
            None => EventOp::Base { id, patch: object },
        };
        updates.push(has_synthetic_ids, op, &mut response);
    }
    for id in will_destroy.iter().copied() {
        let Some(recurrence_key) = id.recurrence_key() else {
            continue;
        };
        if is_destroyed_event(id.document_id()) {
            response.not_destroyed.append(id, SetError::will_destroy());
            continue;
        }
        updates.push(
            has_synthetic_ids,
            EventOp::Instance(InstanceOp {
                id,
                recurrence_key,
                target: None,
                action: InstanceAction::Destroy,
            }),
            &mut response,
        );
    }
    let mut destroy_events = will_destroy;
    if has_synthetic_ids {
        destroy_events.retain(|id| !id.is_synthetic());
    }

    // Process updates: one commit per event (D3).
    for mut update in updates.pending {
        if failed {
            update.fail(&mut response, za_set_incomplete());
            continue;
        }
        let document_id = update.document_id;
        if let Err(err) = writer
            .update(&mut reader, &mut update, &mut response, &mut destroy_events)
            .await
        {
            za_trace_set_failure(account_id, Some(document_id), &err);
            failed = true;
            update.fail(&mut response, za_set_incomplete());
        }
    }

    // Process deletions (upstream's `calendar_event/set.rs:1416-1605`): the
    // pipeline's delete, one commit per event (D3). The container cleanup
    // preload (`:1417-1433`) is not taken (invariant 57).
    for id in destroy_events {
        if failed {
            response.not_destroyed.append(id, za_set_incomplete());
            continue;
        }
        match writer.destroy(&mut reader, id).await {
            Ok(Ok(change_id)) => {
                response.destroyed.push(id);
                // D3: the change id of the last commit.
                response.new_state = State::Exact(change_id).into();
            }
            Ok(Err(err)) => response.not_destroyed.append(id, err),
            Err(err) => {
                za_trace_set_failure(account_id, Some(id.document_id()), &err);
                failed = true;
                response.not_destroyed.append(id, za_set_incomplete());
            }
        }
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

    /// One event of `updates.pending`, in upstream's order
    /// (`calendar_event/set.rs:451-1414`), written by the pipeline. A set
    /// error is answered here, as upstream's `continue 'update`; `Err` is a
    /// store failure, which the caller answers under D28.
    async fn update(
        &mut self,
        reader: &mut ZaEventReader,
        update: &mut EventUpdate<'_>,
        response: &mut SetResponse<calendar_event::CalendarEvent>,
        destroy_events: &mut Vec<Id>,
    ) -> trc::Result<()> {
        let server = self.server;
        let access_token = self.access_token;
        let account_id = self.account_id;
        let document_id = update.document_id;
        let is_owner = access_token.is_member(account_id);
        let is_shared = access_token.is_shared(account_id);

        let Some(calendar_event_) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                account_id,
                Collection::CalendarEvent,
                document_id,
            ))
            .await?
        else {
            update.fail(response, SetError::not_found());
            return Ok(());
        };
        let calendar_event = calendar_event_
            .to_unarchived::<CalendarEvent>()
            .caused_by(trc::location!())?;
        if let Err(err) = assert_privacy_access(
            EventPrivacy::from_flags(calendar_event.inner.flags.to_native()),
            EventViewer::new(is_owner),
        ) {
            update.fail(response, err);
            return Ok(());
        }
        if update.base_id().is_some() && self.uid_privacy_conflicts.updates.contains(document_id) {
            update.fail(response, uid_privacy_conflict());
            return Ok(());
        }
        // The metadata-only write (`:484-518`) is not taken: object metadata
        // was refused when the update was grouped.
        let Some(content_) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                account_id,
                Collection::CalendarEvent,
                document_id,
                CalendarEventField::Content,
            ))
            .await?
        else {
            update.fail(response, SetError::not_found());
            return Ok(());
        };

        // Spec 6.5, D1: the patch applies to the unsealed view; the stored
        // archives stay the update's current values.
        let previous_calendar_ids = calendar_event
            .inner
            .names
            .iter()
            .map(ArchivedDavName::parent_id)
            .collect::<Vec<_>>();
        let Some(za_view) = reader
            .view(
                server,
                access_token,
                document_id,
                &previous_calendar_ids,
                calendar_event.inner,
                &content_,
            )
            .await?
        else {
            update.fail(response, SetError::not_found());
            return Ok(());
        };
        let display_name = za_view.display_name.clone();
        let content = za_view
            .content
            .to_unarchived::<CalendarEventContent>()
            .caused_by(trc::location!())?;
        let mut new_calendar_event = calendar_event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        let mut new_content = content
            .deserialize::<CalendarEventContent>()
            .caused_by(trc::location!())?;
        // PR #90 review C4617611549: a caller that is not a member is held to
        // the event's effective privacy, the higher of the row flags' and the
        // unsealed tree's, as `CalendarEvent/get` reads it. A sealed
        // extension `CLASS` leaves the flags public; checked before an
        // occurrence resolves to the base event.
        if !is_owner
            && let Err(err) = assert_privacy_access(
                EventPrivacy::from_flags(calendar_event.inner.flags.to_native())
                    .max(new_content.data.event.privacy()),
                EventViewer::new(is_owner),
            )
        {
            update.fail(response, err);
            return Ok(());
        }

        // Resolve synthetic ids into recurrence instances
        let mut has_instances = false;
        if update.has_instances() {
            match update.plan_instances(&new_content.data, response) {
                InstancePlan::Instances => {
                    has_instances = true;
                }
                InstancePlan::BaseEvent => {}
                InstancePlan::DestroyEvent(id) => {
                    destroy_events.push(id);
                    return Ok(());
                }
                InstancePlan::Nothing => {
                    return Ok(());
                }
            }
        }

        let personal_id = access_token.personal_id(account_id, Collection::Calendar);
        let ownership = self.identities.event_ownership(&new_content.data.event);
        let was_origin = is_origin(&new_content.data.event, self.account_info.addresses());
        // D10: the owner's form for every caller, as the content holds no
        // preferences.
        let mut view = new_content.data.event.clone();
        let previous_event = std::mem::take(&mut new_content.data.event);
        detach_iana_timezones(&mut view);
        let mut js_calendar_group = view.into_jscalendar::<Id, BlobId>();

        // Spec 6.5: the five flag rulings apply to the event's own patch; an
        // occurrence's patch is checked for blob links only.
        let rulings = match &mut update.ops {
            EventOps::Base { patch, .. } => za_take_rulings(patch, true),
            EventOps::Instances(instances) => instances.iter_mut().try_fold(
                ZaRulings {
                    use_default_alerts: false,
                },
                |rulings, instance| match &mut instance.action {
                    InstanceAction::Update { patch, .. } => {
                        za_take_rulings(patch, false).map(|_| rulings)
                    }
                    InstanceAction::Destroy => Ok(rulings),
                },
            ),
        };
        let rulings = match rulings {
            Ok(rulings) => rulings,
            Err(err) => {
                update.fail(response, err);
                return Ok(());
            }
        };

        let tracked_before = update.base_patch().map(|patch| {
            (
                TrackedValues::from_patch(patch),
                TrackedValues::from_event(&js_calendar_group, Some(was_origin)),
            )
        });
        // Spec 6.11: a blob id was refused above, so no blob property is imported.
        let blob_properties = BlobProperties::default();
        update.track_instance_values(&previous_event, |blob_id| {
            !blob_id.is_embedded_in(account_id, document_id)
        });
        let is_restricted = is_shared
            && !EventAcl::for_calendars(
                &self.cache,
                access_token,
                calendar_event
                    .inner
                    .names
                    .iter()
                    .map(ArchivedDavName::parent_id),
            )
            .may_write(ownership);
        // Spec 6.5: a key account's event never holds `EVENT_HIDE_ATTENDEES`,
        // so no participant is hidden (`:616-618`).
        let per_user_changes = update.per_user_patch_changes();
        let before_group = per_user_changes
            .is_none()
            .then(|| js_calendar_group.clone());

        // Apply per-instance changes to the recurrence overrides of the base event
        if has_instances {
            let metadata = InstanceMetadata {
                is_origin: was_origin,
                flags: new_calendar_event.flags,
                calendar_ids: &new_calendar_event.names,
                use_default_alerts: false,
            };
            if !update.apply_instances(&mut js_calendar_group, &metadata, response) {
                return Ok(());
            }
        }

        // Process changes
        if let Err(err) = update_calendar_event(
            &self.cache,
            personal_id,
            PatchTarget {
                write: EventWrite::Update,
                id: update.base_id(),
                is_origin: Some(was_origin),
            },
            update.take_base_patch(),
            &mut new_calendar_event,
            &mut new_content,
            &mut js_calendar_group,
        ) {
            update.fail(response, err);
            return Ok(());
        }

        // Validate calendarIds limit
        let max_calendars = server.core.groupware.max_calendars_per_event;
        if new_calendar_event.names.len() > max_calendars
            && new_calendar_event.names.len() > calendar_event.inner.names.len()
        {
            update.fail(response, too_many_calendars(max_calendars));
            return Ok(());
        }

        // Validate new calendarIds
        for calendar_id in new_calendar_event.added_calendar_ids(calendar_event.inner) {
            if !self.cache.has_container_id(&calendar_id) {
                update.fail(
                    response,
                    SetError::invalid_properties()
                        .with_property(JSCalendarProperty::CalendarIds)
                        .with_description(format!(
                            "calendarId {} does not exist.",
                            Id::from(calendar_id)
                        )),
                );
                return Ok(());
            } else if self.can_add_calendars.as_ref().is_some_and(|ids| {
                !ids.contains(calendar_id)
                    && !(EventAcl::for_calendar(&self.cache, access_token, calendar_id)
                        .may_manage_own_items()
                        && ownership.may_write_own())
            }) {
                update.fail(response, add_calendar_forbidden(calendar_id));
                return Ok(());
            }
        }

        // Validate deleted calendarIds
        if let Some(can_delete_calendars) = &self.can_delete_calendars {
            for calendar_id in new_calendar_event.removed_calendar_ids(calendar_event.inner) {
                if !can_delete_calendars.contains(calendar_id)
                    && !(EventAcl::for_calendar(&self.cache, access_token, calendar_id)
                        .may_manage_own_items()
                        && ownership.may_write_own())
                {
                    update.fail(
                        response,
                        SetError::forbidden().with_description(format!(
                            "You are not allowed to remove calendar events from calendar {}.",
                            Id::from(calendar_id)
                        )),
                    );
                    return Ok(());
                }
            }
        }
        let is_calendar_ids_changed = new_calendar_event
            .added_calendar_ids(calendar_event.inner)
            .next()
            .is_some()
            || new_calendar_event
                .removed_calendar_ids(calendar_event.inner)
                .next()
                .is_some();
        // Release 6 spec 3.3: a UID held in an entered calendar is the
        // pipeline's check against current archives; upstream's moved UID
        // index (`:717-731`) is not used.
        let is_metadata_changed = is_calendar_ids_changed
            || (calendar_event.inner.flags.to_native() ^ new_calendar_event.flags)
                & EVENT_METADATA_FLAGS
                != 0;

        let mut changes = match &before_group {
            Some(before_group) => EventChanges::between(
                before_group,
                &js_calendar_group,
                calendar_event.inner.flags.to_native(),
                new_calendar_event.flags,
                &self.identities,
            ),
            None => per_user_changes.unwrap_or_default(),
        };
        // D11: the ruling took `useDefaultAlerts` (upstream's `:764-766`).
        if rulings.use_default_alerts {
            changes.insert_per_user();
        }
        let tracked = tracked_before.map(|(client, before)| {
            (
                client,
                before,
                TrackedValues::from_dates(&js_calendar_group),
            )
        });

        // Convert JSCalendar to iCalendar
        let ical = match server
            .export_icalendar(access_token, js_calendar_group)
            .await?
        {
            Ok(ical) => ical,
            Err(err) => {
                update.fail(response, err);
                return Ok(());
            }
        };
        new_content.data.event = ical;
        changes.classify_recurrence_sets(&previous_event, &new_content.data.event);
        if let Err(err) =
            assert_privacy_allowed(&new_content.data.event, EventViewer::new(is_owner))
        {
            update.fail(response, err);
            return Ok(());
        }

        // D11 (in place of `:803-816`): the writer's default alerts, by value,
        // over the event's final calendars.
        let mut new_calendar_ids: Vec<u32> = Vec::with_capacity(new_calendar_event.names.len());
        for calendar_id in new_calendar_event.names.iter().map(DavName::parent_id) {
            if !new_calendar_ids.contains(&calendar_id) {
                new_calendar_ids.push(calendar_id);
            }
        }
        if rulings.use_default_alerts {
            za_apply_default_alerts_by_value(
                server,
                &mut self.default_alerts,
                account_id,
                personal_id,
                &new_calendar_ids,
                &mut new_content.data.event,
            )
            .await
            .caused_by(trc::location!())?;
        }
        if let Err(err) = new_content
            .data
            .event
            .validate_user_data_changes(&previous_event)
        {
            update.fail(response, user_data_error(err));
            return Ok(());
        }

        // D10, invariant 59: the per-user split (`:826-893`) is not run for any
        // caller.
        let mut is_user_data_only = false;

        // Assign an organizer when participants were added to an event that had none
        let is_organizer_assigned = !is_user_data_only
            && self
                .account_info
                .addresses()
                .first()
                .is_some_and(|organizer_address| {
                    itip_assign_organizer(&mut new_content.data.event, organizer_address)
                });
        let has_scheduling_changes = !is_user_data_only
            && new_content
                .data
                .event
                .has_scheduling_changes(&previous_event);
        if is_owner
            && !is_metadata_changed
            && !is_organizer_assigned
            && !has_scheduling_changes
            && new_content.data.event.has_same_sequences(&previous_event)
        {
            is_user_data_only = true;
        }
        let is_origin_event = is_origin(&new_content.data.event, self.account_info.addresses());
        if !is_user_data_only && is_origin_event {
            if has_scheduling_changes
                || (calendar_event.inner.flags.to_native() ^ new_calendar_event.flags)
                    & SCHEDULING_EVENT_FLAGS
                    != 0
            {
                new_content.data.event.increment_sequence(&previous_event);
            }
            stamp_updated(&mut new_content.data.event, now() as i64);
        }
        // Spec 5.4: the SEQUENCE floor, right after upstream's increment, on
        // every update.
        za_sequence_floor(
            &mut new_content.data.event,
            &previous_event,
            self.roles.sender.addresses(),
        );

        if let Err(err) = assert_participants_limit(
            &new_content.data.event,
            server.core.groupware.max_ical_attendees_per_instance,
        ) {
            update.fail(response, err);
            return Ok(());
        }

        // Validate UID
        if !is_user_data_only {
            let description = match (
                new_content.data.event.object_uid(),
                previous_event.object_uid(),
            ) {
                (Some(new_uid), Some(old_uid)) if new_uid == old_uid => None,
                (None, None) => None,
                (None, Some(_)) => Some("The UID of a calendar event cannot be removed."),
                _ => Some("You cannot change the UID of a calendar event."),
            };
            if let Some(description) = description {
                update.fail(
                    response,
                    SetError::invalid_properties()
                        .with_property(JSCalendarProperty::Uid)
                        .with_description(description),
                );
                return Ok(());
            }
        }

        if is_restricted {
            let mut unchanged = new_calendar_event
                .unchanged_calendar_ids(calendar_event.inner)
                .peekable();
            let acl = if unchanged.peek().is_none() {
                EventAcl::for_calendars(
                    &self.cache,
                    access_token,
                    new_calendar_event.added_calendar_ids(calendar_event.inner),
                )
            } else {
                EventAcl::for_calendars(&self.cache, access_token, unchanged)
            };
            if let Err(err) =
                changes.assert_allowed(&acl, ownership, calendar_event.inner.flags.to_native())
            {
                update.fail(
                    response,
                    SetError::forbidden().with_description(err.description()),
                );
                return Ok(());
            }
        }

        // Check size and quota
        new_calendar_event.size = SizeWriter::ical(&new_content.data.event) as u32;
        if new_calendar_event.size as usize > server.core.groupware.max_ical_size {
            update.fail(
                response,
                too_large_event(
                    new_calendar_event.size as usize,
                    server.core.groupware.max_ical_size,
                ),
            );
            return Ok(());
        }

        // Obtain previous alarm, from the view and the stored flags
        let now = now() as i64;
        let prev_alarms = server
            .next_event_alarms(
                account_id,
                &EventAlarmUsers::new(account_id, content.inner)?
                    .with_event_flags(calendar_event.inner.flags.to_native()),
                &content.inner.data,
                &previous_calendar_ids,
                now,
                &mut self.default_alerts,
            )
            .await
            .caused_by(trc::location!())?;
        // Spec 5.2: whether the shaped tree differs from the stored one, read
        // before the build below takes the tree.
        let content_changed = new_content.data.event != previous_event;
        // Build event (in place of `:1019-1037`): no preferences, no default
        // alerts, the key-account flag.
        new_content.data = CalendarEventData::new_for(
            new_content.data.event,
            Tz::Floating,
            server.core.groupware.max_ical_instances,
            &DefaultAlerts::disabled(),
            true,
        );
        let next_alarms = server
            .next_event_alarms(
                account_id,
                &EventAlarmUsers::new(account_id, &new_content)?
                    .with_event_flags(new_calendar_event.flags),
                &new_content.data,
                &new_calendar_event
                    .names
                    .iter()
                    .map(DavName::parent_id)
                    .collect::<Vec<_>>(),
                now,
                &mut self.default_alerts,
            )
            .await
            .caused_by(trc::location!())?;

        // Validate quota
        let extra_bytes = (new_calendar_event.size as u64)
            .saturating_sub(u32::from(calendar_event.inner.size) as u64);
        if self.disk_quota.check(extra_bytes).is_err() {
            update.fail(response, SetError::over_quota());
            return Ok(());
        }

        // The pipeline: the contexts of every calendar the event stays in or,
        // for a change of its calendars, of every calendar it is in and every
        // one it enters (spec 5.2), the scheduling (D4, D18) and the commit
        // with the stored pairs and the stored display name (spec 5.4).
        let stored_event = calendar_event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        // The first kept name, else the first entered one (`patch_parent_ids`).
        let addressed = new_calendar_event.names[0].parent_id;
        // Spec 5.2: the writer builds the plan; `za_placement_contexts` asks
        // every right and gives every refusal.
        let placement = is_calendar_ids_changed
            .then(|| {
                ZaPlacementPlan::jmap(
                    &stored_event.names,
                    &new_calendar_event.names,
                    content_changed,
                )
            })
            .filter(ZaPlacementPlan::is_change);
        // Spec 3.3: a UID held in an entered calendar is answered with the
        // event's UID, which step 29 validated unchanged.
        let uid = placement.as_ref().and_then(|_| previous_event.object_uid());
        let contexts = match &placement {
            Some(plan) => match za_placement_contexts(server, access_token, account_id, plan).await
            {
                Ok(contexts) => contexts,
                Err(err) => return za_update_failed(update, response, err, uid),
            },
            None => match za_write_contexts(
                server,
                access_token,
                account_id,
                &stored_event,
                addressed,
                Acl::ModifyItems,
            )
            .await
            {
                Ok(contexts) => ZaPlacementContexts::unchanged(contexts),
                Err(err) => return za_update_failed(update, response, err, None),
            },
        };
        let schedule = if is_user_data_only {
            ZaScheduling::Off
        } else {
            self.schedule
        };
        let scheduled = match za_schedule_update(
            server,
            access_token,
            &self.roles,
            schedule,
            (!is_user_data_only).then_some(&previous_event),
            &mut new_calendar_event,
            &mut new_content,
            account_id,
            document_id,
        )
        .await
        {
            Ok(scheduled) => scheduled,
            Err(err) => return za_update_failed(update, response, err, None),
        };
        let written = match za_commit_update(
            server,
            access_token,
            &self.cache,
            &self.account,
            &self.keys,
            &self.roles,
            ZaEventUpdate {
                document_id,
                addressed,
                stored_event: &calendar_event_,
                stored_content: &content_,
                new_event: new_calendar_event,
                new_content,
                display_name,
                prev_alarms,
                next_alarms,
                scheduled,
                contexts,
                placement,
            },
        )
        .await
        {
            Ok(written) => written,
            Err(err) => return za_update_failed(update, response, err, uid),
        };
        self.disk_quota.reserve(extra_bytes);

        // Upstream's answer (`:1196-1260`, `:1380-1413`), from the committed tree.
        update.succeed(response);
        let server_set = tracked.map(|(client, before, dates)| {
            let mut server_set = dates
                .with_component(written.ical.main_component())
                .with_origin(is_origin_event)
                .server_set(&client, Some(&before));
            let imported = ImportedValues {
                participants: is_organizer_assigned,
                blob_properties: &blob_properties,
                every_blob_property: false,
                visible_to: None,
            };
            if imported.is_requested() {
                server_set.extend(imported.values(&written.ical, account_id, document_id));
            }
            server_set
        });
        if let (Some(base_id), Some(server_set)) = (update.base_id(), server_set) {
            response.add_server_set_properties(base_id, server_set);
        }
        for instance in update.instances_mut() {
            let (id, recurrence_key) = (instance.id, instance.recurrence_key);
            let Some(tracked) = instance.take_server_set() else {
                continue;
            };
            let mut values = TrackedValues::default()
                .with_component(written.ical.instance_component(tracked.recurrence_id))
                .server_set(&tracked.client, Some(&tracked.before));
            let imported = ImportedValues {
                participants: false,
                blob_properties: &tracked.blob_properties,
                every_blob_property: false,
                visible_to: None,
            };
            if imported.is_requested() {
                let data = CalendarEventData::new_for(
                    written.ical.clone(),
                    Tz::Floating,
                    server.core.groupware.max_ical_instances,
                    &DefaultAlerts::disabled(),
                    true,
                );
                values.extend(instance_imported_values(
                    &data,
                    recurrence_key,
                    &imported,
                    account_id,
                    document_id,
                ));
            }
            response.add_server_set_properties(id, values);
        }
        // D3: the change id of the last commit.
        response.new_state = State::Exact(written.change_id).into();
        Ok(())
    }

    /// One destroy, in upstream's order (`calendar_event/set.rs:1434-1605`),
    /// deleted by the pipeline (spec 6.5). `Ok(Ok(_))` is the commit's change
    /// id; `Err` is a store failure, which the caller answers under D28.
    async fn destroy(&mut self, reader: &mut ZaEventReader, id: Id) -> Answer<u64> {
        let server = self.server;
        let access_token = self.access_token;
        let account_id = self.account_id;
        let document_id = id.document_id();
        let is_owner = access_token.is_member(account_id);

        if !self.cache.has_item_id(&document_id) {
            return Ok(Err(SetError::not_found()));
        }
        let Some(calendar_event_) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                account_id,
                Collection::CalendarEvent,
                document_id,
            ))
            .await
            .caused_by(trc::location!())?
        else {
            return Ok(Err(SetError::not_found()));
        };
        let calendar_event = calendar_event_
            .to_unarchived::<CalendarEvent>()
            .caused_by(trc::location!())?;

        // Validate ACLs
        if let Err(err) = assert_privacy_access(
            EventPrivacy::from_flags(calendar_event.inner.flags.to_native()),
            EventViewer::new(is_owner),
        ) {
            return Ok(Err(err));
        }
        // PR #90 review C4617611549: a caller that is not a member is held to
        // the event's effective privacy, the higher of the row flags' and the
        // unsealed tree's, as `CalendarEvent/get` reads it. No view answers
        // `notFound`, as the update does.
        if !is_owner {
            let calendar_ids = calendar_event.inner.calendar_ids().collect::<Vec<_>>();
            let Some(event) = self
                .view_tree(reader, document_id, &calendar_ids, calendar_event.inner)
                .await?
            else {
                return Ok(Err(SetError::not_found()));
            };
            if let Err(err) = assert_privacy_access(
                EventPrivacy::from_flags(calendar_event.inner.flags.to_native())
                    .max(event.privacy()),
                EventViewer::new(is_owner),
            ) {
                return Ok(Err(err));
            }
        }
        // The notification audience and the stored content (`:1468-1498`)
        // are not taken: the ownership is read from the event's view, once,
        // and only for a calendar that asks for it.
        if let Some(can_delete_calendars) = &self.can_delete_calendars {
            let calendar_ids = calendar_event.inner.calendar_ids().collect::<Vec<_>>();
            let mut may_write_own = None;
            for &parent_id in &calendar_ids {
                if can_delete_calendars.contains(parent_id) {
                    continue;
                }
                if EventAcl::for_calendar(&self.cache, access_token, parent_id)
                    .may_manage_own_items()
                {
                    let owns = match may_write_own {
                        Some(owns) => owns,
                        None => *may_write_own.insert(
                            self.may_write_own(
                                reader,
                                document_id,
                                &calendar_ids,
                                calendar_event.inner,
                            )
                            .await?,
                        ),
                    };
                    if owns {
                        continue;
                    }
                }
                return Ok(Err(SetError::forbidden().with_description(format!(
                    "You are not allowed to remove events from calendar {}.",
                    Id::from(parent_id)
                ))));
            }
        }

        // D18: CalDAV's delete condition, with the owner's addresses, in
        // place of `:1525-1552`.
        let send_itip =
            match za_delete_send_itip(server, access_token, &self.roles.owner, self.schedule) {
                Ok(send_itip) => send_itip,
                Err(err) => return za_event_set_error(err, None).map(Err),
            };

        // Invariant 57: the pipeline deletes the event, in place of `:1554-1602`.
        match za_destroy_event(
            server,
            access_token,
            &self.cache,
            &self.keys,
            &self.roles,
            account_id,
            document_id,
            calendar_event_,
            send_itip,
        )
        .await
        {
            Ok(deleted) => Ok(Ok(deleted.change_id)),
            Err(err) => za_event_set_error(err, None).map(Err),
        }
    }

    /// Upstream's ownership test (`calendar_event/set.rs:1506-1510`) on the
    /// event's view, never on the sealed stored tree. A missing content or
    /// view owns nothing, as upstream's missing content.
    async fn may_write_own(
        &self,
        reader: &mut ZaEventReader,
        document_id: u32,
        calendar_ids: &[u32],
        meta: &ArchivedCalendarEvent,
    ) -> trc::Result<bool> {
        Ok(self
            .view_tree(reader, document_id, calendar_ids, meta)
            .await?
            .is_some_and(|event| self.identities.event_ownership(&event).may_write_own()))
    }

    /// The event's unsealed tree, through the request's reader; `None` for a
    /// missing content or view.
    async fn view_tree(
        &self,
        reader: &mut ZaEventReader,
        document_id: u32,
        calendar_ids: &[u32],
        meta: &ArchivedCalendarEvent,
    ) -> trc::Result<Option<ICalendar>> {
        let server = self.server;
        let Some(content_) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                self.account_id,
                Collection::CalendarEvent,
                document_id,
                CalendarEventField::Content,
            ))
            .await
            .caused_by(trc::location!())?
        else {
            return Ok(None);
        };
        let Some(view) = reader
            .view(
                server,
                self.access_token,
                document_id,
                calendar_ids,
                meta,
                &content_,
            )
            .await?
        else {
            return Ok(None);
        };
        let content = view
            .content
            .unarchive::<CalendarEventContent>()
            .caused_by(trc::location!())?;
        rkyv_deserialize::<_, ICalendar>(&content.data.event)
            .caused_by(trc::location!())
            .map(Some)
    }
}

/// A pipeline error of an update, answered as spec 8 says; `Err` is a store
/// failure (D28). `uid` is the event's UID on a change of its calendars.
fn za_update_failed(
    update: &EventUpdate<'_>,
    response: &mut SetResponse<calendar_event::CalendarEvent>,
    error: ZaPipelineError,
    uid: Option<&str>,
) -> trc::Result<()> {
    update.fail(response, za_event_set_error(error, uid)?);
    Ok(())
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
        // A calendar delete's refusal; not produced on this path.
        ZaPipelineError::CalendarTooLarge => Ok(SetError::forbidden()),
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
/// its kind, with the account id, the document id when known (a create has
/// none yet) and a fixed reason. The original error is not traced: its
/// context may carry stored bytes.
fn za_trace_set_failure(account_id: u32, document_id: Option<u32>, error: &trc::Error) {
    let event = error.event_type().into_err().account_id(account_id);
    let event = match document_id {
        Some(document_id) => event.document_id(document_id),
        None => event,
    };
    trc::error!(event.details("zero-access: calendar event write failed"));
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
                "CalendarTooLarge",
                ZaPipelineError::CalendarTooLarge,
                forbidden.clone(),
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
                "Busy(CalendarChanged)",
                ZaPipelineError::Busy(ZaBusy::CalendarChanged),
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
