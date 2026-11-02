/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{za_calendar_error, za_write_contexts};
use crate::{
    DavError, DavMethod, PropStatBuilder,
    calendar::assert_event_privacy_access,
    common::{
        ETag, ExtractETag,
        dead::{DeadPatch, DeadTarget, DisplayName},
        lock::{LockRequestHandler, ResourceState},
        uri::DavUriResource,
        za::{ZaDeadSeal, za_mutation_context},
    },
};
use calcard::common::timezone::Tz;
use common::storage::metadata::MetadataWrite;
use common::{
    Server,
    auth::AccessToken,
    storage::index::{PresenceFlags, RewritePresence},
};
use dav_proto::{
    RequestHeaders, Return,
    schema::{
        Namespace,
        property::{CalDavProperty, DavProperty, DavValue, ResourceType, WebDavProperty},
        request::{DavPropertyValue, PropertyUpdate, PropertyUpdateOp},
        response::{BaseCondition, CalCondition, MultiStatus, Response},
    },
};
use groupware::{
    cache::GroupwareCache,
    calendar::{
        Calendar, CalendarEvent, CalendarEventContent, SupportedComponent, Timezone,
        alerts::{CalendarAlarmsReschedule, CalendarSettings},
        privacy::EventViewer,
        seal::{
            ZaContainerOf, ZaEvent, calendar_view_in_domain, event_pairs_in_domain,
            rewrap_container, rewrap_event, seal_calendar_changes, seal_error,
            seal_event_in_domain, unseal_event_in_domain,
        },
        sharing::resolve::{ZaCalendarContext, calendars_changed},
    },
};
use http_proto::HttpResponse;
use hyper::StatusCode;
use std::str::FromStr;
use store::write::BatchBuilder;
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes},
};
use trc::AddContext;
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection},
    field::CalendarEventField,
    metadata::MetadataKinds,
};
use utils::map::bitmap::Bitmap;

pub(crate) trait CalendarPropPatchRequestHandler: Sync + Send {
    fn handle_calendar_proppatch_request(
        &self,
        access_token: &AccessToken,
        headers: &RequestHeaders<'_>,
        request: PropertyUpdate,
    ) -> impl Future<Output = crate::Result<HttpResponse>> + Send;

    fn apply_calendar_properties(
        &self,
        personal_id: u32,
        calendar: &mut Calendar,
        is_update: bool,
        properties: impl IntoIterator<Item = DavPropertyValue>,
        items: &mut PropStatBuilder,
    );

    fn apply_event_properties(
        &self,
        event: &mut CalendarEvent,
        properties: impl IntoIterator<Item = DavPropertyValue>,
        items: &mut PropStatBuilder,
    );
}

impl CalendarPropPatchRequestHandler for Server {
    async fn handle_calendar_proppatch_request(
        &self,
        access_token: &AccessToken,
        headers: &RequestHeaders<'_>,
        mut request: PropertyUpdate,
    ) -> crate::Result<HttpResponse> {
        // Validate URI
        let resource_ = self
            .validate_uri(access_token, headers.uri)
            .await?
            .into_owned_uri()?;
        let uri = headers.uri;
        let account_id = resource_.account_id;
        let za_keys = super::za_writer_keys(self, access_token, account_id).await?;
        let resources = self
            .fetch_groupware_resources(
                access_token.account_id(),
                account_id,
                SyncCollection::Calendar,
            )
            .await
            .caused_by(trc::location!())?;
        let resource = resource_
            .resource
            .and_then(|r| resources.by_path(r))
            .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
        let document_id = resource.document_id();
        let collection = if resource.is_container() {
            Collection::Calendar
        } else {
            Collection::CalendarEvent
        };

        if !request.has_changes() {
            return Ok(HttpResponse::new(StatusCode::NO_CONTENT));
        }

        // Verify ACL
        let is_owner = access_token.is_member(account_id);
        if !is_owner {
            let (acl, document_id) = if resource.is_container() {
                (Acl::Modify, resource.document_id())
            } else {
                assert_event_privacy_access(
                    resource.resource.event_flags(),
                    EventViewer::new(is_owner),
                )?;
                (
                    Acl::ModifyItems,
                    resource
                        .parent_id()
                        .ok_or(DavError::Code(StatusCode::NOT_FOUND))?,
                )
            };

            if !resources.has_access_to_container(access_token, document_id, acl) {
                return Err(DavError::Code(StatusCode::FORBIDDEN));
            }
        }

        // Fetch archive
        let archive = self
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                account_id,
                collection,
                document_id,
            ))
            .await
            .caused_by(trc::location!())?
            .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;

        #[cfg(feature = "test_mode")]
        if resource.is_container() {
            common::za::test_hooks::calendar_proppatch_read_pause_point(account_id).await;
        }

        // Calendar properties write only this principal's personal bundle and
        // shared dead properties. The codec retains every unopened personal
        // bundle and ring verbatim.
        let calendar_context = if resource.is_container() && za_keys.is_some() {
            za_mutation_context(
                self,
                access_token,
                account_id,
                document_id,
                Bitmap::new().with_item(Acl::Modify),
            )
            .await
            .map_err(|error| za_calendar_error(error, account_id, document_id))?
        } else {
            None
        };
        // Spec 8.1: an event is edited as plaintext; the stored archives stay
        // the update's current values.
        let event_contexts = if !resource.is_container() && za_keys.is_some() {
            za_write_contexts(
                self,
                access_token,
                account_id,
                &archive
                    .deserialize::<CalendarEvent>()
                    .caused_by(trc::location!())?,
                resource
                    .parent_id()
                    .ok_or(DavError::Code(StatusCode::NOT_FOUND))?,
                Acl::ModifyItems,
            )
            .await?
        } else {
            Vec::new()
        };
        // The conditional headers are evaluated against the archive the final
        // write asserts.
        let archive = calendar_context
            .as_ref()
            .map(|context| &context.stored)
            .unwrap_or(&archive);
        let etag = if resource.is_container() {
            archive.etag()
        } else {
            format!(
                "\"{}\"",
                archive
                    .unarchive::<CalendarEvent>()
                    .caused_by(trc::location!())?
                    .etag
                    .to_native()
            )
        };

        // Validate headers
        self.validate_headers(
            access_token,
            headers,
            vec![ResourceState {
                account_id,
                collection,
                document_id: document_id.into(),
                etag: etag.clone().into(),
                path: resource_.resource.unwrap(),
                ..Default::default()
            }],
            Default::default(),
            DavMethod::PROPPATCH,
        )
        .await?;

        let dead = DeadPatch::take(&mut request.ops, DisplayName::Live);
        let has_live_changes = !request.ops.is_empty();
        let mut batch = BatchBuilder::new();
        if za_keys.is_none() {
            crate::common::za::za_ordinary_fence(self, &mut batch, account_id, None);
        }
        let mut items = PropStatBuilder::default();
        let mut retry_event = None;

        let (is_success, etag) = if resource.is_container() {
            // Deserialize
            let calendar = archive
                .to_unarchived::<Calendar>()
                .caused_by(trc::location!())?;
            let personal_id = calendar_context
                .as_ref()
                .map(|context| context.personal_id)
                .unwrap_or_else(|| access_token.personal_id(account_id, Collection::Calendar));
            let is_member = access_token.is_member(account_id);

            // Apply live properties
            let new_calendar = if has_live_changes {
                let mut new_calendar = match &calendar_context {
                    Some(context) => calendar_view_in_domain(
                        &context.stored,
                        &context.domain,
                        context.personal_id,
                    )
                    .map_err(|err| seal_error(err, account_id, document_id))?
                    .deserialize::<Calendar>()
                    .caused_by(trc::location!())?,
                    None => archive
                        .deserialize::<Calendar>()
                        .caused_by(trc::location!())?,
                };
                if is_member {
                    new_calendar.inherit_owner_preferences(account_id, personal_id);
                }
                for op in request.ops {
                    match op {
                        PropertyUpdateOp::Set(value) => self.apply_calendar_properties(
                            personal_id,
                            &mut new_calendar,
                            true,
                            [value],
                            &mut items,
                        ),
                        PropertyUpdateOp::Remove(property) => remove_calendar_properties(
                            personal_id,
                            &mut new_calendar,
                            [property],
                            &mut items,
                        ),
                    }
                }
                Some(new_calendar)
            } else {
                None
            };

            // Apply dead properties
            let za_seal = calendar_context.as_ref().map(|context| {
                ZaDeadSeal::calendar(&context.domain, account_id, Some(document_id))
            });
            let mut dead_write = dead
                .apply_za(
                    self,
                    DeadTarget::container(
                        account_id,
                        Collection::Calendar,
                        document_id,
                        calendar.inner.metadata_kinds(),
                    ),
                    za_seal.as_ref(),
                    &mut items,
                )
                .await
                .caused_by(trc::location!())?;

            if items.has_errors() {
                (false, etag)
            } else {
                if let Some(context) = &calendar_context {
                    context.assert_in(&mut batch);
                }
                if let Some(mut new_calendar) = new_calendar {
                    if is_member {
                        new_calendar.sync_owner_preferences(account_id, personal_id);
                    }
                    if let Some(context) = &calendar_context {
                        new_calendar = seal_calendar_changes(
                            &archive
                                .deserialize::<Calendar>()
                                .caused_by(trc::location!())?,
                            &new_calendar,
                            &context.domain,
                            personal_id,
                        )
                        .map_err(|err| seal_error(err, account_id, document_id))?;
                    }
                    if let Some(dead_write) = &dead_write {
                        new_calendar.set_metadata_kinds(dead_write.kinds);
                    }
                    self.reschedule_calendar_alarms(
                        &resources,
                        account_id,
                        document_id,
                        CalendarSettings::from(calendar.inner),
                        CalendarSettings::from(&new_calendar),
                        &mut batch,
                    )
                    .await
                    .caused_by(trc::location!())?;
                    // Ruling 70: a plaintext container's ordinary-writer
                    // assertion.
                    if let Some(dead_write) = &dead_write {
                        dead_write.za_fence(&mut batch);
                    }
                    if let Some(write) = dead_write.and_then(|dead_write| dead_write.write) {
                        write
                            .build(document_id.into(), &mut batch)
                            .caused_by(trc::location!())?;
                    }
                    new_calendar
                        .update(
                            access_token.changed_by(),
                            calendar,
                            account_id,
                            document_id,
                            &mut batch,
                        )
                        .caused_by(trc::location!())?;
                } else if let Some(dead_write) = dead_write.as_mut() {
                    dead_write.za_fence(&mut batch);
                    if let Some(write) = dead_write.write.take() {
                        write
                            .build(document_id.into(), &mut batch)
                            .caused_by(trc::location!())?;
                    }
                    Calendar::rewrite_presence(
                        &calendar,
                        dead_write.kinds,
                        account_id,
                        document_id,
                        &mut batch,
                    )
                    .caused_by(trc::location!())?;
                }
                (true, batch.etag().unwrap_or(etag))
            }
        } else {
            // Deserialize
            let event = archive
                .to_unarchived::<CalendarEvent>()
                .caused_by(trc::location!())?;

            // Ruling 15: a key account's display name lives in the sealed
            // content, so a live change is a sealed full write over the
            // stored content archive.
            let za_content = if has_live_changes && !event_contexts.is_empty() {
                Some(
                    self.store()
                        .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                            account_id,
                            Collection::CalendarEvent,
                            document_id,
                            CalendarEventField::Content,
                        ))
                        .await
                        .caused_by(trc::location!())?
                        .ok_or(DavError::Code(StatusCode::NOT_FOUND))?,
                )
            } else {
                None
            };

            // Apply live properties
            let new_event = if has_live_changes {
                let mut new_event = archive
                    .deserialize::<CalendarEvent>()
                    .caused_by(trc::location!())?;
                let mut za_event = None;
                if let (Some(context), Some(content)) = (event_contexts.first(), &za_content) {
                    let mut stored = ZaEvent::new(
                        new_event.clone(),
                        content
                            .deserialize::<CalendarEventContent>()
                            .caused_by(trc::location!())?,
                    );
                    // The plaintext pair, edited below and sealed again.
                    unseal_event_in_domain(&mut stored, &context.domain)
                        .map_err(|err| seal_error(err, account_id, document_id))?;
                    new_event = stored.meta.clone();
                    za_event = Some(stored);
                }
                for op in request.ops {
                    match op {
                        PropertyUpdateOp::Set(value) => {
                            self.apply_event_properties(&mut new_event, [value], &mut items)
                        }
                        PropertyUpdateOp::Remove(property) => {
                            remove_event_properties(&mut new_event, [property], &mut items)
                        }
                    }
                }
                Some((new_event, za_event))
            } else {
                None
            };

            // Apply dead properties
            let za_uid = event.inner.uid.to_string();
            let za_seal = event_contexts.first().map(|context| {
                ZaDeadSeal::event(&context.domain, account_id, document_id, &za_uid)
            });
            let mut dead_write = dead
                .apply_za(
                    self,
                    DeadTarget::item(
                        account_id,
                        Collection::CalendarEvent,
                        document_id,
                        event.inner.metadata_kinds(),
                    ),
                    za_seal.as_ref(),
                    &mut items,
                )
                .await
                .caused_by(trc::location!())?;

            if items.has_errors() {
                (false, etag)
            } else {
                let mut new_etag = None;
                for context in &event_contexts {
                    context.assert_in(&mut batch);
                }
                // Ruling 70: a plaintext container's ordinary-writer assertion.
                if let Some(dead_write) = &dead_write {
                    dead_write.za_fence(&mut batch);
                }
                if let Some(write) = dead_write
                    .as_mut()
                    .and_then(|dead_write| dead_write.write.take())
                {
                    if !event_contexts.is_empty() {
                        retry_event = Some((None, Some(write.clone())));
                    }
                    write
                        .build(document_id.into(), &mut batch)
                        .caused_by(trc::location!())?;
                }
                if let Some((mut new_event, za_event)) = new_event {
                    if let Some(dead_write) = &dead_write {
                        new_event.set_metadata_kinds(dead_write.kinds);
                    }
                    match (za_event, event_contexts.first(), &za_content) {
                        (Some(mut za_event), Some(context), Some(content)) => {
                            let pairs =
                                event_pairs_in_domain(&za_event.meta, content, &context.domain)
                                    .map_err(|err| seal_error(err, account_id, document_id))?;
                            za_event.meta = new_event;
                            seal_event_in_domain(&mut za_event, &context.domain, pairs.as_ref())
                                .map_err(|err| seal_error(err, account_id, document_id))?;
                            retry_event = Some((
                                Some(za_event.clone()),
                                retry_event.and_then(|(_, write)| write),
                            ));
                            let (meta, content_) = za_event.into_parts();
                            new_etag = meta
                                .update_full(
                                    content_,
                                    access_token.changed_by(),
                                    event,
                                    content
                                        .unarchive::<CalendarEventContent>()
                                        .caused_by(trc::location!())?,
                                    account_id,
                                    document_id,
                                    None,
                                    &mut batch,
                                )
                                .caused_by(trc::location!())?
                                .into();
                        }
                        _ => {
                            new_etag = new_event
                                .update_meta(
                                    access_token.changed_by(),
                                    event,
                                    account_id,
                                    document_id,
                                    None,
                                    &mut batch,
                                )
                                .caused_by(trc::location!())?
                                .into();
                        }
                    }
                } else if let Some(dead_write) = &dead_write {
                    CalendarEvent::rewrite_presence(
                        &event,
                        dead_write.kinds,
                        account_id,
                        document_id,
                        &mut batch,
                    )
                    .caused_by(trc::location!())?;
                }
                (true, new_etag.unwrap_or(etag))
            }
        };
        let mut etag = etag;

        if is_success {
            if !batch.is_empty() {
                #[cfg(feature = "test_mode")]
                common::za::test_hooks::update_commit_pause_point(account_id).await;
                match self.commit_batch(batch).await {
                    Ok(_) => {}
                    Err(error) if error.is_assertion_failure() && retry_event.is_some() => {
                        // Ruling 29: one bounded retry after a calendar
                        // change, with the event's and the container's
                        // envelopes rewrapped into the fresh domain.
                        etag = za_proppatch_retry(
                            self,
                            access_token,
                            account_id,
                            document_id,
                            resource
                                .parent_id()
                                .ok_or(DavError::Code(StatusCode::NOT_FOUND))?,
                            &event_contexts,
                            archive,
                            retry_event.take().unwrap(),
                        )
                        .await?
                        .ok_or_else(|| DavError::from(error))?
                        .unwrap_or(etag);
                    }
                    Err(error) => return Err(error.caused_by(trc::location!()).into()),
                }
            }
        } else {
            items.fail_dependencies();
        }

        if headers.ret != Return::Minimal || !is_success {
            Ok(HttpResponse::new(StatusCode::MULTI_STATUS)
                .with_xml_body(
                    MultiStatus::new(vec![Response::new_propstat(uri, items.build())])
                        .with_namespace(Namespace::CalDav)
                        .to_string(),
                )
                .with_etag(etag))
        } else {
            Ok(HttpResponse::new(StatusCode::NO_CONTENT).with_etag(etag))
        }
    }

    fn apply_calendar_properties(
        &self,
        personal_id: u32,
        calendar: &mut Calendar,
        is_update: bool,
        properties: impl IntoIterator<Item = DavPropertyValue>,
        items: &mut PropStatBuilder,
    ) {
        for property in properties {
            match (&property.property, property.value) {
                (DavProperty::WebDav(WebDavProperty::DisplayName), DavValue::String(name)) => {
                    if name.len() <= self.core.groupware.live_property_size {
                        calendar.preferences_mut(personal_id).name = name;
                        items.insert_ok(property.property);
                    } else {
                        items.insert_error_with_description(
                            property.property,
                            StatusCode::INSUFFICIENT_STORAGE,
                            "Property value is too long",
                        );
                    }
                }
                (
                    DavProperty::CalDav(CalDavProperty::CalendarDescription),
                    DavValue::String(name),
                ) => {
                    if name.len() <= self.core.groupware.live_property_size {
                        calendar.preferences_mut(personal_id).description = Some(name);
                        items.insert_ok(property.property);
                    } else {
                        items.insert_error_with_description(
                            property.property,
                            StatusCode::INSUFFICIENT_STORAGE,
                            "Property value is too long",
                        );
                    }
                }
                (
                    DavProperty::CalDav(CalDavProperty::CalendarTimezone),
                    DavValue::ICalendar(ical),
                ) => {
                    if ical.size() > self.core.groupware.max_ical_size {
                        items.insert_error_with_description(
                            property.property,
                            StatusCode::INSUFFICIENT_STORAGE,
                            "Property value is too long",
                        );
                    } else if !ical.is_timezone() {
                        items.insert_precondition_failed_with_description(
                            property.property,
                            StatusCode::PRECONDITION_FAILED,
                            CalCondition::ValidCalendarData,
                            "Invalid calendar timezone",
                        );
                    } else {
                        calendar.preferences_mut(personal_id).time_zone = Timezone::Custom(ical);
                        items.insert_ok(property.property);
                    }
                }
                (DavProperty::CalDav(CalDavProperty::TimezoneId), DavValue::String(tz_id)) => {
                    if let Ok(tz) = Tz::from_str(&tz_id) {
                        calendar.preferences_mut(personal_id).time_zone =
                            Timezone::IANA(tz.as_id());
                        items.insert_ok(property.property);
                    } else {
                        items.insert_precondition_failed_with_description(
                            property.property,
                            StatusCode::PRECONDITION_FAILED,
                            CalCondition::ValidTimezone,
                            "Invalid timezone ID",
                        );
                    }
                }
                (DavProperty::WebDav(WebDavProperty::CreationDate), DavValue::Timestamp(dt)) => {
                    calendar.created = dt;
                    items.insert_ok(property.property);
                }
                (
                    DavProperty::WebDav(WebDavProperty::ResourceType),
                    DavValue::ResourceTypes(types),
                ) => {
                    if !types
                        .0
                        .iter()
                        .all(|rt| matches!(rt, ResourceType::Collection | ResourceType::Calendar))
                    {
                        items.insert_precondition_failed(
                            property.property,
                            StatusCode::FORBIDDEN,
                            BaseCondition::ValidResourceType,
                        );
                    } else {
                        items.insert_ok(property.property);
                    }
                }
                (
                    DavProperty::CalDav(CalDavProperty::SupportedCalendarComponentSet),
                    DavValue::Components(components),
                ) => {
                    if !is_update {
                        calendar.supported_components = Bitmap::<SupportedComponent>::from_iter(
                            components
                                .0
                                .into_iter()
                                .map(|v| SupportedComponent::from(v.0)),
                        )
                        .into_inner();
                        if calendar.supported_components != 0 {
                            items.insert_ok(property.property);
                        } else {
                            items.insert_precondition_failed_with_description(
                                property.property,
                                StatusCode::PRECONDITION_FAILED,
                                CalCondition::SupportedCalendarComponent,
                                "At least one supported component must be specified",
                            );
                        }
                    } else {
                        items.insert_precondition_failed_with_description(
                            property.property,
                            StatusCode::PRECONDITION_FAILED,
                            CalCondition::SupportedCalendarComponent,
                            "Property cannot be modified",
                        );
                    }
                }
                (_, DavValue::Null) => {
                    items.insert_ok(property.property);
                }
                _ => {
                    items.insert_error_with_description(
                        property.property,
                        StatusCode::CONFLICT,
                        "Property cannot be modified",
                    );
                }
            }
        }
    }

    fn apply_event_properties(
        &self,
        event: &mut CalendarEvent,
        properties: impl IntoIterator<Item = DavPropertyValue>,
        items: &mut PropStatBuilder,
    ) {
        for property in properties {
            match (&property.property, property.value) {
                (DavProperty::WebDav(WebDavProperty::DisplayName), DavValue::String(name)) => {
                    if name.len() <= self.core.groupware.live_property_size {
                        event.display_name = Some(name);
                        items.insert_ok(property.property);
                    } else {
                        items.insert_error_with_description(
                            property.property,
                            StatusCode::INSUFFICIENT_STORAGE,
                            "Property value is too long",
                        );
                    }
                }
                (DavProperty::WebDav(WebDavProperty::CreationDate), DavValue::Timestamp(dt)) => {
                    event.created = dt;
                    items.insert_ok(property.property);
                }
                (_, DavValue::Null) => {
                    items.insert_ok(property.property);
                }
                _ => {
                    items.insert_error_with_description(
                        property.property,
                        StatusCode::CONFLICT,
                        "Property cannot be modified",
                    );
                }
            }
        }
    }
}

fn remove_event_properties(
    event: &mut CalendarEvent,
    properties: impl IntoIterator<Item = DavProperty>,
    items: &mut PropStatBuilder,
) {
    for property in properties {
        match &property {
            DavProperty::WebDav(WebDavProperty::DisplayName) => {
                event.display_name = None;
                items.insert_ok(property);
            }
            _ => {
                items.insert_error_with_description(
                    property,
                    StatusCode::CONFLICT,
                    "Property cannot be deleted",
                );
            }
        }
    }
}

fn remove_calendar_properties(
    personal_id: u32,
    calendar: &mut Calendar,
    properties: impl IntoIterator<Item = DavProperty>,
    items: &mut PropStatBuilder,
) {
    for property in properties {
        match &property {
            DavProperty::CalDav(CalDavProperty::CalendarDescription) => {
                calendar.preferences_mut(personal_id).description = None;
                items.insert_ok(property);
            }
            DavProperty::CalDav(CalDavProperty::CalendarTimezone)
            | DavProperty::CalDav(CalDavProperty::TimezoneId) => {
                calendar.preferences_mut(personal_id).time_zone = Timezone::Default;
                items.insert_ok(property);
            }
            _ => {
                items.insert_error_with_description(
                    property,
                    StatusCode::CONFLICT,
                    "Property cannot be deleted",
                );
            }
        }
    }
}

/// Ruling 29's bounded retry of a key account's event PROPPATCH after its
/// assertion lost to a calendar change (a grant, a rotation or a
/// conversion): the same edit, its event and container envelopes rewrapped
/// into the fresh domain. `None` when the calendar did not change, so the
/// caller returns the original error; `Some(etag)` after the commit.
#[allow(clippy::too_many_arguments)]
async fn za_proppatch_retry(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    document_id: u32,
    calendar_id: u32,
    before: &[ZaCalendarContext],
    archive: &Archive<ArchiveBytes>,
    (za_event, dead_write): (Option<ZaEvent>, Option<MetadataWrite>),
) -> crate::Result<Option<Option<String>>> {
    let Some(before_domain) = before.first().map(|context| &context.domain) else {
        return Ok(None);
    };
    let event = archive
        .to_unarchived::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let after = za_write_contexts(
        server,
        access_token,
        account_id,
        &archive
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?,
        calendar_id,
        Acl::ModifyItems,
    )
    .await?;
    if !calendars_changed(before, &after) {
        return Ok(None);
    }
    let Some(after_domain) = after.first().map(|context| &context.domain) else {
        return Ok(None);
    };
    let mut batch = BatchBuilder::new();
    for context in &after {
        context.assert_in(&mut batch);
    }
    let uid = event.inner.uid.to_string();
    let mut kinds = None;
    if let Some(mut write) = dead_write {
        if let Some(next) = &write.next {
            write.next = Some(
                rewrap_container(
                    next.as_bytes(),
                    before_domain,
                    after_domain,
                    ZaContainerOf::Event(&uid),
                )
                .map_err(|err| seal_error(err, account_id, document_id))?,
            );
        }
        kinds = Some(
            write
                .next
                .as_ref()
                .map_or(MetadataKinds::NONE, |next| next.kinds()),
        );
        write
            .build(document_id.into(), &mut batch)
            .caused_by(trc::location!())?;
    }
    let etag = match za_event {
        Some(mut za_event) => {
            rewrap_event(&mut za_event, before_domain, after_domain)
                .map_err(|err| seal_error(err, account_id, document_id))?;
            let content = server
                .store()
                .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                    account_id,
                    Collection::CalendarEvent,
                    document_id,
                    CalendarEventField::Content,
                ))
                .await
                .caused_by(trc::location!())?
                .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
            let (meta, content_) = za_event.into_parts();
            Some(
                meta.update_full(
                    content_,
                    access_token.changed_by(),
                    event,
                    content
                        .unarchive::<CalendarEventContent>()
                        .caused_by(trc::location!())?,
                    account_id,
                    document_id,
                    None,
                    &mut batch,
                )
                .caused_by(trc::location!())?,
            )
        }
        None => {
            if let Some(kinds) = kinds {
                CalendarEvent::rewrite_presence(&event, kinds, account_id, document_id, &mut batch)
                    .caused_by(trc::location!())?;
            }
            None
        }
    };
    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;
    Ok(Some(etag))
}
