/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

pub mod copy_move;
pub mod delete;
pub mod freebusy;
pub mod get;
pub mod mkcol;
pub mod proppatch;
pub mod query;
pub mod scheduling;
pub mod update;

use crate::{DavError, DavErrorCondition};
use common::{
    DavResources, Server,
    za::lease::{ZaUidLease, ZaUidLeaseGuard},
};
use dav_proto::schema::{
    property::{CalDavProperty, CalendarData, DavProperty, WebDavProperty},
    response::CalCondition,
};
use groupware::{calendar::CalendarEvent, scheduling::ItipError};
use http_proto::HttpResponse;
use hyper::StatusCode;
use store::{
    ValueKey,
    write::{AlignedBytes, Archive, BatchBuilder},
};
use trc::AddContext;
use types::{collection::Collection, field::CalendarEventField};

pub(crate) static CALENDAR_CONTAINER_PROPS: [DavProperty; 31] = [
    DavProperty::WebDav(WebDavProperty::CreationDate),
    DavProperty::WebDav(WebDavProperty::DisplayName),
    DavProperty::WebDav(WebDavProperty::GetETag),
    DavProperty::WebDav(WebDavProperty::GetLastModified),
    DavProperty::WebDav(WebDavProperty::ResourceType),
    DavProperty::WebDav(WebDavProperty::LockDiscovery),
    DavProperty::WebDav(WebDavProperty::SupportedLock),
    DavProperty::WebDav(WebDavProperty::CurrentUserPrincipal),
    DavProperty::WebDav(WebDavProperty::SyncToken),
    DavProperty::WebDav(WebDavProperty::Owner),
    DavProperty::WebDav(WebDavProperty::SupportedPrivilegeSet),
    DavProperty::WebDav(WebDavProperty::CurrentUserPrivilegeSet),
    DavProperty::WebDav(WebDavProperty::Acl),
    DavProperty::WebDav(WebDavProperty::AclRestrictions),
    DavProperty::WebDav(WebDavProperty::InheritedAclSet),
    DavProperty::WebDav(WebDavProperty::PrincipalCollectionSet),
    DavProperty::WebDav(WebDavProperty::SupportedReportSet),
    DavProperty::WebDav(WebDavProperty::QuotaAvailableBytes),
    DavProperty::WebDav(WebDavProperty::QuotaUsedBytes),
    DavProperty::CalDav(CalDavProperty::CalendarDescription),
    DavProperty::CalDav(CalDavProperty::SupportedCalendarData),
    DavProperty::CalDav(CalDavProperty::SupportedCollationSet),
    DavProperty::CalDav(CalDavProperty::SupportedCalendarComponentSet),
    DavProperty::CalDav(CalDavProperty::CalendarTimezone),
    DavProperty::CalDav(CalDavProperty::MaxResourceSize),
    DavProperty::CalDav(CalDavProperty::MinDateTime),
    DavProperty::CalDav(CalDavProperty::MaxDateTime),
    DavProperty::CalDav(CalDavProperty::MaxInstances),
    DavProperty::CalDav(CalDavProperty::MaxAttendeesPerInstance),
    DavProperty::CalDav(CalDavProperty::TimezoneServiceSet),
    DavProperty::CalDav(CalDavProperty::TimezoneId),
];

pub(crate) static CALENDAR_ITEM_PROPS: [DavProperty; 20] = [
    DavProperty::WebDav(WebDavProperty::CreationDate),
    DavProperty::WebDav(WebDavProperty::DisplayName),
    DavProperty::WebDav(WebDavProperty::GetETag),
    DavProperty::WebDav(WebDavProperty::GetLastModified),
    DavProperty::WebDav(WebDavProperty::ResourceType),
    DavProperty::WebDav(WebDavProperty::LockDiscovery),
    DavProperty::WebDav(WebDavProperty::SupportedLock),
    DavProperty::WebDav(WebDavProperty::CurrentUserPrincipal),
    DavProperty::WebDav(WebDavProperty::SyncToken),
    DavProperty::WebDav(WebDavProperty::Owner),
    DavProperty::WebDav(WebDavProperty::SupportedPrivilegeSet),
    DavProperty::WebDav(WebDavProperty::CurrentUserPrivilegeSet),
    DavProperty::WebDav(WebDavProperty::Acl),
    DavProperty::WebDav(WebDavProperty::AclRestrictions),
    DavProperty::WebDav(WebDavProperty::InheritedAclSet),
    DavProperty::WebDav(WebDavProperty::PrincipalCollectionSet),
    DavProperty::WebDav(WebDavProperty::GetContentLanguage),
    DavProperty::WebDav(WebDavProperty::GetContentLength),
    DavProperty::WebDav(WebDavProperty::GetContentType),
    DavProperty::CalDav(CalDavProperty::CalendarData(CalendarData {
        properties: vec![],
        expand: None,
        limit_recurrence: None,
        limit_freebusy: None,
    })),
];

pub(crate) async fn assert_is_unique_uid(
    server: &Server,
    resources: &DavResources,
    account_id: u32,
    calendar_id: u32,
    uid: Option<&str>,
) -> crate::Result<()> {
    if let Some(uid) = uid {
        let hits = server
            .document_ids_matching(
                account_id,
                Collection::CalendarEvent,
                CalendarEventField::Uid,
                uid.as_bytes(),
            )
            .await
            .caused_by(trc::location!())?;

        if !hits.is_empty() {
            for path in resources.children(calendar_id) {
                if hits.contains(path.document_id()) {
                    return Err(DavError::Condition(DavErrorCondition::new(
                        StatusCode::PRECONDITION_FAILED,
                        CalCondition::NoUidConflict(resources.format_resource(path).into()),
                    )));
                }
            }
        }
    }

    Ok(())
}

/// Invitations spec 5.1, 6.3 step 1: takes the UID creation lease for a key
/// account's PUT create, then checks the UID against current archives
/// (`za_assert_is_unique_uid`) while holding it. A lease that stays held
/// through the retries (two creators of one UID in one account within a
/// second) answers 503 with a trace; a hit answers `NoUidConflict` after the
/// lease is released.
pub(crate) async fn za_take_uid_lease(
    server: &Server,
    resources: &DavResources,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
) -> crate::Result<ZaUidLeaseGuard> {
    let Some(lease) = ZaUidLease::acquire(server.store(), account_id, uid)
        .await
        .caused_by(trc::location!())?
    else {
        trc::event!(
            Store(trc::StoreEvent::AssertValueFailed),
            AccountId = account_id,
            Details = "zero-access: UID creation lease busy",
        );
        return Err(DavError::Code(StatusCode::SERVICE_UNAVAILABLE));
    };
    let lease = ZaUidLeaseGuard::new(server.store().clone(), lease);
    if let Err(err) = za_assert_is_unique_uid(server, resources, account_id, calendar_id, uid).await
    {
        lease.release().await;
        return Err(err);
    }
    Ok(lease)
}

/// Invitations spec 5.1: the archive-based counterpart of
/// `assert_is_unique_uid` for a key account's create, run under the
/// creation lease. The handler's resource snapshot can predate another
/// creator's commit, so this reads the fresh UID-index hits and each hit's
/// current archive, and refuses the UID when a stored copy names the target
/// calendar (upstream's rule: a UID is unique within one calendar). The
/// snapshot only formats the href.
pub(crate) async fn za_assert_is_unique_uid(
    server: &Server,
    resources: &DavResources,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
) -> crate::Result<()> {
    let hits = server
        .document_ids_matching(
            account_id,
            Collection::CalendarEvent,
            CalendarEventField::Uid,
            uid.as_bytes(),
        )
        .await
        .caused_by(trc::location!())?;
    for document_id in hits {
        let Some(stored) = server
            .store()
            .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
                account_id,
                Collection::CalendarEvent,
                document_id,
            ))
            .await
            .caused_by(trc::location!())?
        else {
            continue;
        };
        let event = stored
            .unarchive::<CalendarEvent>()
            .caused_by(trc::location!())?;
        if let Some(name) = event
            .names
            .iter()
            .find(|name| name.parent_id.to_native() == calendar_id)
        {
            let href = resources
                .container_resource_path_by_id(calendar_id)
                .map(|calendar| {
                    resources.format_item(&format!("{}/{}", calendar.path(), name.name))
                })
                .unwrap_or_default();
            return Err(DavError::Condition(DavErrorCondition::new(
                StatusCode::PRECONDITION_FAILED,
                CalCondition::NoUidConflict(href.into()),
            )));
        }
    }
    Ok(())
}

/// Invitations spec 5.4 and 6.3 step 3: what a key account's create does
/// when its lease was taken over while the request ran, found by the
/// fan-out's fence or by the commit's. Traced first, so a busy lease or a
/// 412 from the retake does not hide that the fence was lost; the lease
/// is abandoned (the key holds a successor's value, or is gone); the lease
/// is taken again to re-check current archives (`NoUidConflict` on a hit)
/// and released; otherwise 503, so the client resends. Never `Ok`.
pub(crate) async fn za_lost_create_fence(
    server: &Server,
    resources: &DavResources,
    lease: ZaUidLeaseGuard,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
) -> crate::Result<HttpResponse> {
    za_finish_create_loss(
        server,
        resources,
        lease,
        account_id,
        calendar_id,
        uid,
        CreateLoss::ConfirmedLease,
    )
    .await
}

enum CreateLoss {
    ConfirmedLease,
    OpaqueSecondCommit,
}

async fn za_finish_create_loss(
    server: &Server,
    resources: &DavResources,
    lease: ZaUidLeaseGuard,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
    loss: CreateLoss,
) -> crate::Result<HttpResponse> {
    trc::event!(
        Store(trc::StoreEvent::AssertValueFailed),
        AccountId = account_id,
        Details = match &loss {
            CreateLoss::ConfirmedLease => "zero-access: UID creation lease lost before the commit",
            CreateLoss::OpaqueSecondCommit => "zero-access: create retry lost its assertion",
        },
    );
    match loss {
        CreateLoss::ConfirmedLease => lease.abandon(),
        CreateLoss::OpaqueSecondCommit => {
            // The second failure may still be the calendar assertion. Release
            // only our exact fence before the existing UID retake. A false
            // result is inconclusive; it does not prove successor ownership.
            let _ = lease.release().await;
        }
    }
    let retry = za_take_uid_lease(server, resources, account_id, calendar_id, uid).await?;
    retry.release().await;
    // The retake found no copy in this calendar. Keep the existing resend
    // answer for either a confirmed lease loss or an opaque second loss.
    Err(DavError::Code(StatusCode::SERVICE_UNAVAILABLE))
}

/// The bounded second create commit. Its assertion loss is opaque: release
/// our fenced lease before the existing UID retake/outcome, without another
/// calendar read, archive retry or fan-out.
pub(crate) async fn za_commit_create(
    server: &Server,
    resources: &DavResources,
    batch: BatchBuilder,
    lease: ZaUidLeaseGuard,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
) -> crate::Result<()> {
    #[cfg(feature = "test_mode")]
    if common::za::test_hooks::take_commit_fail(account_id) {
        lease.release().await;
        return Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access test hook: sender commit failure")
            .caused_by(trc::location!())
            .into());
    }
    #[cfg(feature = "test_mode")]
    common::za::test_hooks::commit_pause_point(account_id).await;
    match server.commit_batch(batch).await {
        Ok(_) => {
            lease.release().await;
            Ok(())
        }
        Err(err) if err.is_assertion_failure() => za_finish_create_loss(
            server,
            resources,
            lease,
            account_id,
            calendar_id,
            uid,
            CreateLoss::OpaqueSecondCommit,
        )
        .await
        .map(|_| ()),
        Err(err) => {
            lease.release().await;
            Err(err.caused_by(trc::location!()).into())
        }
    }
}

pub(crate) trait ItipPrecondition {
    fn failed_precondition(&self) -> Option<CalCondition>;
}

impl ItipPrecondition for ItipError {
    fn failed_precondition(&self) -> Option<CalCondition> {
        match self {
            ItipError::MultipleOrganizer => Some(CalCondition::SameOrganizerInAllComponents),
            ItipError::OrganizerIsLocalAddress
            | ItipError::SenderIsNotParticipant(_)
            | ItipError::OrganizerMismatch => Some(CalCondition::ValidOrganizer),
            ItipError::CannotModifyProperty(_)
            | ItipError::CannotModifyInstance
            | ItipError::CannotModifyAddress => Some(CalCondition::AllowedAttendeeObjectChange),
            ItipError::MissingUid
            | ItipError::MultipleUid
            | ItipError::MultipleObjectTypes
            | ItipError::MultipleObjectInstances
            | ItipError::MissingMethod
            | ItipError::InvalidComponentType
            | ItipError::OutOfSequence
            | ItipError::UnknownParticipant(_)
            | ItipError::UnsupportedMethod(_) => Some(CalCondition::ValidSchedulingMessage),
            _ => None,
        }
    }
}

/// Mutation adapters always resolve fresh state; response memos are read-only.
pub(crate) fn za_calendar_error(
    error: groupware::calendar::sharing::resolve::ZaCalendarError,
    owner_id: u32,
    document_id: u32,
) -> DavError {
    use groupware::calendar::sharing::resolve::ZaCalendarError;
    match error {
        ZaCalendarError::Forbidden => DavError::Code(StatusCode::FORBIDDEN),
        ZaCalendarError::LinkedEvent => DavError::Code(StatusCode::CONFLICT),
        ZaCalendarError::Store(error)
            if error.matches(trc::EventType::Store(trc::StoreEvent::NotFound)) =>
        {
            DavError::Code(StatusCode::NOT_FOUND)
        }
        error => error.into_store_error(owner_id, document_id).into(),
    }
}

pub(crate) async fn za_write_contexts(
    server: &Server,
    token: &common::auth::AccessToken,
    owner_id: u32,
    event: &CalendarEvent,
    calendar_id: u32,
    required: types::acl::Acl,
) -> crate::Result<Vec<groupware::calendar::sharing::resolve::ZaCalendarContext>> {
    let contexts = groupware::calendar::sharing::resolve::za_event_contexts(
        server,
        token,
        owner_id,
        event,
        calendar_id,
        utils::map::bitmap::Bitmap::new().with_item(required),
    )
    .await
    .map_err(|error| za_calendar_error(error, owner_id, calendar_id))?;
    if let Some(keys) = token.session_keys() {
        groupware::calendar::sharing::boundary::za_repair_owned_contexts(server, keys, contexts)
            .await
            .map_err(|error| za_calendar_error(error, owner_id, calendar_id))
    } else {
        Ok(contexts)
    }
}

/// Called once after an ambiguous assertion loss. Keep the original event
/// assertion and already-sealed payload; only its key envelope can change.
pub(crate) async fn za_refresh_event(
    server: &Server,
    token: &common::auth::AccessToken,
    owner_id: u32,
    calendar_id: u32,
    required: types::acl::Acl,
    before: &[groupware::calendar::sharing::resolve::ZaCalendarContext],
    sealed: &mut CalendarEvent,
) -> crate::Result<Option<Vec<groupware::calendar::sharing::resolve::ZaCalendarContext>>> {
    if before.is_empty() {
        return Ok(None);
    }
    let after = za_write_contexts(server, token, owner_id, sealed, calendar_id, required).await?;
    if !groupware::calendar::sharing::resolve::calendars_changed(before, &after) {
        return Ok(None);
    }
    groupware::calendar::seal::rewrap_event(sealed, &before[0].domain, &after[0].domain)
        .map_err(|error| groupware::calendar::seal::seal_error(error, owner_id, calendar_id))?;
    Ok(Some(after))
}

/// Event writers keep the authenticated writer's keys. The fresh calendar
/// resolver below decides whether those keys authorize this owner's calendar.
pub(crate) async fn za_writer_keys(
    server: &Server,
    token: &common::auth::AccessToken,
    owner_id: u32,
) -> crate::Result<Option<std::sync::Arc<vault::session::SessionKeys>>> {
    if !server.za_is_key_account(owner_id).await? {
        return Ok(None);
    }
    token
        .session_keys()
        .filter(|keys| keys.account_id == token.account_id() && !token.za_is_impersonated())
        .cloned()
        .map(Some)
        .ok_or(DavError::Code(StatusCode::FORBIDDEN))
}

/// Exercise the actual private event MOVE helper before foreign DAV URI admission
/// is enabled. This bridge exists only in disposable test-mode builds.
#[cfg(feature = "test_mode")]
pub async fn za_test_move_event(
    server: &common::Server,
    token: &common::auth::AccessToken,
    owner: u32,
    event: u32,
    source: u32,
    overwritten: u32,
    target: u32,
) -> Result<hyper::StatusCode, hyper::StatusCode> {
    copy_move::za_copy_move_event(
        server,
        token,
        token.session_keys().expect("test writer session keys"),
        owner,
        event,
        source,
        Some(overwritten),
        target,
        "event.ics",
        None,
        None,
        true,
    )
    .await
    .map(|response| response.status())
    .map_err(|error| match error {
        crate::DavError::Code(code) => code,
        crate::DavError::Condition(condition) => condition.code,
        crate::DavError::Parse(_) => hyper::StatusCode::BAD_REQUEST,
        crate::DavError::Internal(_) => hyper::StatusCode::INTERNAL_SERVER_ERROR,
    })
}
