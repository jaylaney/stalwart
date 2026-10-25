/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::za_calendar_error;
use crate::{
    DavError, DavMethod,
    common::{
        ETag,
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
        Calendar, CalendarEvent,
        delegation::{
            ZaSelfOperation, za_combine_delegated, za_resolve_delegation, za_self_cancel,
        },
        fanout::{ZaFanOut, ZaFanOutKind, ZaFanOutMessage, ZaSource},
        sharing::resolve::calendars_changed,
    },
};
use http_proto::HttpResponse;
use hyper::StatusCode;
use registry::schema::enums::Permission;
use store::write::{BatchBuilder, ValueClass};
use store::{
    ValueKey,
    write::{AlignedBytes, Archive},
};
use trc::AddContext;
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection},
    field::PrincipalField,
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
            .fetch_dav_resources(
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
        let account_info = self
            .scheduling_account_info(access_token.account_id(), account_id)
            .await
            .caused_by(trc::location!())?;
        let za_keys = super::za_writer_keys(self, access_token, account_id).await?;
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
        let send_itip = self.core.groupware.itip_enabled
            && !headers.no_schedule_reply
            && !account_info.addresses().is_empty()
            && access_token.has_permission(Permission::CalendarSchedulingSend);
        // Invitations spec 5.5: a key account's CANCELs and declines are
        // built from the unsealed stored tree and go to the fan-out.
        let mut za_messages = Vec::new();

        // Fetch entry
        let mut batch = BatchBuilder::new();
        let mut retry_event = None;
        let mut contexts = Vec::new();
        let mut deleted_grantees = Vec::new();
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
                .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
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
            if !access_token.is_member(account_id)
                && !calendar
                    .inner
                    .acls
                    .effective_acl(access_token)
                    .contains_all([Acl::Delete, Acl::RemoveItems].into_iter())
            {
                return Err(DavError::Code(StatusCode::FORBIDDEN));
            }

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
            za_messages = DestroyArchive(calendar)
                .delete_with_events_za(
                    self,
                    &account_info,
                    account_id,
                    document_id,
                    resources
                        .subtree(delete_path)
                        .filter(|r| !r.is_container())
                        .map(|r| r.document_id())
                        .collect::<Vec<_>>(),
                    resources.format_resource(delete_resource).into(),
                    send_itip,
                    za_keys.as_deref(),
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;

            // Reset default calendar id
            let default_calendar_id = self
                .store()
                .get_value::<u32>(ValueKey {
                    account_id,
                    collection: Collection::Principal.into(),
                    document_id: 0,
                    class: ValueClass::Property(PrincipalField::DefaultCalendarId.into()),
                })
                .await
                .caused_by(trc::location!())?;
            if default_calendar_id.is_some_and(|id| id == document_id) {
                batch
                    .with_account_id(account_id)
                    .with_collection(Collection::Principal)
                    .with_document(0)
                    .clear(PrincipalField::DefaultCalendarId);
            }
        } else {
            // Validate ACL
            let calendar_id = delete_resource.parent_id().unwrap();
            if !access_token.is_member(account_id)
                && !resources.has_access_to_container(access_token, calendar_id, Acl::RemoveItems)
            {
                return Err(DavError::Code(StatusCode::FORBIDDEN));
            }

            let event_ = self
                .store()
                .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                    account_id,
                    Collection::CalendarEvent,
                    document_id,
                ))
                .await
                .caused_by(trc::location!())?
                .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;

            // Validate headers
            self.validate_headers(
                access_token,
                headers,
                vec![ResourceState {
                    account_id,
                    collection: Collection::CalendarEvent,
                    document_id: document_id.into(),
                    etag: event_.etag().into(),
                    path: delete_path,
                    ..Default::default()
                }],
                Default::default(),
                DavMethod::DELETE,
            )
            .await?;

            // Validate schedule tag
            let event = event_
                .to_unarchived::<CalendarEvent>()
                .caused_by(trc::location!())?;
            if headers.if_schedule_tag.is_some()
                && event.inner.schedule_tag.as_ref().map(|t| t.to_native())
                    != headers.if_schedule_tag
            {
                return Err(DavError::Code(StatusCode::PRECONDITION_FAILED));
            }

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
            }
            let cutoff = store::write::now() as i64;
            retry_event = Some((event_.clone(), calendar_id, cutoff));
            // Delete event
            za_messages.extend(
                DestroyArchive(event)
                    .delete_za_at(
                        &account_info,
                        account_id,
                        document_id,
                        calendar_id,
                        resources.format_resource(delete_resource).into(),
                        send_itip,
                        za_keys.as_deref(),
                        contexts.first().map(|context| &context.domain),
                        cutoff,
                        &mut batch,
                    )
                    .caused_by(trc::location!())?,
            );
        }

        if let Some(keys) = &za_keys {
            // Spec 5.4: the items first, immediately before the commit.
            for message in za_messages {
                let resolution = za_resolve_delegation(
                    self,
                    &account_info,
                    writer_account,
                    event_account,
                    ZaSelfOperation::Delete,
                    Some(&message.event),
                    None,
                )
                .await?;
                let messages = if let Some(resolution) = &resolution {
                    let second =
                        za_self_cancel(&message.event, writer_account, event_account, resolution)
                            .ok()
                            .flatten();
                    za_combine_delegated(message.message.into_iter().collect(), second, resolution)
                } else {
                    message
                        .message
                        .into_iter()
                        .map(ZaFanOutMessage::upstream)
                        .collect()
                };
                self.za_fan_out(
                    &account_info,
                    writer_account,
                    event_account,
                    keys,
                    messages,
                    message.pairs,
                    &message.event,
                    ZaSource {
                        document_id: message.document_id,
                        uid: &message.uid,
                    },
                    ZaFanOutKind::Deletion,
                    None,
                    resolution.as_ref(),
                )
                .await
                .caused_by(trc::location!())?;
            }
        }

        #[cfg(feature = "test_mode")]
        if retry_event.is_some() {
            common::za::test_hooks::delete_commit_pause_point(account_id).await;
        }
        match self.commit_batch(batch).await {
            Ok(_) => {}
            Err(error) if error.is_assertion_failure() && !contexts.is_empty() => {
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
                DestroyArchive(
                    original
                        .to_unarchived::<CalendarEvent>()
                        .caused_by(trc::location!())?,
                )
                .delete_za_at(
                    &account_info,
                    account_id,
                    document_id,
                    *calendar_id,
                    resources.format_resource(delete_resource).into(),
                    false,
                    za_keys.as_deref(),
                    fresh.first().map(|context| &context.domain),
                    *cutoff,
                    &mut batch,
                )
                .caused_by(trc::location!())?;
                self.commit_batch(batch).await.caused_by(trc::location!())?;
            }
            Err(error) => return Err(error.caused_by(trc::location!()).into()),
        }
        // The committed deletion retires every grant. Evict the grantees'
        // resident collection-key sessions and authorization only after that
        // commit succeeds, using the same channel as share mutations.
        let mut invalidation = CacheInvalidationBuilder::default();
        for grantee in deleted_grantees {
            invalidation.invalidate(CacheInvalidation::AccessToken(grantee));
        }
        self.invalidate_caches(invalidation).await?;
        self.notify_task_queue();

        Ok(HttpResponse::new(StatusCode::NO_CONTENT))
    }
}
