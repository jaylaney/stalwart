/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{CalendarComponentSupport, assert_event_privacy_access, assert_is_unique_uid};
use crate::{
    DavError, DavMethod,
    common::{
        ContainerOperation, assert_parent_limit,
        dead::{
            ContainerRequest, ContainerWrites, DeadContainers, check_growth, container_kinds,
            copy_growth, read_container,
        },
        lock::{LockRequestHandler, ResourceState},
        uri::DavUriResource,
        za::{ZeroAccessGate, za_mutation_context, za_reseal_container},
    },
    file::DavFileResource,
};
use common::{
    DavName, GroupwareResources, Server,
    auth::AccessToken,
    storage::{
        batch::MAX_BATCH_DOCUMENTS,
        index::PresenceFlags,
        metadata::{StoredContainer, StoredEntry},
    },
};
use dav_proto::{Depth, RequestHeaders};
use groupware::calendar::{
    seal::{
        ZaContainerOf, ZaContainerScope, ZaEvent, classify_event_storage, rewrap_event, seal_error,
    },
    sharing::resolve::{ZaCalendarContext, calendars_changed, za_calendar_context},
};
use groupware::{
    DestroyArchive,
    cache::GroupwareCache,
    calendar::{
        CALENDAR_SUBSCRIBED, Calendar, CalendarEvent, CalendarEventContent, CalendarPreferences,
        EVENT_HAS_ALARMS, Timezone,
        alerts::{CalendarSettings, DefaultAlertsResolver},
        identity::{CalendarAddresses, ParticipantIdentityAddresses},
        privacy::{EventPrivacy, EventViewer},
        rights::{EventAcl, StoredEventOwnership},
        schedule::{AlarmTasks, EventAlarmScheduler, EventAlarmUsers},
        storage::{DirectChangeNotification, NotificationQuota},
    },
};
use http_proto::HttpResponse;
use hyper::StatusCode;
use registry::schema::enums::StorageQuota;
use std::collections::{BTreeMap, BTreeSet};
use store::write::{BatchBuilder, PendingId, now};
use store::write::{MergeResult, ValueClass, metadata::MetadataBuf};
use store::{
    ValueKey,
    roaring::RoaringBitmap,
    write::{Archive, ArchiveBytes},
};
use trc::AddContext;
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection},
    field::{CalendarEventField, Field, PrincipalField},
    metadata::MetadataKinds,
};
use utils::map::bitmap::Bitmap;

pub(crate) trait CalendarCopyMoveRequestHandler: Sync + Send {
    fn handle_calendar_copy_move_request(
        &self,
        access_token: &AccessToken,
        headers: &RequestHeaders<'_>,
        is_move: bool,
    ) -> impl Future<Output = crate::Result<HttpResponse>> + Send;
}

impl CalendarCopyMoveRequestHandler for Server {
    async fn handle_calendar_copy_move_request(
        &self,
        access_token: &AccessToken,
        headers: &RequestHeaders<'_>,
        is_move: bool,
    ) -> crate::Result<HttpResponse> {
        // Validate source
        let from_resource_ = self
            .validate_uri(access_token, headers.uri)
            .await?
            .into_owned_uri()?;
        let from_account_id = from_resource_.account_id;
        let from_resources = self
            .fetch_groupware_resources(
                access_token.account_id(),
                from_account_id,
                SyncCollection::Calendar,
            )
            .await
            .caused_by(trc::location!())?;
        let from_resource_name = from_resource_
            .resource
            .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
        let from_resource = from_resources
            .by_path(from_resource_name)
            .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
        #[cfg(not(any(feature = "dev_mode", feature = "test_mode")))]
        if is_move
            && from_resource.is_container()
            && self
                .core
                .groupware
                .default_calendar_name
                .as_ref()
                .is_some_and(|name| name == from_resource_name)
        {
            return Err(DavError::Condition(crate::DavErrorCondition::new(
                StatusCode::FORBIDDEN,
                dav_proto::schema::response::CalCondition::DefaultCalendarNeeded,
            )));
        }

        // Validate ACL
        let is_from_owner = access_token.is_member(from_account_id);
        if !from_resource.is_container() {
            assert_event_privacy_access(
                from_resource.resource.event_flags(),
                EventViewer::new(is_from_owner),
            )?;
        }
        if !is_from_owner
            && !from_resources.has_access_to_container(
                access_token,
                if from_resource.is_container() {
                    from_resource.document_id()
                } else {
                    from_resource
                        .parent_id()
                        .ok_or(DavError::Code(StatusCode::NOT_FOUND))?
                },
                Acl::ReadItems,
            )
        {
            return Err(DavError::Code(StatusCode::FORBIDDEN));
        }
        let container_operation = ContainerOperation::from_move(is_move);

        // Validate destination
        let destination = self
            .validate_uri_with_status(
                access_token,
                headers
                    .destination
                    .ok_or(DavError::Code(StatusCode::BAD_GATEWAY))?,
                StatusCode::BAD_GATEWAY,
            )
            .await?;
        if destination.collection != Collection::Calendar {
            return Err(DavError::Code(StatusCode::BAD_GATEWAY));
        }
        let to_account_id = destination
            .account_id
            .ok_or(DavError::Code(StatusCode::BAD_GATEWAY))?;
        self.za_refuse_cross_account(from_account_id, to_account_id)
            .await?;
        let is_to_owner = access_token.is_member(to_account_id);
        if !from_resource.is_container()
            && !is_to_owner
            && !EventPrivacy::from_flags(from_resource.resource.event_flags().unwrap_or_default())
                .is_public()
        {
            return Err(DavError::Code(StatusCode::FORBIDDEN));
        }
        let to_resources = if to_account_id == from_account_id {
            from_resources.clone()
        } else {
            self.fetch_groupware_resources(
                access_token.account_id(),
                to_account_id,
                SyncCollection::Calendar,
            )
            .await
            .caused_by(trc::location!())?
        };

        // Validate headers
        let destination_resource_name = destination
            .resource
            .ok_or(DavError::Code(StatusCode::BAD_GATEWAY))?;
        let to_resource = to_resources.by_path(destination_resource_name);
        let visible_to_resource = to_resource.filter(|resource| {
            is_to_owner
                || EventPrivacy::from_flags(resource.resource.event_flags().unwrap_or_default())
                    != EventPrivacy::Secret
        });
        self.validate_headers(
            access_token,
            headers,
            vec![
                ResourceState {
                    account_id: from_account_id,
                    collection: if from_resource.is_container() {
                        Collection::Calendar
                    } else {
                        Collection::CalendarEvent
                    },
                    document_id: Some(from_resource.document_id()),
                    path: from_resource_name,
                    ..Default::default()
                },
                ResourceState {
                    account_id: to_account_id,
                    collection: visible_to_resource
                        .map(|r| {
                            if r.is_container() {
                                Collection::Calendar
                            } else {
                                Collection::CalendarEvent
                            }
                        })
                        .unwrap_or(Collection::Calendar),
                    document_id: Some(
                        visible_to_resource
                            .map(|r| r.document_id())
                            .unwrap_or(u32::MAX),
                    ),
                    path: destination_resource_name,
                    ..Default::default()
                },
            ],
            Default::default(),
            if is_move {
                DavMethod::MOVE
            } else {
                DavMethod::COPY
            },
        )
        .await?;

        // Map destination
        if let Some(to_resource) = to_resource {
            if from_resource.path() == to_resource.path() {
                // Same resource
                return Err(DavError::Code(StatusCode::BAD_GATEWAY));
            }
            let new_name = destination_resource_name
                .rsplit_once('/')
                .map(|(_, name)| name)
                .unwrap_or(destination_resource_name);

            match (from_resource.is_container(), to_resource.is_container()) {
                (true, true) => {
                    let from_children_ids = container_operation.event_ids(
                        &from_resources,
                        from_resource_name,
                        EventViewer::new(is_from_owner),
                    )?;
                    let to_children = to_resources
                        .subtree(destination_resource_name)
                        .filter(|r| !r.is_container())
                        .map(|r| (r.document_id(), r.resource.metadata_kinds()))
                        .collect::<Vec<_>>();

                    // Validate ACLs
                    if !is_to_owner
                        || (!is_from_owner
                            && !from_resources
                                .container_acl(access_token, from_resource.document_id())
                                .contains_all(container_operation.required_acls()))
                    {
                        return Err(DavError::Code(StatusCode::FORBIDDEN));
                    }

                    // Overwrite container
                    copy_container(
                        self,
                        access_token,
                        &from_resources,
                        &to_resources,
                        from_account_id,
                        from_resource.document_id(),
                        from_children_ids,
                        from_resources.format_collection(from_resource_name),
                        to_account_id,
                        to_resource.document_id().into(),
                        to_children,
                        new_name,
                        is_move,
                    )
                    .await
                }
                (false, false) => {
                    // Overwrite event
                    let from_calendar_id = from_resource.parent_id().unwrap();
                    let to_calendar_id = to_resource.parent_id().unwrap();

                    // Validate ACL
                    let mut own_events = OwnEventAccess::new(self, access_token);
                    let may_take = access_token.is_member(from_account_id)
                        || from_resources.has_access_to_container(
                            access_token,
                            from_calendar_id,
                            if is_move {
                                Acl::RemoveItems
                            } else {
                                Acl::ReadItems
                            },
                        )
                        || (is_move
                            && own_events
                                .may_manage(
                                    &from_resources,
                                    from_calendar_id,
                                    from_account_id,
                                    from_resource.document_id(),
                                )
                                .await?);
                    let may_overwrite = may_take
                        && (access_token.is_member(to_account_id)
                            || to_resources
                                .container_acl(access_token, to_calendar_id)
                                .contains_all([Acl::AddItems, Acl::RemoveItems].into_iter())
                            || (own_events
                                .may_manage(
                                    &to_resources,
                                    to_calendar_id,
                                    to_account_id,
                                    to_resource.document_id(),
                                )
                                .await?
                                && own_events
                                    .may_manage(
                                        &to_resources,
                                        to_calendar_id,
                                        from_account_id,
                                        from_resource.document_id(),
                                    )
                                    .await?));
                    if !may_overwrite {
                        return Err(DavError::Code(StatusCode::FORBIDDEN));
                    }
                    assert_event_privacy_access(
                        to_resource.resource.event_flags(),
                        EventViewer::new(is_to_owner),
                    )?;

                    if is_move {
                        move_event(
                            self,
                            access_token,
                            &to_resources,
                            from_account_id,
                            from_resource.document_id(),
                            from_calendar_id,
                            from_resources.format_item(from_resource_name),
                            to_account_id,
                            to_resource.document_id().into(),
                            to_calendar_id,
                            new_name,
                            headers.if_schedule_tag,
                        )
                        .await
                    } else {
                        copy_event(
                            self,
                            access_token,
                            &to_resources,
                            from_account_id,
                            from_resource.document_id(),
                            from_calendar_id,
                            to_account_id,
                            to_resource.document_id().into(),
                            to_calendar_id,
                            new_name,
                        )
                        .await
                    }
                }
                _ => Err(DavError::Code(StatusCode::BAD_GATEWAY)),
            }
        } else if let Some((parent_resource, new_name)) =
            to_resources.map_parent(destination_resource_name)
        {
            if let Some(parent_resource) = parent_resource {
                // Creating items under an event is not allowed
                // Copying/moving containers under a container is not allowed
                if !parent_resource.is_container() || from_resource.is_container() {
                    return Err(DavError::Code(StatusCode::BAD_GATEWAY));
                }

                // Validate ACL
                let from_calendar_id = from_resource.parent_id().unwrap();
                let to_calendar_id = parent_resource.document_id();
                let mut own_events = OwnEventAccess::new(self, access_token);
                let may_take = access_token.is_member(from_account_id)
                    || from_resources.has_access_to_container(
                        access_token,
                        from_calendar_id,
                        if is_move {
                            Acl::RemoveItems
                        } else {
                            Acl::ReadItems
                        },
                    )
                    || (is_move
                        && own_events
                            .may_manage(
                                &from_resources,
                                from_calendar_id,
                                from_account_id,
                                from_resource.document_id(),
                            )
                            .await?);
                let may_add = may_take
                    && (access_token.is_member(to_account_id)
                        || to_resources.has_access_to_container(
                            access_token,
                            to_calendar_id,
                            Acl::AddItems,
                        )
                        || own_events
                            .may_manage(
                                &to_resources,
                                to_calendar_id,
                                from_account_id,
                                from_resource.document_id(),
                            )
                            .await?);
                if !may_add {
                    return Err(DavError::Code(StatusCode::FORBIDDEN));
                }

                // Copy/move event
                if is_move {
                    if from_account_id != to_account_id
                        || parent_resource.document_id() != from_calendar_id
                    {
                        move_event(
                            self,
                            access_token,
                            &to_resources,
                            from_account_id,
                            from_resource.document_id(),
                            from_calendar_id,
                            from_resources.format_item(from_resource_name),
                            to_account_id,
                            None,
                            to_calendar_id,
                            new_name,
                            headers.if_schedule_tag,
                        )
                        .await
                    } else {
                        rename_event(
                            self,
                            access_token,
                            from_account_id,
                            from_resource.document_id(),
                            from_calendar_id,
                            new_name,
                            from_resources.format_item(from_resource_name),
                        )
                        .await
                    }
                } else {
                    copy_event(
                        self,
                        access_token,
                        &to_resources,
                        from_account_id,
                        from_resource.document_id(),
                        from_calendar_id,
                        to_account_id,
                        None,
                        to_calendar_id,
                        new_name,
                    )
                    .await
                }
            } else {
                // Copying/moving events to the root is not allowed
                if !from_resource.is_container() {
                    return Err(DavError::Code(StatusCode::BAD_GATEWAY));
                }

                // Shared users cannot create containers
                if !is_to_owner {
                    return Err(DavError::Code(StatusCode::FORBIDDEN));
                }

                // Validate ACLs
                if !is_from_owner
                    && !from_resources
                        .container_acl(access_token, from_resource.document_id())
                        .contains_all(container_operation.required_acls())
                {
                    return Err(DavError::Code(StatusCode::FORBIDDEN));
                }

                // Copy/move container
                let from_children_ids = container_operation.event_ids(
                    &from_resources,
                    from_resource_name,
                    EventViewer::new(is_from_owner),
                )?;
                if is_move {
                    if from_account_id != to_account_id {
                        copy_container(
                            self,
                            access_token,
                            &from_resources,
                            &to_resources,
                            from_account_id,
                            from_resource.document_id(),
                            if headers.depth != Depth::Zero {
                                from_children_ids
                            } else {
                                return Err(DavError::Code(StatusCode::BAD_GATEWAY));
                            },
                            from_resources.format_collection(from_resource_name),
                            to_account_id,
                            None,
                            vec![],
                            new_name,
                            true,
                        )
                        .await
                    } else {
                        rename_container(
                            self,
                            access_token,
                            from_account_id,
                            from_resource.document_id(),
                            new_name,
                            from_resources.format_collection(from_resource_name),
                        )
                        .await
                    }
                } else {
                    copy_container(
                        self,
                        access_token,
                        &from_resources,
                        &to_resources,
                        from_account_id,
                        from_resource.document_id(),
                        if headers.depth != Depth::Zero {
                            from_children_ids
                        } else {
                            vec![]
                        },
                        from_resources.format_collection(from_resource_name),
                        to_account_id,
                        None,
                        vec![],
                        new_name,
                        false,
                    )
                    .await
                }
            }
        } else {
            Err(DavError::Code(StatusCode::CONFLICT))
        }
    }
}

struct OwnEventAccess<'x> {
    server: &'x Server,
    access_token: &'x AccessToken,
    identities: Option<CalendarAddresses>,
}

impl<'x> OwnEventAccess<'x> {
    fn new(server: &'x Server, access_token: &'x AccessToken) -> Self {
        OwnEventAccess {
            server,
            access_token,
            identities: None,
        }
    }

    async fn may_manage(
        &mut self,
        resources: &GroupwareResources,
        calendar_id: u32,
        account_id: u32,
        document_id: u32,
    ) -> crate::Result<bool> {
        if !EventAcl::for_calendar(resources, self.access_token, calendar_id).may_manage_own_items()
        {
            return Ok(false);
        }
        let identities = match self.identities.as_ref() {
            Some(identities) => identities,
            None => self.identities.get_or_insert(
                self.server
                    .account_identity_addresses(self.access_token.account_id())
                    .await
                    .caused_by(trc::location!())?,
            ),
        };

        Ok(self
            .server
            .stored_event_ownership(account_id, document_id, identities)
            .await
            .caused_by(trc::location!())?
            .may_write_own())
    }
}

#[allow(clippy::too_many_arguments)]
async fn copy_event(
    server: &Server,
    access_token: &AccessToken,
    to_resources: &GroupwareResources,
    from_account_id: u32,
    from_document_id: u32,
    from_calendar_id: u32,
    to_account_id: u32,
    to_document_id: Option<u32>,
    to_calendar_id: u32,
    new_name: &str,
) -> crate::Result<HttpResponse> {
    if let Some(keys) = super::za_writer_keys(server, access_token, from_account_id).await? {
        return za_copy_move_event(
            server,
            access_token,
            &keys,
            from_account_id,
            from_document_id,
            from_calendar_id,
            to_document_id,
            to_calendar_id,
            new_name,
            None,
            None,
            false,
        )
        .await;
    }
    // Fetch event
    let event_ = server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            from_account_id,
            Collection::CalendarEvent,
            from_document_id,
        ))
        .await
        .caused_by(trc::location!())?
        .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
    let event = event_
        .to_unarchived::<CalendarEvent>()
        .caused_by(trc::location!())?;
    let mut batch = BatchBuilder::new();
    crate::common::za::za_ordinary_fence(server, &mut batch, to_account_id, None);

    // Validate UID
    assert_is_unique_uid(
        to_resources,
        access_token,
        to_account_id,
        to_calendar_id,
        Some(event.inner.uid.as_str()).filter(|uid| !uid.is_empty()),
    )?;
    let checked_content = server
        .assert_stored_event_supported(
            from_account_id,
            from_document_id,
            to_account_id,
            to_calendar_id,
        )
        .await?;

    let changed_by = access_token.changed_by();
    let event_flags = event.inner.flags.to_native();
    let mut quota = NotificationQuota::default();
    if from_account_id == to_account_id {
        let mut new_event = event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        new_event.names.push(DavName {
            name: new_name.to_string(),
            parent_id: to_calendar_id,
        });
        server
            .notify_calendar_addition(
                access_token,
                to_account_id,
                PendingId::Assigned(from_document_id),
                to_calendar_id,
                event_flags,
                (from_account_id, from_document_id),
                &mut quota,
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
        assert_parent_limit(
            event.inner.names.len(),
            new_event.names.len(),
            server.core.groupware.max_calendars_per_event,
        )?;
        server
            .reschedule_event_alarms(
                from_account_id,
                from_document_id,
                event.inner,
                None,
                &new_event
                    .names
                    .iter()
                    .map(DavName::parent_id)
                    .collect::<Vec<_>>(),
                &mut AlarmTasks::None,
                &mut DefaultAlertsResolver::default(),
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
        new_event
            .update_meta(
                changed_by,
                event,
                from_account_id,
                from_document_id,
                None,
                &mut batch,
            )
            .caused_by(trc::location!())?;
    } else {
        let content_ = match checked_content {
            Some(content_) => content_,
            None => server
                .store()
                .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                    from_account_id,
                    Collection::CalendarEvent,
                    from_document_id,
                    CalendarEventField::Content,
                ))
                .await
                .caused_by(trc::location!())?
                .ok_or(DavError::Code(StatusCode::NOT_FOUND))?,
        };
        let mut new_content = content_
            .deserialize::<CalendarEventContent>()
            .caused_by(trc::location!())?;
        new_content
            .preferences
            .retain(|preferences| preferences.account_id == access_token.account_id());
        let next_email_alarms = server
            .next_event_alarms(
                to_account_id,
                &EventAlarmUsers::new(to_account_id, &new_content)?
                    .with_event_flags(event.inner.flags.to_native()),
                &new_content.data,
                &[to_calendar_id],
                now() as i64,
                &mut DefaultAlertsResolver::default(),
            )
            .await
            .caused_by(trc::location!())?;
        let mut new_event = event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        new_event.names = vec![DavName {
            name: new_name.to_string(),
            parent_id: to_calendar_id,
        }];
        let container = read_container(
            server,
            from_account_id,
            Collection::CalendarEvent,
            from_document_id,
            event.inner.metadata_kinds(),
        )
        .await
        .caused_by(trc::location!())?;
        check_growth(
            server,
            to_account_id,
            container
                .as_ref()
                .map_or(0, |container| copy_growth(container, None)),
        )
        .await
        .caused_by(trc::location!())?;
        new_event.set_metadata_kinds(container_kinds(container.as_ref()));
        if to_document_id.is_none() {
            server.assert_object_quota(
                &*server.account(to_account_id).await?,
                StorageQuota::MaxCalendarEvents,
                1,
                || to_resources.resources.count(false),
            )?;
        }
        let to_document_id = batch.reserve_document_id(to_account_id, Collection::CalendarEvent);
        server
            .notify_calendar_addition(
                access_token,
                to_account_id,
                to_document_id.into(),
                to_calendar_id,
                event_flags,
                (from_account_id, from_document_id),
                &mut quota,
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
        if let Some(container) = &container {
            let mut copies = ContainerWrites::new(to_account_id, Collection::CalendarEvent);
            copies
                .copy(server, container, to_document_id, None, &mut batch)
                .await
                .caused_by(trc::location!())?;
        }
        new_event
            .insert(
                new_content,
                changed_by,
                to_account_id,
                to_document_id,
                None,
                next_email_alarms,
                &mut batch,
            )
            .caused_by(trc::location!())?;
    }

    let response = if let Some(to_document_id) = to_document_id {
        // Overwrite event on destination
        let event_ = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                to_account_id,
                Collection::CalendarEvent,
                to_document_id,
            ))
            .await
            .caused_by(trc::location!())?;
        if let Some(event_) = event_ {
            let event = event_
                .to_unarchived::<CalendarEvent>()
                .caused_by(trc::location!())?;
            let account_info = server
                .account_info(access_token.account_id())
                .await
                .caused_by(trc::location!())?;

            server
                .notify_calendar_removal(
                    access_token,
                    to_account_id,
                    to_document_id,
                    to_calendar_id,
                    event.inner.flags.to_native(),
                    &mut quota,
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
            DestroyArchive(event)
                .delete(
                    server,
                    &account_info,
                    to_account_id,
                    to_document_id,
                    to_calendar_id,
                    None,
                    None,
                    false,
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
        }

        Ok(HttpResponse::new(StatusCode::NO_CONTENT))
    } else {
        Ok(HttpResponse::new(StatusCode::CREATED))
    };

    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;

    response
}

#[allow(clippy::too_many_arguments)]
async fn move_event(
    server: &Server,
    access_token: &AccessToken,
    to_resources: &GroupwareResources,
    from_account_id: u32,
    from_document_id: u32,
    from_calendar_id: u32,
    from_resource_path: String,
    to_account_id: u32,
    to_document_id: Option<u32>,
    to_calendar_id: u32,
    new_name: &str,
    if_schedule_tag: Option<u32>,
) -> crate::Result<HttpResponse> {
    if let Some(keys) = super::za_writer_keys(server, access_token, from_account_id).await? {
        return za_copy_move_event(
            server,
            access_token,
            &keys,
            from_account_id,
            from_document_id,
            from_calendar_id,
            to_document_id,
            to_calendar_id,
            new_name,
            Some(from_resource_path),
            if_schedule_tag,
            true,
        )
        .await;
    }
    // Fetch event
    let event_ = server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            from_account_id,
            Collection::CalendarEvent,
            from_document_id,
        ))
        .await
        .caused_by(trc::location!())?
        .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
    let event = event_
        .to_unarchived::<CalendarEvent>()
        .caused_by(trc::location!())?;

    // Validate headers
    if if_schedule_tag.is_some()
        && event.inner.schedule_tag.as_ref().map(|t| t.to_native()) != if_schedule_tag
    {
        return Err(DavError::Code(StatusCode::PRECONDITION_FAILED));
    }

    // Validate UID
    if from_account_id != to_account_id
        || from_calendar_id != to_calendar_id
        || to_document_id.is_none()
    {
        assert_is_unique_uid(
            to_resources,
            access_token,
            to_account_id,
            to_calendar_id,
            Some(event.inner.uid.as_str()).filter(|uid| !uid.is_empty()),
        )?;
    }
    let checked_content = if from_account_id != to_account_id || from_calendar_id != to_calendar_id
    {
        server
            .assert_stored_event_supported(
                from_account_id,
                from_document_id,
                to_account_id,
                to_calendar_id,
            )
            .await?
    } else {
        None
    };

    let account_info = server
        .account_info(access_token.account_id())
        .await
        .caused_by(trc::location!())?;
    let mut batch = BatchBuilder::new();
    crate::common::za::za_ordinary_fence(server, &mut batch, to_account_id, Some(from_account_id));
    let event_flags = event.inner.flags.to_native();
    let mut quota = NotificationQuota::default();
    if from_calendar_id != to_calendar_id || from_account_id != to_account_id {
        server
            .notify_calendar_removal(
                access_token,
                from_account_id,
                from_document_id,
                from_calendar_id,
                event_flags,
                &mut quota,
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
    }
    if from_account_id == to_account_id {
        let name_idx = event
            .inner
            .names
            .iter()
            .position(|name| name.parent_id == from_calendar_id)
            .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;

        let mut new_event = event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        new_event.names.swap_remove(name_idx);
        new_event.names.push(DavName {
            name: new_name.to_string(),
            parent_id: to_calendar_id,
        });
        if from_calendar_id != to_calendar_id {
            server
                .notify_calendar_addition(
                    access_token,
                    to_account_id,
                    PendingId::Assigned(from_document_id),
                    to_calendar_id,
                    event_flags,
                    (from_account_id, from_document_id),
                    &mut quota,
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
        }
        server
            .reschedule_event_alarms(
                from_account_id,
                from_document_id,
                event.inner,
                None,
                &new_event
                    .names
                    .iter()
                    .map(DavName::parent_id)
                    .collect::<Vec<_>>(),
                &mut AlarmTasks::None,
                &mut DefaultAlertsResolver::default(),
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
        new_event
            .update_meta(
                access_token.changed_by(),
                event.clone(),
                from_account_id,
                from_document_id,
                None,
                &mut batch,
            )
            .caused_by(trc::location!())?;
        batch.log_calendar_vanished(from_calendar_id, from_resource_path);
    } else {
        let content_ = match checked_content {
            Some(content_) => content_,
            None => server
                .store()
                .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                    from_account_id,
                    Collection::CalendarEvent,
                    from_document_id,
                    CalendarEventField::Content,
                ))
                .await
                .caused_by(trc::location!())?
                .ok_or(DavError::Code(StatusCode::NOT_FOUND))?,
        };
        let mut new_content = content_
            .deserialize::<CalendarEventContent>()
            .caused_by(trc::location!())?;
        new_content
            .preferences
            .retain(|preferences| preferences.account_id == access_token.account_id());
        let next_email_alarms = server
            .next_event_alarms(
                to_account_id,
                &EventAlarmUsers::new(to_account_id, &new_content)?
                    .with_event_flags(event.inner.flags.to_native()),
                &new_content.data,
                &[to_calendar_id],
                now() as i64,
                &mut DefaultAlertsResolver::default(),
            )
            .await
            .caused_by(trc::location!())?;
        let mut new_event = event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        new_event.names = vec![DavName {
            name: new_name.to_string(),
            parent_id: to_calendar_id,
        }];
        let container = read_container(
            server,
            from_account_id,
            Collection::CalendarEvent,
            from_document_id,
            event.inner.metadata_kinds(),
        )
        .await
        .caused_by(trc::location!())?;
        check_growth(
            server,
            to_account_id,
            container
                .as_ref()
                .map_or(0, |container| copy_growth(container, None)),
        )
        .await
        .caused_by(trc::location!())?;
        new_event.set_metadata_kinds(container_kinds(container.as_ref()));

        let source_cleanup = server
            .container_cleanup_from(
                access_token.changed_by(),
                from_account_id,
                Collection::CalendarEvent,
                container
                    .as_ref()
                    .map(|container| StoredEntry::from_container(from_document_id, container)),
            )
            .await
            .caused_by(trc::location!())?;
        DestroyArchive(event)
            .delete_with_cleanup(
                server,
                &account_info,
                from_account_id,
                from_document_id,
                from_calendar_id,
                Some(content_),
                &mut AlarmTasks::None,
                from_resource_path.into(),
                &source_cleanup,
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;

        let to_account = server.account(to_account_id).await?;
        if to_document_id.is_none()
            && server
                .object_quota_limit(&to_account, StorageQuota::MaxCalendarEvents)
                .is_some()
        {
            server.assert_object_quota(&to_account, StorageQuota::MaxCalendarEvents, 1, || {
                to_resources.resources.count(false)
            })?;
        }

        let to_document_id = batch.reserve_document_id(to_account_id, Collection::CalendarEvent);
        server
            .notify_calendar_addition(
                access_token,
                to_account_id,
                to_document_id.into(),
                to_calendar_id,
                event_flags,
                (from_account_id, from_document_id),
                &mut quota,
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
        if let Some(container) = &container {
            let mut copies = ContainerWrites::new(to_account_id, Collection::CalendarEvent);
            copies
                .copy(server, container, to_document_id, None, &mut batch)
                .await
                .caused_by(trc::location!())?;
        }
        new_event
            .insert(
                new_content,
                access_token.changed_by(),
                to_account_id,
                to_document_id,
                None,
                next_email_alarms,
                &mut batch,
            )
            .caused_by(trc::location!())?;
    }

    let response = if let Some(to_document_id) = to_document_id {
        // Overwrite event on destination
        let event_ = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                to_account_id,
                Collection::CalendarEvent,
                to_document_id,
            ))
            .await
            .caused_by(trc::location!())?;
        if let Some(event_) = event_ {
            let event = event_
                .to_unarchived::<CalendarEvent>()
                .caused_by(trc::location!())?;

            server
                .notify_calendar_removal(
                    access_token,
                    to_account_id,
                    to_document_id,
                    to_calendar_id,
                    event.inner.flags.to_native(),
                    &mut quota,
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
            DestroyArchive(event)
                .delete(
                    server,
                    &account_info,
                    to_account_id,
                    to_document_id,
                    to_calendar_id,
                    None,
                    None,
                    false,
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
        }

        Ok(HttpResponse::new(StatusCode::NO_CONTENT))
    } else {
        Ok(HttpResponse::new(StatusCode::CREATED))
    };

    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;

    response
}

#[allow(clippy::too_many_arguments)]
async fn rename_event(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    document_id: u32,
    calendar_id: u32,
    new_name: &str,
    from_resource_path: String,
) -> crate::Result<HttpResponse> {
    if let Some(keys) = super::za_writer_keys(server, access_token, account_id).await? {
        return za_copy_move_event(
            server,
            access_token,
            &keys,
            account_id,
            document_id,
            calendar_id,
            None,
            calendar_id,
            new_name,
            Some(from_resource_path),
            None,
            true,
        )
        .await;
    }
    // Fetch event
    let event_ = server
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

    let name_idx = event
        .inner
        .names
        .iter()
        .position(|n| n.parent_id == calendar_id)
        .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
    let mut new_event = event
        .deserialize::<CalendarEvent>()
        .caused_by(trc::location!())?;
    new_event.names[name_idx].name = new_name.to_string();

    let mut batch = BatchBuilder::new();
    crate::common::za::za_ordinary_fence(server, &mut batch, account_id, None);
    new_event
        .update_meta(
            access_token.changed_by(),
            event,
            account_id,
            document_id,
            None,
            &mut batch,
        )
        .caused_by(trc::location!())?;
    batch.log_calendar_vanished(calendar_id, from_resource_path);
    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;

    Ok(HttpResponse::new(StatusCode::CREATED))
}

#[allow(clippy::too_many_arguments)]
async fn copy_container(
    server: &Server,
    access_token: &AccessToken,
    from_resources: &GroupwareResources,
    to_resources: &GroupwareResources,
    from_account_id: u32,
    from_document_id: u32,
    from_children_ids: Vec<u32>,
    from_resource_path: String,
    to_account_id: u32,
    to_document_id: Option<u32>,
    to_children: Vec<(u32, MetadataKinds)>,
    new_name: &str,
    remove_source: bool,
) -> crate::Result<HttpResponse> {
    if let Some(keys) = super::za_writer_keys(server, access_token, from_account_id).await? {
        return za_copy_container(
            server,
            access_token,
            &keys,
            from_account_id,
            from_document_id,
            from_children_ids,
            from_resource_path,
            to_document_id,
            to_children
                .into_iter()
                .map(|(document_id, _)| document_id)
                .collect(),
            new_name,
            remove_source,
        )
        .await;
    }
    // Fetch calendar
    let calendar_ = server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            from_account_id,
            Collection::Calendar,
            from_document_id,
        ))
        .await
        .caused_by(trc::location!())?
        .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
    let old_calendar = calendar_
        .to_unarchived::<Calendar>()
        .caused_by(trc::location!())?;
    let mut calendar = old_calendar
        .deserialize::<Calendar>()
        .caused_by(trc::location!())?;
    let calendar_container = read_container(
        server,
        from_account_id,
        Collection::Calendar,
        from_document_id,
        old_calendar.inner.metadata_kinds(),
    )
    .await
    .caused_by(trc::location!())?;
    calendar.set_metadata_kinds(container_kinds(calendar_container.as_ref()));
    let event_containers = if from_account_id != to_account_id {
        let mut request = ContainerRequest::default();
        for &document_id in &from_children_ids {
            if from_resources
                .item_by_id(document_id)
                .is_some_and(|item| !item.metadata_kinds().is_empty())
            {
                request.insert(from_account_id, Collection::CalendarEvent, document_id);
            }
        }
        request.load(server).await.caused_by(trc::location!())?
    } else {
        DeadContainers::default()
    };
    let mut event_copies = ContainerWrites::new(to_account_id, Collection::CalendarEvent);
    let source_cleanup = server
        .container_cleanup_from(
            access_token.changed_by(),
            from_account_id,
            Collection::CalendarEvent,
            event_containers
                .stored_entries(from_account_id, Collection::CalendarEvent)
                .filter(|_| remove_source),
        )
        .await
        .caused_by(trc::location!())?;
    check_growth(
        server,
        to_account_id,
        calendar_container
            .as_ref()
            .map_or(0, |container| copy_growth(container, None))
            + event_containers.stored_len(),
    )
    .await
    .caused_by(trc::location!())?;

    // Validate quota
    let to_account = server.account(to_account_id).await?;
    let has_container_quota = to_document_id.is_none()
        && server
            .object_quota_limit(&to_account, StorageQuota::MaxCalendars)
            .is_some();
    let has_item_quota = from_account_id != to_account_id
        && server
            .object_quota_limit(&to_account, StorageQuota::MaxCalendarEvents)
            .is_some();
    let to_alarm_ids = if to_document_id.is_some() {
        to_resources.event_ids_with_flags(EVENT_HAS_ALARMS)
    } else {
        RoaringBitmap::new()
    };
    let to_resources = (has_container_quota || has_item_quota).then_some(to_resources);
    if has_container_quota && let Some(to_resources) = &to_resources {
        server.assert_object_quota(&to_account, StorageQuota::MaxCalendars, 1, || {
            to_resources.resources.count(true)
        })?;
    }
    let deleted_events = match &to_resources {
        Some(to_resources) if has_item_quota => to_children
            .iter()
            .filter(|(document_id, _)| {
                to_resources
                    .resources
                    .find(*document_id, false)
                    .is_some_and(|item| item.child_names().len() <= 1)
            })
            .count(),
        _ => 0,
    };

    // Prepare write batch
    let mut batch = BatchBuilder::new();
    crate::common::za::za_ordinary_fence(
        server,
        &mut batch,
        to_account_id,
        remove_source.then_some(from_account_id),
    );

    let personal_id = access_token.personal_id(from_account_id, Collection::Calendar);
    let preference = calendar
        .preferences
        .iter()
        .position(|preferences| preferences.account_id == personal_id)
        .or_else(|| {
            calendar
                .preferences
                .iter()
                .position(|preferences| preferences.account_id == from_account_id)
        })
        .map(|idx| calendar.preferences.swap_remove(idx))
        .unwrap_or_default();
    let mut default_alerts = DefaultAlertsResolver::default();
    calendar.name = new_name.to_string();
    calendar.acls.clear();
    calendar.preferences = vec![CalendarPreferences {
        account_id: to_account_id,
        name: preference.name,
        description: preference.description,
        default_alerts: preference.default_alerts,
        sort_order: 0,
        color: preference.color,
        flags: CALENDAR_SUBSCRIBED,
        time_zone: Timezone::Default,
    }];

    let account_info = server
        .account_info(access_token.account_id())
        .await
        .caused_by(trc::location!())?;
    let is_overwrite = to_document_id.is_some();
    let to_document_id = if let Some(to_document_id) = to_document_id {
        // Overwrite destination
        let calendar_ = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                to_account_id,
                Collection::Calendar,
                to_document_id,
            ))
            .await
            .caused_by(trc::location!())?;
        if let Some(calendar_) = calendar_ {
            let calendar = calendar_
                .to_unarchived::<Calendar>()
                .caused_by(trc::location!())?;
            let cleanup = server
                .preload_container_cleanup(
                    access_token.changed_by(),
                    to_account_id,
                    Collection::CalendarEvent,
                    &to_children
                        .iter()
                        .filter(|(_, kinds)| !kinds.is_empty())
                        .map(|(document_id, _)| *document_id)
                        .collect(),
                )
                .await
                .caused_by(trc::location!())?;

            DestroyArchive(calendar)
                .delete_with_events(
                    server,
                    access_token,
                    &account_info,
                    to_account_id,
                    to_document_id,
                    to_children
                        .into_iter()
                        .map(|(document_id, _)| document_id)
                        .collect(),
                    &to_alarm_ids,
                    &cleanup,
                    None,
                    false,
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
        }

        PendingId::Assigned(to_document_id)
    } else {
        PendingId::Slot(batch.reserve_document_id(to_account_id, Collection::Calendar))
    };
    let parent_id = to_document_id;
    let alarm_calendar_id = parent_id.assigned().unwrap_or(u32::MAX);
    default_alerts.set_calendar_settings(
        to_account_id,
        alarm_calendar_id,
        CalendarSettings::from(&calendar),
    );
    if let Some(container) = &calendar_container {
        let mut copies = ContainerWrites::new(to_account_id, Collection::Calendar);
        copies
            .copy(server, container, to_document_id, None, &mut batch)
            .await
            .caused_by(trc::location!())?;
    }
    calendar
        .insert(
            access_token.changed_by(),
            to_account_id,
            to_document_id,
            &mut batch,
        )
        .caused_by(trc::location!())?;

    // Copy children
    let mut required_space = 0;
    let mut created_events = 0;
    let mut from_children_ids = from_children_ids;
    from_children_ids.sort_unstable();
    from_children_ids.dedup();
    let is_cross_account = from_account_id != to_account_id;
    let alarm_ids = from_resources.event_ids_with_flags(EVENT_HAS_ALARMS);
    let mut content_ids = Vec::new();
    for chunk in from_children_ids.chunks(MAX_BATCH_DOCUMENTS) {
        content_ids.clear();
        content_ids.extend(
            chunk
                .iter()
                .copied()
                .filter(|document_id| is_cross_account || alarm_ids.contains(*document_id)),
        );
        let mut archives = server
            .archive_chunk(
                from_account_id,
                Collection::CalendarEvent,
                chunk,
                Some((CalendarEventField::Content.field(), &content_ids)),
            )
            .await
            .caused_by(trc::location!())?;
        let mut tasks = if !is_cross_account || remove_source {
            AlarmTasks::for_chunk(from_account_id, chunk, &alarm_ids, &archives)?
        } else {
            AlarmTasks::None
        };
        for &from_child_document_id in chunk {
            if let Some(event_) = archives.take_archive(from_child_document_id) {
                let event = event_
                    .to_unarchived::<CalendarEvent>()
                    .caused_by(trc::location!())?;
                let mut new_name = None;

                for name in event.inner.names.iter() {
                    if parent_id
                        .assigned()
                        .is_some_and(|parent| name.parent_id == parent)
                    {
                        continue;
                    } else if name.parent_id == from_document_id {
                        new_name = Some(name.name.to_string());
                    }
                }
                let new_name = if let Some(new_name) = new_name {
                    DavName {
                        name: new_name,
                        parent_id: parent_id.assigned().unwrap_or_default(),
                    }
                } else {
                    continue;
                };
                let mut new_event = event
                    .deserialize::<CalendarEvent>()
                    .caused_by(trc::location!())?;

                if from_account_id == to_account_id {
                    if remove_source {
                        new_event
                            .names
                            .retain(|name| name.parent_id != from_document_id);
                    }

                    let calendar_ids = new_event
                        .names
                        .iter()
                        .map(DavName::parent_id)
                        .chain([alarm_calendar_id])
                        .collect::<Vec<_>>();
                    new_event.names.push(new_name);
                    assert_parent_limit(
                        event.inner.names.len(),
                        new_event.names.len(),
                        server.core.groupware.max_calendars_per_event,
                    )?;
                    server
                        .reschedule_event_alarms(
                            from_account_id,
                            from_child_document_id,
                            event.inner,
                            archives.take_value(from_child_document_id),
                            &calendar_ids,
                            &mut tasks,
                            &mut default_alerts,
                            &mut batch,
                        )
                        .await
                        .caused_by(trc::location!())?;
                    new_event
                        .update_meta(
                            access_token.changed_by(),
                            event,
                            from_account_id,
                            from_child_document_id,
                            parent_id.slot(),
                            &mut batch,
                        )
                        .caused_by(trc::location!())?;
                } else {
                    let content_ = archives
                        .take_value(from_child_document_id)
                        .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
                    let mut new_content = content_
                        .deserialize::<CalendarEventContent>()
                        .caused_by(trc::location!())?;
                    new_content
                        .preferences
                        .retain(|preferences| preferences.account_id == access_token.account_id());
                    let next_email_alarms = server
                        .next_event_alarms(
                            to_account_id,
                            &EventAlarmUsers::new(to_account_id, &new_content)?
                                .with_event_flags(event.inner.flags.to_native()),
                            &new_content.data,
                            &[alarm_calendar_id],
                            now() as i64,
                            &mut default_alerts,
                        )
                        .await
                        .caused_by(trc::location!())?;

                    if remove_source {
                        DestroyArchive(event)
                            .delete_with_cleanup(
                                server,
                                &account_info,
                                from_account_id,
                                from_child_document_id,
                                from_document_id,
                                Some(content_),
                                &mut tasks,
                                None,
                                &source_cleanup,
                                &mut batch,
                            )
                            .await
                            .caused_by(trc::location!())?;
                    }
                    let to_document_id =
                        batch.reserve_document_id(to_account_id, Collection::CalendarEvent);
                    let container = event_containers.get(
                        from_account_id,
                        Collection::CalendarEvent,
                        from_child_document_id,
                    );
                    new_event.set_metadata_kinds(container_kinds(container));
                    if let Some(container) = container {
                        event_copies
                            .copy(server, container, to_document_id, None, &mut batch)
                            .await
                            .caused_by(trc::location!())?;
                    }
                    new_event.names = vec![new_name];
                    required_space += new_event.size as u64;
                    created_events += 1;
                    new_event
                        .insert(
                            new_content,
                            access_token.changed_by(),
                            to_account_id,
                            to_document_id,
                            parent_id.slot(),
                            next_email_alarms,
                            &mut batch,
                        )
                        .caused_by(trc::location!())?;
                }
            }
        }
    }

    if has_item_quota
        && created_events > deleted_events
        && let Some(to_resources) = &to_resources
    {
        server.assert_object_quota(
            &to_account,
            StorageQuota::MaxCalendarEvents,
            created_events - deleted_events,
            || to_resources.resources.count(false),
        )?;
    }

    if from_account_id != to_account_id && required_space > 0 {
        server
            .has_available_quota(&to_account, required_space)
            .await?;
    }

    if remove_source {
        let calendar_cleanup = server
            .container_cleanup_from(
                access_token.changed_by(),
                from_account_id,
                Collection::Calendar,
                calendar_container
                    .as_ref()
                    .map(|container| StoredEntry::from_container(from_document_id, container)),
            )
            .await
            .caused_by(trc::location!())?;
        DestroyArchive(old_calendar)
            .delete_with_cleanup(
                server,
                access_token.changed_by(),
                from_account_id,
                from_document_id,
                from_resource_path.into(),
                &calendar_cleanup,
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;

        batch
            .with_account_id(from_account_id)
            .with_collection(Collection::Principal)
            .with_document(0)
            .clear_if_equals(PrincipalField::DefaultCalendarId, from_document_id);
    }

    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;

    if !is_overwrite {
        Ok(HttpResponse::new(StatusCode::CREATED))
    } else {
        Ok(HttpResponse::new(StatusCode::NO_CONTENT))
    }
}

#[allow(clippy::too_many_arguments)]
async fn rename_container(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    document_id: u32,
    new_name: &str,
    from_resource_path: String,
) -> crate::Result<HttpResponse> {
    let context = crate::common::za::za_mutation_context(
        server,
        access_token,
        account_id,
        document_id,
        utils::map::bitmap::Bitmap::new().with_item(Acl::Modify),
    )
    .await
    .map_err(|error| super::za_calendar_error(error, account_id, document_id))?;
    // Fetch calendar
    let calendar_ = server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
            account_id,
            Collection::Calendar,
            document_id,
        ))
        .await
        .caused_by(trc::location!())?
        .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
    let calendar_ = context
        .as_ref()
        .map(|context| context.stored.clone())
        .unwrap_or(calendar_);
    let calendar = calendar_
        .to_unarchived::<Calendar>()
        .caused_by(trc::location!())?;
    let mut new_calendar = calendar
        .deserialize::<Calendar>()
        .caused_by(trc::location!())?;
    new_calendar.name = new_name.to_string();
    let shared = context
        .as_ref()
        .is_some_and(|context| context.domain.calendar_id().is_some());
    if shared {
        groupware::calendar::sharing::boundary::mark_boundary_pending(
            &mut new_calendar,
            &context.as_ref().unwrap().domain,
        )
        .map_err(|error| groupware::calendar::seal::seal_error(error, account_id, document_id))?;
    }

    let mut batch = BatchBuilder::new();
    if context.is_none() {
        crate::common::za::za_ordinary_fence(server, &mut batch, account_id, None);
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
    batch.log_calendar_vanished(document_id, from_resource_path);
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::share_rename_pause_point(account_id).await;
    let assigned = server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;
    if shared {
        groupware::calendar::sharing::boundary::za_complete_boundary(
            server,
            access_token
                .session_keys()
                .ok_or(DavError::Code(StatusCode::FORBIDDEN))?,
            document_id,
            Some(groupware::calendar::sharing::boundary::ZaBoundaryCommit {
                change_id: assigned.last_change_id(account_id, SyncCollection::Calendar),
            }),
        )
        .await
        .map_err(|error| super::za_calendar_error(error, account_id, document_id))?;
    }

    Ok(HttpResponse::new(StatusCode::CREATED))
}

fn shared_copy_refusal(owner_id: u32) -> DavError {
    trc::event!(
        Security(trc::SecurityEvent::Unauthorized),
        AccountId = owner_id,
        Details = "zero-access: copy into or out of a shared calendar"
    );
    DavError::Code(StatusCode::FORBIDDEN)
}
fn linked_move_refusal(owner_id: u32) -> DavError {
    trc::event!(
        Security(trc::SecurityEvent::Unauthorized),
        AccountId = owner_id,
        Details = "zero-access: event linked into a shared calendar"
    );
    DavError::Code(StatusCode::FORBIDDEN)
}

/// COPY has an operation-specific 403 contract. Do not pass it through the
/// seal/write resolver helper, whose shared-linked refusal is a PUT's 409.
#[allow(clippy::too_many_arguments)]
async fn za_move_contexts(
    server: &Server,
    token: &AccessToken,
    owner: u32,
    event: &CalendarEvent,
    source: u32,
    target: u32,
    is_move: bool,
    overwrite: bool,
    is_collection: bool,
) -> crate::Result<Vec<ZaCalendarContext>> {
    let source_required = Bitmap::new().with_item(if is_move {
        Acl::RemoveItems
    } else {
        Acl::ReadItems
    });
    let mut contexts = Vec::new();
    let source_context = za_mutation_context(server, token, owner, source, source_required)
        .await
        .map_err(|error| super::za_calendar_error(error, owner, source))?
        .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
    if !event.names.iter().any(|name| name.parent_id == source) {
        return Err(DavError::Code(StatusCode::NOT_FOUND));
    }
    if source_context.domain.calendar_id().is_some() {
        if !is_move || is_collection {
            return Err(shared_copy_refusal(owner));
        }
        if event.names.len() != 1 {
            return Err(linked_move_refusal(owner));
        }
    }
    contexts.push(source_context);
    for parent in event
        .names
        .iter()
        .map(|name| name.parent_id)
        .collect::<BTreeSet<_>>()
    {
        if parent == source {
            continue;
        }
        let context = za_mutation_context(server, token, owner, parent, source_required)
            .await
            .map_err(|error| super::za_calendar_error(error, owner, parent))?
            .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
        if context.domain.calendar_id().is_some() {
            return Err(if is_move && !is_collection {
                linked_move_refusal(owner)
            } else {
                shared_copy_refusal(owner)
            });
        }
        contexts.push(context);
    }
    let mut required = Bitmap::new().with_item(Acl::AddItems);
    if overwrite {
        required = required.with_item(Acl::RemoveItems);
    }
    let target_context = za_mutation_context(server, token, owner, target, required)
        .await
        .map_err(|error| super::za_calendar_error(error, owner, target))?
        .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
    if target_context.domain.calendar_id().is_some() {
        if !is_move || is_collection {
            return Err(shared_copy_refusal(owner));
        }
        if event.names.len() != 1 {
            return Err(linked_move_refusal(owner));
        }
    }
    // Keep a separate target entry even for a same-calendar rename: its operation
    // bits were freshly checked and callers have a stable destination position.
    contexts.push(target_context);
    Ok(contexts)
}

/// Raw surviving-parent fences carry no authority or collection keys: unlinking
/// the addressed destination does not change the other parent's link or payload.
struct ZaOverwriteFence {
    calendar_id: u32,
    stored: Option<Archive<ArchiveBytes>>,
}

impl ZaOverwriteFence {
    fn assert_in(&self, owner: u32, batch: &mut BatchBuilder) {
        batch
            .with_account_id(owner)
            .with_collection(Collection::Calendar)
            .with_document(self.calendar_id);
        if let Some(stored) = &self.stored {
            batch.assert_value(Field::ARCHIVE, stored);
        } else {
            batch.assert_value(Field::ARCHIVE, ());
        }
    }
}

fn overwrite_calendars_changed(before: &[ZaOverwriteFence], after: &[ZaOverwriteFence]) -> bool {
    before.len() != after.len()
        || before.iter().zip(after).any(|(before, after)| {
            before.calendar_id != after.calendar_id
                || before.stored.as_ref().map(|stored| stored.version)
                    != after.stored.as_ref().map(|stored| stored.version)
        })
}

/// Authorize the addressed destination first. Surviving parents need fresh raw
/// archive (or absence) fences, without requiring their keys or operation bits.
async fn za_overwrite_fences(
    server: &Server,
    token: &AccessToken,
    owner: u32,
    event: &CalendarEvent,
    target: u32,
) -> crate::Result<Vec<ZaOverwriteFence>> {
    let target_context = za_mutation_context(
        server,
        token,
        owner,
        target,
        Bitmap::new().with_item(Acl::RemoveItems),
    )
    .await
    .map_err(|error| super::za_calendar_error(error, owner, target))?
    .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
    let mut fences = vec![ZaOverwriteFence {
        calendar_id: target,
        stored: Some(target_context.stored),
    }];
    for parent in event
        .names
        .iter()
        .map(|name| name.parent_id)
        .filter(|parent| *parent != target)
        .collect::<BTreeSet<_>>()
    {
        fences.push(ZaOverwriteFence {
            calendar_id: parent,
            stored: server
                .store()
                .get_value(ValueKey::archive(owner, Collection::Calendar, parent))
                .await
                .caused_by(trc::location!())?,
        });
    }
    Ok(fences)
}

/// The stored content archive of an event (upgrade: the tree lives outside
/// the metadata row at 1.0).
async fn za_event_content(
    server: &Server,
    owner: u32,
    event_id: u32,
) -> crate::Result<Archive<ArchiveBytes>> {
    server
        .store()
        .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
            owner,
            Collection::CalendarEvent,
            event_id,
            CalendarEventField::Content,
        ))
        .await
        .caused_by(trc::location!())?
        .ok_or(DavError::Code(StatusCode::NOT_FOUND))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn za_copy_move_event(
    server: &Server,
    token: &AccessToken,
    keys: &vault::session::SessionKeys,
    owner: u32,
    event_id: u32,
    source: u32,
    overwrite: Option<u32>,
    target: u32,
    new_name: &str,
    vanished: Option<String>,
    schedule_tag: Option<u32>,
    is_move: bool,
) -> crate::Result<HttpResponse> {
    let original: Archive<ArchiveBytes> = server
        .store()
        .get_value(ValueKey::archive(
            owner,
            Collection::CalendarEvent,
            event_id,
        ))
        .await
        .caused_by(trc::location!())?
        .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
    za_copy_move_stored_event(
        server,
        token,
        keys,
        owner,
        event_id,
        source,
        overwrite,
        target,
        new_name,
        vanished,
        schedule_tag,
        is_move,
        false,
        original,
    )
    .await
}

/// A key account's event COPY (a new link to the same document) or MOVE
/// (its link moved; across key domains, the event's envelope and its dead
/// property container rewrapped into the target's, ruling 29). Every
/// calendar involved is asserted in the write, which retries once after a
/// calendar change.
#[allow(clippy::too_many_arguments)]
async fn za_copy_move_stored_event(
    server: &Server,
    token: &AccessToken,
    _keys: &vault::session::SessionKeys,
    owner: u32,
    event_id: u32,
    source: u32,
    overwrite: Option<u32>,
    target: u32,
    new_name: &str,
    vanished: Option<String>,
    schedule_tag: Option<u32>,
    is_move: bool,
    is_collection: bool,
    original: Archive<ArchiveBytes>,
) -> crate::Result<HttpResponse> {
    let event = original
        .deserialize::<CalendarEvent>()
        .caused_by(trc::location!())?;
    if schedule_tag.is_some() && event.schedule_tag != schedule_tag {
        return Err(DavError::Code(StatusCode::PRECONDITION_FAILED));
    }
    let mut contexts = za_move_contexts(
        server,
        token,
        owner,
        &event,
        source,
        target,
        is_move,
        overwrite.is_some(),
        is_collection,
    )
    .await?;
    let content_ = za_event_content(server, owner, event_id).await?;
    let content = content_
        .deserialize::<CalendarEventContent>()
        .caused_by(trc::location!())?;
    classify_event_storage(&content).map_err(|error| seal_error(error, owner, event_id))?;
    if source != target || (!is_move && overwrite.is_none()) {
        assert_is_unique_uid(
            server
                .fetch_groupware_resources(token.account_id(), owner, SyncCollection::Calendar)
                .await
                .caused_by(trc::location!())?
                .as_ref(),
            token,
            owner,
            target,
            Some(event.uid.as_str()).filter(|uid| !uid.is_empty()),
        )?;
    }
    if source != target {
        server
            .assert_stored_event_supported(owner, event_id, owner, target)
            .await?;
    }
    let names_after = event.names.len() + usize::from(!is_move);
    assert_parent_limit(
        event.names.len(),
        names_after,
        server.core.groupware.max_calendars_per_event,
    )?;
    // The event's dead-property container (ruling 29): it follows the event
    // into the target's key domain.
    let container = read_container(
        server,
        owner,
        Collection::CalendarEvent,
        event_id,
        event.metadata_kinds(),
    )
    .await
    .caused_by(trc::location!())?;
    let overwritten: Option<Archive<ArchiveBytes>> = if let Some(id) = overwrite {
        server
            .store()
            .get_value(ValueKey::archive(owner, Collection::CalendarEvent, id))
            .await
            .caused_by(trc::location!())?
    } else {
        None
    };
    let mut overwrite_fences = if let Some(stored) = &overwritten {
        za_overwrite_fences(
            server,
            token,
            owner,
            &stored
                .deserialize::<CalendarEvent>()
                .caused_by(trc::location!())?,
            target,
        )
        .await?
    } else {
        Vec::new()
    };
    let account = server
        .account_info(token.account_id())
        .await
        .caused_by(trc::location!())?;
    let cutoff = now() as i64;
    for attempt in 0..2 {
        let mut changed = ZaEvent::new(event.clone(), content.clone());
        let from = &contexts[0].domain;
        let to = &contexts.last().unwrap().domain;
        // A reverting shared domain still owns old CK envelopes. Its write
        // mode selects the new wrap only after the original DEK is opened.
        let domain_changes =
            (from.calendar_id(), from.domain_id()) != (to.calendar_id(), to.domain_id());
        let rewraps = is_move && domain_changes;
        if rewraps {
            rewrap_event(&mut changed, from, to)
                .map_err(|error| seal_error(error, owner, event_id))?;
        }
        if is_move {
            let index = changed
                .meta
                .names
                .iter()
                .position(|name| name.parent_id == source)
                .unwrap();
            changed.meta.names.swap_remove(index);
        }
        changed.meta.names.push(DavName {
            name: new_name.to_owned(),
            parent_id: target,
        });
        let mut batch = BatchBuilder::new();
        for context in &contexts {
            context.assert_in(&mut batch);
        }
        for fence in &overwrite_fences {
            fence.assert_in(owner, &mut batch);
        }
        if let Some(container) = container.as_ref().filter(|_| rewraps) {
            let resealed = za_reseal_container(
                container,
                from,
                ZaContainerOf::Event(&event.uid),
                to,
                ZaContainerScope::Event(&event.uid),
                owner,
                event_id,
            )?;
            ContainerWrites::new(owner, Collection::CalendarEvent)
                .copy(
                    server,
                    &resealed,
                    event_id,
                    Some(StoredContainer::from(container)),
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
        }
        let stored_event = original
            .to_unarchived::<CalendarEvent>()
            .caused_by(trc::location!())?;
        let (meta, new_content) = changed.into_parts();
        if rewraps {
            meta.update_full_at(
                new_content,
                token.changed_by(),
                stored_event,
                content_
                    .unarchive::<CalendarEventContent>()
                    .caused_by(trc::location!())?,
                owner,
                event_id,
                None,
                cutoff,
                &mut batch,
            )
            .caused_by(trc::location!())?;
        } else {
            meta.update_meta_at(
                token.changed_by(),
                stored_event,
                owner,
                event_id,
                None,
                cutoff,
                &mut batch,
            )
            .caused_by(trc::location!())?;
        }
        if let Some(path) = &vanished {
            batch.log_calendar_vanished(source, path.clone());
        }
        if let (Some(id), Some(stored)) = (overwrite, &overwritten) {
            // The event write can end a large chunk; fence the overwrite's chunk too.
            for context in &contexts {
                context.assert_in(&mut batch);
            }
            for fence in &overwrite_fences {
                fence.assert_in(owner, &mut batch);
            }
            let result = DestroyArchive(
                stored
                    .to_unarchived::<CalendarEvent>()
                    .caused_by(trc::location!())?,
            )
            .delete_za_at(
                server,
                &account,
                owner,
                id,
                target,
                None,
                None,
                false,
                Some(to),
                cutoff,
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
            #[cfg(feature = "test_mode")]
            common::za::test_hooks::record_move_overwrite_message(owner, id, result.is_some());
            let _ = result;
        }
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::copy_pause_point(owner).await;
        match server.commit_batch(batch).await {
            Ok(_) => {
                server.notify_task_queue();
                return Ok(HttpResponse::new(if overwrite.is_some() {
                    StatusCode::NO_CONTENT
                } else {
                    StatusCode::CREATED
                }));
            }
            Err(error) if error.is_assertion_failure() && attempt == 0 => {
                let fresh = za_move_contexts(
                    server,
                    token,
                    owner,
                    &event,
                    source,
                    target,
                    is_move,
                    overwrite.is_some(),
                    is_collection,
                )
                .await?;
                let fresh_overwrite_fences = if let Some(stored) = &overwritten {
                    za_overwrite_fences(
                        server,
                        token,
                        owner,
                        &stored
                            .deserialize::<CalendarEvent>()
                            .caused_by(trc::location!())?,
                        target,
                    )
                    .await?
                } else {
                    Vec::new()
                };
                if !calendars_changed(&contexts, &fresh)
                    && !overwrite_calendars_changed(&overwrite_fences, &fresh_overwrite_fences)
                {
                    return Err(error.into());
                }
                contexts = fresh;
                overwrite_fences = fresh_overwrite_fences;
            }
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!()
}

/// The source calendar's container sealed for a key-account calendar copy's
/// destination (rulings 16 and 29), and the kinds the destination records.
fn za_copy_calendar_container(
    container: Option<&MetadataBuf>,
    from: &ZaCalendarContext,
    to: &groupware::calendar::seal::ZaKeyDomain,
    target: u32,
) -> crate::Result<(Option<MetadataBuf>, MetadataKinds)> {
    let Some(container) = container else {
        return Ok((None, MetadataKinds::NONE));
    };
    let resealed = za_reseal_container(
        container,
        &from.domain,
        ZaContainerOf::Calendar(from.calendar_id),
        to,
        ZaContainerScope::Calendar(Some(target)),
        from.owner_id,
        target,
    )?;
    let kinds = resealed.kinds();
    Ok((Some(resealed), kinds))
}

/// Existing-destination key collection MOVE is deliberately bounded to one
/// physical transaction. No event or destination prefix is published on loss.
#[allow(clippy::too_many_arguments)]
async fn za_move_overwrite_container(
    server: &Server,
    token: &AccessToken,
    _keys: &vault::session::SessionKeys,
    owner: u32,
    source: u32,
    source_path: String,
    target: u32,
    new_name: &str,
) -> crate::Result<HttpResponse> {
    let remove = Bitmap::new().with_item(Acl::RemoveItems);
    let target_required = remove.with_item(Acl::AddItems);
    let mut contexts = Vec::new();
    for (calendar_id, required) in [(source, remove), (target, target_required)] {
        // The supported route is personal-only. Resolve read-only so refusing a
        // shared calendar cannot publish boundary repair before admission.
        let context = za_calendar_context(server, token, owner, calendar_id, required)
            .await
            .map_err(|error| super::za_calendar_error(error, owner, calendar_id))?
            .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
        if context.domain.calendar_id().is_some() {
            return Err(shared_copy_refusal(owner));
        }
        contexts.push(context);
    }

    // These stores cannot establish this route's ordered membership snapshot
    // and rollback contract. Keep shared-domain refusals above authoritative.
    if server.store().is_foundationdb()
        || server.store().is_read_replica()
        || server.store().is_ephemeral()
    {
        return Err(DavError::Code(StatusCode::CONFLICT));
    }

    // Sample before the direct scan. Indexed membership writers allocate the
    // next account change id transactionally, before evaluating the predicate.
    let sampled = server
        .store()
        .get_counter(ValueKey {
            account_id: owner,
            collection: 0,
            document_id: 0,
            class: ValueClass::ChangeId(SyncCollection::Calendar.into()),
        })
        .await
        .caused_by(trc::location!())?;
    let expected = sampled
        .checked_add(1)
        .filter(|_| sampled >= 0)
        .ok_or(DavError::Code(StatusCode::CONFLICT))? as u64;
    let mut events = Vec::new();
    let mut is_over_cap = false;
    server
        .archives(
            owner,
            Collection::CalendarEvent,
            Field::ARCHIVE,
            &(),
            |id, stored| {
                let event = stored.unarchive::<CalendarEvent>()?;
                if event.names.iter().any(|name| {
                    let parent = name.parent_id.to_native();
                    parent == source || parent == target
                }) {
                    events.push((id, stored));
                    // Even one selector per document would exceed the operation cap.
                    if events.len() > 1000 {
                        is_over_cap = true;
                        return Ok(false);
                    }
                }
                Ok(true)
            },
        )
        .await
        .caused_by(trc::location!())?;
    if is_over_cap {
        return Err(DavError::Code(StatusCode::CONFLICT));
    }

    let mut other_fences = BTreeMap::new();
    for (id, stored) in &events {
        let event = stored
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        let from_source = event.names.iter().any(|name| name.parent_id == source);
        if from_source {
            let content = za_event_content(server, owner, *id).await?;
            classify_event_storage(
                &content
                    .deserialize::<CalendarEventContent>()
                    .caused_by(trc::location!())?,
            )
            .map_err(|error| seal_error(error, owner, *id))?;
        }
        for parent in event
            .names
            .iter()
            .map(|name| name.parent_id)
            .collect::<BTreeSet<_>>()
        {
            if contexts.iter().any(|context| context.calendar_id == parent) {
                continue;
            }
            if from_source {
                let context = za_calendar_context(server, token, owner, parent, remove)
                    .await
                    .map_err(|error| super::za_calendar_error(error, owner, parent))?
                    .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
                if context.domain.calendar_id().is_some() {
                    return Err(shared_copy_refusal(owner));
                }
                contexts.push(context);
                other_fences.remove(&parent);
            } else if !other_fences.contains_key(&parent) {
                // Destination unlink preserves these parents without needing
                // their keys/operation bits, including admitted recovery links.
                other_fences.insert(
                    parent,
                    ZaOverwriteFence {
                        calendar_id: parent,
                        stored: server
                            .store()
                            .get_value(ValueKey::archive(owner, Collection::Calendar, parent))
                            .await
                            .caused_by(trc::location!())?,
                    },
                );
            }
        }
    }

    let source_calendar = contexts[0]
        .stored
        .to_unarchived::<Calendar>()
        .caused_by(trc::location!())?;
    let source_container = read_container(
        server,
        owner,
        Collection::Calendar,
        source,
        source_calendar.inner.metadata_kinds(),
    )
    .await
    .caused_by(trc::location!())?;
    let (container, kinds) = za_copy_calendar_container(
        source_container.as_ref(),
        &contexts[0],
        &contexts[1].domain,
        target,
    )?;
    let mut calendar = contexts[0]
        .stored
        .deserialize::<Calendar>()
        .caused_by(trc::location!())?;
    calendar.name = new_name.to_owned();
    calendar.acls.clear();
    calendar.set_metadata_kinds(kinds);
    let preference = calendar
        .preferences
        .into_iter()
        .find(|entry| entry.account_id == owner)
        .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
    calendar.preferences = vec![CalendarPreferences {
        account_id: owner,
        name: preference.name,
        description: preference.description,
        default_alerts: preference.default_alerts,
        sort_order: 0,
        color: preference.color,
        flags: CALENDAR_SUBSCRIBED,
        time_zone: Timezone::Default,
    }];
    let default = server
        .store()
        .get_value::<u32>(ValueKey {
            account_id: owner,
            collection: Collection::Principal.into(),
            document_id: 0,
            class: ValueClass::Property(PrincipalField::DefaultCalendarId.into()),
        })
        .await
        .caused_by(trc::location!())?;
    let account = server
        .account_info(token.account_id())
        .await
        .caused_by(trc::location!())?;
    let cutoff = now() as i64;
    let mut batch = BatchBuilder::new();
    // Every retained original calendar assertion precedes any replacement of
    // those keys. Event builders retain their original archive assertions too.
    for context in &contexts {
        context.assert_in(&mut batch);
    }
    for fence in other_fences.values() {
        fence.assert_in(owner, &mut batch);
    }
    batch
        .with_account_id(owner)
        .with_collection(Collection::Calendar)
        .with_document(source)
        .merge_fnc(Field::ARCHIVE, move |ids, _| {
            if ids.last_change_id(owner, SyncCollection::Calendar) != expected {
                return Err(trc::StoreEvent::AssertValueFailed.into_err());
            }
            Ok(MergeResult::Skip)
        });
    for (id, stored) in &events {
        let mut event = stored
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        if let Some(name) = event
            .names
            .iter()
            .find(|name| name.parent_id == source)
            .map(|name| name.name.clone())
        {
            // A document present in both collections has exactly one write.
            // Preserve all other links and use the source's original name.
            event
                .names
                .retain(|name| name.parent_id != source && name.parent_id != target);
            event.names.push(DavName {
                name,
                parent_id: target,
            });
            event
                .update_meta_at(
                    token.changed_by(),
                    stored
                        .to_unarchived::<CalendarEvent>()
                        .caused_by(trc::location!())?,
                    owner,
                    *id,
                    None,
                    cutoff,
                    &mut batch,
                )
                .caused_by(trc::location!())?;
        } else {
            DestroyArchive(
                stored
                    .to_unarchived::<CalendarEvent>()
                    .caused_by(trc::location!())?,
            )
            .delete_za_at(
                server,
                &account,
                owner,
                *id,
                target,
                None,
                None,
                false,
                Some(&contexts[1].domain),
                cutoff,
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
        }
    }
    DestroyArchive(
        contexts[1]
            .stored
            .to_unarchived::<Calendar>()
            .caused_by(trc::location!())?,
    )
    .delete(server, token.changed_by(), owner, target, None, &mut batch)
    .await
    .caused_by(trc::location!())?;
    if let Some(container) = &container {
        ContainerWrites::new(owner, Collection::Calendar)
            .copy(server, container, target, None, &mut batch)
            .await
            .caused_by(trc::location!())?;
    }
    calendar
        .insert(token.changed_by(), owner, target, &mut batch)
        .caused_by(trc::location!())?;
    // Keep source id explicit: this tombstone belongs to the removed source.
    DestroyArchive(source_calendar)
        .delete(
            server,
            token.changed_by(),
            owner,
            source,
            Some(source_path),
            &mut batch,
        )
        .await
        .caused_by(trc::location!())?;
    if default == Some(source) {
        batch
            .with_account_id(owner)
            .with_collection(Collection::Principal)
            .with_document(0)
            .assert_value(PrincipalField::DefaultCalendarId, source)
            .clear(PrincipalField::DefaultCalendarId);
    }
    // One physical transaction: no commit point may have been taken, the
    // open chunk is within upstream's commit limits, and its input bytes
    // (`accounted_input_bytes`, which first materializes the change logs and
    // counters; its merges are the metadata viewer states and
    // `clear_if_equals`, within its per-operation allowance) are within 5 MB.
    let bytes = batch.accounted_input_bytes();
    let slices = batch.commit_point_count() + 1;
    let operations = batch.ops().len();
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::record_calendar_move_plan(owner, operations, bytes, slices);
    if slices != 1 || operations > 1000 || bytes > 5_000_000 || batch.is_large_batch() {
        return Err(DavError::Code(StatusCode::CONFLICT));
    }
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::copy_pause_point(owner).await;
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::calendar_move_delete_pause_point(owner).await;
    if let Err(error) = server.commit_batch(batch).await {
        if error.is_assertion_failure() {
            // Re-resolution only classifies terminal shared-domain refusal.
            // It never rebuilds or replays a write from changed metadata/events.
            for context in &contexts {
                let required = if context.calendar_id == target {
                    target_required
                } else {
                    remove
                };
                if let Ok(Some(fresh)) =
                    za_calendar_context(server, token, owner, context.calendar_id, required).await
                    && fresh.domain.calendar_id().is_some()
                {
                    return Err(shared_copy_refusal(owner));
                }
            }
        }
        return Err(error.into());
    }
    server.notify_task_queue();
    Ok(HttpResponse::new(StatusCode::NO_CONTENT))
}

/// Key collection copies create the destination first, then use its actual
/// read-back archive in each physical child transaction. Collection operations
/// retain upstream partial-progress semantics: a conflict never replays creation.
#[allow(clippy::too_many_arguments)]
async fn za_copy_container(
    server: &Server,
    token: &AccessToken,
    keys: &vault::session::SessionKeys,
    owner: u32,
    source: u32,
    children: Vec<u32>,
    source_path: String,
    overwrite: Option<u32>,
    overwritten_children: Vec<u32>,
    new_name: &str,
    is_move: bool,
) -> crate::Result<HttpResponse> {
    if is_move && let Some(target) = overwrite {
        return za_move_overwrite_container(
            server,
            token,
            keys,
            owner,
            source,
            source_path,
            target,
            new_name,
        )
        .await;
    }
    let required = Bitmap::new().with_item(if is_move {
        Acl::RemoveItems
    } else {
        Acl::ReadItems
    });
    let source_context = za_mutation_context(server, token, owner, source, required)
        .await
        .map_err(|error| super::za_calendar_error(error, owner, source))?
        .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
    if source_context.domain.calendar_id().is_some() {
        return Err(shared_copy_refusal(owner));
    }
    let mut contexts = vec![source_context];
    let mut events = Vec::with_capacity(children.len());
    for child in children {
        if let Some(stored) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                owner,
                Collection::CalendarEvent,
                child,
            ))
            .await
            .caused_by(trc::location!())?
        {
            let event = stored
                .deserialize::<CalendarEvent>()
                .caused_by(trc::location!())?;
            let content = za_event_content(server, owner, child).await?;
            classify_event_storage(
                &content
                    .deserialize::<CalendarEventContent>()
                    .caused_by(trc::location!())?,
            )
            .map_err(|error| seal_error(error, owner, child))?;
            for parent in event
                .names
                .iter()
                .map(|name| name.parent_id)
                .collect::<BTreeSet<_>>()
            {
                if contexts.iter().any(|context| context.calendar_id == parent) {
                    continue;
                }
                let context = za_mutation_context(server, token, owner, parent, required)
                    .await
                    .map_err(|error| super::za_calendar_error(error, owner, parent))?
                    .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
                if context.domain.calendar_id().is_some() {
                    return Err(shared_copy_refusal(owner));
                }
                contexts.push(context);
            }
            if let Some(name) = event.names.iter().find(|name| name.parent_id == source) {
                events.push((child, name.name.clone(), stored));
            }
        }
    }
    let source_calendar = contexts[0]
        .stored
        .to_unarchived::<Calendar>()
        .caused_by(trc::location!())?;
    let source_container = read_container(
        server,
        owner,
        Collection::Calendar,
        source,
        source_calendar.inner.metadata_kinds(),
    )
    .await
    .caused_by(trc::location!())?;
    let mut calendar = contexts[0]
        .stored
        .deserialize::<Calendar>()
        .caused_by(trc::location!())?;
    calendar.name = new_name.to_owned();
    calendar.acls.clear();
    // The source is unshared; preserve its owner's complete encrypted entry.
    let preference = calendar
        .preferences
        .into_iter()
        .find(|entry| entry.account_id == owner)
        .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
    calendar.preferences = vec![CalendarPreferences {
        account_id: owner,
        name: preference.name,
        description: preference.description,
        default_alerts: preference.default_alerts,
        sort_order: 0,
        color: preference.color,
        flags: CALENDAR_SUBSCRIBED,
        time_zone: Timezone::Default,
    }];
    let owner_domain = groupware::calendar::seal::ZaKeyDomain::owner(keys);
    let account = server
        .account_info(token.account_id())
        .await
        .caused_by(trc::location!())?;
    let cutoff = now() as i64;
    let target = if let Some(id) = overwrite {
        let context = za_mutation_context(
            server,
            token,
            owner,
            id,
            Bitmap::new().with_item(Acl::RemoveItems),
        )
        .await
        .map_err(|error| super::za_calendar_error(error, owner, id))?
        .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
        if context.domain.calendar_id().is_some() {
            return Err(shared_copy_refusal(owner));
        }
        // Each overwrite chunk fences the old destination and all source parents.
        // Its events use the same scheduling-off cutoff throughout this request.
        for child in overwritten_children {
            if let Some(stored) = server
                .store()
                .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                    owner,
                    Collection::CalendarEvent,
                    child,
                ))
                .await
                .caused_by(trc::location!())?
            {
                let unlink_fences = za_overwrite_fences(
                    server,
                    token,
                    owner,
                    &stored
                        .deserialize::<CalendarEvent>()
                        .caused_by(trc::location!())?,
                    id,
                )
                .await?;
                let mut batch = BatchBuilder::new();
                for source in &contexts {
                    source.assert_in(&mut batch);
                }
                for parent in &unlink_fences {
                    parent.assert_in(owner, &mut batch);
                }
                context.assert_in(&mut batch);
                DestroyArchive(
                    stored
                        .to_unarchived::<CalendarEvent>()
                        .caused_by(trc::location!())?,
                )
                .delete_za_at(
                    server,
                    &account,
                    owner,
                    child,
                    id,
                    None,
                    None,
                    false,
                    Some(&context.domain),
                    cutoff,
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
                #[cfg(feature = "test_mode")]
                common::za::test_hooks::delete_commit_pause_point(owner).await;
                server
                    .commit_batch(batch)
                    .await
                    .caused_by(trc::location!())?;
            }
        }
        let (container, kinds) =
            za_copy_calendar_container(source_container.as_ref(), &contexts[0], &owner_domain, id)?;
        calendar.set_metadata_kinds(kinds);
        let mut batch = BatchBuilder::new();
        for source in &contexts {
            source.assert_in(&mut batch);
        }
        context.assert_in(&mut batch);
        DestroyArchive(
            context
                .stored
                .to_unarchived::<Calendar>()
                .caused_by(trc::location!())?,
        )
        .delete(server, token.changed_by(), owner, id, None, &mut batch)
        .await
        .caused_by(trc::location!())?;
        for source in &contexts {
            source.assert_in(&mut batch);
        }
        if let Some(container) = &container {
            ContainerWrites::new(owner, Collection::Calendar)
                .copy(server, container, id, None, &mut batch)
                .await
                .caused_by(trc::location!())?;
        }
        calendar
            .insert(token.changed_by(), owner, id, &mut batch)
            .caused_by(trc::location!())?;
        server
            .commit_batch(batch)
            .await
            .caused_by(trc::location!())?;
        id
    } else {
        // Ruling 14: a reservation-only batch assigns an id that is never
        // reused, so the copied container binds the destination's id (ruling 16).
        let mut reservation = BatchBuilder::new();
        let slot = reservation.reserve_document_id(owner, Collection::Calendar);
        let id = server
            .commit_batch(reservation)
            .await
            .caused_by(trc::location!())?
            .slot(slot);
        let (container, kinds) =
            za_copy_calendar_container(source_container.as_ref(), &contexts[0], &owner_domain, id)?;
        calendar.set_metadata_kinds(kinds);
        let mut batch = BatchBuilder::new();
        for context in &contexts {
            context.assert_in(&mut batch);
        }
        if let Some(container) = &container {
            ContainerWrites::new(owner, Collection::Calendar)
                .copy(server, container, id, None, &mut batch)
                .await
                .caused_by(trc::location!())?;
        }
        calendar
            .insert(token.changed_by(), owner, id, &mut batch)
            .caused_by(trc::location!())?;
        server
            .commit_batch(batch)
            .await
            .caused_by(trc::location!())?;
        id
    };
    let target_context = za_mutation_context(
        server,
        token,
        owner,
        target,
        Bitmap::new().with_item(Acl::AddItems),
    )
    .await
    .map_err(|error| super::za_calendar_error(error, owner, target))?
    .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
    if target_context.domain.calendar_id().is_some() {
        return Err(shared_copy_refusal(owner));
    }
    for (child, name, stored) in events {
        // The original preflight archive is the event assertion, even across a
        // calendar retry. Each child reads/fences the actual destination anew.
        za_copy_move_stored_event(
            server, token, keys, owner, child, source, None, target, &name, None, None, is_move,
            true, stored,
        )
        .await?;
    }
    if is_move {
        let current = za_mutation_context(server, token, owner, source, required)
            .await
            .map_err(|error| super::za_calendar_error(error, owner, source))?
            .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
        if current.domain.calendar_id().is_some() {
            return Err(shared_copy_refusal(owner));
        }
        let target_context = za_mutation_context(
            server,
            token,
            owner,
            target,
            Bitmap::new().with_item(Acl::AddItems),
        )
        .await
        .map_err(|error| super::za_calendar_error(error, owner, target))?
        .ok_or(DavError::Code(StatusCode::FORBIDDEN))?;
        if target_context.domain.calendar_id().is_some() {
            return Err(shared_copy_refusal(owner));
        }
        let mut batch = BatchBuilder::new();
        target_context.assert_in(&mut batch);
        // The destination holds the preflight source metadata. Keep that
        // original source CAS so a newer property or slug survives a conflict.
        DestroyArchive(source_calendar)
            .delete(
                server,
                token.changed_by(),
                owner,
                source,
                Some(source_path),
                &mut batch,
            )
            .await
            .caused_by(trc::location!())?;
        let default = server
            .store()
            .get_value::<u32>(ValueKey {
                account_id: owner,
                collection: Collection::Principal.into(),
                document_id: 0,
                class: ValueClass::Property(PrincipalField::DefaultCalendarId.into()),
            })
            .await
            .caused_by(trc::location!())?;
        if default == Some(source) {
            batch
                .with_account_id(owner)
                .with_collection(Collection::Principal)
                .with_document(0)
                .clear(PrincipalField::DefaultCalendarId);
        }
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::calendar_move_delete_pause_point(owner).await;
        server
            .commit_batch(batch)
            .await
            .caused_by(trc::location!())?;
    }
    server.notify_task_queue();
    Ok(HttpResponse::new(if overwrite.is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::CREATED
    }))
}
