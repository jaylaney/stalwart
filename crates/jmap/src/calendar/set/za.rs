/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 spec 6.7: `Calendar/set` for a key account. Upstream's private
//! validation runs in upstream's order; each object is its own commit (D3)
//! and a store error answers that object and every later one (D28).

use super::{CalendarServerSet, DefaultAlertValidator, RequestDefaultAlerts, update_calendar};
use crate::{
    api::{
        metadata::{
            MetadataPatches, MetadataType, MetadataWriter, ObjectMetadata, ZA_METADATA_REFUSED,
        },
        za_calendar::{ZA_CALENDAR_SHARE_WITH, za_set_incomplete},
    },
    changes::state::JmapCacheState,
};
use common::{
    GroupwareResources, Server, auth::AccessToken, sharing::EffectiveAcl,
    storage::quota::ObjectQuotaUsage, za::calendar::za_calendar_keyless_refusal,
};
use groupware::{
    cache::GroupwareCache,
    calendar::{
        CALENDAR_SUBSCRIBED, Calendar, CalendarPreferences,
        alerts::{CalendarAlarmsReschedule, CalendarSettings},
        pipeline::{
            ZaScheduling,
            admission::{ZaRoles, za_roles, za_writer_keys},
            context::za_mutation_context,
            delete::{za_calendar_delete_target, za_delete_calendar},
            error::ZaPipelineError,
            schedule::za_delete_send_itip,
        },
        privacy::EventPrivacy,
        seal::{calendar_view_in_domain, seal_calendar, seal_calendar_changes, seal_error},
    },
};
use jmap_proto::{
    error::set::SetError,
    method::set::{SetRequest, SetResponse},
    object::calendar::{self, CalendarProperty, CalendarValue},
    request::{MaybeInvalid, capability::CapabilityIds, reference::MaybeIdReference},
    types::state::State,
};
use jmap_tools::{JsonPointerItem, Key, Value};
use rand::{RngExt, distr::Alphanumeric};
use registry::schema::enums::StorageQuota;
use std::sync::Arc;
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes, BatchBuilder, PendingId, ValueClass},
};
use trc::{AddContext, EventType, LimitEvent, SecurityEvent, StoreEvent};
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection},
    field::PrincipalField,
    id::Id,
};
use utils::map::bitmap::Bitmap;
use vault::session::SessionKeys;

/// Upstream's text for a lost assertion (`calendar_set`'s `Err(err) if err.is_assertion_failure()`
/// arm, after "Write changes").
const ZA_MODIFIED: &str = "Another process modified this calendar, please try again.";
/// PR #90 review C4617611521: a destroy whose one transaction would exceed
/// the store's chunk size. The client can delete events first.
const ZA_TOO_MANY_EVENTS: &str =
    "This calendar holds too many events to delete at once. Delete some of its events first.";
/// Upstream's text for a caller that may not modify the calendar (`calendar_set`'s update
/// loop, "Validate ACL").
const ZA_NOT_ALLOWED: &str = "You are not allowed to modify this calendar.";
/// Upstream's text for a caller that may not delete the calendar (`calendar_set`'s destroy
/// loop, "Validate ACLs").
const ZA_NOT_ALLOWED_DELETE: &str = "You are not allowed to delete this calendar.";

type Answer<T> = trc::Result<Result<T, SetError<CalendarProperty>>>;
type ServerSet = Vec<(
    CalendarProperty,
    Value<'static, CalendarProperty, CalendarValue>,
)>;

struct ZaCreated {
    document_id: u32,
    change_id: Option<u64>,
    server_set: ServerSet,
}

/// The state every object of one call shares: the head's values, read once.
struct ZaCalendarWriter<'x> {
    server: &'x Server,
    access_token: &'x AccessToken,
    account_id: u32,
    cache: &'x Arc<GroupwareResources>,
    keys: &'x SessionKeys,
    is_shared: bool,
    personal_id: u32,
    metadata_writer: MetadataWriter,
    request_alerts: RequestDefaultAlerts,
    quota: ObjectQuotaUsage,
}

pub(super) async fn za_calendar_set(
    server: &Server,
    mut request: SetRequest<'_, calendar::Calendar>,
    access_token: &AccessToken,
    using: CapabilityIds,
) -> trc::Result<SetResponse<calendar::Calendar>> {
    // Head, once per call (upstream's `calendar_set` from its start to "Process creates").
    let account_id = request.account_id.document_id();
    let metadata = ObjectMetadata::new(server, access_token, using, MetadataType::Calendar);
    let sampled = metadata.viewer_change_id(server, account_id).await?;
    let cache = server
        .fetch_groupware_resources(
            access_token.account_id(),
            account_id,
            SyncCollection::Calendar,
        )
        .await?;
    let mut response = SetResponse::from_request(&request, server.core.jmap.set_max_objects)?
        .with_state(sampled.assert_state(cache.get_state(true), &request.if_in_state)?);
    let metadata_writer = MetadataWriter::new(metadata, account_id);
    let will_destroy = response.collect_will_destroy(request.unwrap_destroy());
    let is_shared = access_token.is_shared(account_id);
    let personal_id = access_token.personal_id(account_id, Collection::Calendar);
    let request_alerts = RequestDefaultAlerts::default();

    // Obtain quota
    let quota = if request.has_creates() && !is_shared {
        let account = server
            .account(account_id)
            .await
            .caused_by(trc::location!())?;
        server.object_quota_usage(&account, StorageQuota::MaxCalendars, || {
            cache.resources.count(true)
        })
    } else {
        ObjectQuotaUsage::unlimited()
    };

    // Spec 6.1: the writer's own keys, and the roles a destroy schedules with.
    let keys = match za_writer_keys(server, access_token, account_id).await {
        Ok(Some(keys)) => keys,
        // Spec 8: a store error before the first object fails the method.
        Err(ZaPipelineError::Store(error)) => return Err(error),
        Ok(None) | Err(_) => return Err(za_calendar_keyless_refusal().account_id(account_id)),
    };
    let roles = if will_destroy.is_empty() {
        None
    } else {
        Some(
            za_roles(server, access_token, account_id)
                .await
                .map_err(|error| match error {
                    ZaPipelineError::Store(error) => error,
                    _ => za_calendar_keyless_refusal().account_id(account_id),
                })?,
        )
    };

    let mut writer = ZaCalendarWriter {
        server,
        access_token,
        account_id,
        cache: &cache,
        keys: &keys,
        is_shared,
        personal_id,
        metadata_writer,
        request_alerts,
        quota,
    };
    // D28: after a store failure no later object is attempted.
    let mut failed = false;
    let mut last_change_id = None;
    let mut set_default: Option<u32> = None;
    let mut created_server_set = Vec::new();
    let mut destroyed_calendars: Vec<u32> = Vec::new();

    // Process creates
    for (id, object) in request.unwrap_create() {
        if failed {
            response.not_created.append(id, za_set_incomplete());
            continue;
        }
        match writer.create(object, created_server_set.len()).await {
            Ok(Ok(created)) => {
                response.created(id.clone(), created.document_id);
                if let Some(change_id) = created.change_id {
                    last_change_id = Some(change_id);
                }
                if let Some(MaybeIdReference::Reference(id_ref)) =
                    &request.arguments.on_success_set_is_default
                    && id_ref == &id
                {
                    set_default = Some(created.document_id);
                }
                created_server_set.push((id, created.document_id, created.server_set));
            }
            Ok(Err(err)) => response.not_created.append(id, err),
            Err(err) => {
                za_trace_set_failure(account_id, None, &err);
                failed = true;
                response.not_created.append(id, za_set_incomplete());
            }
        }
    }

    // Process updates
    for (id, object) in request.unwrap_update() {
        if failed {
            response.not_updated.append(id, za_set_incomplete());
            continue;
        }
        let id = match id {
            MaybeInvalid::Value(id) => id,
            invalid => {
                response.not_updated.append(invalid, SetError::not_found());
                continue;
            }
        };
        // Make sure id won't be destroyed
        if will_destroy.contains(&id) {
            response.not_updated.append(id, SetError::will_destroy());
            continue;
        }
        match writer.update(id, object).await {
            Ok(Ok(change_id)) => {
                if let Some(change_id) = change_id {
                    last_change_id = Some(change_id);
                }
                response.updated.append(id, None);
            }
            Ok(Err(err)) => response.not_updated.append(id, err),
            Err(err) => {
                za_trace_set_failure(account_id, Some(id.document_id()), &err);
                failed = true;
                response.not_updated.append(id, za_set_incomplete());
            }
        }
    }

    // Process deletions
    if let Some(roles) = &roles {
        let on_destroy_remove_events = request.arguments.on_destroy_remove_events.unwrap_or(false);
        for id in will_destroy {
            if failed {
                response.not_destroyed.append(id, za_set_incomplete());
                continue;
            }
            match writer.destroy(roles, id, on_destroy_remove_events).await {
                Ok(Ok(change_id)) => {
                    last_change_id = Some(change_id);
                    destroyed_calendars.push(id.document_id());
                    response.destroyed.push(id);
                }
                Ok(Err(err)) => response.not_destroyed.append(id, err),
                Err(err) => {
                    za_trace_set_failure(account_id, Some(id.document_id()), &err);
                    failed = true;
                    response.not_destroyed.append(id, za_set_incomplete());
                }
            }
        }
    }

    // Set default calendar (upstream's rule, `calendar_set`'s "Set default calendar" block)
    if let Some(MaybeIdReference::Id(id)) = &request.arguments.on_success_set_is_default {
        let document_id = id.document_id();
        let is_visible = if is_shared {
            cache
                .shared_containers(access_token, [Acl::Read, Acl::ReadItems], true)
                .contains(document_id)
        } else {
            cache.has_container_id(&document_id)
        };
        if is_visible && !destroyed_calendars.contains(&document_id) {
            set_default = Some(document_id);
        }
    }
    let mut changed_default = None;
    if let Some(default_calendar_id) = set_default.filter(|_| !is_shared)
        && response.not_created.is_empty()
        && response.not_updated.is_empty()
        && response.not_destroyed.is_empty()
    {
        // Spec 6.7: an id only, in a batch of its own. A store error leaves
        // the default as it was; the method still answers (D28).
        match za_write_default(server, &cache, account_id, default_calendar_id).await {
            Ok(changed) => changed_default = changed,
            Err(err) => za_trace_set_failure(account_id, Some(default_calendar_id), &err),
        }
    }

    // Server-set properties (upstream's `calendar_set`, after "Write changes": the
    // `created_server_set` loop and the default calendar's `isDefault` properties)
    if !created_server_set.is_empty() {
        let stored_default = match za_stored_default(server, account_id).await {
            Ok(stored_default) => stored_default,
            Err(err) => {
                za_trace_set_failure(account_id, None, &err);
                None
            }
        };
        let default_calendar_id = stored_default.or_else(|| {
            cache
                .document_ids(true)
                .filter(|id| !destroyed_calendars.contains(id))
                .chain(
                    created_server_set
                        .iter()
                        .map(|(_, document_id, _)| *document_id),
                )
                .min()
        });
        for (create_id, document_id, mut server_set) in created_server_set {
            server_set.push((
                CalendarProperty::IsDefault,
                Value::Bool(default_calendar_id == Some(document_id)),
            ));
            response.add_created_properties(&create_id, server_set);
        }
    }
    if let Some((default_calendar_id, previous_default_id)) = changed_default {
        response.add_server_set_property(
            Id::from(default_calendar_id),
            CalendarProperty::IsDefault,
            true,
        );
        if let Some(previous_default_id) = previous_default_id
            .filter(|id| *id != default_calendar_id && !destroyed_calendars.contains(id))
        {
            response.add_server_set_property(
                Id::from(previous_default_id),
                CalendarProperty::IsDefault,
                false,
            );
        }
    }

    // D3: the change id of the last commit, or the old state.
    if let Some(change_id) = last_change_id {
        response.new_state = State::Exact(change_id).into();
    }

    Ok(response)
}

impl ZaCalendarWriter<'_> {
    async fn create(
        &mut self,
        mut object: Value<'_, CalendarProperty, CalendarValue>,
        created: usize,
    ) -> Answer<ZaCreated> {
        let account_id = self.account_id;
        let personal_id = self.personal_id;
        if !self.quota.has_room(created) {
            return Ok(Err(SetError::over_quota().with_description(concat!(
                "There are too many calendars, ",
                "please delete some before adding a new one."
            ))));
        }
        if self.is_shared {
            return Ok(Err(SetError::forbidden()
                .with_description("Cannot create calendars in a shared account.")));
        }
        // Spec 6.11: sharing is the account page's.
        if names_share_with(&object) {
            return Ok(Err(
                SetError::forbidden().with_description(ZA_CALENDAR_SHARE_WITH)
            ));
        }

        let mut calendar = Calendar {
            name: rand::rng()
                .sample_iter(Alphanumeric)
                .take(10)
                .map(char::from)
                .collect::<String>(),
            preferences: vec![CalendarPreferences {
                account_id,
                name: "".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        // Process changes
        match self
            .metadata_writer
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
        let client_properties = object
            .as_object()
            .map(|object| {
                object
                    .keys()
                    .filter_map(|key| key.as_property().cloned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        calendar.preferences_mut(personal_id).flags |= CALENDAR_SUBSCRIBED;
        let changes =
            match update_calendar(None, object, &mut calendar, self.access_token, account_id) {
                Ok(changes) => changes,
                Err(err) => return Ok(Err(err)),
            };
        calendar.sync_owner_preferences(account_id, personal_id);
        if changes.default_alerts
            && let Err(err) = self
                .server
                .assert_unique_default_alerts(
                    self.cache,
                    account_id,
                    personal_id,
                    None,
                    &calendar,
                    &self.request_alerts,
                )
                .await?
        {
            return Ok(Err(err));
        }

        // Both read the plaintext entry, before sealing; the alert ids are
        // accepted only once the object's commit returns `Ok` (D3).
        let server_set = calendar.server_set_values(personal_id, &client_properties);
        let accepted = calendar.clone();

        // Spec 6.7: the name, description, colour and a custom time zone are
        // sealed under the owner's keys (MKCOL's form).
        seal_calendar(&mut calendar, self.keys, account_id)
            .map_err(|err| seal_error(err, account_id, u32::MAX))?;

        // D3: a commit of its own.
        let mut batch = BatchBuilder::new();
        let slot = batch.reserve_document_id(account_id, Collection::Calendar);
        calendar
            .insert(self.access_token.changed_by(), account_id, slot, &mut batch)
            .caused_by(trc::location!())?;
        let assigned_ids = match self.server.commit_batch(batch).await {
            Ok(assigned_ids) => assigned_ids,
            Err(err) => return za_calendar_set_error(ZaPipelineError::Store(err)).map(Err),
        };
        self.request_alerts.accept(None, &accepted, personal_id);
        Ok(Ok(ZaCreated {
            document_id: assigned_ids.slot(slot),
            change_id: assigned_ids.change_id(account_id, SyncCollection::Calendar),
            server_set,
        }))
    }

    async fn update(
        &mut self,
        id: Id,
        mut object: Value<'_, CalendarProperty, CalendarValue>,
    ) -> Answer<Option<u64>> {
        let account_id = self.account_id;
        let personal_id = self.personal_id;
        match self
            .metadata_writer
            .extract(MetadataPatches::for_update(), &mut object)
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

        // Obtain calendar
        let document_id = id.document_id();
        let Some(calendar_) = self
            .server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                account_id,
                Collection::Calendar,
                document_id,
            ))
            .await?
        else {
            return Ok(Err(SetError::not_found()));
        };
        let acl = calendar_
            .to_unarchived::<Calendar>()
            .caused_by(trc::location!())?
            .inner
            .acls
            .effective_acl(self.access_token);
        if self.is_shared && !acl.contains_any([Acl::Read, Acl::ReadItems].into_iter()) {
            return Ok(Err(SetError::not_found()));
        }

        // Spec 6.11: sharing is the account page's.
        if names_share_with(&object) {
            return Ok(Err(
                SetError::forbidden().with_description(ZA_CALENDAR_SHARE_WITH)
            ));
        }

        // PROPPATCH's context: the caller's own entry, at `Modify`.
        let context = match za_mutation_context(
            self.server,
            self.access_token,
            account_id,
            document_id,
            Bitmap::new().with_item(Acl::Modify),
        )
        .await
        {
            Ok(Some(context)) => context,
            Ok(None) => return Ok(Err(SetError::forbidden())),
            Err(err) => {
                return za_calendar_set_error(ZaPipelineError::from_calendar(
                    err,
                    account_id,
                    document_id,
                ))
                .map(Err);
            }
        };
        let stored = context
            .stored
            .deserialize::<Calendar>()
            .caused_by(trc::location!())?;
        // Spec 6.7: the handler never adds a preference entry.
        if stored.personal_preferences(context.personal_id).is_none() {
            return Ok(Err(SetError::forbidden().with_description(ZA_NOT_ALLOWED)));
        }
        let mut new_calendar =
            calendar_view_in_domain(&context.stored, &context.domain, context.personal_id)
                .map_err(|err| seal_error(err, account_id, document_id))?
                .deserialize::<Calendar>()
                .caused_by(trc::location!())?;

        // Apply changes
        if !self.is_shared {
            new_calendar.inherit_owner_preferences(account_id, personal_id);
        }
        let changes = match update_calendar(
            Some(id),
            object,
            &mut new_calendar,
            self.access_token,
            account_id,
        ) {
            Ok(changes) => changes,
            Err(err) => return Ok(Err(err)),
        };
        if !self.is_shared {
            new_calendar.sync_owner_preferences(account_id, personal_id);
        }
        if changes.default_alerts
            && let Err(err) = self
                .server
                .assert_unique_default_alerts(
                    self.cache,
                    account_id,
                    personal_id,
                    Some(document_id),
                    &new_calendar,
                    &self.request_alerts,
                )
                .await?
        {
            return Ok(Err(err));
        }

        // D3: a commit of its own, in PROPPATCH's order.
        let mut batch = BatchBuilder::new();
        context.assert_in(&mut batch);
        let sealed =
            seal_calendar_changes(&stored, &new_calendar, &context.domain, context.personal_id)
                .map_err(|err| seal_error(err, account_id, document_id))?;
        let current = context
            .stored
            .to_unarchived::<Calendar>()
            .caused_by(trc::location!())?;
        self.server
            .reschedule_calendar_alarms(
                self.cache,
                account_id,
                document_id,
                CalendarSettings::from(current.inner),
                CalendarSettings::from(&sealed),
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
        sealed
            .update(
                self.access_token.changed_by(),
                current,
                account_id,
                document_id,
                &mut batch,
            )
            .caused_by(trc::location!())?;
        // D3: the request's alert ids are accepted once the commit returns `Ok`.
        match self.server.commit_batch(batch).await {
            Ok(assigned_ids) => {
                self.request_alerts
                    .accept(Some(document_id), &new_calendar, personal_id);
                Ok(Ok(
                    assigned_ids.change_id(account_id, SyncCollection::Calendar)
                ))
            }
            Err(err) => za_calendar_set_error(ZaPipelineError::Store(err)).map(Err),
        }
    }

    async fn destroy(
        &mut self,
        roles: &ZaRoles,
        id: Id,
        on_destroy_remove_events: bool,
    ) -> Answer<u64> {
        let account_id = self.account_id;
        let cache = self.cache;
        let document_id = id.document_id();

        if !cache.has_container_id(&document_id) {
            return Ok(Err(SetError::not_found()));
        }
        let Some(calendar_) = self
            .server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                account_id,
                Collection::Calendar,
                document_id,
            ))
            .await
            .caused_by(trc::location!())?
        else {
            return Ok(Err(SetError::not_found()));
        };

        // Validate ACLs
        if self.is_shared
            && !calendar_
                .to_unarchived::<Calendar>()
                .caused_by(trc::location!())?
                .inner
                .acls
                .effective_acl(self.access_token)
                .contains(Acl::Delete)
        {
            return Ok(Err(
                SetError::forbidden().with_description(ZA_NOT_ALLOWED_DELETE)
            ));
        }

        // Obtain children ids
        let mut has_visible_children = false;
        let mut has_restricted_children = false;
        for flags in cache
            .children(document_id)
            .map(|child| child.resource.event_flags().unwrap_or_default())
        {
            match EventPrivacy::from_flags(flags) {
                EventPrivacy::Public => has_visible_children = true,
                EventPrivacy::Private => {
                    has_visible_children = true;
                    has_restricted_children = true;
                }
                EventPrivacy::Secret => {
                    has_visible_children |= !self.is_shared;
                    has_restricted_children = true;
                }
            }
        }
        if has_visible_children && !on_destroy_remove_events {
            return Ok(Err(SetError::calendar_has_event()));
        }
        if self.is_shared && has_restricted_children {
            return Ok(Err(
                SetError::forbidden().with_description(ZA_NOT_ALLOWED_DELETE)
            ));
        }

        // D16: the pipeline's collection delete schedules its cancellations.
        let target = match za_calendar_delete_target(
            self.server,
            self.access_token,
            account_id,
            document_id,
            calendar_,
        )
        .await
        {
            Ok(target) => target,
            Err(err) => return za_calendar_set_error(err).map(Err),
        };
        let send_itip = match za_delete_send_itip(
            self.server,
            self.access_token,
            &roles.owner,
            ZaScheduling::On,
        ) {
            Ok(send_itip) => send_itip,
            Err(err) => return za_calendar_set_error(err).map(Err),
        };
        let children = cache.children_ids(document_id).collect::<Vec<_>>();
        let href = cache
            .container_resource_path_by_id(document_id)
            .map(|resource| cache.format_resource(resource));
        let cleanup = self
            .server
            .preload_container_cleanup(
                self.access_token.changed_by(),
                account_id,
                Collection::CalendarEvent,
                &children
                    .iter()
                    .copied()
                    .filter(|&document_id| {
                        cache
                            .item_by_id(document_id)
                            .is_some_and(|item| !item.metadata_kinds().is_empty())
                    })
                    .collect(),
            )
            .await
            .caused_by(trc::location!())?;
        match za_delete_calendar(
            self.server,
            self.access_token,
            cache,
            self.keys,
            roles,
            account_id,
            document_id,
            target,
            children,
            &cleanup,
            href,
            send_itip,
        )
        .await
        {
            Ok(deleted) => Ok(Ok(deleted.change_id)),
            Err(err) => za_calendar_set_error(err).map(Err),
        }
    }
}

/// Spec 6.11: `shareWith`, as a property or as a patch pointer whose first
/// item it is.
fn names_share_with(object: &Value<'_, CalendarProperty, CalendarValue>) -> bool {
    object.as_object().is_some_and(|object| {
        object.keys().any(|key| match key {
            Key::Property(CalendarProperty::ShareWith) => true,
            Key::Property(CalendarProperty::Pointer(pointer)) => matches!(
                pointer.iter().next(),
                Some(JsonPointerItem::Key(Key::Property(
                    CalendarProperty::ShareWith
                )))
            ),
            _ => false,
        })
    })
}

/// Upstream's default rule after the objects: write `default_calendar_id`
/// unless it is already the stored id or the lowest calendar id. Returns the
/// new and the previous default when it wrote.
async fn za_write_default(
    server: &Server,
    cache: &GroupwareResources,
    account_id: u32,
    default_calendar_id: u32,
) -> trc::Result<Option<(u32, Option<u32>)>> {
    let previous_default_id = za_stored_default(server, account_id)
        .await?
        .or_else(|| cache.document_ids(true).min());
    if Some(default_calendar_id) == previous_default_id {
        return Ok(None);
    }
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .set(
            PrincipalField::DefaultCalendarId,
            PendingId::Assigned(default_calendar_id),
        );
    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;
    Ok(Some((default_calendar_id, previous_default_id)))
}

async fn za_stored_default(server: &Server, account_id: u32) -> trc::Result<Option<u32>> {
    server
        .store()
        .get_value::<u32>(ValueKey {
            account_id,
            collection: Collection::Principal.into(),
            document_id: 0,
            class: ValueClass::Property(PrincipalField::DefaultCalendarId.into()),
        })
        .await
        .caused_by(trc::location!())
}

/// Spec 8, D28 (I §7.5): a store failure is traced once as a fresh event of
/// its kind, with the ids and a fixed reason. The original error is not
/// traced: its context may carry stored bytes.
fn za_trace_set_failure(account_id: u32, document_id: Option<u32>, error: &trc::Error) {
    let event = error
        .event_type()
        .into_err()
        .account_id(account_id)
        .details("zero-access: calendar write failed");
    trc::error!(match document_id {
        Some(document_id) => event.document_id(document_id),
        None => event,
    });
}

/// Spec 8: the pipeline's errors as `Calendar/set` answers them. `Err` is a
/// store failure (D28).
fn za_calendar_set_error(error: ZaPipelineError) -> Result<SetError<CalendarProperty>, trc::Error> {
    match error {
        ZaPipelineError::Forbidden(_) | ZaPipelineError::LinkedEvent => Ok(SetError::forbidden()),
        ZaPipelineError::NotFound => Ok(SetError::not_found()),
        ZaPipelineError::Busy(_) => Ok(SetError::forbidden().with_description(ZA_MODIFIED)),
        ZaPipelineError::CalendarTooLarge => {
            Ok(SetError::forbidden().with_description(ZA_TOO_MANY_EVENTS))
        }
        // Not produced on these paths.
        ZaPipelineError::UidConflict { .. }
        | ZaPipelineError::TooManyRecipients
        | ZaPipelineError::Scheduling(_)
        | ZaPipelineError::SchedulingDenied(_) => Ok(SetError::forbidden()),
        ZaPipelineError::Store(error) => match error.event_type() {
            EventType::Store(StoreEvent::AssertValueFailed) => {
                Ok(SetError::forbidden().with_description(ZA_MODIFIED))
            }
            EventType::Limit(LimitEvent::Quota) => Ok(SetError::over_quota()),
            EventType::Security(SecurityEvent::Unauthorized) => Ok(SetError::forbidden()),
            _ => Err(error),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::za_calendar_set_error;
    use groupware::{
        calendar::pipeline::error::{ZaBusy, ZaPipelineError, ZaRefusal},
        scheduling::ItipError,
    };
    use serde_json::{Value, json};
    use trc::{LimitEvent, SecurityEvent, StoreEvent};

    fn answer(error: ZaPipelineError) -> Value {
        serde_json::to_value(
            za_calendar_set_error(error).expect("a set error, not a store failure"),
        )
        .unwrap()
    }

    #[test]
    fn calendar_set_error_table() {
        let forbidden = json!({ "type": "forbidden" });
        let modified = json!({
            "type": "forbidden",
            "description": "Another process modified this calendar, please try again.",
        });
        let over_quota = json!({ "type": "overQuota", "description": "Account quota exceeded." });
        let too_many_events = json!({
            "type": "forbidden",
            "description": "This calendar holds too many events to delete at once. Delete some of its events first.",
        });
        for (label, error, expected) in [
            (
                "Forbidden(Context)",
                ZaPipelineError::Forbidden(ZaRefusal::Context),
                &forbidden,
            ),
            (
                "Forbidden(SharedCopy)",
                ZaPipelineError::Forbidden(ZaRefusal::SharedCopy),
                &forbidden,
            ),
            ("LinkedEvent", ZaPipelineError::LinkedEvent, &forbidden),
            (
                "Store(Security(Unauthorized))",
                ZaPipelineError::Store(SecurityEvent::Unauthorized.into_err()),
                &forbidden,
            ),
            (
                "NotFound",
                ZaPipelineError::NotFound,
                &json!({ "type": "notFound" }),
            ),
            (
                "Busy(LeaseBusy)",
                ZaPipelineError::Busy(ZaBusy::LeaseBusy),
                &modified,
            ),
            (
                "Busy(CreateLost)",
                ZaPipelineError::Busy(ZaBusy::CreateLost),
                &modified,
            ),
            (
                "Busy(CalendarChanged)",
                ZaPipelineError::Busy(ZaBusy::CalendarChanged),
                &modified,
            ),
            (
                "CalendarTooLarge",
                ZaPipelineError::CalendarTooLarge,
                &too_many_events,
            ),
            (
                "Store(AssertValueFailed)",
                ZaPipelineError::Store(StoreEvent::AssertValueFailed.into_err()),
                &modified,
            ),
            (
                "Store(Limit(Quota))",
                ZaPipelineError::Store(LimitEvent::Quota.into_err()),
                &over_quota,
            ),
            (
                "UidConflict",
                ZaPipelineError::UidConflict { href: None },
                &forbidden,
            ),
            (
                "TooManyRecipients",
                ZaPipelineError::TooManyRecipients,
                &forbidden,
            ),
            (
                "Scheduling",
                ZaPipelineError::Scheduling(ItipError::MultipleOrganizer),
                &forbidden,
            ),
            (
                "SchedulingDenied",
                ZaPipelineError::SchedulingDenied("x"),
                &forbidden,
            ),
        ] {
            assert_eq!(&answer(error), expected, "{label}");
        }

        assert!(
            za_calendar_set_error(ZaPipelineError::Store(
                StoreEvent::UnexpectedError.into_err()
            ))
            .is_err(),
            "Store(UnexpectedError) is a store failure"
        );
    }
}
