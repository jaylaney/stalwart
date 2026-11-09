/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    addressbook::{get::AddressBookGet, set::AddressBookSet},
    api::auth::JmapAuthorization,
    blob::{copy::BlobCopy, get::BlobOperations, upload::BlobUpload},
    calendar::{get::CalendarGet, set::CalendarSet},
    calendar_event::{
        copy::JmapCalendarEventCopy, get::CalendarEventGet, parse::CalendarEventParse,
        query::CalendarEventQuery, set::CalendarEventSet,
    },
    calendar_event_notification::{
        get::CalendarEventNotificationGet, query::CalendarEventNotificationQuery,
        set::CalendarEventNotificationSet,
    },
    changes::{get::ChangesLookup, query::QueryChanges},
    contact::{
        copy::JmapContactCardCopy, get::ContactCardGet, parse::ContactCardParse,
        query::ContactCardQuery, set::ContactCardSet,
    },
    email::{
        copy::JmapEmailCopy, get::EmailGet, import::EmailImport, parse::EmailParse,
        query::EmailQuery, set::EmailSet, snippet::EmailSearchSnippet,
    },
    file::{copy::FileNodeCopy, get::FileNodeGet, query::FileNodeQuery, set::FileNodeSet},
    identity::{get::IdentityGet, set::IdentitySet},
    mailbox::{get::MailboxGet, query::MailboxQuery, set::MailboxSet},
    participant_identity::{get::ParticipantIdentityGet, set::ParticipantIdentitySet},
    principal::{availability::PrincipalGetAvailability, get::PrincipalGet, query::PrincipalQuery},
    push::{get::PushSubscriptionFetch, set::PushSubscriptionSet},
    quota::{get::QuotaGet, query::QuotaQuery},
    registry::{get::RegistryGet, query::RegistryQuery, set::RegistrySet},
    share_notification::{
        get::ShareNotificationGet, query::ShareNotificationQuery, set::ShareNotificationSet,
    },
    sieve::{
        get::SieveScriptGet, query::SieveScriptQuery, set::SieveScriptSet,
        validate::SieveScriptValidate,
    },
    submission::{get::EmailSubmissionGet, query::EmailSubmissionQuery, set::EmailSubmissionSet},
    thread::get::ThreadGet,
    vacation::{get::VacationResponseGet, set::VacationResponseSet},
};
use common::{Server, auth::AccessToken, network::memory::MemoryReservation};
use compact_str::format_compact;
use email::message::{
    ingest::IngestedEmail,
    za::{search::ensure::ZaSearchEnsure, view::ZaMailView},
};
use http_proto::HttpSessionData;
use jmap_proto::{
    request::{
        Call, CopyRequestMethod, GetRequestMethod, INVALID_ACCOUNT_ID, ParseRequestMethod,
        QueryRequestMethod, Request, RequestMethod, SetRequestMethod,
        capability::{Capability, CapabilityIds},
        method::{MethodName, MethodObject},
    },
    response::{CopyResponseMethod, Response, ResponseMethod, SetResponseMethod},
};
use std::future::Future;
use std::time::Instant;
use trc::JmapEvent;
use types::{collection::Collection, id::Id};
use vault::session::SessionKeys;

pub struct PostWriteCall<'a> {
    keys: &'a SessionKeys,
    pending: &'a mut bool,
    selected: bool,
}

impl<'a> PostWriteCall<'a> {
    pub(crate) fn acknowledge(&mut self, results: &[(String, trc::Result<IngestedEmail>)]) {
        // A quota fallback can commit one item after a fatal result. Count
        // all acknowledgments before recording can return at that error.
        *self.pending |= results.iter().any(|(_, result)| {
            result
                .as_ref()
                .is_ok_and(|email| email.change_id != u64::MAX)
        });
    }

    fn take_attempt(&mut self) -> Option<&'a SessionKeys> {
        if self.selected && std::mem::take(self.pending) {
            // Consume before awaiting the hook, including busy/failed passes.
            self.selected = false;
            Some(self.keys)
        } else {
            None
        }
    }

    pub(crate) async fn attempt(&mut self, server: &Server) {
        if let Some(keys) = self.take_attempt() {
            server.za_search_pass_hook(keys.account_id, keys).await;
        }
    }
}

pub(crate) fn is_mail_method(obj: MethodObject) -> bool {
    matches!(
        obj,
        MethodObject::Email
            | MethodObject::Thread
            | MethodObject::Mailbox
            | MethodObject::SearchSnippet
            | MethodObject::EmailSubmission
            | MethodObject::Blob
    )
}

fn last_adding_call(calls: &[Call<RequestMethod<'_>>], owner: u32) -> Option<usize> {
    calls.iter().rposition(|call| {
        let target = match &call.method {
            RequestMethod::Set(SetRequestMethod::Email(request)) if request.has_creates() => {
                request.account_id
            }
            RequestMethod::ImportEmail(request) if !request.emails.is_empty() => request.account_id,
            _ => return false,
        };
        target.id() < INVALID_ACCOUNT_ID && target.document_id() == owner
    })
}

pub trait RequestHandler: Sync + Send {
    fn handle_jmap_request<'x>(
        &self,
        request: Request<'x>,
        access_token: &AccessToken,
        session: &HttpSessionData,
        reservation: &mut MemoryReservation,
    ) -> impl Future<Output = Response<'x>> + Send;

    #[allow(clippy::too_many_arguments)]
    fn handle_method_call<'x>(
        &self,
        method: RequestMethod<'x>,
        method_name: MethodName,
        access_token: &AccessToken,
        next_call: &mut Option<Call<RequestMethod<'x>>>,
        session: &HttpSessionData,
        using: CapabilityIds,
        reservation: &mut MemoryReservation,
        post_write: Option<&mut PostWriteCall<'_>>,
    ) -> impl Future<Output = trc::Result<ResponseMethod<'x>>> + Send;
}

impl RequestHandler for Server {
    async fn handle_jmap_request<'x>(
        &self,
        request: Request<'x>,
        access_token: &AccessToken,
        session: &HttpSessionData,
        reservation: &mut MemoryReservation,
    ) -> Response<'x> {
        let add_created_ids = request.created_ids.is_some();
        let using = request.using;
        let mut response = Response::new(
            access_token.state(),
            request.created_ids.unwrap_or_default(),
            request.method_calls.len(),
        );

        let owner = access_token.account_id();
        let keys = access_token.za_keys_for(owner).map(|keys| keys.as_ref());
        let last_adding = if let Some(keys) = keys {
            if request
                .method_calls
                .iter()
                .any(|call| is_mail_method(call.name.obj))
            {
                self.za_search_pass_hook(owner, keys).await;
            }
            last_adding_call(&request.method_calls, owner)
        } else {
            None
        };
        let mut pending = false;

        for (ordinal, mut call) in request.method_calls.into_iter().enumerate() {
            let mut post_write = last_adding.zip(keys).map(|(selected, keys)| PostWriteCall {
                keys,
                pending: &mut pending,
                selected: ordinal == selected,
            });
            // Resolve result and id references
            if let Err(error) = response.resolve_references(&mut call.method) {
                if let Some(post_write) = &mut post_write {
                    post_write.attempt(self).await;
                }
                let method_error = error.clone();
                let error = za_method_error_for_trace(self, access_token, error).await;

                trc::error!(error.span_id(session.session_id));

                response.push_response(call.id, MethodName::error(), method_error);
                continue;
            }

            if !matches!(call.method, RequestMethod::Error(_)) {
                let capability = call.name.capability();
                if capability != Capability::Stalwart && !using.contains(capability) {
                    if let Some(post_write) = &mut post_write {
                        post_write.attempt(self).await;
                    }
                    response.push_response(
                        call.id,
                        MethodName::error(),
                        trc::JmapEvent::UnknownMethod.into_err().details(format_compact!(
                            "Method {} requires capability {} which is not present in the \"using\" property.",
                            call.name,
                            capability.as_str()
                        )),
                    );
                    continue;
                }
            }

            loop {
                // Follow-up calls can reuse the outer call's id, but never
                // inherit its post-write context.
                let mut post_write = post_write.take();
                let mut next_call = None;

                // Add response
                let method_name = call.name.as_str();
                match self
                    .handle_method_call(
                        call.method,
                        call.name,
                        access_token,
                        &mut next_call,
                        session,
                        using,
                        reservation,
                        post_write.as_mut(),
                    )
                    .await
                {
                    Ok(mut method_response) => {
                        match &mut method_response {
                            ResponseMethod::Set(set_response) => {
                                // Add created ids
                                match set_response {
                                    SetResponseMethod::Email(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::Mailbox(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::Identity(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::EmailSubmission(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::PushSubscription(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::Sieve(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::VacationResponse(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::AddressBook(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::ContactCard(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::FileNode(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::ShareNotification(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::Calendar(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::CalendarEvent(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::ParticipantIdentity(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                    SetResponseMethod::CalendarEventNotification(_) => {}
                                    SetResponseMethod::Registry(set_response) => {
                                        set_response.update_created_ids(&mut response);
                                    }
                                }
                            }
                            ResponseMethod::Copy(copy_response) => {
                                // Add created ids
                                match copy_response {
                                    CopyResponseMethod::Email(copy_response) => {
                                        copy_response.update_created_ids(&mut response);
                                    }
                                    CopyResponseMethod::ContactCard(copy_response) => {
                                        copy_response.update_created_ids(&mut response);
                                    }
                                    CopyResponseMethod::CalendarEvent(copy_response) => {
                                        copy_response.update_created_ids(&mut response);
                                    }
                                    CopyResponseMethod::FileNode(copy_response) => {
                                        copy_response.update_created_ids(&mut response);
                                    }
                                    CopyResponseMethod::Blob(_) => {}
                                }
                            }
                            ResponseMethod::ImportEmail(import_response) => {
                                // Add created ids
                                import_response.update_created_ids(&mut response);
                            }
                            ResponseMethod::UploadBlob(upload_response) => {
                                // Add created blobIds
                                upload_response.update_created_ids(&mut response);
                            }
                            _ => {}
                        }

                        response.push_response(call.id, call.name, method_response);
                    }
                    Err(error) => {
                        if let Some(post_write) = &mut post_write {
                            post_write.attempt(self).await;
                        }
                        let method_error = error.clone();
                        let error = za_method_error_for_trace(self, access_token, error).await;

                        trc::error!(
                            error
                                .span_id(session.session_id)
                                .ctx_unique(trc::Key::AccountId, access_token.account_id())
                                .caused_by(method_name)
                        );

                        response.push_error(call.id, method_error);
                    }
                }

                // Process next call
                if let Some(next_call) = next_call {
                    call = next_call;
                    call.id
                        .clone_from(&response.method_responses.last().unwrap().id);
                } else {
                    break;
                }
            }
        }

        if !add_created_ids {
            response.created_ids.clear();
        }

        response
    }

    async fn handle_method_call<'x>(
        &self,
        method: RequestMethod<'x>,
        method_name: MethodName,
        access_token: &AccessToken,
        next_call: &mut Option<Call<RequestMethod<'x>>>,
        session: &HttpSessionData,
        using: CapabilityIds,
        reservation: &mut MemoryReservation,
        post_write: Option<&mut PostWriteCall<'_>>,
    ) -> trc::Result<ResponseMethod<'x>> {
        let op_start = Instant::now();

        // Check permissions
        access_token.assert_has_jmap_permission(&method, method_name.obj)?;

        // Handle method
        let response = match method {
            RequestMethod::Get(req) => match req {
                GetRequestMethod::Email(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Email)?;
                    za_assert_mail_keys(self, access_token, req.account_id).await?;

                    self.email_get(*req, access_token, using, reservation)
                        .await?
                        .into()
                }
                GetRequestMethod::Mailbox(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Mailbox)?;
                    za_assert_mail_keys(self, access_token, req.account_id).await?;

                    self.mailbox_get(*req, access_token, using).await?.into()
                }
                GetRequestMethod::Thread(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Email)?;
                    za_assert_mail_keys(self, access_token, req.account_id).await?;

                    self.thread_get(*req, access_token).await?.into()
                }
                GetRequestMethod::Identity(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.identity_get(*req).await?.into()
                }
                GetRequestMethod::EmailSubmission(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;
                    za_assert_mail_keys(self, access_token, req.account_id).await?;

                    self.email_submission_get(*req, access_token).await?.into()
                }
                GetRequestMethod::PushSubscription(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    self.push_subscription_get(*req, access_token).await?.into()
                }
                GetRequestMethod::Sieve(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.sieve_script_get(*req, access_token, using)
                        .await?
                        .into()
                }
                GetRequestMethod::VacationResponse(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.vacation_response_get(*req).await?.into()
                }
                GetRequestMethod::Principal(req) => {
                    self.principal_get(*req, access_token).await?.into()
                }
                GetRequestMethod::Quota(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.quota_get(*req, access_token).await?.into()
                }
                GetRequestMethod::Blob(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;
                    za_assert_mail_keys(self, access_token, req.account_id).await?;

                    self.blob_get(*req, access_token).await?.into()
                }
                GetRequestMethod::AddressBook(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::AddressBook)?;

                    self.address_book_get(*req, access_token, using)
                        .await?
                        .into()
                }
                GetRequestMethod::ContactCard(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::ContactCard)?;

                    self.contact_card_get(*req, access_token, using, reservation)
                        .await?
                        .into()
                }
                GetRequestMethod::FileNode(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::FileNode)?;

                    self.file_node_get(*req, access_token, using).await?.into()
                }
                GetRequestMethod::PrincipalAvailability(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;

                    self.principal_get_availability(*req, access_token)
                        .await?
                        .into()
                }
                GetRequestMethod::Calendar(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Calendar)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.calendar_get(*req, access_token, using).await?.into()
                }
                GetRequestMethod::CalendarEvent(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::CalendarEvent)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.calendar_event_get(*req, access_token, using, reservation)
                        .await?
                        .into()
                }
                GetRequestMethod::CalendarEventNotification(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::CalendarEvent)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.calendar_event_notification_get(*req, access_token)
                        .await?
                        .into()
                }
                GetRequestMethod::ParticipantIdentity(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::CalendarEvent)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.participant_identity_get(*req, access_token)
                        .await?
                        .into()
                }
                GetRequestMethod::ShareNotification(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.share_notification_get(*req).await?.into()
                }
                GetRequestMethod::Registry(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.registry_get(method_name.obj.unwrap_registry(), *req, access_token)
                        .await?
                        .into()
                }
            },
            RequestMethod::Query(req) => match req {
                QueryRequestMethod::Email(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Email)?;
                    za_assert_mail_keys(self, access_token, req.account_id).await?;

                    self.email_query(*req, access_token, using).await?.into()
                }
                QueryRequestMethod::Mailbox(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Mailbox)?;
                    self.za_assert_mail_route(access_token, req.account_id.document_id())
                        .await?;

                    self.mailbox_query(*req, access_token, using).await?.into()
                }
                QueryRequestMethod::EmailSubmission(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;
                    za_assert_mail_keys(self, access_token, req.account_id).await?;

                    self.email_submission_query(*req).await?.into()
                }
                QueryRequestMethod::Sieve(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.sieve_script_query(*req, access_token, using)
                        .await?
                        .into()
                }
                QueryRequestMethod::Principal(req) => self
                    .principal_query(*req, access_token, using)
                    .await?
                    .into(),
                QueryRequestMethod::Quota(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.quota_query(*req, access_token).await?.into()
                }
                QueryRequestMethod::AddressBook(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::AddressBook)?;

                    self.address_book_query(*req, access_token, using)
                        .await?
                        .into()
                }
                QueryRequestMethod::ContactCard(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::ContactCard)?;

                    self.contact_card_query(*req, access_token, using)
                        .await?
                        .into()
                }
                QueryRequestMethod::FileNode(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::FileNode)?;

                    self.file_node_query(*req, access_token, using)
                        .await?
                        .into()
                }
                QueryRequestMethod::Calendar(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Calendar)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.calendar_query(*req, access_token, using).await?.into()
                }
                QueryRequestMethod::CalendarEvent(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::CalendarEvent)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.calendar_event_query(*req, access_token, using)
                        .await?
                        .into()
                }
                QueryRequestMethod::CalendarEventNotification(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::CalendarEvent)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.calendar_event_notification_query(*req, access_token)
                        .await?
                        .into()
                }
                QueryRequestMethod::ShareNotification(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.share_notification_query(*req).await?.into()
                }
                QueryRequestMethod::Registry(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.registry_query(method_name.obj.unwrap_registry(), *req, access_token)
                        .await?
                        .into()
                }
            },
            RequestMethod::Set(req) => match req {
                SetRequestMethod::Email(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Email)?;
                    self.za_assert_mail_route(access_token, req.account_id.document_id())
                        .await?;

                    let post_write = post_write
                        .filter(|context| context.keys.account_id == req.account_id.document_id());
                    self.email_set(*req, access_token, session, using, post_write)
                        .await?
                        .into()
                }
                SetRequestMethod::Mailbox(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Mailbox)?;
                    self.za_assert_mail_route(access_token, req.account_id.document_id())
                        .await?;

                    self.mailbox_set(*req, access_token, using).await?.into()
                }
                SetRequestMethod::Identity(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.identity_set(*req).await?.into()
                }
                SetRequestMethod::EmailSubmission(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;
                    za_assert_mail_keys(self, access_token, req.account_id).await?;

                    self.email_submission_set(*req, access_token, &session.instance, next_call)
                        .await?
                        .into()
                }
                SetRequestMethod::PushSubscription(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    self.push_subscription_set(*req, access_token).await?.into()
                }
                SetRequestMethod::Sieve(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.sieve_script_set(*req, access_token, session, using)
                        .await?
                        .into()
                }
                SetRequestMethod::VacationResponse(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.vacation_response_set(*req, access_token).await?.into()
                }
                SetRequestMethod::AddressBook(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::AddressBook)?;

                    self.address_book_set(*req, access_token, session, using)
                        .await?
                        .into()
                }
                SetRequestMethod::ContactCard(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::ContactCard)?;

                    self.contact_card_set(*req, access_token, session, using)
                        .await?
                        .into()
                }
                SetRequestMethod::FileNode(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::FileNode)?;

                    self.file_node_set(*req, access_token, session, using)
                        .await?
                        .into()
                }
                SetRequestMethod::ShareNotification(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    self.share_notification_set(*req).await?.into()
                }
                SetRequestMethod::Calendar(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Calendar)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.calendar_set(*req, access_token, session, using)
                        .await?
                        .into()
                }
                SetRequestMethod::CalendarEvent(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::CalendarEvent)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.calendar_event_set(*req, access_token, session, using)
                        .await?
                        .into()
                }
                SetRequestMethod::CalendarEventNotification(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::CalendarEvent)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.calendar_event_notification_set(*req, access_token, session)
                        .await?
                        .into()
                }
                SetRequestMethod::ParticipantIdentity(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::CalendarEvent)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.participant_identity_set(*req, access_token)
                        .await?
                        .into()
                }
                SetRequestMethod::Registry(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_is_member(req.account_id)?;

                    Box::pin(self.registry_set(
                        method_name.obj.unwrap_registry(),
                        *req,
                        access_token,
                        session,
                    ))
                    .await?
                    .into()
                }
            },
            RequestMethod::Changes(mut req) => {
                resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;

                self.changes(*req, method_name.obj, access_token, using)
                    .await?
                    .into_method_response()
            }
            RequestMethod::Copy(req) => match req {
                CopyRequestMethod::Email(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    resolve_from_account_id(
                        &mut req.from_account_id,
                        method_name.obj,
                        access_token,
                    )?;

                    access_token
                        .assert_has_access(req.account_id, Collection::Email)?
                        .assert_has_source_access(req.from_account_id, Collection::Email)?;
                    za_assert_mail_keys(self, access_token, req.account_id).await?;
                    za_assert_mail_keys(self, access_token, req.from_account_id).await?;

                    self.email_copy(*req, access_token, next_call, session, using)
                        .await?
                        .into()
                }
                CopyRequestMethod::Blob(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    resolve_from_account_id(
                        &mut req.from_account_id,
                        method_name.obj,
                        access_token,
                    )?;
                    access_token.assert_is_member(req.account_id)?;
                    if !access_token.has_account_access(req.from_account_id.document_id()) {
                        return Err(trc::JmapEvent::FromAccountNotFound.into_err());
                    }
                    za_assert_mail_keys(self, access_token, req.account_id).await?;

                    self.blob_copy(*req, access_token).await?.into()
                }
                CopyRequestMethod::ContactCard(mut req) => {
                    resolve_from_account_id(
                        &mut req.from_account_id,
                        method_name.obj,
                        access_token,
                    )?;
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;

                    access_token
                        .assert_has_access(req.account_id, Collection::ContactCard)?
                        .assert_has_source_access(req.from_account_id, Collection::ContactCard)?;

                    self.contact_card_copy(*req, access_token, next_call, session, using)
                        .await?
                        .into()
                }
                CopyRequestMethod::CalendarEvent(mut req) => {
                    resolve_from_account_id(
                        &mut req.from_account_id,
                        method_name.obj,
                        access_token,
                    )?;
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;

                    access_token
                        .assert_has_access(req.account_id, Collection::CalendarEvent)?
                        .assert_has_source_access(req.from_account_id, Collection::CalendarEvent)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;
                    za_assert_calendar_allowed(self, req.from_account_id).await?;

                    self.calendar_event_copy(*req, access_token, next_call, session, using)
                        .await?
                        .into()
                }
                CopyRequestMethod::FileNode(mut req) => {
                    resolve_from_account_id(
                        &mut req.from_account_id,
                        method_name.obj,
                        access_token,
                    )?;
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;

                    access_token
                        .assert_has_access(req.account_id, Collection::FileNode)?
                        .assert_has_source_access(req.from_account_id, Collection::FileNode)?;

                    self.file_node_copy(*req, access_token, next_call, session, using)
                        .await?
                        .into()
                }
            },
            RequestMethod::ImportEmail(mut req) => {
                resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                access_token.assert_has_access(req.account_id, Collection::Email)?;
                za_assert_mail_keys(self, access_token, req.account_id).await?;

                let post_write = post_write
                    .filter(|context| context.keys.account_id == req.account_id.document_id());
                self.email_import(*req, access_token, session, using, post_write)
                    .await?
                    .into()
            }
            RequestMethod::Parse(req) => match req {
                ParseRequestMethod::Email(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::Email)?;
                    za_assert_mail_keys(self, access_token, req.account_id).await?;

                    self.email_parse(*req, access_token).await?.into()
                }
                ParseRequestMethod::ContactCard(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::ContactCard)?;

                    self.contact_card_parse(*req, access_token).await?.into()
                }
                ParseRequestMethod::CalendarEvent(mut req) => {
                    resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                    access_token.assert_has_access(req.account_id, Collection::CalendarEvent)?;
                    za_assert_calendar_allowed(self, req.account_id).await?;

                    self.calendar_event_parse(*req, access_token).await?.into()
                }
            },
            RequestMethod::QueryChanges(req) => {
                self.query_changes(req, access_token, using).await?.into()
            }
            RequestMethod::SearchSnippet(mut req) => {
                resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                access_token.assert_has_access(req.account_id, Collection::Email)?;
                za_assert_mail_keys(self, access_token, req.account_id).await?;

                self.email_search_snippet(*req, access_token).await?.into()
            }
            RequestMethod::ValidateScript(mut req) => {
                resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                access_token.assert_is_member(req.account_id)?;

                self.sieve_script_validate(*req, access_token).await?.into()
            }
            RequestMethod::LookupBlob(mut req) => {
                resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                access_token.assert_is_member(req.account_id)?;
                za_assert_mail_keys(self, access_token, req.account_id).await?;

                self.blob_lookup(*req, access_token).await?.into()
            }
            RequestMethod::UploadBlob(mut req) => {
                resolve_account_id(&mut req.account_id, method_name.obj, access_token)?;
                access_token.assert_is_member(req.account_id)?;
                za_assert_mail_keys(self, access_token, req.account_id).await?;

                self.blob_upload_many(*req, access_token).await?.into()
            }
            RequestMethod::Echo(req) => req.into(),
            RequestMethod::Error(error) => return Err(error),
        };

        trc::event!(
            Jmap(JmapEvent::MethodCall),
            Command = method_name.as_str(),
            SpanId = session.session_id,
            AccountId = access_token.account_id(),
            Elapsed = op_start.elapsed(),
        );

        Ok(response)
    }
}

/// Spec 9: JMAP calendar methods are not offered for key accounts in this
/// release. An unknown account is not a key account: upstream's outcome stands.
pub(crate) async fn za_assert_calendar_allowed(server: &Server, account_id: Id) -> trc::Result<()> {
    if server.za_is_key_account(account_id.document_id()).await? {
        Err(trc::JmapEvent::AccountNotSupportedByMethod.into_err())
    } else {
        Ok(())
    }
}

/// Search spec 6.2: keyed mail operations require the target account's keys.
/// This covers Email reads, queries, changes, import, parse, copy and Set
/// create, Thread get/changes, Mailbox/get, EmailSubmission get/query/changes
/// and set, Blob lookup/get/upload/copy, and SearchSnippet/get. The keyless
/// Email/set update and destroy routes and mailbox query/changes/set are
/// checked separately. The fixed keyless refusal is traced in za_mail_keys.
pub(crate) async fn za_assert_mail_keys(
    server: &Server,
    access_token: &AccessToken,
    account_id: Id,
) -> trc::Result<()> {
    server
        .za_mail_keys(access_token, account_id.document_id())
        .await
        .map(|_| ())
}

/// Spec section 10, "Traces": a key account's JMAP request and response
/// bodies are never traced, since a request can carry the account's password
/// (`x:AccountPassword/set`). Fails closed: a failed lookup counts as a key
/// account.
pub(crate) async fn za_jmap_untraced(server: &Server, account_id: u32) -> bool {
    server.za_is_key_account(account_id).await.unwrap_or(true)
}

/// Spec section 10, "Traces": the five method errors whose details carry
/// free text derived from the request (the argument parser's error echoes
/// the arguments, an unknown method its name, an invalid result reference
/// its call id or path) are traced for a key account with a fixed reason
/// instead.
/// The client still receives the detailed error.
async fn za_method_error_for_trace(
    server: &Server,
    access_token: &AccessToken,
    error: trc::Error,
) -> trc::Error {
    let event_type = error.event_type();
    if matches!(
        event_type,
        trc::EventType::Jmap(
            JmapEvent::InvalidArguments
                | JmapEvent::UnsupportedFilter
                | JmapEvent::UnsupportedSort
                | JmapEvent::UnknownMethod
                | JmapEvent::InvalidResultReference
        )
    ) && za_jmap_untraced(server, access_token.account_id()).await
    {
        event_type.into_err().reason("invalid method arguments")
    } else {
        error
    }
}

pub(crate) fn resolve_from_account_id(
    account_id: &mut Id,
    obj: MethodObject,
    access_token: &AccessToken,
) -> trc::Result<()> {
    resolve_account_id(account_id, obj, access_token).map_err(|err| {
        if err.matches(trc::EventType::Jmap(JmapEvent::AccountNotFound)) {
            trc::JmapEvent::FromAccountNotFound.into_err()
        } else {
            err
        }
    })
}

pub(crate) fn resolve_account_id(
    account_id: &mut Id,
    obj: MethodObject,
    access_token: &AccessToken,
) -> trc::Result<()> {
    if account_id.id() < INVALID_ACCOUNT_ID {
        Ok(())
    } else if matches!(
        obj,
        MethodObject::Core | MethodObject::PushSubscription | MethodObject::Registry(_)
    ) {
        *account_id = Id::from(access_token.account_id());
        Ok(())
    } else if account_id.id() == INVALID_ACCOUNT_ID {
        Err(trc::JmapEvent::AccountNotFound.into_err())
    } else {
        Err(trc::JmapEvent::InvalidArguments
            .into_err()
            .details("The \"accountId\" property is required."))
    }
}

#[cfg(test)]
mod za_post_write_tests {
    use super::*;
    use vault::keys::Secret;

    #[test]
    fn failed_or_busy_attempt_does_not_reopen_selected_slot() {
        let keys = SessionKeys::new(7, 0, Secret::random());
        let mut pending = false;
        let mut selected = PostWriteCall {
            keys: &keys,
            pending: &mut pending,
            selected: true,
        };
        assert!(selected.take_attempt().is_none());
        selected.acknowledge(&[(
            "created".into(),
            Ok(IngestedEmail {
                change_id: 42,
                ..IngestedEmail::default()
            }),
        )]);

        // The hook returns no success flag. Even if it was busy or failed,
        // the dispatcher fallback has no second attempt to make.
        assert_eq!(selected.take_attempt().map(|keys| keys.account_id), Some(7));
        assert!(selected.take_attempt().is_none());
        assert!(!pending);
    }

    #[test]
    fn acknowledgment_scans_after_fatal_results_and_survives_later_errors() {
        let keys = SessionKeys::new(7, 0, Secret::random());
        let mut pending = false;
        let mut earlier = PostWriteCall {
            keys: &keys,
            pending: &mut pending,
            selected: false,
        };
        earlier.acknowledge(&[
            (
                "fatal".into(),
                Err(trc::StoreEvent::AssertValueFailed.into_err()),
            ),
            (
                "duplicate".into(),
                Ok(IngestedEmail {
                    change_id: u64::MAX,
                    ..IngestedEmail::default()
                }),
            ),
        ]);
        assert!(!*earlier.pending);

        earlier.acknowledge(&[
            (
                "fatal".into(),
                Err(trc::StoreEvent::AssertValueFailed.into_err()),
            ),
            (
                "committed".into(),
                Ok(IngestedEmail {
                    change_id: 42,
                    ..IngestedEmail::default()
                }),
            ),
        ]);
        assert!(earlier.take_attempt().is_none());
        earlier.acknowledge(&[(
            "later error".into(),
            Err(trc::StoreEvent::AssertValueFailed.into_err()),
        )]);
        assert!(pending);

        let mut selected = PostWriteCall {
            keys: &keys,
            pending: &mut pending,
            selected: true,
        };
        selected.acknowledge(&[]);
        assert!(selected.take_attempt().is_some());
        assert!(selected.take_attempt().is_none());
    }
}
