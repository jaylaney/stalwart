/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::assert_is_unique_uid;
use crate::{
    DavError, DavMethod,
    common::{
        lock::{LockRequestHandler, ResourceState},
        uri::DavUriResource,
        za::ZeroAccessGate,
    },
    file::DavFileResource,
};
use calcard::common::timezone::Tz;
use common::{DavName, Server, auth::AccessToken};
use dav_proto::{Depth, RequestHeaders};
use groupware::{
    DestroyArchive,
    cache::GroupwareCache,
    calendar::{Calendar, CalendarEvent, CalendarPreferences, Timezone},
};
use http_proto::HttpResponse;
use hyper::StatusCode;
use store::write::{BatchBuilder, now};
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, ValueClass},
};
use trc::AddContext;
use types::{
    acl::Acl,
    collection::{Collection, SyncCollection, VanishedCollection},
    field::PrincipalField,
};

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
            .fetch_dav_resources(
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
        if !access_token.is_member(from_account_id)
            && !from_resources.has_access_to_container(
                access_token,
                if from_resource.is_container() {
                    from_resource.document_id()
                } else {
                    from_resource.parent_id().unwrap()
                },
                Acl::ReadItems,
            )
        {
            return Err(DavError::Code(StatusCode::FORBIDDEN));
        }

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
        let to_resources = if to_account_id == from_account_id {
            from_resources.clone()
        } else {
            self.fetch_dav_resources(
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
                    collection: to_resource
                        .map(|r| {
                            if r.is_container() {
                                Collection::Calendar
                            } else {
                                Collection::CalendarEvent
                            }
                        })
                        .unwrap_or(Collection::Calendar),
                    document_id: Some(to_resource.map(|r| r.document_id()).unwrap_or(u32::MAX)),
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
                    let from_children_ids = from_resources
                        .subtree(from_resource_name)
                        .filter(|r| !r.is_container())
                        .map(|r| r.document_id())
                        .collect::<Vec<_>>();
                    let to_document_ids = to_resources
                        .subtree(destination_resource_name)
                        .filter(|r| !r.is_container())
                        .map(|r| r.document_id())
                        .collect::<Vec<_>>();

                    // Validate ACLs
                    if !access_token.is_member(to_account_id)
                        || (!access_token.is_member(from_account_id)
                            && !from_resources.has_access_to_container(
                                access_token,
                                from_resource.document_id(),
                                if is_move {
                                    Acl::RemoveItems
                                } else {
                                    Acl::ReadItems
                                },
                            ))
                    {
                        return Err(DavError::Code(StatusCode::FORBIDDEN));
                    }

                    // Overwrite container
                    copy_container(
                        self,
                        access_token,
                        from_account_id,
                        from_resource.document_id(),
                        from_children_ids,
                        from_resources.format_collection(from_resource_name),
                        to_account_id,
                        to_resource.document_id().into(),
                        to_document_ids,
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
                    if (!access_token.is_member(from_account_id)
                        && !from_resources.has_access_to_container(
                            access_token,
                            from_calendar_id,
                            if is_move {
                                Acl::RemoveItems
                            } else {
                                Acl::ReadItems
                            },
                        ))
                        || (!access_token.is_member(to_account_id)
                            && !to_resources.has_access_to_container(
                                access_token,
                                to_calendar_id,
                                Acl::RemoveItems,
                            ))
                    {
                        return Err(DavError::Code(StatusCode::FORBIDDEN));
                    }

                    if is_move {
                        move_event(
                            self,
                            access_token,
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
                if (!access_token.is_member(from_account_id)
                    && !from_resources.has_access_to_container(
                        access_token,
                        from_calendar_id,
                        if is_move {
                            Acl::RemoveItems
                        } else {
                            Acl::ReadItems
                        },
                    ))
                    || (!access_token.is_member(to_account_id)
                        && !to_resources.has_access_to_container(
                            access_token,
                            to_calendar_id,
                            Acl::AddItems,
                        ))
                {
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
                if !access_token.is_member(to_account_id) {
                    return Err(DavError::Code(StatusCode::FORBIDDEN));
                }

                // Validate ACLs
                if !access_token.is_member(from_account_id)
                    && !from_resources.has_access_to_container(
                        access_token,
                        from_resource.document_id(),
                        if is_move {
                            Acl::RemoveItems
                        } else {
                            Acl::ReadItems
                        },
                    )
                {
                    return Err(DavError::Code(StatusCode::FORBIDDEN));
                }

                // Copy/move container
                let from_children_ids = from_resources
                    .subtree(from_resource_name)
                    .filter(|r| !r.is_container())
                    .map(|r| r.document_id())
                    .collect::<Vec<_>>();
                if is_move {
                    if from_account_id != to_account_id {
                        copy_container(
                            self,
                            access_token,
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

#[allow(clippy::too_many_arguments)]
async fn copy_event(
    server: &Server,
    access_token: &AccessToken,
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
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
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

    // Validate UID
    assert_is_unique_uid(
        server,
        server
            .fetch_dav_resources(
                access_token.account_id(),
                to_account_id,
                SyncCollection::Calendar,
            )
            .await
            .caused_by(trc::location!())?
            .as_ref(),
        to_account_id,
        to_calendar_id,
        event.inner.data.event.uids().next(),
    )
    .await?;

    let changed_by = access_token.account_tenant_ids();
    if from_account_id == to_account_id {
        let mut new_event = event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        new_event.names.push(DavName {
            name: new_name.to_string(),
            parent_id: to_calendar_id,
        });
        new_event
            .update(
                changed_by,
                event,
                from_account_id,
                from_document_id,
                &mut batch,
            )
            .caused_by(trc::location!())?;
    } else {
        let next_email_alarm = event.inner.data.next_alarm(now() as i64, Tz::Floating);
        let mut new_event = event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        new_event.names = vec![DavName {
            name: new_name.to_string(),
            parent_id: to_calendar_id,
        }];
        let to_document_id = server
            .store()
            .assign_document_ids(to_account_id, Collection::CalendarEvent, 1)
            .await
            .caused_by(trc::location!())?;
        new_event
            .insert(
                changed_by,
                to_account_id,
                to_document_id,
                next_email_alarm,
                &mut batch,
            )
            .caused_by(trc::location!())?;
    }

    let response = if let Some(to_document_id) = to_document_id {
        // Overwrite event on destination
        let event_ = server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
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

            DestroyArchive(event)
                .delete(
                    &account_info,
                    to_account_id,
                    to_document_id,
                    to_calendar_id,
                    None,
                    false,
                    &mut batch,
                )
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
    server.notify_task_queue();

    response
}

#[allow(clippy::too_many_arguments)]
async fn move_event(
    server: &Server,
    access_token: &AccessToken,
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
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
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
            server,
            server
                .fetch_dav_resources(
                    access_token.account_id(),
                    to_account_id,
                    SyncCollection::Calendar,
                )
                .await
                .caused_by(trc::location!())?
                .as_ref(),
            to_account_id,
            to_calendar_id,
            event.inner.data.event.uids().next(),
        )
        .await?;
    }

    let account_info = server
        .account_info(access_token.account_id())
        .await
        .caused_by(trc::location!())?;
    let mut batch = BatchBuilder::new();
    if from_account_id == to_account_id {
        let mut name_idx = None;
        for (idx, name) in event.inner.names.iter().enumerate() {
            if name.parent_id == from_calendar_id {
                name_idx = Some(idx);
                break;
            }
        }

        let name_idx = if let Some(name_idx) = name_idx {
            name_idx
        } else {
            return Err(DavError::Code(StatusCode::NOT_FOUND));
        };

        let mut new_event = event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        new_event.names.swap_remove(name_idx);
        new_event.names.push(DavName {
            name: new_name.to_string(),
            parent_id: to_calendar_id,
        });
        new_event
            .update(
                access_token.account_tenant_ids(),
                event.clone(),
                from_account_id,
                from_document_id,
                &mut batch,
            )
            .caused_by(trc::location!())?;
        batch.log_vanished_item(VanishedCollection::Calendar, from_resource_path);
    } else {
        let next_email_alarm = event.inner.data.next_alarm(now() as i64, Tz::Floating);
        let mut new_event = event
            .deserialize::<CalendarEvent>()
            .caused_by(trc::location!())?;
        new_event.names = vec![DavName {
            name: new_name.to_string(),
            parent_id: to_calendar_id,
        }];

        DestroyArchive(event)
            .delete(
                &account_info,
                from_account_id,
                from_document_id,
                from_calendar_id,
                from_resource_path.into(),
                false,
                &mut batch,
            )
            .caused_by(trc::location!())?;

        let to_document_id = server
            .store()
            .assign_document_ids(to_account_id, Collection::CalendarEvent, 1)
            .await
            .caused_by(trc::location!())?;
        new_event
            .insert(
                access_token.account_tenant_ids(),
                to_account_id,
                to_document_id,
                next_email_alarm,
                &mut batch,
            )
            .caused_by(trc::location!())?;
    }

    let response = if let Some(to_document_id) = to_document_id {
        // Overwrite event on destination
        let event_ = server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
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

            DestroyArchive(event)
                .delete(
                    &account_info,
                    to_account_id,
                    to_document_id,
                    to_calendar_id,
                    None,
                    false,
                    &mut batch,
                )
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
    server.notify_task_queue();

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
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
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
    new_event
        .update(
            access_token.account_tenant_ids(),
            event,
            account_id,
            document_id,
            &mut batch,
        )
        .caused_by(trc::location!())?;
    batch.log_vanished_item(VanishedCollection::Calendar, from_resource_path);
    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;
    server.notify_task_queue();

    Ok(HttpResponse::new(StatusCode::CREATED))
}

#[allow(clippy::too_many_arguments)]
async fn copy_container(
    server: &Server,
    access_token: &AccessToken,
    from_account_id: u32,
    from_document_id: u32,
    from_children_ids: Vec<u32>,
    from_resource_path: String,
    to_account_id: u32,
    to_document_id: Option<u32>,
    to_children_ids: Vec<u32>,
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
            to_children_ids,
            new_name,
            remove_source,
        )
        .await;
    }
    // Fetch calendar
    let calendar_ = server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
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

    // Prepare write batch
    let mut batch = BatchBuilder::new();

    if remove_source {
        DestroyArchive(old_calendar)
            .delete(
                access_token.account_tenant_ids(),
                from_account_id,
                from_document_id,
                from_resource_path.into(),
                &mut batch,
            )
            .caused_by(trc::location!())?;

        // Reset default calendar id
        let default_calendar_id = server
            .store()
            .get_value::<u32>(ValueKey {
                account_id: from_account_id,
                collection: Collection::Principal.into(),
                document_id: 0,
                class: ValueClass::Property(PrincipalField::DefaultCalendarId.into()),
            })
            .await
            .caused_by(trc::location!())?;
        if default_calendar_id.is_some_and(|id| id == from_document_id) {
            batch
                .with_account_id(from_account_id)
                .with_collection(Collection::Principal)
                .with_document(0)
                .clear(PrincipalField::DefaultCalendarId);
        }
    }

    let preference = calendar.preferences.into_iter().next().unwrap();
    calendar.name = new_name.to_string();
    calendar.acls.clear();
    calendar.preferences = vec![CalendarPreferences {
        account_id: to_account_id,
        name: preference.name,
        description: preference.description,
        default_alerts: preference.default_alerts,
        sort_order: 0,
        color: preference.color,
        flags: 0,
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
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
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

            DestroyArchive(calendar)
                .delete_with_events(
                    server,
                    &account_info,
                    to_account_id,
                    to_document_id,
                    to_children_ids,
                    None,
                    false,
                    &mut batch,
                )
                .await
                .caused_by(trc::location!())?;
        }

        to_document_id
    } else {
        server
            .store()
            .assign_document_ids(to_account_id, Collection::Calendar, 1)
            .await
            .caused_by(trc::location!())?
    };
    calendar
        .insert(
            access_token.account_tenant_ids(),
            to_account_id,
            to_document_id,
            &mut batch,
        )
        .caused_by(trc::location!())?;

    // Copy children
    let mut required_space = 0;
    for from_child_document_id in from_children_ids {
        if let Some(event_) = server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                from_account_id,
                Collection::CalendarEvent,
                from_child_document_id,
            ))
            .await?
        {
            let event = event_
                .to_unarchived::<CalendarEvent>()
                .caused_by(trc::location!())?;
            let mut new_name = None;

            for name in event.inner.names.iter() {
                if name.parent_id == to_document_id {
                    continue;
                } else if name.parent_id == from_document_id {
                    new_name = Some(name.name.to_string());
                }
            }
            let new_name = if let Some(new_name) = new_name {
                DavName {
                    name: new_name,
                    parent_id: to_document_id,
                }
            } else {
                continue;
            };
            let event = event_
                .to_unarchived::<CalendarEvent>()
                .caused_by(trc::location!())?;
            let mut new_event = event
                .deserialize::<CalendarEvent>()
                .caused_by(trc::location!())?;

            if from_account_id == to_account_id {
                if remove_source {
                    new_event
                        .names
                        .retain(|name| name.parent_id != from_document_id);
                }

                new_event.names.push(new_name);
                new_event
                    .update(
                        access_token.account_tenant_ids(),
                        event,
                        from_account_id,
                        from_child_document_id,
                        &mut batch,
                    )
                    .caused_by(trc::location!())?;
            } else {
                let next_email_alarm = event.inner.data.next_alarm(now() as i64, Tz::Floating);
                if remove_source {
                    DestroyArchive(event)
                        .delete(
                            &account_info,
                            from_account_id,
                            from_child_document_id,
                            from_document_id,
                            None,
                            false,
                            &mut batch,
                        )
                        .caused_by(trc::location!())?;
                }
                let to_document_id = server
                    .store()
                    .assign_document_ids(to_account_id, Collection::CalendarEvent, 1)
                    .await
                    .caused_by(trc::location!())?;
                new_event.names = vec![new_name];
                required_space += new_event.size as u64;
                new_event
                    .insert(
                        access_token.account_tenant_ids(),
                        to_account_id,
                        to_document_id,
                        next_email_alarm,
                        &mut batch,
                    )
                    .caused_by(trc::location!())?;
            }
        }
    }

    if from_account_id != to_account_id && required_space > 0 {
        server
            .has_available_quota(
                server.account(to_account_id).await?.as_ref(),
                required_space,
            )
            .await?;
    }

    server
        .commit_batch(batch)
        .await
        .caused_by(trc::location!())?;
    server.notify_task_queue();

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
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
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
    new_calendar
        .update(
            access_token.account_tenant_ids(),
            calendar,
            account_id,
            document_id,
            &mut batch,
        )
        .caused_by(trc::location!())?;
    batch.log_vanished_item(VanishedCollection::Calendar, from_resource_path);
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
                change_id: assigned.last_change_id(account_id)?,
            }),
        )
        .await
        .map_err(|error| super::za_calendar_error(error, account_id, document_id))?;
    }
    server.notify_task_queue();

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
) -> crate::Result<Vec<groupware::calendar::sharing::resolve::ZaCalendarContext>> {
    use crate::common::za::za_mutation_context;
    use utils::map::bitmap::Bitmap;
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
        .collect::<std::collections::BTreeSet<_>>()
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
    stored: Option<Archive<AlignedBytes>>,
}

impl ZaOverwriteFence {
    fn assert_in(&self, owner: u32, batch: &mut BatchBuilder) {
        batch
            .with_account_id(owner)
            .with_collection(Collection::Calendar)
            .with_document(self.calendar_id);
        if let Some(stored) = &self.stored {
            batch.assert_value(types::field::Field::ARCHIVE, stored);
        } else {
            batch.assert_value(types::field::Field::ARCHIVE, ());
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
    let target_context = crate::common::za::za_mutation_context(
        server,
        token,
        owner,
        target,
        utils::map::bitmap::Bitmap::new().with_item(Acl::RemoveItems),
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
        .collect::<std::collections::BTreeSet<_>>()
    {
        fences.push(ZaOverwriteFence {
            calendar_id: parent,
            stored: server
                .store()
                .get_value(ValueKey::archive(owner, Collection::Calendar, parent))
                .await?,
        });
    }
    Ok(fences)
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
    let original: Archive<AlignedBytes> = server
        .store()
        .get_value(ValueKey::archive(
            owner,
            Collection::CalendarEvent,
            event_id,
        ))
        .await?
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

#[allow(clippy::too_many_arguments)]
async fn za_copy_move_stored_event(
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
    is_collection: bool,
    original: Archive<AlignedBytes>,
) -> crate::Result<HttpResponse> {
    use groupware::calendar::{
        seal::{classify_event_storage, rewrap_event, seal_error},
        sharing::resolve::calendars_changed,
    };
    let event = original.deserialize::<CalendarEvent>()?;
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
    classify_event_storage(&event).map_err(|error| seal_error(error, owner, event_id))?;
    if source != target || (!is_move && overwrite.is_none()) {
        assert_is_unique_uid(
            server,
            server
                .fetch_dav_resources(token.account_id(), owner, SyncCollection::Calendar)
                .await?
                .as_ref(),
            owner,
            target,
            event.data.event.uids().next(),
        )
        .await?;
    }
    let overwritten: Option<Archive<AlignedBytes>> = if let Some(id) = overwrite {
        server
            .store()
            .get_value(ValueKey::archive(owner, Collection::CalendarEvent, id))
            .await?
    } else {
        None
    };
    let mut overwrite_fences = if let Some(stored) = &overwritten {
        za_overwrite_fences(
            server,
            token,
            owner,
            &stored.deserialize::<CalendarEvent>()?,
            target,
        )
        .await?
    } else {
        Vec::new()
    };
    let account = server.account_info(token.account_id()).await?;
    let cutoff = now() as i64;
    for attempt in 0..2 {
        let mut changed = event.clone();
        let from = &contexts[0].domain;
        let to = &contexts.last().unwrap().domain;
        // A reverting shared domain still owns old CK envelopes. Its write
        // mode selects the new wrap only after the original DEK is opened.
        let domain_changes =
            (from.calendar_id(), from.domain_id()) != (to.calendar_id(), to.domain_id());
        if is_move && domain_changes {
            rewrap_event(&mut changed, from, to)
                .map_err(|error| seal_error(error, owner, event_id))?;
        }
        if is_move {
            let index = changed
                .names
                .iter()
                .position(|name| name.parent_id == source)
                .unwrap();
            changed.names.swap_remove(index);
        }
        changed.names.push(DavName {
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
        changed.update_at(
            token.account_tenant_ids(),
            original.to_unarchived::<CalendarEvent>()?,
            owner,
            event_id,
            cutoff,
            &mut batch,
        )?;
        if let Some(path) = &vanished {
            batch.log_vanished_item(VanishedCollection::Calendar, path.clone());
        }
        if let (Some(id), Some(stored)) = (overwrite, &overwritten) {
            // update_at can end a large chunk; fence the overwrite's chunk too.
            for context in &contexts {
                context.assert_in(&mut batch);
            }
            for fence in &overwrite_fences {
                fence.assert_in(owner, &mut batch);
            }
            let result = DestroyArchive(stored.to_unarchived::<CalendarEvent>()?).delete_za_at(
                &account,
                owner,
                id,
                target,
                None,
                false,
                Some(keys),
                Some(to),
                cutoff,
                &mut batch,
            )?;
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
                        &stored.deserialize::<CalendarEvent>()?,
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
    use crate::common::za::za_mutation_context;
    use utils::map::bitmap::Bitmap;
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
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                owner,
                Collection::CalendarEvent,
                child,
            ))
            .await?
        {
            let event = stored.deserialize::<CalendarEvent>()?;
            groupware::calendar::seal::classify_event_storage(&event)
                .map_err(|error| groupware::calendar::seal::seal_error(error, owner, child))?;
            for parent in event
                .names
                .iter()
                .map(|name| name.parent_id)
                .collect::<std::collections::BTreeSet<_>>()
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
    let mut calendar = contexts[0].stored.deserialize::<Calendar>()?;
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
        flags: 0,
        time_zone: Timezone::Default,
    }];
    let account = server.account_info(token.account_id()).await?;
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
                .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                    owner,
                    Collection::CalendarEvent,
                    child,
                ))
                .await?
            {
                let unlink_fences = za_overwrite_fences(
                    server,
                    token,
                    owner,
                    &stored.deserialize::<CalendarEvent>()?,
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
                DestroyArchive(stored.to_unarchived::<CalendarEvent>()?).delete_za_at(
                    &account,
                    owner,
                    child,
                    id,
                    None,
                    false,
                    Some(keys),
                    Some(&context.domain),
                    cutoff,
                    &mut batch,
                )?;
                #[cfg(feature = "test_mode")]
                common::za::test_hooks::delete_commit_pause_point(owner).await;
                server.commit_batch(batch).await?;
            }
        }
        let mut batch = BatchBuilder::new();
        for source in &contexts {
            source.assert_in(&mut batch);
        }
        context.assert_in(&mut batch);
        DestroyArchive(context.stored.to_unarchived::<Calendar>()?).delete(
            token.account_tenant_ids(),
            owner,
            id,
            None,
            &mut batch,
        )?;
        for source in &contexts {
            source.assert_in(&mut batch);
        }
        calendar.insert(token.account_tenant_ids(), owner, id, &mut batch)?;
        server.commit_batch(batch).await?;
        id
    } else {
        let id = server
            .store()
            .assign_document_ids(owner, Collection::Calendar, 1)
            .await?;
        let mut batch = BatchBuilder::new();
        for context in &contexts {
            context.assert_in(&mut batch);
        }
        calendar.insert(token.account_tenant_ids(), owner, id, &mut batch)?;
        server.commit_batch(batch).await?;
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
        DestroyArchive(contexts[0].stored.to_unarchived::<Calendar>()?).delete(
            token.account_tenant_ids(),
            owner,
            source,
            Some(source_path),
            &mut batch,
        )?;
        let default = server
            .store()
            .get_value::<u32>(ValueKey {
                account_id: owner,
                collection: Collection::Principal.into(),
                document_id: 0,
                class: ValueClass::Property(PrincipalField::DefaultCalendarId.into()),
            })
            .await?;
        if default == Some(source) {
            batch
                .with_account_id(owner)
                .with_collection(Collection::Principal)
                .with_document(0)
                .clear(PrincipalField::DefaultCalendarId);
        }
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::calendar_move_delete_pause_point(owner).await;
        server.commit_batch(batch).await?;
    }
    server.notify_task_queue();
    Ok(HttpResponse::new(if overwrite.is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::CREATED
    }))
}
