/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
<<<<<<< HEAD
    assert_is_unique_uid, za_commit_create, za_lost_create_fence, za_refresh_event,
    za_take_uid_lease, za_write_contexts,
=======
    CalendarComponentSupport, CalendarEventView, assert_event_privacy_access,
    assert_event_privacy_allowed, assert_is_unique_uid, serves_ical, user_data_error,
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
};
use crate::{
    DavError, DavErrorCondition, DavMethod,
    calendar::ItipPrecondition,
    common::{
        lock::{LockRequestHandler, ResourceState},
        uri::DavUriResource,
        za::za_event_view,
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
<<<<<<< HEAD
        CalendarEvent, CalendarEventData,
        delegation::{
            ZaSelfOperation, za_combine_delegated, za_finalize, za_resolve_delegation,
            za_self_create, za_self_update,
        },
        fanout::{ZaFanOut, ZaFanOutKind, ZaFanOutMessage, ZaSource, za_set_schedule_status},
        itip::ItipSendStatus,
        seal::{event_pairs_in_domain, event_view_in_domain, seal_error, seal_event_in_domain},
        sequence::{za_sequence_floor, za_set_sequences},
    },
    scheduling::{
        ItipError, ItipMessages, event_create::itip_create, event_update::itip_update,
        itip::itip_set_unreachable_status,
=======
        CalendarEvent, CalendarEventContent, CalendarEventData, SupportedComponent,
        alerts::{DefaultAlertsResolver, DefaultAlertsView, ICalendarDefaultAlerts},
        identity::ParticipantIdentityAddresses,
        itip::ItipSendStatus,
        notification::{NotificationAudience, may_have_viewers},
        privacy::EventViewer,
        rights::{EventAcl, EventChanges},
        schedule::{AlarmTasks, EventAlarmScheduler, EventAlarmUsers},
        storage::{DirectChange, DirectChangeNotification, NotificationQuota},
        user::{ICalendarUserData, UpdatedPolicy, UserDataSplit, UserDataUpdate, UserDataView},
    },
    scheduling::{
        ItipMessages, event_create::itip_create, event_update::itip_update,
        itip::itip_set_unreachable_status, recipient::RecipientPolicy,
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
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
<<<<<<< HEAD
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::put_pause_point(account_id).await;
        let resource_name = fix_percent_encoding(
            resource
                .resource
                .ok_or(DavError::Code(StatusCode::CONFLICT))?,
        );
=======
        let resource_name = resource
            .resource
            .ok_or(DavError::Code(StatusCode::CONFLICT))?;
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184

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
        let roles = if za_keys.is_some() && account_info.account_id != account_id {
            Some((
                self.build_account_info(account_info.account.clone())
                    .await
                    .caused_by(trc::location!())?,
                self.account_info(account_id)
                    .await
                    .caused_by(trc::location!())?,
            ))
        } else {
            None
        };
        let (writer_account, event_account) = roles
            .as_ref()
            .map(|(writer, owner)| (writer, owner))
            .unwrap_or((&account_info, &account_info));

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
<<<<<<< HEAD
            // Spec 8.1: the stored sealed archive stays `current` for the
            // index builder; the unsealed view is used for comparison, editing
            // and response bodies only.
            let mut contexts = if za_keys.is_some() {
                za_write_contexts(
                    self,
                    access_token,
                    account_id,
                    &event_
                        .deserialize::<CalendarEvent>()
                        .caused_by(trc::location!())?,
                    parent_id,
                    Acl::ModifyItems,
                )
                .await?
            } else {
                Vec::new()
            };
            let view_ = match contexts.first() {
                Some(context) => std::borrow::Cow::Owned(
                    event_view_in_domain(&event_, &context.domain)
                        .map_err(|error| seal_error(error, account_id, document_id))?,
                ),
                None => za_event_view(&event_, None, account_id, document_id)?,
            };
            let view = view_
                .to_unarchived::<CalendarEvent>()
=======
            let content_ = content_.ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
            let content = content_
                .to_unarchived::<CalendarEventContent>()
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
                .caused_by(trc::location!())?;

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
<<<<<<< HEAD
                        .with_binary_body(view.inner.data.event.to_string()));
=======
                        .with_binary_body(body));
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
                }
                Err(e) => return Err(e),
            }

<<<<<<< HEAD
            if ical == view.inner.data.event {
=======
            if ical == content.inner.data.event {
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
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
<<<<<<< HEAD
            let mut next_email_alarm = None;
            let mut new_event = view
                .deserialize::<CalendarEvent>()
                .caused_by(trc::location!())?;
            let old_ical = new_event.data.event;
            // Key pinning spec 5.1: the SEQUENCE floor, on every PUT of an
            // existing event of a key account, before the scheduling step.
            if za_keys.is_some() {
                za_sequence_floor(&mut ical, &old_ical, account_info.addresses());
            }
            new_event.size = bytes.len() as u32;
            new_event.data = CalendarEventData::new_for(
                ical,
                Tz::Floating,
                self.core.groupware.max_ical_instances,
                &mut next_email_alarm,
                za_keys.is_some(),
            );

            // Scheduling
            let mut itip_messages = None;
            let mut za_messages = None;
            let mut za_resolution = None;
            let itip_status = ItipSendStatus::resolve(
                self,
                access_token,
                &account_info,
                new_event.data.event_range_end(),
                za_keys.is_some(),
            );
            if itip_status.builds_messages() {
                let is_update = new_event.schedule_tag.is_some();
                if itip_status.is_send_sealed() {
                    za_resolution = za_resolve_delegation(
                        self,
                        &account_info,
                        writer_account,
                        event_account,
                        if is_update {
                            ZaSelfOperation::Update
                        } else {
                            ZaSelfOperation::Create
                        },
                        Some(&old_ical),
                        Some(&new_event.data.event),
                    )
                    .await?;
                }
                // The second run sees the pre-finalize trees.
                let second = za_resolution.as_ref().and_then(|resolution| {
                    if is_update {
                        za_self_update(
                            &old_ical,
                            &new_event.data.event,
                            writer_account,
                            event_account,
                            resolution,
                        )
                    } else {
                        za_self_create(
                            &new_event.data.event,
                            writer_account,
                            event_account,
                            resolution,
                        )
                    }
                    .ok()
                    .flatten()
                });
                let result = if is_update {
=======
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

            let is_user_data_only = if is_owner {
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
                new_content.data = CalendarEventData::new_with_default_alerts(
                    ical,
                    Tz::Floating,
                    self.core.groupware.max_ical_instances,
                    &owner_default_alerts,
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
            let itip_status = if is_user_data_only {
                ItipSendStatus::NotRequested
            } else {
                ItipSendStatus::resolve(
                    self,
                    access_token,
                    &account_info,
                    new_content.data.event_range_end(),
                )
            };
            if itip_status.is_send() {
                let policy = RecipientPolicy::new(&self.core.groupware, new_event.flags);
                let result = if let Some(old_ical) = old_ical
                    .as_ref()
                    .filter(|_| new_event.schedule_tag.is_some())
                {
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
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
                let first_built = result.is_ok();
                let first_nothing = matches!(&result, Err(ItipError::NothingToSend));
                let first = match result {
                    Ok(messages) => Some(messages),
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
<<<<<<< HEAD
                            Reason = err.to_string()
=======
                            Reason = err.to_compact_string(),
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
                        );
                        // Preserve the original error-arm bump exactly once.
                        if let Some(schedule_tag) = &mut new_event.schedule_tag {
                            *schedule_tag += 1;
                        }
                        None
                    }
                };
                if first_built || second.is_some() {
                    if first_nothing
                        && let Some(second) = &second
                        && second.finalize()
                    {
                        za_finalize(&mut new_event.data.event, second.increments());
                    }
                    let is_organizer = second.is_some()
                        || first.as_ref().is_some_and(|messages| {
                            messages.iter().any(|message| message.from_organizer)
                        });
                    if is_organizer {
                        if first_built {
                            if let Some(schedule_tag) = &mut new_event.schedule_tag {
                                *schedule_tag += 1;
                            } else {
                                new_event.schedule_tag = Some(1);
                            }
                        } else if new_event.schedule_tag.is_none() {
                            new_event.schedule_tag = Some(1);
                        }
                    }
                    let first = first.unwrap_or_default();
                    if itip_status.is_send_sealed() {
                        let messages = if let Some(resolution) = &za_resolution {
                            za_combine_delegated(first, second, resolution)
                        } else {
                            first.into_iter().map(ZaFanOutMessage::upstream).collect()
                        };
                        if messages
                            .iter()
                            .map(ZaFanOutMessage::recipient_count)
                            .sum::<usize>()
                            >= self.core.groupware.itip_outbound_max_recipients
                        {
                            return Err(DavError::Condition(DavErrorCondition::new(
                                StatusCode::PRECONDITION_FAILED,
                                CalCondition::MaxAttendeesPerInstance,
                            )));
                        }
                        za_messages = Some(messages);
                    } else {
                        if first.iter().map(|message| message.to.len()).sum::<usize>()
                            >= self.core.groupware.itip_outbound_max_recipients
                        {
                            return Err(DavError::Condition(DavErrorCondition::new(
                                StatusCode::PRECONDITION_FAILED,
                                CalCondition::MaxAttendeesPerInstance,
                            )));
                        }
                        itip_messages = Some(ItipMessages::new(first));
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

<<<<<<< HEAD
            if let Some(keys) = &za_keys {
                // Invitations spec 3.3: every rewrite carries the stored pairs.
                let mut pairs = event_pairs_in_domain(&event_, &contexts[0].domain)
                    .map_err(|err| seal_error(err, account_id, document_id))?;
                // Spec 5.3-5.4: after every check of the request and
                // immediately before the sealed commit, the items go out; the
                // fan-out decides the pairs the bundle keeps.
                if let Some(messages) = za_messages {
                    // The stored UID, which the new one was validated equal
                    // to above.
                    let uid = event.inner.data.event.uids().next().unwrap_or_default();
                    let fan_out = self
                        .za_fan_out(
                            &account_info,
                            writer_account,
                            event_account,
                            keys,
                            messages,
                            pairs,
                            &new_event.data.event,
                            ZaSource { document_id, uid },
                            ZaFanOutKind::Change,
                            None,
                            za_resolution.as_ref(),
                        )
                        .await
                        .caused_by(trc::location!())?;
                    za_set_schedule_status(&mut new_event.data.event, &fan_out.statuses);
                    za_set_sequences(&mut new_event.data.event, &fan_out.sequences);
                    pairs = fan_out.pairs;
                }
                seal_event_in_domain(&mut new_event, &contexts[0].domain, pairs.as_ref())
                    .map_err(|err| seal_error(err, account_id, document_id))?;
            }

            // The same event/index/alarm intent is rebuilt once, without scheduling.
            let schedule_tag = new_event.schedule_tag;
            let timestamp = store::write::now() as i64;
            let mut etag;
            for attempt in 0..=1 {
                let mut batch = BatchBuilder::new();
                for context in &contexts {
                    context.assert_in(&mut batch);
                }
                etag = new_event
                    .clone()
                    .update_at(
                        access_token.account_tenant_ids(),
                        event_
                            .to_unarchived::<CalendarEvent>()
                            .caused_by(trc::location!())?,
                        account_id,
                        document_id,
                        timestamp,
                        &mut batch,
                    )
                    .caused_by(trc::location!())?
                    .etag();
                if prev_email_alarm != next_email_alarm {
                    if let Some(prev_alarm) = &prev_email_alarm {
                        prev_alarm.delete_task(&mut batch);
                    }
                    if let Some(next_alarm) = &next_email_alarm {
                        next_alarm.write_task(&mut batch);
                    }
                }
                // Retries are sealed/key-only; plain itip_messages cannot be
                // consumed on a reachable retry path.
                if let Some(messages) = itip_messages.take() {
                    messages.queue(&mut batch).caused_by(trc::location!())?;
                }
                #[cfg(feature = "test_mode")]
                common::za::test_hooks::update_commit_pause_point(account_id).await;
                match self.commit_batch(batch).await {
                    Ok(_) => {
                        return Ok({
                            self.notify_task_queue();
                            HttpResponse::new(StatusCode::NO_CONTENT)
                                .with_etag_opt(etag)
                                .with_schedule_tag_opt(schedule_tag)
                        });
                    }
                    Err(error) if error.is_assertion_failure() && attempt == 0 => {
                        if let Some(fresh) = za_refresh_event(
                            self,
                            access_token,
                            account_id,
                            parent_id,
                            Acl::ModifyItems,
                            &contexts,
                            &mut new_event,
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
            unreachable!("bounded event retry always returns")
        } else if let Some((Some(parent), name)) = resources.map_parent(resource_name.as_ref()) {
=======
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
            let etag = new_event
                .update_full(
                    new_content,
                    access_token.changed_by(),
                    event,
                    content.inner,
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
            self.commit_batch(batch).await.caused_by(trc::location!())?;

            Ok(HttpResponse::new(StatusCode::NO_CONTENT)
                .with_etag_opt(serves_submitted_ical.then_some(etag))
                .with_schedule_tag_opt(schedule_tag))
        } else if let Some((Some(parent), name)) = resources.map_parent(resource_name) {
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
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
<<<<<<< HEAD
            let uid = validate_ical(&ical)?;
            // Invitations spec 5.1, 6.3: a key account's create holds the UID
            // creation lease from here through the insert, and checks current
            // archives instead of the snapshot.
            let mut za_lease = if za_keys.is_some() {
                Some((
                    za_take_uid_lease(self, &resources, account_id, parent.document_id(), uid)
                        .await?,
                    uid.to_string(),
                ))
            } else {
                assert_is_unique_uid(
                    self,
                    &resources,
                    account_id,
                    parent.document_id(),
                    uid.into(),
                )
                .await?;
                None
            };
=======
            let (uid, component) = validate_ical(&ical)?;
            assert_is_unique_uid(
                &resources,
                access_token,
                account_id,
                parent.document_id(),
                Some(uid),
            )?;
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
            if !is_owner {
                let extracted = ical
                    .extract_user_data(personal_id)
                    .map_err(user_data_error)?;
                ical.apply_user_data(None, UserDataView::Full);
                if !extracted.is_empty() {
                    preferences.push(extracted);
                }
            }
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184

            // Build event
            let mut event = CalendarEvent {
                names: vec![DavName {
                    name: name.to_string(),
                    parent_id: parent.document_id(),
                }],
<<<<<<< HEAD
                data: CalendarEventData::new_for(
                    ical,
                    Tz::Floating,
                    self.core.groupware.max_ical_instances,
                    &mut next_email_alarm,
                    za_keys.is_some(),
=======
                ..Default::default()
            };
            let mut content = CalendarEventContent {
                data: CalendarEventData::new(
                    ical,
                    Tz::Floating,
                    self.core.groupware.max_ical_instances,
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
                ),
                preferences,
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

            // Scheduling
            let mut itip_messages = None;
            let mut za_messages = None;
            let mut za_resolution = None;
            let itip_status = ItipSendStatus::resolve(
                self,
                access_token,
                &account_info,
<<<<<<< HEAD
                event.data.event_range_end(),
                za_keys.is_some(),
            );
            if itip_status.builds_messages() {
                if itip_status.is_send_sealed() {
                    za_resolution = za_resolve_delegation(
                        self,
                        &account_info,
                        writer_account,
                        event_account,
                        ZaSelfOperation::Create,
                        None,
                        Some(&event.data.event),
                    )
                    .await?;
                }
                let second = za_resolution.as_ref().and_then(|resolution| {
                    za_self_create(&event.data.event, writer_account, event_account, resolution)
                        .ok()
                        .flatten()
                });
                let result = itip_create(&mut event.data.event, account_info.addresses());
                let first_built = result.is_ok();
                let first_nothing = matches!(&result, Err(ItipError::NothingToSend));
                let first = match result {
                    Ok(messages) => messages,
=======
                content.data.event_range_end(),
            );
            if itip_status.is_send() {
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
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
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
<<<<<<< HEAD
                            Reason = err.to_string()
=======
                            Reason = err.to_compact_string(),
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
                        );
                        Vec::new()
                    }
                };
                if first_built || second.is_some() {
                    if first_nothing
                        && let Some(second) = &second
                        && second.finalize()
                    {
                        za_finalize(&mut event.data.event, second.increments());
                    }
                    event.schedule_tag = Some(1);
                    if itip_status.is_send_sealed() {
                        let messages = if let Some(resolution) = &za_resolution {
                            za_combine_delegated(first, second, resolution)
                        } else {
                            first.into_iter().map(ZaFanOutMessage::upstream).collect()
                        };
                        if messages
                            .iter()
                            .map(ZaFanOutMessage::recipient_count)
                            .sum::<usize>()
                            >= self.core.groupware.itip_outbound_max_recipients
                        {
                            return Err(DavError::Condition(DavErrorCondition::new(
                                StatusCode::PRECONDITION_FAILED,
                                CalCondition::MaxAttendeesPerInstance,
                            )));
                        }
                        za_messages = Some(messages);
                    } else {
                        if first.iter().map(|message| message.to.len()).sum::<usize>()
                            >= self.core.groupware.itip_outbound_max_recipients
                        {
                            return Err(DavError::Condition(DavErrorCondition::new(
                                StatusCode::PRECONDITION_FAILED,
                                CalCondition::MaxAttendeesPerInstance,
                            )));
                        }
                        itip_messages = Some(ItipMessages::new(first));
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

<<<<<<< HEAD
            // Key pinning spec 5.1: assigned before the fan-out, which
            // records it as a withheld entry's source (a failed create's id
            // is never used again).
            let document_id = self
                .store()
                .assign_document_ids(account_id, Collection::CalendarEvent, 1)
                .await
                .caused_by(trc::location!())?;
            if let Some(keys) = &za_keys {
                // Spec 5.3-5.4: after every check, immediately before the
                // sealed commit; the fan-out hands out the attendees' tokens.
                // Fenced on the creation lease (spec 5.4): a creator whose
                // lease was taken over writes no item after its successor's.
                let mut pairs = None;
                if let Some(messages) = za_messages {
                    // The lease's copy of the UID: the tree moved into the
                    // event.
                    let uid = za_lease.as_ref().map_or("", |(_, uid)| uid.as_str());
                    let fan_out = match self
                        .za_fan_out(
                            &account_info,
                            writer_account,
                            event_account,
                            keys,
                            messages,
                            None,
                            &event.data.event,
                            ZaSource { document_id, uid },
                            ZaFanOutKind::Change,
                            za_lease.as_ref().map(|(lease, _)| lease.lease()),
                            za_resolution.as_ref(),
                        )
                        .await
                    {
                        Ok(fan_out) => fan_out,
                        Err(err) if err.is_assertion_failure() => {
                            let (lease, uid) = za_lease.take().expect("a lost fence needs a lease");
                            return za_lost_create_fence(
                                self,
                                &resources,
                                lease,
                                account_id,
                                parent.document_id(),
                                &uid,
                            )
                            .await;
                        }
                        Err(err) => return Err(err.caused_by(trc::location!()).into()),
                    };
                    za_set_schedule_status(&mut event.data.event, &fan_out.statuses);
                    za_set_sequences(&mut event.data.event, &fan_out.sequences);
                    pairs = fan_out.pairs;
                }
                seal_event_in_domain(&mut event, &contexts[0].domain, pairs.as_ref())
                    .map_err(|err| seal_error(err, account_id, u32::MAX))?;
            }

            let timestamp = now() as i64;
            let schedule_tag = event.schedule_tag;
            let mut build =
                |event: &CalendarEvent,
                 contexts: &[groupware::calendar::sharing::resolve::ZaCalendarContext]|
                 -> trc::Result<(BatchBuilder, Option<String>)> {
                    let mut batch = BatchBuilder::new();
                    if let Some((lease, _)) = &za_lease {
                        lease.lease().assert_in(&mut batch);
                    }
                    for context in contexts {
                        context.assert_in(&mut batch);
                    }
                    let etag = event
                        .clone()
                        .insert_at(
                            access_token.account_tenant_ids(),
                            account_id,
                            document_id,
                            next_email_alarm.clone(),
                            timestamp,
                            &mut batch,
                        )
                        .caused_by(trc::location!())?
                        .etag();
                    // Retries are sealed/key-only; plain itip_messages cannot be
                    // consumed on a reachable retry path.
                    if let Some(messages) = itip_messages.take() {
                        messages.queue(&mut batch).caused_by(trc::location!())?;
                    }
                    Ok((batch, etag))
                };
            let (batch, mut etag) = build(&event, &contexts)?;
            if za_lease.is_some() {
                #[cfg(feature = "test_mode")]
                common::za::test_hooks::commit_pause_point(account_id).await;
                #[cfg(feature = "test_mode")]
                if common::za::test_hooks::take_commit_fail(account_id) {
                    za_lease.take().unwrap().0.release().await;
                    return Err(trc::StoreEvent::UnexpectedError
                        .into_err()
                        .details("zero-access test hook: sender commit failure")
                        .into());
                }
                match self.commit_batch(batch).await {
                    Ok(_) => {
                        za_lease.take().unwrap().0.release().await;
                    }
                    Err(error) if error.is_assertion_failure() => {
                        if let Some(fresh) = za_refresh_event(
                            self,
                            access_token,
                            account_id,
                            parent.document_id(),
                            Acl::AddItems,
                            &contexts,
                            &mut event,
                        )
                        .await?
                        {
                            contexts = fresh;
                            let (batch, retry_etag) = build(&event, &contexts)?;
                            etag = retry_etag;
                            let (lease, uid) = za_lease.take().unwrap();
                            za_commit_create(
                                self,
                                &resources,
                                batch,
                                lease,
                                account_id,
                                parent.document_id(),
                                &uid,
                            )
                            .await?;
                        } else {
                            let (lease, uid) = za_lease.take().unwrap();
                            return za_lost_create_fence(
                                self,
                                &resources,
                                lease,
                                account_id,
                                parent.document_id(),
                                &uid,
                            )
                            .await;
                        }
                    }
                    Err(error) => return Err(error.caused_by(trc::location!()).into()),
                }
            } else {
                self.commit_batch(batch).await.caused_by(trc::location!())?;
            }
            self.notify_task_queue();
=======
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
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184

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
