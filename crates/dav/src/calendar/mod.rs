/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

pub mod copy_move;
pub mod delete;
pub mod filter;
pub mod freebusy;
pub mod get;
pub mod mkcol;
pub mod proppatch;
pub mod query;
pub mod scheduling;
pub mod update;
pub mod view;

use crate::{
    DavError, DavErrorCondition,
    common::{ContainerOperation, MergedOverrides},
};
use calcard::icalendar::{
    ArchivedICalendar, ArchivedICalendarComponentType, ICalendar, ICalendarComponent,
    ICalendarComponentType,
};
use common::{
    ArchivedDavName, GroupwareResources, Server,
    auth::{AccessToken, AccountInfo},
    za::lease::{ZaUidLease, ZaUidLeaseGuard},
};
use dav_proto::schema::{
    property::{CalDavProperty, CalendarData, DavProperty, WebDavProperty},
    response::CalCondition,
};
use groupware::{
    calendar::{
        Alarm, ArchivedCalendarEvent, ArchivedCalendarEventContent, Calendar, CalendarEvent,
        CalendarEventContent, EVENT_HAS_ALARMS, SupportedComponent,
        alarm::ExpandAlarm,
        alerts::{DefaultAlertsResolver, DefaultAlertsView, ICalendarDefaultAlerts},
        compare::{ComparisonScope, RecurrenceComponents},
        privacy::{EventPrivacy, EventViewer, ICalendarPrivacy, PrivacyDenied},
        user::{ICalendarUserData, UserDataError, UserDataView},
    },
    scheduling::ItipError,
};
use http_proto::HttpResponse;
use hyper::StatusCode;
use store::{
    ValueKey,
    write::{Archive, ArchiveBytes, BatchBuilder, serialize::rkyv_deserialize},
};
use trc::AddContext;
use types::{collection::Collection, field::CalendarEventField};
use utils::map::bitmap::Bitmap;

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

pub(crate) fn assert_is_unique_uid(
    resources: &GroupwareResources,
    access_token: &AccessToken,
    account_id: u32,
    calendar_id: u32,
    uid: Option<&str>,
) -> crate::Result<()> {
    let Some(uid) = uid else {
        return Ok(());
    };

    let is_owner = access_token.is_member(account_id);
    let mut has_hidden_conflict = false;
    for path in resources.children_with_uid(calendar_id, uid) {
        if is_owner
            || EventPrivacy::from_flags(path.resource.event_flags().unwrap_or_default())
                != EventPrivacy::Secret
        {
            return Err(DavError::Condition(DavErrorCondition::new(
                StatusCode::FORBIDDEN,
                CalCondition::NoUidConflict(resources.format_resource(path).into()),
            )));
        }
        has_hidden_conflict = true;
    }

    if has_hidden_conflict {
        Err(DavError::Code(StatusCode::FORBIDDEN))
    } else {
        Ok(())
    }
}

impl ContainerOperation {
    pub(crate) fn event_ids(
        self,
        resources: &GroupwareResources,
        path: &str,
        viewer: EventViewer,
    ) -> crate::Result<Vec<u32>> {
        let mut document_ids = Vec::new();
        for resource in resources.subtree(path).filter(|r| !r.is_container()) {
            match EventPrivacy::from_flags(resource.resource.event_flags().unwrap_or_default()) {
                _ if viewer.is_owner() => document_ids.push(resource.document_id()),
                EventPrivacy::Public => document_ids.push(resource.document_id()),
                EventPrivacy::Secret if self == ContainerOperation::Copy => {}
                EventPrivacy::Private | EventPrivacy::Secret => {
                    return Err(DavError::Code(StatusCode::FORBIDDEN));
                }
            }
        }
        Ok(document_ids)
    }
}

pub(crate) struct SupportedComponents(Bitmap<SupportedComponent>);

impl SupportedComponents {
    fn is_restricted(&self) -> bool {
        !self.0.is_empty()
    }

    pub(crate) fn assert_supports(&self, component: SupportedComponent) -> crate::Result<()> {
        if !self.is_restricted() || self.0.contains(component) {
            Ok(())
        } else {
            Err(DavError::Condition(DavErrorCondition::new(
                StatusCode::FORBIDDEN,
                CalCondition::SupportedCalendarComponent,
            )))
        }
    }
}

trait CalendarObjectComponent {
    fn object_component(&self) -> SupportedComponent;
}

impl CalendarObjectComponent for ArchivedICalendar {
    fn object_component(&self) -> SupportedComponent {
        self.components
            .iter()
            .find_map(|component| match component.component_type {
                ArchivedICalendarComponentType::VEvent => Some(SupportedComponent::VEvent),
                ArchivedICalendarComponentType::VTodo => Some(SupportedComponent::VTodo),
                ArchivedICalendarComponentType::VJournal => Some(SupportedComponent::VJournal),
                ArchivedICalendarComponentType::VFreebusy => Some(SupportedComponent::VFreebusy),
                ArchivedICalendarComponentType::VAvailability => {
                    Some(SupportedComponent::VAvailability)
                }
                _ => None,
            })
            .unwrap_or(SupportedComponent::Other)
    }
}

pub(crate) trait CalendarComponentSupport: Sync + Send {
    fn supported_components(
        &self,
        account_id: u32,
        calendar_id: u32,
    ) -> impl Future<Output = crate::Result<SupportedComponents>> + Send;

    fn assert_stored_event_supported(
        &self,
        event_account_id: u32,
        event_document_id: u32,
        account_id: u32,
        calendar_id: u32,
    ) -> impl Future<Output = crate::Result<Option<Archive<ArchiveBytes>>>> + Send;
}

impl CalendarComponentSupport for Server {
    async fn supported_components(
        &self,
        account_id: u32,
        calendar_id: u32,
    ) -> crate::Result<SupportedComponents> {
        self.store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                account_id,
                Collection::Calendar,
                calendar_id,
            ))
            .await
            .caused_by(trc::location!())?
            .map(|archive| {
                archive
                    .unarchive::<Calendar>()
                    .map(|calendar| calendar.supported_components.to_native())
            })
            .transpose()
            .caused_by(trc::location!())
            .map(|supported_components| {
                SupportedComponents(Bitmap::from(supported_components.unwrap_or_default()))
            })
            .map_err(Into::into)
    }

    async fn assert_stored_event_supported(
        &self,
        event_account_id: u32,
        event_document_id: u32,
        account_id: u32,
        calendar_id: u32,
    ) -> crate::Result<Option<Archive<ArchiveBytes>>> {
        let supported_components = self.supported_components(account_id, calendar_id).await?;
        if !supported_components.is_restricted() {
            return Ok(None);
        }

        let content = self
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                event_account_id,
                Collection::CalendarEvent,
                event_document_id,
                CalendarEventField::Content,
            ))
            .await
            .caused_by(trc::location!())?
            .ok_or(DavError::Code(StatusCode::NOT_FOUND))?;
        supported_components.assert_supports(
            content
                .unarchive::<CalendarEventContent>()
                .caused_by(trc::location!())?
                .data
                .event
                .object_component(),
        )?;
        Ok(Some(content))
    }
}

pub(crate) fn assert_event_privacy_access(
    flags: Option<u16>,
    viewer: EventViewer,
) -> crate::Result<()> {
    flags
        .map(EventPrivacy::from_flags)
        .unwrap_or_default()
        .check_access(viewer)
        .map_err(|denied| {
            DavError::Code(match denied {
                PrivacyDenied::Forbidden => StatusCode::FORBIDDEN,
                PrivacyDenied::NotFound => StatusCode::NOT_FOUND,
            })
        })
}

pub(crate) fn assert_event_privacy_allowed(
    ical: &ICalendar,
    viewer: EventViewer,
) -> crate::Result<()> {
    if ical.privacy().may_be_set_by(viewer) {
        Ok(())
    } else {
        Err(DavError::Code(StatusCode::FORBIDDEN))
    }
}

pub(crate) fn serves_ical(view: &ICalendar, submitted: &ICalendar) -> bool {
    view == submitted || ComparisonScope::SharedData.component_eq(view, 0, submitted, 0)
}

pub(crate) trait CalendarEventView: Sync + Send {
    fn calendar_event_view(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        event: &ArchivedCalendarEvent,
        content: &ArchivedCalendarEventContent,
        default_alerts: &mut DefaultAlertsResolver,
    ) -> impl Future<Output = crate::Result<Option<(CalendarEventContent, MergedOverrides)>>> + Send;
}

impl CalendarEventView for Server {
    async fn calendar_event_view(
        &self,
        access_token: &AccessToken,
        account_id: u32,
        event: &ArchivedCalendarEvent,
        content: &ArchivedCalendarEventContent,
        default_alerts: &mut DefaultAlertsResolver,
    ) -> crate::Result<Option<(CalendarEventContent, MergedOverrides)>> {
        let flags = event.flags.to_native();
        let is_owner = access_token.is_member(account_id);
        let personal_id = access_token.personal_id(account_id, Collection::Calendar);
        let is_private_view = !is_owner && EventPrivacy::from_flags(flags) != EventPrivacy::Public;
        let preferences = content.preferences(personal_id);
        let use_default_alerts = preferences.is_some_and(|p| p.use_default_alerts());
        if !is_private_view
            && !use_default_alerts
            && (is_owner
                || (flags & EVENT_HAS_ALARMS == 0
                    && preferences.is_none()
                    && !content
                        .data
                        .event
                        .components
                        .iter()
                        .any(|component| component.component_type.is_alarm())))
        {
            return Ok(None);
        }

        if is_private_view {
            let mut view = rkyv_deserialize::<_, CalendarEventContent>(content)
                .map(CalendarEventContent::into_private_view)
                .caused_by(trc::location!())?;
            let merged_overrides = view.data.event.blank_unreachable_components();
            return Ok(Some((view, merged_overrides)));
        }

        let resolved_default_alerts = default_alerts
            .resolve_for_ical_view(
                self,
                account_id,
                personal_id,
                content,
                event.names.iter().map(ArchivedDavName::parent_id),
            )
            .await
            .caused_by(trc::location!())?;
        let mut view =
            rkyv_deserialize::<_, CalendarEventContent>(content).caused_by(trc::location!())?;
        if !is_owner {
            let preferences = view
                .preferences
                .iter()
                .find(|p| p.account_id == personal_id);
            view.data
                .event
                .apply_user_data(preferences, UserDataView::AlertsOnly);
        }
        view.data
            .event
            .apply_default_alerts(&resolved_default_alerts, DefaultAlertsView::ICalendar);
        let merged_overrides = view.data.event.blank_unreachable_components();
        view.data.alarms = view.data.event.served_alarms();
        Ok(Some((view, merged_overrides)))
    }
}

trait UnreachableComponents {
    fn blank_unreachable_components(&mut self) -> MergedOverrides;
}

impl UnreachableComponents for ICalendar {
    fn blank_unreachable_components(&mut self) -> MergedOverrides {
        let mut is_reachable = vec![false; self.components.len()];
        let mut pending = vec![0u32];
        while let Some(component_id) = pending.pop() {
            match is_reachable.get_mut(component_id as usize) {
                Some(is_reachable) if !*is_reachable => *is_reachable = true,
                _ => continue,
            }
            if let Some(component) = self.components.get(component_id as usize) {
                pending.extend_from_slice(&component.component_ids);
            }
        }

        let mut merged_overrides = MergedOverrides::default();
        for (component_id, (component, is_reachable)) in
            self.components.iter_mut().zip(&is_reachable).enumerate()
        {
            if *is_reachable {
                continue;
            }
            if component.component_type.is_event_or_todo() && component.is_recurrence_override() {
                merged_overrides.push(component_id as u32);
            }
            *component = ICalendarComponent {
                component_type: ICalendarComponentType::Other(Default::default()),
                entries: vec![],
                component_ids: vec![],
            };
        }
        merged_overrides.set_base(self.base_component_id());
        merged_overrides
    }
}

trait ServedAlarms {
    fn served_alarms(&self) -> Box<[Alarm]>;
}

impl ServedAlarms for ICalendar {
    fn served_alarms(&self) -> Box<[Alarm]> {
        self.components
            .iter()
            .enumerate()
            .filter(|(_, component)| component.component_type.is_event_or_todo())
            .flat_map(|(parent_id, component)| {
                component.component_ids.iter().filter_map(move |alarm_id| {
                    self.components
                        .get(*alarm_id as usize)
                        .filter(|alarm| alarm.component_type.is_alarm())
                        .and_then(|alarm| alarm.expand_alarm(*alarm_id as u16, parent_id as u16))
                })
            })
            .collect()
    }
}

pub(crate) fn user_data_error(err: UserDataError) -> DavError {
    DavError::Condition(
        DavErrorCondition::new(
            StatusCode::PRECONDITION_FAILED,
            CalCondition::ValidCalendarData,
        )
        .with_details(match err {
            UserDataError::TooManyKeywords => "Too many categories",
            UserDataError::KeywordTooLong => "Category too long",
            UserDataError::TooManyAlerts => "Too many alarms",
            UserDataError::TooManyInstances => {
                "Too many recurrence instances with personal properties"
            }
            UserDataError::InvalidColor => "Invalid color",
        }),
    )
}

/// Invitations spec 5.1, 6.3 step 1: takes the UID creation lease for a key
/// account's PUT create, then checks the UID against current archives
/// (`za_assert_is_unique_uid`) while holding it. A lease that stays held
/// through the retries (two creators of one UID in one account within a
/// second) answers 503 with a trace; a hit answers `NoUidConflict` after the
/// lease is released.
pub(crate) async fn za_take_uid_lease(
    server: &Server,
    resources: &GroupwareResources,
    access_token: &AccessToken,
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
    if let Err(err) = za_assert_is_unique_uid(
        server,
        resources,
        access_token,
        account_id,
        calendar_id,
        uid,
    )
    .await
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
    resources: &GroupwareResources,
    access_token: &AccessToken,
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
    // Upstream's rule at 1.0 (`assert_is_unique_uid`): a conflict answers
    // 403 `NoUidConflict`, and a sharee whose only conflict is a secret
    // event gets a plain 403, without its href.
    let is_owner = access_token.is_member(account_id);
    let mut has_hidden_conflict = false;
    for document_id in hits {
        let Some(stored) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
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
            if !is_owner
                && EventPrivacy::from_flags(event.flags.to_native()) == EventPrivacy::Secret
            {
                has_hidden_conflict = true;
                continue;
            }
            let href = resources
                .container_resource_path_by_id(calendar_id)
                .map(|calendar| {
                    resources.format_item(&format!("{}/{}", calendar.path(), name.name))
                })
                .unwrap_or_default();
            return Err(DavError::Condition(DavErrorCondition::new(
                StatusCode::FORBIDDEN,
                CalCondition::NoUidConflict(href.into()),
            )));
        }
    }
    if has_hidden_conflict {
        Err(DavError::Code(StatusCode::FORBIDDEN))
    } else {
        Ok(())
    }
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
    resources: &GroupwareResources,
    access_token: &AccessToken,
    lease: ZaUidLeaseGuard,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
) -> crate::Result<HttpResponse> {
    let Err(error) = za_finish_create_loss(
        server,
        resources,
        access_token,
        lease,
        account_id,
        calendar_id,
        uid,
        CreateLoss::ConfirmedLease,
    )
    .await;
    Err(error)
}

enum CreateLoss {
    ConfirmedLease,
    OpaqueSecondCommit,
}

/// `za_lost_create_fence`'s body and the second commit's loss: its type
/// cannot be `Ok` (final review, ledger 181), so the compiler keeps a lost
/// create from reaching the mail submission (iMIP invariant 46).
#[allow(clippy::too_many_arguments)]
async fn za_finish_create_loss(
    server: &Server,
    resources: &GroupwareResources,
    access_token: &AccessToken,
    lease: ZaUidLeaseGuard,
    account_id: u32,
    calendar_id: u32,
    uid: &str,
    loss: CreateLoss,
) -> crate::Result<std::convert::Infallible> {
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
    let retry = za_take_uid_lease(
        server,
        resources,
        access_token,
        account_id,
        calendar_id,
        uid,
    )
    .await?;
    retry.release().await;
    // The retake found no copy in this calendar. Keep the existing resend
    // answer for either a confirmed lease loss or an opaque second loss.
    Err(DavError::Code(StatusCode::SERVICE_UNAVAILABLE))
}

/// The bounded second create commit. Its assertion loss is opaque: release
/// our fenced lease before the existing UID retake/outcome, without another
/// calendar read, archive retry or fan-out.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn za_commit_create(
    server: &Server,
    resources: &GroupwareResources,
    access_token: &AccessToken,
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
        Err(err) if err.is_assertion_failure() => {
            let Err(error) = za_finish_create_loss(
                server,
                resources,
                access_token,
                lease,
                account_id,
                calendar_id,
                uid,
                CreateLoss::OpaqueSecondCommit,
            )
            .await;
            Err(error)
        }
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
            ItipError::TooManyRecipients => Some(CalCondition::MaxAttendeesPerInstance),
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
    sealed: &mut groupware::calendar::seal::ZaEvent,
) -> crate::Result<Option<Vec<groupware::calendar::sharing::resolve::ZaCalendarContext>>> {
    if before.is_empty() {
        return Ok(None);
    }
    let after =
        za_write_contexts(server, token, owner_id, &sealed.meta, calendar_id, required).await?;
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

/// The scheduling roles of a key account's write (release 4 delegated
/// scheduling): the sender is the writer's account info with the owner's
/// addresses added (0.16's `scheduling_account_info`, which 1.0 removed; its
/// handlers now pass the owner's), the writer's and the owner's own infos.
pub(crate) struct ZaRoles {
    pub sender: AccountInfo,
    pub writer: AccountInfo,
    pub owner: AccountInfo,
}

pub(crate) async fn za_roles(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
) -> crate::Result<ZaRoles> {
    let writer = server
        .account_info(token.account_id())
        .await
        .caused_by(trc::location!())?;
    let owner = if owner_id == token.account_id() {
        writer.clone()
    } else {
        server
            .account_info(owner_id)
            .await
            .caused_by(trc::location!())?
    };
    let mut sender = writer.clone();
    if owner_id != token.account_id() {
        for address in &owner.addresses {
            if !sender.addresses.contains(address) {
                sender.addresses.push(address.clone());
            }
        }
    }
    Ok(ZaRoles {
        sender,
        writer,
        owner,
    })
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
