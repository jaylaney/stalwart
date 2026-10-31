/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access PROPFIND and REPORT hooks (upgrade Task 4, the 15 call sites
//! of 0.16's `propfind.rs`): key-account calendar items are read through a
//! fresh own-id read context and unsealed before any use, on the
//! batch-loaded and the resource-cache paths alike; a pending inbox item is
//! never listed; dead-property containers are opened before their names or
//! values are served; foreign key calendars sync only through authorized
//! tombstones.

use super::PropFindItem;
use crate::{
    DavError, DavErrorCondition,
    common::{
        ArchivedResource, EventContent, SyncType,
        za::{
            ZaReadMemo, ZeroAccessGate, za_open_container, za_read_calendar_view,
            za_read_event_context,
        },
    },
};
use common::{GroupwareResources, Server, auth::AccessToken, storage::dav::canonical_path_segment};
use dav_proto::schema::response::{BaseCondition, MultiStatus};
use groupware::calendar::{
    ArchivedCalendarEvent, ArchivedCalendarEventContent, ArchivedCalendarEventNotificationContent,
    Calendar, CalendarEvent, CalendarEventContent, CalendarEventNotificationContent,
    seal::{ZaContainerOf, ZaEvent, open_drained_item, seal_error, unseal_event_in_domain},
    sharing::resolve::{ZaCalendarContext, ZaCalendarError, za_calendar_context},
};
use http_proto::HttpResponse;
use hyper::StatusCode;
use std::{collections::BTreeMap, sync::Arc};
use store::{
    Deserialize, Serialize, ValueKey,
    write::{
        Archive, ArchiveBytes, Archiver, log::CalendarTombstone, metadata::MetadataBuf,
        serialize::rkyv_deserialize,
    },
};
use trc::AddContext;
use types::{acl::Acl, collection::Collection, field::CalendarEventField};
use utils::map::bitmap::Bitmap;
use vault::session::SessionKeys;

/// What a PROPFIND does with one item of a key account's calendar store.
pub(super) enum ZaItem {
    /// Not a key account's calendar item: upstream's path.
    Plain,
    /// Leave the item out of the listing (a refused calendar in a home
    /// listing, a pending inbox item).
    Skip,
    /// Answer the item with this status alone.
    Status(StatusCode),
    /// A key account's item, read through these views.
    View(ZaViews),
}

/// The read views of one key-account item: they replace the stored archives
/// the loader returned, and keep their versions, so every ETag stays bound
/// to the stored bytes. Read-only; nothing here is written back.
#[derive(Default)]
pub(super) struct ZaViews {
    meta: Option<Archive<ArchiveBytes>>,
    content: Option<Archive<ArchiveBytes>>,
    context: Option<Arc<ZaCalendarContext>>,
    uid: Option<String>,
}

impl ZaViews {
    /// Points `archive` at the views (an event's metadata row and content, a
    /// calendar, an inbox item's content).
    pub fn apply<'x>(&'x self, archive: &mut ArchivedResource<'x>) -> trc::Result<()> {
        match archive {
            ArchivedResource::Calendar(calendar) => {
                if let Some(meta) = &self.meta {
                    *calendar = meta.to_unarchived::<Calendar>()?;
                }
            }
            ArchivedResource::CalendarEvent(event, content) => {
                if let Some(meta) = &self.meta {
                    *event = meta.to_unarchived::<CalendarEvent>()?;
                }
                if let Some(view) = &self.content {
                    *content = Some(EventContent::Stored(
                        view.unarchive::<CalendarEventContent>()?,
                    ));
                }
            }
            ArchivedResource::CalendarEventNotification(_, content) => {
                if let Some(view) = &self.content {
                    *content = Some(view.unarchive::<CalendarEventNotificationContent>()?);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The context whose key domain opens this item's dead properties.
    pub fn context(&self) -> Option<&ZaCalendarContext> {
        self.context.as_deref()
    }
}

/// Spec 8.2 and invitations spec 6.6, for one PROPFIND or REPORT item. The
/// read context is resolved fresh (or from this request's memo) for every
/// key-account calendar item, whether its archive was batch-loaded or is
/// answered from the resource cache; a refusal fails the request, except a
/// calendar refused inside a home listing, which is left out. An unseal
/// failure fails one item (500), a missing calendar answers 404.
pub(super) async fn za_propfind_item(
    server: &Server,
    access_token: &AccessToken,
    item: &PropFindItem,
    collection: Collection,
    archive: &ArchivedResource<'_>,
    home_listing: bool,
    memo: &mut ZaReadMemo,
) -> crate::Result<ZaItem> {
    let account_id = item.account_id;
    match collection {
        Collection::Calendar
            if !matches!(
                archive,
                ArchivedResource::CalendarEventNotificationCollection(_)
            ) =>
        {
            match za_read_calendar_view(server, access_token, account_id, item.document_id, memo)
                .await
            {
                Ok(Ok(Some((view, context)))) => Ok(ZaItem::View(ZaViews {
                    meta: Some(view),
                    context: Some(context),
                    ..Default::default()
                })),
                Ok(Ok(None)) => Ok(ZaItem::Plain),
                Ok(Err(error)) => Ok(item_failure(error)),
                Err(error) => refusal(error, home_listing),
            }
        }
        Collection::CalendarEvent => {
            let Some(calendar_id) = item.parent_id else {
                return Ok(ZaItem::Status(StatusCode::NOT_FOUND));
            };
            let context = match za_read_event_context(
                server,
                access_token,
                account_id,
                item.document_id,
                calendar_id,
                memo,
            )
            .await
            {
                Ok(Ok(Some(context))) => context,
                Ok(Ok(None)) => return Ok(ZaItem::Plain),
                Ok(Err(error)) => return Ok(item_failure(error)),
                Err(error) => return refusal(error, home_listing),
            };
            match archive {
                ArchivedResource::CalendarEvent(event, content) => {
                    // The loader reads the content only on demand; a key
                    // account's display name and size live in it.
                    let loaded;
                    let content = match content {
                        Some(content) => content.stored(),
                        None => {
                            match server
                                .store()
                                .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                                    account_id,
                                    Collection::CalendarEvent,
                                    item.document_id,
                                    CalendarEventField::Content,
                                ))
                                .await
                                .caused_by(trc::location!())?
                            {
                                Some(content) => {
                                    loaded = content;
                                    loaded
                                        .unarchive::<CalendarEventContent>()
                                        .caused_by(trc::location!())?
                                }
                                None => return Ok(ZaItem::Status(StatusCode::NOT_FOUND)),
                            }
                        }
                    };
                    match za_event_views(&context, event, content, account_id, item.document_id) {
                        Ok(views) => Ok(ZaItem::View(views)),
                        Err(error) => Ok(item_failure(error)),
                    }
                }
                _ => Ok(ZaItem::View(ZaViews {
                    context: Some(context),
                    ..Default::default()
                })),
            }
        }
        Collection::CalendarEventNotification if !item.is_container => {
            let ArchivedResource::CalendarEventNotification(_, content) = archive else {
                return Ok(ZaItem::Plain);
            };
            let keys = match server.za_session_keys(access_token, account_id).await? {
                Some(keys) => keys,
                None => return Ok(ZaItem::Plain),
            };
            if za_row_is_pending(archive) {
                return Ok(ZaItem::Skip);
            }
            match content {
                Some(content) => {
                    match za_item_content_view(content, &keys, account_id, item.document_id) {
                        Ok(view) => Ok(ZaItem::View(ZaViews {
                            content: Some(view),
                            ..Default::default()
                        })),
                        Err(error) => Ok(item_failure(error)),
                    }
                }
                None => Ok(ZaItem::View(ZaViews::default())),
            }
        }
        _ => Ok(ZaItem::Plain),
    }
}

fn za_row_is_pending(archive: &ArchivedResource<'_>) -> bool {
    match archive {
        ArchivedResource::CalendarEventNotification(row, _) => {
            groupware::calendar::seal::archived_item_is_pending(row.inner)
        }
        _ => false,
    }
}

/// The views of a key account's event read through `context`: the opened
/// content, and a metadata row carrying the bundle's display name (upgrade
/// ruling 15) and the plaintext size, with the stored row's version.
fn za_event_views(
    context: &Arc<ZaCalendarContext>,
    event: &Archive<&ArchivedCalendarEvent>,
    content: &ArchivedCalendarEventContent,
    account_id: u32,
    document_id: u32,
) -> trc::Result<ZaViews> {
    let mut opened = ZaEvent::new(
        rkyv_deserialize::<_, CalendarEvent>(event.inner).caused_by(trc::location!())?,
        rkyv_deserialize::<_, CalendarEventContent>(content).caused_by(trc::location!())?,
    );
    unseal_event_in_domain(&mut opened, &context.domain)
        .map_err(|error| seal_error(error, account_id, document_id))?;
    let uid = opened.meta.uid.clone();
    opened.meta.size = opened.content.data.event.to_string().len() as u32;
    let (meta, content) = opened.into_parts();
    let mut meta = archive_of_event(meta)?;
    meta.version = event.version;
    Ok(ZaViews {
        meta: Some(meta),
        content: Some(archive_of_content(content)?),
        context: Some(context.clone()),
        uid: Some(uid),
    })
}

fn archive_of_event(event: CalendarEvent) -> trc::Result<Archive<ArchiveBytes>> {
    let bytes = Archiver::new(event)
        .serialize()
        .caused_by(trc::location!())?;
    <Archive<ArchiveBytes> as Deserialize>::deserialize(&bytes).caused_by(trc::location!())
}

fn archive_of_content(content: CalendarEventContent) -> trc::Result<Archive<ArchiveBytes>> {
    let bytes = Archiver::new(content)
        .serialize()
        .caused_by(trc::location!())?;
    <Archive<ArchiveBytes> as Deserialize>::deserialize(&bytes).caused_by(trc::location!())
}

/// Invitations spec 6.6: a drained inbox item's content, opened under the
/// owner's keys; content the fork did not seal is returned as it is stored.
fn za_item_content_view(
    content: &ArchivedCalendarEventNotificationContent,
    keys: &SessionKeys,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Archive<ArchiveBytes>> {
    let mut item = rkyv_deserialize::<_, CalendarEventNotificationContent>(content)
        .caused_by(trc::location!())?;
    if let CalendarEventNotificationContent::Itip {
        message,
        previous: None,
        current: None,
    } = &mut item
    {
        open_drained_item(message, keys, account_id)
            .map_err(|error| seal_error(error, account_id, document_id))?;
    }
    let bytes = Archiver::new(item)
        .serialize()
        .caused_by(trc::location!())?;
    <Archive<ArchiveBytes> as Deserialize>::deserialize(&bytes).caused_by(trc::location!())
}

fn refusal(error: DavError, home_listing: bool) -> crate::Result<ZaItem> {
    match error {
        DavError::Code(StatusCode::FORBIDDEN) if home_listing => Ok(ZaItem::Skip),
        DavError::Code(StatusCode::NOT_FOUND) => Ok(ZaItem::Status(StatusCode::NOT_FOUND)),
        error => Err(error),
    }
}

fn item_failure(error: trc::Error) -> ZaItem {
    trc::error!(error);
    ZaItem::Status(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Rulings 16 and 23: a key account's dead-property container opened in its
/// calendar's key domain before its names or values are served. `Ok(None)`
/// when there is no container; `Err` is one item's failure (500).
pub(super) async fn za_open_dead_container(
    server: &Server,
    views: &ZaViews,
    item: &PropFindItem,
    collection: Collection,
    container: &MetadataBuf,
) -> trc::Result<MetadataBuf> {
    let context = views.context().ok_or_else(|| {
        trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access: dead properties without a read context")
    })?;
    match collection {
        Collection::Calendar => za_open_container(
            container,
            &context.domain,
            ZaContainerOf::Calendar(item.document_id),
            item.account_id,
            item.document_id,
        ),
        Collection::CalendarEvent => {
            let uid = match &views.uid {
                Some(uid) => uid.clone(),
                None => server
                    .store()
                    .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                        item.account_id,
                        Collection::CalendarEvent,
                        item.document_id,
                    ))
                    .await
                    .caused_by(trc::location!())?
                    .ok_or_else(|| trc::StoreEvent::NotFound.into_err())?
                    .unarchive::<CalendarEvent>()
                    .caused_by(trc::location!())?
                    .uid
                    .to_string(),
            };
            za_open_container(
                container,
                &context.domain,
                ZaContainerOf::Event(&uid),
                item.account_id,
                item.document_id,
            )
        }
        _ => Err(trc::StoreEvent::UnexpectedError
            .into_err()
            .details("zero-access: dead properties of another collection")),
    }
}

/// The `propname` form of PROPFIND reads no archive. Invitations spec 6.6
/// and the sharing read rule still apply: a foreign key-account item needs
/// a fresh own-id read context (a refused calendar is left out of a home
/// listing), and a pending inbox item is not listed. `None` leaves the item
/// out; `Some(views)` carries the context for its dead-property names.
pub(super) async fn za_propname_item(
    server: &Server,
    access_token: &AccessToken,
    item: &PropFindItem,
    collection_container: Collection,
    collection: Collection,
    home_listing: bool,
    memo: &mut ZaReadMemo,
) -> crate::Result<Option<ZaViews>> {
    let mut views = ZaViews::default();
    if collection_container == Collection::Calendar
        && crate::common::za::is_key_account(server, item.account_id).await?
    {
        let (calendar_id, required) = if item.is_container {
            (item.document_id, Acl::Read)
        } else {
            match item.parent_id {
                Some(parent_id) => (parent_id, Acl::ReadItems),
                None => return Ok(None),
            }
        };
        match crate::common::za::za_read_context(
            server,
            access_token,
            item.account_id,
            calendar_id,
            Bitmap::new().with_item(required),
            memo,
        )
        .await
        {
            Ok(context) => views.context = context,
            Err(ZaCalendarError::Forbidden) if home_listing => return Ok(None),
            Err(error) => {
                return Err(crate::calendar::za_calendar_error(
                    error,
                    item.account_id,
                    calendar_id,
                ));
            }
        }
    }
    // Invitations spec 6.6: a pending inbox item is not listed.
    if !item.is_container
        && crate::common::za::za_cached_item_hidden(
            server,
            access_token,
            item.account_id,
            collection,
            item.document_id,
        )
        .await?
    {
        return Ok(None);
    }
    Ok(Some(views))
}

/// A PROPFIND that matched nothing (outside sync-collection) is 404, every
/// other query answers 207; shared by the `propname` branch and the tail.
pub(super) fn dav_query_response(
    is_propfind: bool,
    response: &MultiStatus,
    sync_type: &SyncType,
) -> HttpResponse {
    if !is_propfind || !response.response.0.is_empty() || !sync_type.is_none() {
        HttpResponse::new(StatusCode::MULTI_STATUS).with_xml_body(response.to_string())
    } else {
        HttpResponse::new(StatusCode::NOT_FOUND)
    }
}

/// Resolve fresh own-id grants before any foreign path is emitted. Corrupt eligible
/// archives remain errors; only an actual access refusal or deleted calendar is omitted.
pub(super) async fn foreign_calendar_contexts(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    resources: &GroupwareResources,
    requested: Option<u32>,
) -> crate::Result<BTreeMap<u32, Arc<ZaCalendarContext>>> {
    let mut eligible = BTreeMap::new();
    for id in resources
        .document_ids(true)
        .filter(|id| requested.is_none_or(|requested| requested == *id))
    {
        match za_calendar_context(
            server,
            token,
            owner_id,
            id,
            Bitmap::new().with_item(Acl::Read),
        )
        .await
        {
            Ok(Some(context)) => {
                eligible.insert(id, Arc::new(context));
            }
            Ok(None) | Err(ZaCalendarError::Forbidden) => (),
            Err(ZaCalendarError::Store(error))
                if error.matches(trc::EventType::Store(trc::StoreEvent::NotFound)) => {}
            Err(error) => return Err(crate::calendar::za_calendar_error(error, owner_id, id)),
        }
    }
    Ok(eligible)
}

pub(super) fn invalid_calendar_sync_token() -> DavError {
    DavErrorCondition::new(StatusCode::FORBIDDEN, BaseCondition::ValidSyncToken).into()
}

/// The href prefix of each eligible calendar the response may disclose
/// tombstones under (inventory finding 6: 1.0 names a stored calendar's path
/// segment with `canonical_path_segment`).
pub(super) fn tombstone_prefixes(
    eligible: &BTreeMap<u32, Arc<ZaCalendarContext>>,
    resources: &GroupwareResources,
    requested: Option<u32>,
) -> crate::Result<Vec<(u32, String)>> {
    let mut prefixes = Vec::new();
    for (&calendar_id, context) in eligible {
        if requested.is_none_or(|id| id == calendar_id) {
            let calendar = context
                .stored
                .deserialize::<Calendar>()
                .caused_by(trc::location!())?;
            prefixes.push((
                calendar_id,
                format!(
                    "{}{}/",
                    resources.base_path,
                    canonical_path_segment(&calendar.name)
                ),
            ));
        }
    }
    Ok(prefixes)
}

/// Coverage is a multiset of physical commit id plus exact original path.
/// Private identities cover raw entries but never authorize disclosure.
pub(super) fn authorize_calendar_tombstones(
    raw: Vec<(u64, String)>,
    identities: Vec<(u64, String)>,
    prefixes: &[(u32, String)],
) -> crate::Result<Vec<String>> {
    let relevant = |path: &str| prefixes.iter().any(|(_, prefix)| path.starts_with(prefix));
    let mut coverage = BTreeMap::<(u64, String), usize>::new();
    for (change_id, path) in raw {
        if relevant(&path) {
            *coverage.entry((change_id, path)).or_default() += 1;
        }
    }
    let mut authorized = Vec::new();
    for (change_id, encoded) in identities {
        // Parse after scoping: malformed unrelated history must not expand a
        // direct calendar request's error or ring-resolution scope.
        let path = encoded
            .split_once(':')
            .map(|(_, path)| path)
            .unwrap_or(&encoded);
        if !relevant(path) {
            continue;
        }
        let identity =
            CalendarTombstone::decode(&encoded).ok_or_else(invalid_calendar_sync_token)?;
        let key = (change_id, identity.path.clone());
        let count = coverage
            .get_mut(&key)
            .ok_or_else(invalid_calendar_sync_token)?;
        *count -= 1;
        if *count == 0 {
            coverage.remove(&key);
        }
        if prefixes
            .iter()
            .any(|(id, prefix)| *id == identity.calendar_id && identity.path.starts_with(prefix))
        {
            authorized.push(identity.path);
        }
    }
    if !coverage.is_empty() {
        return Err(invalid_calendar_sync_token());
    }
    Ok(authorized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tombstones_need_identity_coverage() {
        let prefixes = vec![(7, "/dav/cal/a/shared/".to_string())];
        let path = "/dav/cal/a/shared/e.ics".to_string();
        let authorized = authorize_calendar_tombstones(
            vec![(3, path.clone())],
            vec![(3, CalendarTombstone::encode(7, &path))],
            &prefixes,
        )
        .unwrap_or_else(|_| panic!("tombstones refused"));
        assert_eq!(authorized, vec![path.clone()]);
        // A raw entry without its identity refuses the token.
        assert!(authorize_calendar_tombstones(vec![(3, path.clone())], vec![], &prefixes).is_err());
        // An identity of another calendar covers but does not disclose.
        assert!(
            authorize_calendar_tombstones(
                vec![(3, path.clone())],
                vec![(3, CalendarTombstone::encode(8, &path))],
                &prefixes,
            )
            .unwrap_or_else(|_| panic!("tombstones refused"))
            .is_empty()
        );
        // Unrelated history is out of scope.
        assert!(
            authorize_calendar_tombstones(
                vec![(4, "/dav/cal/a/other/x.ics".to_string())],
                vec![(4, "garbage".to_string())],
                &prefixes,
            )
            .unwrap_or_else(|_| panic!("tombstones refused"))
            .is_empty()
        );
    }
}
