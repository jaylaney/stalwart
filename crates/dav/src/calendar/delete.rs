/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{assert_event_privacy_access, za_calendar_error};
use crate::{
    DavError, DavMethod,
    common::{
        ContainerOperation, ETag,
        lock::{LockRequestHandler, ResourceState},
        uri::DavUriResource,
    },
};
use common::{
    Server, auth::AccessToken, cache::invalidate::CacheInvalidationBuilder, ipc::CacheInvalidation,
    sharing::EffectiveAcl,
};
use dav_proto::RequestHeaders;
use groupware::{
    DestroyArchive,
    cache::GroupwareCache,
    calendar::{
        Calendar, CalendarEvent, CalendarEventContent, EVENT_HAS_ALARMS,
        delegation::{
            ZaSelfOperation, za_combine_delegated, za_resolve_delegation, za_self_cancel,
        },
        fanout::{ZaFanOut, ZaFanOutKind, ZaFanOutMessage, ZaSource},
        identity::ParticipantIdentityAddresses,
        notification::{NotificationAudience, may_have_viewers},
        privacy::EventViewer,
        rights::EventAcl,
        sharing::resolve::calendars_changed,
        storage::{DirectChange, DirectChangeNotification, NotificationQuota},
    },
};
use http_proto::HttpResponse;
use hyper::StatusCode;
use registry::schema::enums::Permission;
use store::write::BatchBuilder;
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes},
};
use trc::AddContext;
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection},
    field::{CalendarEventField, PrincipalField},
};

pub(crate) trait CalendarDeleteRequestHandler: Sync + Send {
    fn handle_calendar_delete_request(
        &self,
        access_token: &AccessToken,
        headers: &RequestHeaders<'_>,
    ) -> impl Future<Output = crate::Result<HttpResponse>> + Send;
}

impl CalendarDeleteRequestHandler for Server {
    async fn handle_calendar_delete_request(
        &self,
        access_token: &AccessToken,
        headers: &RequestHeaders<'_>,
    ) -> crate::Result<HttpResponse> {
        // Validate URI
        let resource = self
            .validate_uri(access_token, headers.uri)
            .await?
            .into_owned_uri()?;
        let account_id = resource.account_id;
        let delete_path = resource
            .resource
            .filter(|r| !r.is_empty())
            .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
        let resources = self
            .fetch_groupware_resources(
                access_token.account_id(),
                account_id,
                SyncCollection::Calendar,
            )
            .await
            .caused_by(trc::location!())?;

        // Check resource type
        let delete_resource = resources
            .by_path(delete_path)
            .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
        let document_id = delete_resource.document_id();
        let account_info = self.account_info(account_id).await?;
        // Release 4: a key account's write keeps its writer, owner and
        // sender roles apart (delegated scheduling carries SENT-BY).
        let za_keys = super::za_writer_keys(self, access_token, account_id).await?;
        let za_roles = match &za_keys {
            Some(_) => Some(super::za_roles(self, access_token, account_id).await?),
            None => None,
        };
        let send_itip = self.core.groupware.itip_enabled
            && !headers.no_schedule_reply
            && !account_info.addresses().is_empty()
            && access_token.has_permission(Permission::CalendarSchedulingSend);
        // Invitations spec 5.5: a key account's CANCELs and declines are
        // built from the unsealed stored tree and go to the fan-out.
        let mut za_messages = Vec::new();
        let mut retry_event = None;
        let mut contexts = Vec::new();
        let mut deleted_grantees = Vec::new();

        // Fetch entry
        let mut batch = BatchBuilder::new();
        if za_keys.is_none() {
            crate::common::za::za_ordinary_fence(self, &mut batch, account_id, None);
        }
        if delete_resource.is_container() {
            // Deleting the default calendar is not allowed
            #[cfg(not(any(feature = "dev_mode", feature = "test_mode")))]
            if self
                .core
                .groupware
                .default_calendar_name
                .as_ref()
                .is_some_and(|name| name == delete_path)
            {
                return Err(DavError::Condition(crate::DavErrorCondition::new(
                    StatusCode::FORBIDDEN,
                    dav_proto::schema::response::CalCondition::DefaultCalendarNeeded,
                )));
            }

            let calendar_ = self
                .store()
                .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                    account_id,
                    Collection::Calendar,
                    document_id,
                ))
                .await
                .caused_by(trc::location!())?
                .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;

            let calendar = calendar_
                .to_unarchived::<Calendar>()
                .caused_by(trc::location!())?;

            // Validate ACL
            let is_owner = access_token.is_member(account_id);
            if !is_owner
                && !calendar
                    .inner
                    .acls
                    .effective_acl(access_token)
                    .contains_all(ContainerOperation::Remove.required_acls())
            {
                return Err(DavError::Code(StatusCode::FORBIDDEN));
            }
            let children_ids = ContainerOperation::Remove.event_ids(
                &resources,
                delete_path,
                EventViewer::new(is_owner),
            )?;

            // Even collection deletion uses the fresh exact-principal grant.
            let repaired = crate::common::za::za_mutation_context(
                self,
                access_token,
                account_id,
                document_id,
                utils::map::bitmap::Bitmap::new()
                    .with_item(Acl::Delete)
                    .with_item(Acl::RemoveItems),
            )
            .await
            .map_err(|error| za_calendar_error(error, account_id, document_id))?;
            let was_shared = repaired
                .as_ref()
                .is_some_and(|context| context.domain.calendar_id().is_some());
            let calendar_ = repaired.map(|context| context.stored).unwrap_or(calendar_);
            let calendar = calendar_
                .to_unarchived::<Calendar>()
                .caused_by(trc::location!())?;
            if was_shared {
                deleted_grantees.extend(
                    calendar
                        .inner
                        .acls
                        .iter()
                        .map(|grant| grant.account_id.to_native())
                        .filter(|grantee| *grantee != account_id),
                );
            }

            // Validate headers
            self.validate_headers(
                access_token,
                headers,
                vec![ResourceState {
                    account_id,
                    collection: Collection::Calendar,
                    document_id: document_id.into(),
                    etag: calendar.etag().into(),
                    path: delete_path,
                    ..Default::default()
                }],
                Default::default(),
                DavMethod::DELETE,
            )
            .await?;

            // Delete calendar and events
            let cleanup = self
                .preload_container_cleanup(
                    account_info.changed_by(),
                    account_id,
                    Collection::CalendarEvent,
                    &children_ids
                        .iter()
                        .copied()
                        .filter(|&document_id| {
                            resources
                                .item_by_id(document_id)
                                .is_some_and(|item| !item.metadata_kinds().is_empty())
                        })
                        .collect(),
                )
                .await
                .caused_by(trc::location!())?;
            if let Some(roles) = &za_roles {
                za_messages = DestroyArchive(calendar)
                    .delete_with_events_za(
                        self,
                        access_token,
                        &roles.sender,
                        account_id,
                        document_id,
                        children_ids,
                        &resources.event_ids_with_flags(EVENT_HAS_ALARMS),
                        &cleanup,
                        resources.format_resource(delete_resource).into(),
                        send_itip,
                        za_keys.as_deref(),
                        &mut batch,
                    )
                    .await
                    .caused_by(trc::location!())?;
            } else {
                DestroyArchive(calendar)
                    .delete_with_events(
                        self,
                        access_token,
                        &account_info,
                        account_id,
                        document_id,
                        children_ids,
                        &resources.event_ids_with_flags(EVENT_HAS_ALARMS),
                        &cleanup,
                        resources.format_resource(delete_resource).into(),
                        send_itip,
                        &mut batch,
                    )
                    .await
                    .caused_by(trc::location!())?;
            }

            // Reset default calendar id
            batch
                .with_account_id(account_id)
                .with_collection(Collection::Principal)
                .with_document(0)
                .clear_if_equals(PrincipalField::DefaultCalendarId, document_id);
        } else {
            // Validate ACL
            let calendar_id = delete_resource
                .parent_id()
                .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
            let is_owner = access_token.is_member(account_id);
            assert_event_privacy_access(
                delete_resource.resource.event_flags(),
                EventViewer::new(is_owner),
            )?;
            let may_remove_items = is_owner
                || resources.has_access_to_container(access_token, calendar_id, Acl::RemoveItems);
            if !may_remove_items
                && !EventAcl::for_calendar(&resources, access_token, calendar_id)
                    .may_manage_own_items()
            {
                return Err(DavError::Code(StatusCode::FORBIDDEN));
            }

            let event_ = self
                .store()
                .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                    account_id,
                    Collection::CalendarEvent,
                    document_id,
                ))
                .await
                .caused_by(trc::location!())?
                .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
            let event = event_
                .to_unarchived::<CalendarEvent>()
                .caused_by(trc::location!())?;
            let event_flags = event.inner.flags.to_native();
            let may_notify = may_have_viewers(
                access_token,
                account_id,
                &resources,
                &[calendar_id],
                NotificationAudience::of_event(event_flags),
            );
            let content_ = if !may_remove_items || may_notify {
                self.store()
                    .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                        account_id,
                        Collection::CalendarEvent,
                        document_id,
                        CalendarEventField::Content,
                    ))
                    .await
                    .caused_by(trc::location!())?
            } else {
                None
            };
            let previous_event = content_
                .as_ref()
                .map(|content| content.deserialize::<CalendarEventContent>())
                .transpose()
                .caused_by(trc::location!())?
                .map(|content| content.data.event);
            if !may_remove_items {
                let identities = self
                    .account_identity_addresses(access_token.account_id())
                    .await
                    .caused_by(trc::location!())?;
                if !previous_event
                    .as_ref()
                    .is_some_and(|ical| identities.event_ownership(ical).may_write_own())
                {
                    return Err(DavError::Code(StatusCode::FORBIDDEN));
                }
            }

            // Validate headers
            self.validate_headers(
                access_token,
                headers,
                vec![ResourceState {
                    account_id,
                    collection: Collection::CalendarEvent,
                    document_id: document_id.into(),
                    etag: format!("\"{}\"", event.inner.etag.to_native()).into(),
                    path: delete_path,
                    ..Default::default()
                }],
                Default::default(),
                DavMethod::DELETE,
            )
            .await?;

            // Validate schedule tag
            if headers.if_schedule_tag.is_some()
                && event.inner.schedule_tag.as_ref().map(|t| t.to_native())
                    != headers.if_schedule_tag
            {
                return Err(DavError::Code(StatusCode::PRECONDITION_FAILED));
            }

            if za_keys.is_some() {
                if let Some(context) = crate::common::za::za_mutation_context(
                    self,
                    access_token,
                    account_id,
                    calendar_id,
                    utils::map::bitmap::Bitmap::new().with_item(Acl::RemoveItems),
                )
                .await
                .map_err(|error| za_calendar_error(error, account_id, calendar_id))?
                {
                    context.assert_in(&mut batch);
                    contexts.push(context);
                } else {
                    // Upgrade ruling 176 (B-M2): a key account's delete needs
                    // its key domain; it never takes upstream's queueing.
                    return Err(za_calendar_error(
                        groupware::calendar::sharing::resolve::ZaCalendarError::Forbidden,
                        account_id,
                        calendar_id,
                    ));
                }
            }
            let cutoff = store::write::now() as i64;
            if za_keys.is_some() {
                retry_event = Some((event_.clone(), calendar_id, cutoff));
            }

            // Delete event
            if let Some(previous) = previous_event {
                self.notify_direct_change(
                    access_token,
                    account_id,
                    DirectChange::Destroyed {
                        event_id: document_id,
                        previous,
                        calendar_ids: vec![calendar_id],
                        event_flags,
                    },
                    Some(&resources),
                    &mut NotificationQuota::default(),
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
            }
            if let Some(roles) = &za_roles {
                za_messages.extend(
                    DestroyArchive(event)
                        .delete_za_at(
                            self,
                            &roles.sender,
                            account_id,
                            document_id,
                            calendar_id,
                            content_,
                            resources.format_resource(delete_resource).into(),
                            send_itip,
                            contexts.first().map(|context| &context.domain),
                            cutoff,
                            &mut batch,
                        )
                        .await
                        .caused_by(trc::location!())?,
                );
            } else {
                DestroyArchive(event)
                    .delete(
                        self,
                        &account_info,
                        account_id,
                        document_id,
                        calendar_id,
                        content_,
                        resources.format_resource(delete_resource).into(),
                        send_itip,
                        &mut batch,
                    )
                    .await
                    .caused_by(trc::location!())?;
            }
        }

        // iMIP spec 5.4, decision 9: each fan-out's mail attempts wait for
        // the deletion's one commit.
        let mut za_mail = common::za::imip::ZaImipPending::default();
        if let (Some(keys), Some(roles)) = (&za_keys, &za_roles) {
            // Spec 5.4: the items first, immediately before the commit.
            for message in za_messages {
                let resolution = za_resolve_delegation(
                    self,
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
                    self.za_fan_out(
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
        }

        #[cfg(feature = "test_mode")]
        if retry_event.is_some() {
            common::za::test_hooks::delete_commit_pause_point(account_id).await;
        }
        #[cfg(feature = "test_mode")]
        let fail = za_keys.is_some() && common::za::test_hooks::take_delete_commit_fail(account_id);
        #[cfg(not(feature = "test_mode"))]
        let fail = false;
        // iMIP spec 5.4: every error return of the commit drops the mail
        // attempts, traced, before returning.
        let committed: crate::Result<()> = async {
            let committed = if fail {
                Err(trc::StoreEvent::UnexpectedError
                    .into_err()
                    .details("zero-access test hook: delete commit failure"))
            } else {
                self.commit_batch(batch).await
            };
            match committed {
                Ok(_) => {}
                Err(error) if error.is_assertion_failure() && !contexts.is_empty() => {
                    // A calendar change (a grant, a rotation or a conversion) lost
                    // the assertion: one bounded retry in the fresh domain, with
                    // scheduling off (its items already went out).
                    let (original, calendar_id, cutoff) = retry_event.as_ref().unwrap();
                    let fresh = crate::common::za::za_mutation_context(
                        self,
                        access_token,
                        account_id,
                        *calendar_id,
                        utils::map::bitmap::Bitmap::new().with_item(Acl::RemoveItems),
                    )
                    .await
                    .map_err(|error| za_calendar_error(error, account_id, *calendar_id))?
                    .into_iter()
                    .collect::<Vec<_>>();
                    if !calendars_changed(&contexts, &fresh) {
                        return Err(error.caused_by(trc::location!()).into());
                    }
                    let mut batch = BatchBuilder::new();
                    for context in &fresh {
                        context.assert_in(&mut batch);
                    }
                    let roles = za_roles.as_ref().unwrap();
                    DestroyArchive(
                        original
                            .to_unarchived::<CalendarEvent>()
                            .caused_by(trc::location!())?,
                    )
                    .delete_za_at(
                        self,
                        &roles.sender,
                        account_id,
                        document_id,
                        *calendar_id,
                        None,
                        resources.format_resource(delete_resource).into(),
                        false,
                        fresh.first().map(|context| &context.domain),
                        *cutoff,
                        &mut batch,
                    )
                    .await
                    .caused_by(trc::location!())?;
                    self.commit_batch(batch).await.caused_by(trc::location!())?;
                }
                Err(error) => return Err(error.caused_by(trc::location!()).into()),
            }
            Ok(())
        }
        .await;
        if let Err(error) = committed {
            za_mail.dropped();
            return Err(error);
        }
        // The committed deletion retires every grant. Evict the grantees'
        // resident collection-key sessions and authorization only after that
        // commit succeeds, using the same channel as share mutations.
        let invalidated = if !deleted_grantees.is_empty() {
            let mut invalidation = CacheInvalidationBuilder::default();
            for grantee in deleted_grantees {
                invalidation.invalidate(CacheInvalidation::AccessToken(grantee));
            }
            self.invalidate_caches(invalidation).await
        } else {
            Ok(())
        };
        if invalidated.is_ok() && za_keys.is_some() {
            self.notify_task_queue();
        }
        // iMIP spec 5.4, section 7: the deletion (or its retry) committed, so
        // its mail goes whether or not the invalidation failed; the
        // invalidation's error is returned after.
        za_mail.submit(self).await;
        invalidated?;

        Ok(HttpResponse::new(StatusCode::NO_CONTENT))
    }
}
