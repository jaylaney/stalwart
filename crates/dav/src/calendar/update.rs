/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    CalendarComponentSupport, CalendarEventView, ZaRoles, assert_event_privacy_access,
    assert_event_privacy_allowed, assert_is_unique_uid, serves_ical, user_data_error,
    za_commit_create, za_lost_create_fence, za_refresh_event, za_roles, za_take_uid_lease,
    za_write_contexts,
};
use crate::{
    DavError, DavErrorCondition, DavMethod,
    calendar::ItipPrecondition,
    common::{
        lock::{LockRequestHandler, ResourceState},
        uri::DavUriResource,
    },
    file::DavFileResource,
};
use calcard::{
    Entry, Parser,
    common::timezone::Tz,
    icalendar::{ICalendar, ICalendarComponentType},
};
use common::{ArchivedDavName, DavName, Server, auth::AccessToken};
use compact_str::ToCompactString;
use dav_proto::{
    RequestHeaders, Return,
    schema::{property::Rfc1123DateTime, response::CalCondition},
};
// SPDX-SnippetBegin
// SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
// SPDX-License-Identifier: LicenseRef-SEL
#[cfg(feature = "enterprise")]
use groupware::SizeWriter;
// SPDX-SnippetEnd
use groupware::{
    cache::GroupwareCache,
    calendar::{
        CalendarEvent, CalendarEventContent, CalendarEventData, SupportedComponent,
        alerts::{DefaultAlerts, DefaultAlertsResolver, DefaultAlertsView, ICalendarDefaultAlerts},
        delegation::{
            ZaDelegationResolution, ZaSelfOperation, za_combine_delegated, za_finalize,
            za_resolve_delegation, za_self_create, za_self_update,
        },
        fanout::{ZaFanOut, ZaFanOutKind, ZaFanOutMessage, ZaSource, za_set_schedule_status},
        identity::ParticipantIdentityAddresses,
        itip::ItipSendStatus,
        notification::{NotificationAudience, may_have_viewers},
        privacy::EventViewer,
        rights::{EventAcl, EventChanges},
        schedule::{AlarmTasks, EventAlarmScheduler, EventAlarmUsers},
        seal::{
            ZaEvent, event_pairs_in_domain, event_view_in_domain, seal_error, seal_event_in_domain,
        },
        sequence_floor::{za_sequence_floor, za_set_sequences},
        storage::{DirectChange, DirectChangeNotification, NotificationQuota},
        user::{ICalendarUserData, UpdatedPolicy, UserDataSplit, UserDataUpdate, UserDataView},
    },
    scheduling::{
        ItipError, ItipMessages, event_create::itip_create, event_update::itip_update,
        itip::itip_set_unreachable_status, recipient::RecipientPolicy,
    },
};
use http_proto::HttpResponse;
use hyper::StatusCode;
use registry::schema::enums::StorageQuota;
use store::write::{BatchBuilder, now};
use trc::AddContext;
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection},
    field::CalendarEventField,
};
use utils::map::bitmap::Bitmap;

pub(crate) trait CalendarUpdateRequestHandler: Sync + Send {
    fn handle_calendar_update_request(
        &self,
        access_token: &AccessToken,
        headers: &RequestHeaders<'_>,
        bytes: &[u8],
        is_patch: bool,
    ) -> impl Future<Output = crate::Result<HttpResponse>> + Send;
}

impl CalendarUpdateRequestHandler for Server {
    async fn handle_calendar_update_request(
        &self,
        access_token: &AccessToken,
        headers: &RequestHeaders<'_>,
        bytes: &[u8],
        _is_patch: bool,
    ) -> crate::Result<HttpResponse> {
        // Validate URI
        let resource = self
            .validate_uri(access_token, headers.uri)
            .await?
            .into_owned_uri()?;
        let account_id = resource.account_id;
        let za_keys = super::za_writer_keys(self, access_token, account_id).await?;
        let resources = self
            .fetch_groupware_resources(
                access_token.account_id(),
                account_id,
                SyncCollection::Calendar,
            )
            .await
            .caused_by(trc::location!())?;
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::put_pause_point(account_id).await;
        let resource_name = resource
            .resource
            .ok_or(DavError::Code(StatusCode::CONFLICT))?;

        if bytes.len() > self.core.groupware.max_ical_size {
            return Err(DavError::Condition(DavErrorCondition::new(
                StatusCode::PRECONDITION_FAILED,
                CalCondition::MaxResourceSize(self.core.groupware.max_ical_size as u32),
            )));
        }
        let ical_raw = std::str::from_utf8(bytes).map_err(|_| {
            DavError::Condition(
                DavErrorCondition::new(
                    StatusCode::PRECONDITION_FAILED,
                    CalCondition::SupportedCalendarData,
                )
                .with_details("Invalid UTF-8 in iCalendar data"),
            )
        })?;

        let mut ical = match Parser::new(ical_raw).entry() {
            Entry::ICalendar(ical) => ical,
            _ => {
                return Err(DavError::Condition(
                    DavErrorCondition::new(
                        StatusCode::PRECONDITION_FAILED,
                        CalCondition::SupportedCalendarData,
                    )
                    .with_details("Failed to parse iCalendar data"),
                ));
            }
        };
        let attachments_size = ical.embedded_size();
        if self.core.groupware.max_attachments_size != 0
            && attachments_size > self.core.groupware.max_attachments_size
        {
            return Err(DavError::Condition(
                DavErrorCondition::new(
                    StatusCode::PRECONDITION_FAILED,
                    CalCondition::MaxResourceSize(self.core.groupware.max_attachments_size as u32),
                )
                .with_details(format!(
                    "The size of the embedded attachments ({attachments_size} bytes) exceeds the maximum of {} bytes.",
                    self.core.groupware.max_attachments_size
                )),
            ));
        }
        let personal_id = access_token.personal_id(account_id, Collection::Calendar);

        let account_info = self
            .account_info(account_id)
            .await
            .caused_by(trc::location!())?;
        // Release 4: a key account's write keeps its writer, owner and sender
        // roles apart (delegated scheduling carries SENT-BY).
        let za_roles = match &za_keys {
            Some(_) => Some(za_roles(self, access_token, account_id).await?),
            None => None,
        };

        if let Some(resource) = resources.by_path(resource_name) {
            if resource.is_container() {
                return Err(DavError::Code(StatusCode::METHOD_NOT_ALLOWED));
            }

            // Validate ACL
            let parent_id = resource.parent_id().unwrap();
            let document_id = resource.document_id();
            let is_owner = access_token.is_member(account_id);
            if !is_owner
                && !resources.has_access_to_container(
                    access_token,
                    parent_id,
                    Bitmap::from_iter([
                        Acl::ModifyItems,
                        Acl::ModifyItemsOwn,
                        Acl::ModifyPrivateProperties,
                        Acl::ModifyRSVP,
                    ]),
                )
            {
                return Err(DavError::Code(StatusCode::FORBIDDEN));
            }
            let viewer = EventViewer::new(is_owner);
            assert_event_privacy_access(resource.resource.event_flags(), viewer)?;
            assert_event_privacy_allowed(&ical, viewer)?;

            // Update
            let (event_, content_) = self
                .archive_and_field(
                    account_id,
                    Collection::CalendarEvent,
                    document_id,
                    CalendarEventField::Content.field(),
                )
                .await
                .caused_by(trc::location!())?;
            let event_ = event_.ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
            let event = event_
                .to_unarchived::<CalendarEvent>()
                .caused_by(trc::location!())?;
            let content_ = content_.ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
            let stored_content = content_
                .to_unarchived::<CalendarEventContent>()
                .caused_by(trc::location!())?;
            // Spec 8.1: the stored archives stay the update's current values
            // (and the index builder's); a key account's unsealed view is used
            // for comparison, editing and response bodies only.
            let mut contexts = if za_keys.is_some() {
                za_write_contexts(
                    self,
                    access_token,
                    account_id,
                    &event
                        .deserialize::<CalendarEvent>()
                        .caused_by(trc::location!())?,
                    parent_id,
                    Acl::ModifyItems,
                )
                .await?
            } else {
                Vec::new()
            };
            let za_view = match contexts.first() {
                Some(context) => Some(
                    event_view_in_domain(
                        &event
                            .deserialize::<CalendarEvent>()
                            .caused_by(trc::location!())?,
                        &content_,
                        &context.domain,
                    )
                    .map_err(|err| seal_error(err, account_id, document_id))?,
                ),
                None => None,
            };
            let content = match &za_view {
                Some(view) => view
                    .content
                    .to_unarchived::<CalendarEventContent>()
                    .caused_by(trc::location!())?,
                None => stored_content.clone(),
            };

            // Validate headers
            match self
                .validate_headers(
                    access_token,
                    headers,
                    vec![ResourceState {
                        account_id,
                        collection: Collection::CalendarEvent,
                        document_id: Some(document_id),
                        etag: format!("\"{}\"", event.inner.etag.to_native()).into(),
                        path: resource_name,
                        ..Default::default()
                    }],
                    Default::default(),
                    DavMethod::PUT,
                )
                .await
            {
                Ok(_) => {}
                Err(DavError::Code(StatusCode::PRECONDITION_FAILED))
                    if headers.ret == Return::Representation =>
                {
                    let body = match self
                        .calendar_event_view(
                            access_token,
                            account_id,
                            event.inner,
                            content.inner,
                            &mut DefaultAlertsResolver::default(),
                        )
                        .await?
                    {
                        Some((view, _)) => view.data.event.to_string(),
                        None => content.inner.data.event.to_string(),
                    };
                    return Ok(HttpResponse::new(StatusCode::PRECONDITION_FAILED)
                        .with_content_type("text/calendar; charset=utf-8")
                        .with_etag(format!("\"{}\"", event.inner.etag.to_native()))
                        .with_last_modified(
                            Rfc1123DateTime::new(i64::from(event.inner.modified)).to_string(),
                        )
                        .with_header("Preference-Applied", "return=representation")
                        .with_binary_body(body));
                }
                Err(e) => return Err(e),
            }

            if ical == content.inner.data.event {
                // No changes, return existing event
                return Ok(HttpResponse::new(StatusCode::NO_CONTENT));
            }

            // Validate iCal
            let (uid, component) = validate_ical(&ical)?;
            if event.inner.uid.as_str() != uid {
                return Err(DavError::Condition(DavErrorCondition::new(
                    StatusCode::FORBIDDEN,
                    CalCondition::NoUidConflict(resources.format_resource(resource).into()),
                )));
            }
            self.supported_components(account_id, parent_id)
                .await?
                .assert_supports(component)?;

            // Validate schedule tag
            if headers.if_schedule_tag.is_some()
                && event.inner.schedule_tag.as_ref().map(|t| t.to_native())
                    != headers.if_schedule_tag
            {
                return Err(DavError::Code(StatusCode::PRECONDITION_FAILED));
            }

            // Obtain previous alarm
            let now = now() as i64;
            let calendar_ids = event
                .inner
                .names
                .iter()
                .map(ArchivedDavName::parent_id)
                .collect::<Vec<_>>();
            let mut default_alerts_resolver =
                DefaultAlertsResolver::with_resources(account_id, resources.clone());
            let prev_email_alarms = self
                .next_event_alarms(
                    account_id,
                    &EventAlarmUsers::new(account_id, content.inner)?
                        .with_event_flags(event.inner.flags.to_native()),
                    &content.inner.data,
                    &calendar_ids,
                    now,
                    &mut default_alerts_resolver,
                )
                .await
                .caused_by(trc::location!())?;

            // Build event
            let mut new_event = event
                .deserialize::<CalendarEvent>()
                .caused_by(trc::location!())?;
            let mut new_content = content
                .deserialize::<CalendarEventContent>()
                .caused_by(trc::location!())?;

            let writer_default_alerts = default_alerts_resolver
                .resolve_for_content(
                    self,
                    account_id,
                    personal_id,
                    &new_content,
                    event.inner.names.iter().map(ArchivedDavName::parent_id),
                )
                .await
                .caused_by(trc::location!())?;
            let has_default_alerts = writer_default_alerts.is_enabled();
            let submitted_ical = ical.clone();
            ical.strip_default_alerts(&writer_default_alerts);
            let sharee_alerts_view = |content: &CalendarEventContent| {
                let mut view = content.data.event.clone();
                view.apply_user_data(content.preferences(personal_id), UserDataView::AlertsOnly);
                view
            };
            let mut sharee_view = None;
            if has_default_alerts {
                if is_owner {
                    ical.restore_hidden_alerts(&new_content.data.event, &writer_default_alerts);
                } else {
                    ical.restore_hidden_alerts(
                        sharee_view.insert(sharee_alerts_view(&new_content)),
                        &writer_default_alerts,
                    );
                }
            }
            let changes_shared_user_data =
                !is_owner && !ical.has_same_user_data(&new_content.data.event);
            if is_owner || changes_shared_user_data {
                ical.validate_user_data_changes(&new_content.data.event)
                    .map_err(user_data_error)?;
            }

            if !is_owner {
                let identities = self
                    .account_identity_addresses(access_token.account_id())
                    .await
                    .caused_by(trc::location!())?;
                let ownership = identities.event_ownership(&new_content.data.event);
                let acl = EventAcl::for_calendars(
                    &resources,
                    access_token,
                    event.inner.names.iter().map(ArchivedDavName::parent_id),
                );
                if !acl.may_write(ownership) {
                    if changes_shared_user_data {
                        return Err(DavError::Code(StatusCode::FORBIDDEN));
                    }
                    let event_flags = event.inner.flags.to_native();
                    let sharee_view =
                        sharee_view.unwrap_or_else(|| sharee_alerts_view(&new_content));
                    let mut changes = EventChanges::between(
                        &sharee_view.into_jscalendar(),
                        &ical.clone().into_jscalendar(),
                        event_flags,
                        event_flags,
                        &identities,
                    );
                    changes.classify_recurrence_sets(&new_content.data.event, &ical);
                    changes
                        .assert_allowed(&acl, ownership, event_flags)
                        .map_err(|_| DavError::Code(StatusCode::FORBIDDEN))?;
                }
            }

            // A key account's content never holds per-user data (upgrade
            // design, "Per-user event data"): a sharee's write is shared data.
            let is_user_data_only = if is_owner || za_keys.is_some() {
                false
            } else {
                let stored_preferences = new_content
                    .preferences
                    .iter()
                    .find(|p| p.account_id == personal_id);
                let (preferences, split) = ical
                    .split_user_data(
                        &new_content.data.event,
                        UserDataUpdate {
                            account_id: personal_id,
                            view: UserDataView::AlertsOnly,
                            previous: stored_preferences,
                            current: stored_preferences,
                            updated: UpdatedPolicy::Server(now),
                        },
                    )
                    .map_err(user_data_error)?;
                new_content.set_preferences(preferences);
                split == UserDataSplit::UserDataOnly
            };
            let old_ical = if is_user_data_only {
                None
            } else {
                let old_ical = std::mem::take(&mut new_content.data.event);
                let owner_default_alerts = default_alerts_resolver
                    .resolve_for_content(
                        self,
                        account_id,
                        account_id,
                        &new_content,
                        event.inner.names.iter().map(ArchivedDavName::parent_id),
                    )
                    .await
                    .caused_by(trc::location!())?;
                // Key pinning spec 5.1: the SEQUENCE floor, on every PUT of an
                // existing event of a key account, before the scheduling step.
                if let Some(roles) = &za_roles {
                    za_sequence_floor(&mut ical, &old_ical, roles.sender.addresses());
                }
                new_content.data = CalendarEventData::new_for(
                    ical,
                    Tz::Floating,
                    self.core.groupware.max_ical_instances,
                    &owner_default_alerts,
                    za_keys.is_some(),
                );
                Some(old_ical)
            };
            let next_email_alarms = self
                .next_event_alarms(
                    account_id,
                    &EventAlarmUsers::new(account_id, &new_content)?
                        .with_event_flags(new_event.flags),
                    &new_content.data,
                    &calendar_ids,
                    now,
                    &mut default_alerts_resolver,
                )
                .await
                .caused_by(trc::location!())?;

            // Scheduling
            let mut itip_messages = None;
            let mut za_messages = None;
            let mut za_resolution = None;
            let itip_status = if is_user_data_only {
                ItipSendStatus::NotRequested
            } else {
                ItipSendStatus::resolve(
                    self,
                    access_token,
                    za_roles
                        .as_ref()
                        .map_or(&account_info, |roles| &roles.sender),
                    new_content.data.event_range_end(),
                    za_keys.is_some(),
                )
            };
            if itip_status.is_send_sealed()
                && let Some(roles) = &za_roles
            {
                // Invitations spec 5.1: upstream's builders on the plaintext
                // tree; the messages go to the fan-out.
                let is_update = new_event.schedule_tag.is_some() && old_ical.is_some();
                za_resolution = za_resolve_delegation(
                    self,
                    &roles.sender,
                    &roles.writer,
                    &roles.owner,
                    if is_update {
                        ZaSelfOperation::Update
                    } else {
                        ZaSelfOperation::Create
                    },
                    old_ical.as_ref(),
                    Some(&new_content.data.event),
                )
                .await
                .caused_by(trc::location!())?;
                za_messages = za_schedule(
                    self,
                    roles,
                    za_resolution.as_ref(),
                    old_ical.as_ref().filter(|_| is_update),
                    &mut new_content.data.event,
                    RecipientPolicy::new(&self.core.groupware, new_event.flags),
                    &mut new_event.schedule_tag,
                    account_id,
                    Some(document_id),
                )?;
                itip_set_unreachable_status(&mut new_content.data.event, roles.sender.addresses());
            } else if itip_status.is_send() {
                let policy = RecipientPolicy::new(&self.core.groupware, new_event.flags);
                let result = if let Some(old_ical) = old_ical
                    .as_ref()
                    .filter(|_| new_event.schedule_tag.is_some())
                {
                    itip_update(
                        &mut new_content.data.event,
                        old_ical,
                        account_info.addresses(),
                        policy,
                    )
                } else {
                    itip_create(
                        &mut new_content.data.event,
                        account_info.addresses(),
                        policy,
                    )
                };

                match result {
                    Ok(messages) => {
                        let mut is_organizer = false;
                        if messages
                            .iter()
                            .map(|r| {
                                is_organizer = r.from_organizer;
                                r.to.len()
                            })
                            .sum::<usize>()
                            < self.core.groupware.itip_outbound_max_recipients
                        {
                            // Only update schedule tag if the user is the organizer
                            if is_organizer {
                                if let Some(schedule_tag) = &mut new_event.schedule_tag {
                                    *schedule_tag += 1;
                                } else {
                                    new_event.schedule_tag = Some(1);
                                }
                            }

                            itip_messages = Some(ItipMessages::new(messages));
                        } else {
                            return Err(DavError::Condition(DavErrorCondition::new(
                                StatusCode::PRECONDITION_FAILED,
                                CalCondition::MaxAttendeesPerInstance,
                            )));
                        }
                    }
                    Err(err) => {
                        if let Some(failed_precondition) = err.failed_precondition() {
                            return Err(DavError::Condition(
                                DavErrorCondition::new(
                                    StatusCode::PRECONDITION_FAILED,
                                    failed_precondition,
                                )
                                .with_details(err.to_string()),
                            ));
                        }

                        trc::event!(
                            Calendar(trc::CalendarEvent::ItipMessageError),
                            AccountId = account_id,
                            DocumentId = document_id,
                            Reason = err.to_compact_string(),
                        );

                        // Event changed, but there are no iTIP messages to send
                        if let Some(schedule_tag) = &mut new_event.schedule_tag {
                            *schedule_tag += 1;
                        }
                    }
                }

                itip_set_unreachable_status(&mut new_content.data.event, account_info.addresses());
            } else if let Some(reason) = itip_status.reason() {
                trc::event!(
                    Calendar(trc::CalendarEvent::ItipMessageError),
                    AccountId = account_id,
                    DocumentId = document_id,
                    Reason = reason,
                );
            }

            let account = self.account(account_id).await?;
            // SPDX-SnippetBegin
            // SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
            // SPDX-License-Identifier: LicenseRef-SEL
            // Validate quota
            #[cfg(feature = "enterprise")]
            let extra_bytes = (SizeWriter::ical(&new_content.data.event) as u64)
                .saturating_sub(u32::from(event.inner.size) as u64);
            #[cfg(feature = "enterprise")]
            if extra_bytes > 0 {
                self.has_available_tenant_quota(&account, extra_bytes)
                    .await?;
            }
            // SPDX-SnippetEnd

            // Prepare write batch
            let mut batch = BatchBuilder::new();
            batch.with_account_id(account_id);
            self.limit_disk_quota(&mut batch, &account);
            let notify_event_flags = new_event.flags | event.inner.flags.to_native();
            if let Some(old_ical) = old_ical.filter(|_| {
                may_have_viewers(
                    access_token,
                    account_id,
                    &resources,
                    &calendar_ids,
                    NotificationAudience::of_event(notify_event_flags),
                )
            }) {
                self.notify_direct_change(
                    access_token,
                    account_id,
                    DirectChange::Updated {
                        event_id: document_id,
                        previous: old_ical,
                        current: new_content.data.event.clone(),
                        calendar_ids: calendar_ids.clone(),
                        event_flags: notify_event_flags,
                    },
                    Some(&resources),
                    &mut NotificationQuota::default(),
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
            }
            let serves_submitted_ical = if is_owner && !has_default_alerts {
                serves_ical(&new_content.data.event, &submitted_ical)
            } else {
                let view_default_alerts = default_alerts_resolver
                    .resolve_for_ical_view(
                        self,
                        account_id,
                        personal_id,
                        &new_content,
                        calendar_ids.iter().copied(),
                    )
                    .await
                    .caused_by(trc::location!())?;
                let mut view = new_content.data.event.clone();
                if !is_owner {
                    view.apply_user_data(
                        new_content.preferences(personal_id),
                        UserDataView::AlertsOnly,
                    );
                }
                view.apply_default_alerts(&view_default_alerts, DefaultAlertsView::ICalendar);
                serves_ical(&view, &submitted_ical)
            };

            let schedule_tag = new_event.schedule_tag;
            if let (Some(keys), Some(roles), Some(view)) = (&za_keys, &za_roles, &za_view) {
                // Invitations spec 3.3: every rewrite carries the stored pairs.
                let mut pairs = event_pairs_in_domain(
                    &event
                        .deserialize::<CalendarEvent>()
                        .caused_by(trc::location!())?,
                    &content_,
                    &contexts[0].domain,
                )
                .map_err(|err| seal_error(err, account_id, document_id))?;
                // Spec 5.3-5.4: after every check of the request and
                // immediately before the sealed commit, the items go out; the
                // fan-out decides the pairs the bundle keeps.
                if let Some(messages) = za_messages {
                    // The stored UID, which the new one was validated equal to.
                    let fan_out = self
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
                            None,
                            za_resolution.as_ref(),
                        )
                        .await
                        .caused_by(trc::location!())?;
                    za_set_schedule_status(&mut new_content.data.event, &fan_out.statuses);
                    za_set_sequences(&mut new_content.data.event, &fan_out.sequences);
                    pairs = fan_out.pairs;
                }
                // Ruling 15: the name stays in the sealed bundle.
                new_event.display_name = view.display_name.clone();
                let mut sealed = ZaEvent::new(new_event, new_content);
                seal_event_in_domain(&mut sealed, &contexts[0].domain, pairs.as_ref())
                    .map_err(|err| seal_error(err, account_id, document_id))?;
                // The same event and alarm intent is rebuilt once, without
                // scheduling, after a calendar change.
                let timestamp = now;
                for attempt in 0..=1 {
                    let mut batch = BatchBuilder::new();
                    batch.with_account_id(account_id);
                    self.limit_disk_quota(&mut batch, &account);
                    for context in &contexts {
                        context.assert_in(&mut batch);
                    }
                    let (meta, content) = sealed.clone().into_parts();
                    let etag = meta
                        .update_full_at(
                            content,
                            access_token.changed_by(),
                            event.clone(),
                            stored_content.inner,
                            account_id,
                            document_id,
                            None,
                            timestamp,
                            &mut batch,
                        )
                        .caused_by(trc::location!())?;
                    self.replace_event_alarms(
                        account_id,
                        document_id,
                        prev_email_alarms.clone(),
                        next_email_alarms.clone(),
                        &mut AlarmTasks::None,
                        &mut batch,
                    )
                    .await
                    .caused_by(trc::location!())?;
                    #[cfg(feature = "test_mode")]
                    common::za::test_hooks::update_commit_pause_point(account_id).await;
                    match self.commit_batch(batch).await {
                        Ok(_) => {
                            self.notify_task_queue();
                            return Ok(HttpResponse::new(StatusCode::NO_CONTENT)
                                .with_etag_opt(serves_submitted_ical.then_some(etag))
                                .with_schedule_tag_opt(schedule_tag));
                        }
                        Err(error) if error.is_assertion_failure() && attempt == 0 => {
                            if let Some(fresh) = za_refresh_event(
                                self,
                                access_token,
                                account_id,
                                parent_id,
                                Acl::ModifyItems,
                                &contexts,
                                &mut sealed,
                            )
                            .await?
                            {
                                contexts = fresh;
                            } else {
                                return Err(error.caused_by(trc::location!()).into());
                            }
                        }
                        Err(error) => return Err(error.caused_by(trc::location!()).into()),
                    }
                }
                unreachable!("bounded event retry always returns");
            }
            let etag = new_event
                .update_full(
                    new_content,
                    access_token.changed_by(),
                    event,
                    stored_content.inner,
                    account_id,
                    document_id,
                    None,
                    &mut batch,
                )
                .caused_by(trc::location!())?;
            self.replace_event_alarms(
                account_id,
                document_id,
                prev_email_alarms,
                next_email_alarms,
                &mut AlarmTasks::None,
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
            if let Some(itip_messages) = itip_messages {
                itip_messages
                    .queue(&mut batch)
                    .caused_by(trc::location!())?;
            }
            #[cfg(feature = "test_mode")]
            common::za::test_hooks::update_commit_pause_point(account_id).await;
            self.commit_batch(batch).await.caused_by(trc::location!())?;

            Ok(HttpResponse::new(StatusCode::NO_CONTENT)
                .with_etag_opt(serves_submitted_ical.then_some(etag))
                .with_schedule_tag_opt(schedule_tag))
        } else if let Some((Some(parent), name)) = resources.map_parent(resource_name) {
            if !parent.is_container() {
                return Err(DavError::Code(StatusCode::METHOD_NOT_ALLOWED));
            }

            // Validate ACL
            let is_owner = access_token.is_member(account_id);
            if !is_owner
                && !resources.has_access_to_container(
                    access_token,
                    parent.document_id(),
                    Acl::AddItems,
                )
                && !(EventAcl::for_calendar(&resources, access_token, parent.document_id())
                    .may_manage_own_items()
                    && self
                        .account_identity_addresses(access_token.account_id())
                        .await
                        .caused_by(trc::location!())?
                        .event_ownership(&ical)
                        .may_write_own())
            {
                return Err(DavError::Code(StatusCode::FORBIDDEN));
            }
            assert_event_privacy_allowed(&ical, EventViewer::new(is_owner))?;
            ical.validate_user_data().map_err(user_data_error)?;

            // Validate headers
            self.validate_headers(
                access_token,
                headers,
                vec![ResourceState {
                    account_id,
                    collection: resource.collection,
                    document_id: Some(u32::MAX),
                    path: resource_name,
                    ..Default::default()
                }],
                Default::default(),
                DavMethod::PUT,
            )
            .await?;

            // Validate ical object
            let (uid, component) = validate_ical(&ical)?;
            // Invitations spec 5.1, 6.3: a key account's create holds the UID
            // creation lease from here through the insert, and checks current
            // archives instead of the snapshot.
            let mut za_lease = if za_keys.is_some() {
                Some((
                    za_take_uid_lease(
                        self,
                        &resources,
                        access_token,
                        account_id,
                        parent.document_id(),
                        uid,
                    )
                    .await?,
                    uid.to_string(),
                ))
            } else {
                assert_is_unique_uid(
                    &resources,
                    access_token,
                    account_id,
                    parent.document_id(),
                    Some(uid),
                )?;
                None
            };
            self.supported_components(account_id, parent.document_id())
                .await?
                .assert_supports(component)?;

            // Validate object quota
            let account = self.account(account_id).await?;
            self.assert_object_quota(&account, StorageQuota::MaxCalendarEvents, 1, || {
                resources.resources.count(false)
            })?;

            let submitted_ical = ical.clone();
            let mut preferences = Vec::new();
            // A key account's content never holds per-user data.
            if !is_owner && za_keys.is_none() {
                let extracted = ical
                    .extract_user_data(personal_id)
                    .map_err(user_data_error)?;
                ical.apply_user_data(None, UserDataView::Full);
                if !extracted.is_empty() {
                    preferences.push(extracted);
                }
            }

            // Build event
            let mut event = CalendarEvent {
                names: vec![DavName {
                    name: name.to_string(),
                    parent_id: parent.document_id(),
                }],
                ..Default::default()
            };
            let mut content = CalendarEventContent {
                data: CalendarEventData::new_for(
                    ical,
                    Tz::Floating,
                    self.core.groupware.max_ical_instances,
                    &DefaultAlerts::disabled(),
                    za_keys.is_some(),
                ),
                preferences,
            };
            let mut contexts = if za_keys.is_some() {
                za_write_contexts(
                    self,
                    access_token,
                    account_id,
                    &event,
                    parent.document_id(),
                    Acl::AddItems,
                )
                .await?
            } else {
                Vec::new()
            };
            let next_email_alarms = self
                .next_event_alarms(
                    account_id,
                    &EventAlarmUsers::new(account_id, &content)?,
                    &content.data,
                    &[parent.document_id()],
                    now() as i64,
                    &mut DefaultAlertsResolver::with_resources(account_id, resources.clone()),
                )
                .await
                .caused_by(trc::location!())?;

            // Scheduling
            let mut itip_messages = None;
            let mut za_messages = None;
            let mut za_resolution = None;
            let itip_status = ItipSendStatus::resolve(
                self,
                access_token,
                za_roles
                    .as_ref()
                    .map_or(&account_info, |roles| &roles.sender),
                content.data.event_range_end(),
                za_keys.is_some(),
            );
            if itip_status.is_send_sealed()
                && let Some(roles) = &za_roles
            {
                za_resolution = za_resolve_delegation(
                    self,
                    &roles.sender,
                    &roles.writer,
                    &roles.owner,
                    ZaSelfOperation::Create,
                    None,
                    Some(&content.data.event),
                )
                .await
                .caused_by(trc::location!())?;
                za_messages = za_schedule(
                    self,
                    roles,
                    za_resolution.as_ref(),
                    None,
                    &mut content.data.event,
                    RecipientPolicy::new(&self.core.groupware, event.flags),
                    &mut event.schedule_tag,
                    account_id,
                    None,
                )?;
                itip_set_unreachable_status(&mut content.data.event, roles.sender.addresses());
            } else if itip_status.is_send() {
                match itip_create(
                    &mut content.data.event,
                    account_info.addresses(),
                    RecipientPolicy::new(&self.core.groupware, event.flags),
                ) {
                    Ok(messages) => {
                        if messages.iter().map(|r| r.to.len()).sum::<usize>()
                            < self.core.groupware.itip_outbound_max_recipients
                        {
                            event.schedule_tag = Some(1);
                            itip_messages = Some(ItipMessages::new(messages));
                        } else {
                            return Err(DavError::Condition(DavErrorCondition::new(
                                StatusCode::PRECONDITION_FAILED,
                                CalCondition::MaxAttendeesPerInstance,
                            )));
                        }
                    }
                    Err(err) => {
                        if let Some(failed_precondition) = err.failed_precondition() {
                            return Err(DavError::Condition(
                                DavErrorCondition::new(
                                    StatusCode::PRECONDITION_FAILED,
                                    failed_precondition,
                                )
                                .with_details(err.to_string()),
                            ));
                        }

                        trc::event!(
                            Calendar(trc::CalendarEvent::ItipMessageError),
                            AccountId = account_id,
                            Reason = err.to_compact_string(),
                        );
                    }
                }

                itip_set_unreachable_status(&mut content.data.event, account_info.addresses());
            } else if let Some(reason) = itip_status.reason() {
                trc::event!(
                    Calendar(trc::CalendarEvent::ItipMessageError),
                    AccountId = account_id,
                    Reason = reason,
                );
            }

            // SPDX-SnippetBegin
            // SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
            // SPDX-License-Identifier: LicenseRef-SEL
            // Validate quota
            #[cfg(feature = "enterprise")]
            if !bytes.is_empty() {
                self.has_available_tenant_quota(&account, bytes.len() as u64)
                    .await?;
            }
            // SPDX-SnippetEnd

            if let (Some(keys), Some(roles)) = (&za_keys, &za_roles) {
                return za_create_event(
                    self,
                    access_token,
                    &resources,
                    &account,
                    keys,
                    roles,
                    ZaCreate {
                        event,
                        content,
                        submitted_ical,
                        is_owner,
                        personal_id,
                        next_email_alarms,
                        messages: za_messages,
                        resolution: za_resolution,
                        lease: za_lease.take(),
                        contexts: std::mem::take(&mut contexts),
                        calendar_id: parent.document_id(),
                    },
                )
                .await;
            }

            // Prepare write batch
            let mut batch = BatchBuilder::new();
            batch.with_account_id(account_id);
            self.limit_disk_quota(&mut batch, &account);
            let document_id = batch.reserve_document_id(account_id, Collection::CalendarEvent);
            let notify_calendar_ids = event.calendar_ids().collect::<Vec<_>>();
            if may_have_viewers(
                access_token,
                account_id,
                &resources,
                &notify_calendar_ids,
                NotificationAudience::of_event(event.flags),
            ) {
                self.notify_direct_change(
                    access_token,
                    account_id,
                    DirectChange::Created {
                        event_id: document_id.into(),
                        current: content.data.event.clone(),
                        calendar_ids: notify_calendar_ids,
                        event_flags: event.flags,
                    },
                    Some(&resources),
                    &mut NotificationQuota::default(),
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
            }
            let serves_submitted_ical = if is_owner {
                serves_ical(&content.data.event, &submitted_ical)
            } else {
                let mut view = content.data.event.clone();
                view.apply_user_data(content.preferences(personal_id), UserDataView::AlertsOnly);
                serves_ical(&view, &submitted_ical)
            };

            let schedule_tag = event.schedule_tag;
            let etag = event
                .insert(
                    content,
                    access_token.changed_by(),
                    account_id,
                    document_id,
                    None,
                    next_email_alarms,
                    &mut batch,
                )
                .caused_by(trc::location!())?;
            if let Some(itip_messages) = itip_messages {
                itip_messages
                    .queue(&mut batch)
                    .caused_by(trc::location!())?;
            }
            self.commit_batch(batch).await.caused_by(trc::location!())?;

            Ok(HttpResponse::new(StatusCode::CREATED)
                .with_etag_opt(serves_submitted_ical.then_some(etag))
                .with_schedule_tag_opt(schedule_tag))
        } else {
            Err(DavError::Code(StatusCode::CONFLICT))?
        }
    }
}

fn validate_ical(ical: &ICalendar) -> crate::Result<(&str, SupportedComponent)> {
    let mut uid = None;
    let mut has_mixed_uids = false;
    let mut object_type = None;
    let mut has_mixed_types = false;
    for comp in &ical.components {
        let component = match comp.component_type {
            ICalendarComponentType::VEvent => SupportedComponent::VEvent,
            ICalendarComponentType::VTodo => SupportedComponent::VTodo,
            ICalendarComponentType::VJournal => SupportedComponent::VJournal,
            ICalendarComponentType::VFreebusy => SupportedComponent::VFreebusy,
            ICalendarComponentType::VAvailability => SupportedComponent::VAvailability,
            _ => {
                continue;
            }
        };
        has_mixed_types |= object_type
            .replace(component)
            .is_some_and(|previous| previous != component);

        if let Some(comp_uid) = comp.uid() {
            has_mixed_uids |= uid
                .replace(comp_uid)
                .is_some_and(|previous| previous != comp_uid);
        }
    }

    match (uid.filter(|_| !has_mixed_uids), object_type) {
        (Some(uid), Some(object_type)) if !has_mixed_types => Ok((uid, object_type)),
        _ => Err(DavError::Condition(
            DavErrorCondition::new(
                StatusCode::PRECONDITION_FAILED,
                CalCondition::ValidCalendarObjectResource,
            )
            .with_details("iCalendar must contain exactly one UID and same component types"),
        )),
    }
}

/// Invitations spec 5.1 (release 2) and delegated scheduling (release 4): a
/// key account's scheduling (`SendSealed`). Upstream's builders run on the
/// plaintext tree, with the writer's self-delegated run beside them; the
/// messages go to the fan-out, never to the task queue. `None` when nothing
/// was built. `old` is `Some` for an update.
#[allow(clippy::too_many_arguments)]
fn za_schedule(
    server: &Server,
    roles: &ZaRoles,
    resolution: Option<&ZaDelegationResolution>,
    old: Option<&ICalendar>,
    ical: &mut ICalendar,
    policy: RecipientPolicy,
    schedule_tag: &mut Option<u32>,
    account_id: u32,
    document_id: Option<u32>,
) -> crate::Result<Option<Vec<ZaFanOutMessage>>> {
    // The second run sees the pre-finalize trees.
    let second = resolution.and_then(|resolution| {
        match old {
            Some(old) => za_self_update(old, ical, &roles.writer, &roles.owner, resolution),
            None => za_self_create(ical, &roles.writer, &roles.owner, resolution),
        }
        .ok()
        .flatten()
    });
    let result = match old {
        Some(old) => itip_update(ical, old, roles.sender.addresses(), policy),
        None => itip_create(ical, roles.sender.addresses(), policy),
    };
    let first_built = result.is_ok();
    let first_nothing = matches!(&result, Err(ItipError::NothingToSend));
    let first = match result {
        Ok(messages) => Some(messages),
        Err(err) => {
            if let Some(failed_precondition) = err.failed_precondition() {
                return Err(DavError::Condition(
                    DavErrorCondition::new(StatusCode::PRECONDITION_FAILED, failed_precondition)
                        .with_details(err.to_string()),
                ));
            }
            trc::event!(
                Calendar(trc::CalendarEvent::ItipMessageError),
                AccountId = account_id,
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
        return Err(DavError::Condition(DavErrorCondition::new(
            StatusCode::PRECONDITION_FAILED,
            CalCondition::MaxAttendeesPerInstance,
        )));
    }
    Ok(Some(messages))
}

/// What a key account's PUT create carries from the handler's checks to
/// its sealed commit.
struct ZaCreate {
    event: CalendarEvent,
    content: CalendarEventContent,
    submitted_ical: ICalendar,
    is_owner: bool,
    personal_id: u32,
    next_email_alarms: groupware::calendar::schedule::EventAlarms,
    messages: Option<Vec<ZaFanOutMessage>>,
    resolution: Option<ZaDelegationResolution>,
    lease: Option<(common::za::lease::ZaUidLeaseGuard, String)>,
    contexts: Vec<groupware::calendar::sharing::resolve::ZaCalendarContext>,
    calendar_id: u32,
}

/// A key account's PUT create (invitations spec 5.1-5.4, key pinning spec
/// 5.1): the document id is assigned by a reservation-only batch before the
/// fan-out (ruling 14), which records it as a withheld entry's source; the
/// fan-out and the insert are fenced on the UID creation lease; the event is
/// sealed in its calendar's domain before upstream's builder; one bounded
/// retry follows a calendar change.
async fn za_create_event(
    server: &Server,
    access_token: &AccessToken,
    resources: &common::GroupwareResources,
    account: &common::auth::AccountCache,
    keys: &vault::session::SessionKeys,
    roles: &ZaRoles,
    create: ZaCreate,
) -> crate::Result<HttpResponse> {
    let ZaCreate {
        event,
        mut content,
        submitted_ical,
        is_owner,
        personal_id,
        next_email_alarms,
        messages,
        resolution,
        mut lease,
        mut contexts,
        calendar_id,
    } = create;
    let account_id = account.id;
    let uid = lease
        .as_ref()
        .map(|(_, uid)| uid.clone())
        .unwrap_or_default();

    // Ruling 14: an id reserved by a committed batch is never reused.
    let mut reservation = BatchBuilder::new();
    let slot = reservation.reserve_document_id(account_id, Collection::CalendarEvent);
    let document_id = match server.commit_batch(reservation).await {
        Ok(assigned) => assigned.slot(slot),
        Err(err) => {
            if let Some((lease, _)) = lease.take() {
                lease.release().await;
            }
            return Err(err.caused_by(trc::location!()).into());
        }
    };

    // Spec 5.3-5.4: after every check, immediately before the sealed commit;
    // the fan-out hands out the attendees' tokens. Fenced on the creation
    // lease: a creator whose lease was taken over writes no item after its
    // successor's.
    let mut pairs = None;
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
                lease.as_ref().map(|(lease, _)| lease.lease()),
                resolution.as_ref(),
            )
            .await
        {
            Ok(fan_out) => fan_out,
            Err(err) if err.is_assertion_failure() => {
                let (lease, uid) = lease.take().expect("a lost fence needs a lease");
                return za_lost_create_fence(
                    server,
                    resources,
                    access_token,
                    lease,
                    account_id,
                    calendar_id,
                    &uid,
                )
                .await;
            }
            Err(err) => {
                if let Some((lease, _)) = lease.take() {
                    lease.release().await;
                }
                return Err(err.caused_by(trc::location!()).into());
            }
        };
        za_set_schedule_status(&mut content.data.event, &fan_out.statuses);
        za_set_sequences(&mut content.data.event, &fan_out.sequences);
        pairs = fan_out.pairs;
    }

    let serves_submitted_ical = if is_owner {
        serves_ical(&content.data.event, &submitted_ical)
    } else {
        let mut view = content.data.event.clone();
        view.apply_user_data(content.preferences(personal_id), UserDataView::AlertsOnly);
        serves_ical(&view, &submitted_ical)
    };
    let schedule_tag = event.schedule_tag;
    let mut sealed = ZaEvent::new(event, content);
    if let Err(err) = seal_event_in_domain(&mut sealed, &contexts[0].domain, pairs.as_ref()) {
        if let Some((lease, _)) = lease.take() {
            lease.release().await;
        }
        return Err(seal_error(err, account_id, document_id).into());
    }

    let timestamp = now() as i64;
    let build = |sealed: &ZaEvent,
                 contexts: &[groupware::calendar::sharing::resolve::ZaCalendarContext],
                 lease: Option<&common::za::lease::ZaUidLease>|
     -> trc::Result<(BatchBuilder, String)> {
        let mut batch = BatchBuilder::new();
        batch.with_account_id(account_id);
        server.limit_disk_quota(&mut batch, account);
        if let Some(lease) = lease {
            lease.assert_in(&mut batch);
        }
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
                next_email_alarms.clone(),
                timestamp,
                &mut batch,
            )
            .caused_by(trc::location!())?;
        Ok((batch, etag))
    };
    let (batch, mut etag) = build(
        &sealed,
        &contexts,
        lease.as_ref().map(|(lease, _)| lease.lease()),
    )?;
    if lease.is_some() {
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::commit_pause_point(account_id).await;
        #[cfg(feature = "test_mode")]
        if common::za::test_hooks::take_commit_fail(account_id) {
            lease.take().unwrap().0.release().await;
            return Err(trc::StoreEvent::UnexpectedError
                .into_err()
                .details("zero-access test hook: sender commit failure")
                .into());
        }
        match server.commit_batch(batch).await {
            Ok(_) => {
                lease.take().unwrap().0.release().await;
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
                    let (lease_guard, uid) = lease.take().unwrap();
                    let (batch, retry_etag) = build(&sealed, &contexts, Some(lease_guard.lease()))?;
                    etag = retry_etag;
                    za_commit_create(
                        server,
                        resources,
                        access_token,
                        batch,
                        lease_guard,
                        account_id,
                        calendar_id,
                        &uid,
                    )
                    .await?;
                } else {
                    let (lease, uid) = lease.take().unwrap();
                    return za_lost_create_fence(
                        server,
                        resources,
                        access_token,
                        lease,
                        account_id,
                        calendar_id,
                        &uid,
                    )
                    .await;
                }
            }
            Err(error) => {
                lease.take().unwrap().0.release().await;
                return Err(error.caused_by(trc::location!()).into());
            }
        }
    } else {
        server
            .commit_batch(batch)
            .await
            .caused_by(trc::location!())?;
    }
    server.notify_task_queue();

    Ok(HttpResponse::new(StatusCode::CREATED)
        .with_etag_opt(serves_submitted_ical.then_some(etag))
        .with_schedule_tag_opt(schedule_tag))
}
