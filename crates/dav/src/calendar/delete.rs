/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    DavError, DavMethod,
    common::{
        ETag,
        lock::{LockRequestHandler, ResourceState},
        uri::DavUriResource,
        za::ZeroAccessGate,
    },
};
use common::{Server, auth::AccessToken, sharing::EffectiveAcl};
use dav_proto::RequestHeaders;
use groupware::{
    DestroyArchive,
    cache::GroupwareCache,
    calendar::{
        Calendar, CalendarEvent,
        fanout::{ZaFanOut, ZaFanOutKind, ZaSource},
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
            .await?;
        let send_itip = self.core.groupware.itip_enabled
            && !headers.no_schedule_reply
            && !account_info.addresses().is_empty()
            && access_token.has_permission(Permission::CalendarSchedulingSend);
        // Invitations spec 5.5: a key account's CANCELs and declines are
        // built from the unsealed stored tree and go to the fan-out.
        let za_keys = self.za_session_keys(access_token, account_id).await?;
        let mut za_messages = Vec::new();

        // Fetch entry
        let mut batch = BatchBuilder::new();
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

            // Delete event
            za_messages.extend(
                DestroyArchive(event)
                    .delete_za(
                        &account_info,
                        account_id,
                        document_id,
                        calendar_id,
                        resources.format_resource(delete_resource).into(),
                        send_itip,
                        za_keys.as_deref(),
                        &mut batch,
                    )
                    .caused_by(trc::location!())?,
            );
        }

        if let Some(keys) = &za_keys {
            // Spec 5.4: the items first, immediately before the commit.
            for message in za_messages {
                self.za_fan_out(
                    &account_info,
                    keys,
                    message.message.into_iter().collect(),
                    message.pairs,
                    &message.event,
                    ZaSource {
                        document_id: message.document_id,
                        uid: &message.uid,
                    },
                    ZaFanOutKind::Deletion,
                    None,
                )
                .await
                .caused_by(trc::location!())?;
            }
        }

        self.commit_batch(batch).await.caused_by(trc::location!())?;
        self.notify_task_queue();

        Ok(HttpResponse::new(StatusCode::NO_CONTENT))
    }
}
