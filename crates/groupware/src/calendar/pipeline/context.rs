/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::error::{ZaPipelineError, za_read_refusal};
use crate::calendar::{
    ArchivedCalendarEvent, CalendarEvent,
    seal::{
        ZaEvent, ZaEventView, calendar_view_in_domain, event_view_in_domain, rewrap_event,
        seal_error,
    },
    sharing::{
        boundary::{za_repair_owned_context, za_repair_owned_contexts},
        resolve::{
            ZaCalendarContext, ZaCalendarError, calendars_changed, za_calendar_context,
            za_event_contexts,
        },
    },
};
use common::{Server, auth::AccessToken};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};
use store::write::{Archive, ArchiveBytes, serialize::rkyv_deserialize};
use trc::AddContext;
use types::acl::Acl;
use utils::map::bitmap::Bitmap;

/// This map belongs to one read request only. Writers never consume it.
pub type ZaReadMemo = HashMap<(u32, u32), Arc<ZaCalendarContext>>;

/// Explicit owner-session repair; the groupware resolver remains read-only.
pub async fn za_mutation_context(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    required: Bitmap<Acl>,
) -> Result<Option<ZaCalendarContext>, ZaCalendarError> {
    let context = za_calendar_context(server, token, owner_id, calendar_id, required).await?;
    match (context, token.session_keys()) {
        (Some(context), Some(keys)) => za_repair_owned_context(server, keys, context)
            .await
            .map(Some),
        (context, _) => Ok(context),
    }
}

pub async fn za_read_context(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    required: Bitmap<Acl>,
    memo: &mut ZaReadMemo,
) -> Result<Option<Arc<ZaCalendarContext>>, ZaCalendarError> {
    if let Some(context) = memo.get(&(owner_id, calendar_id)) {
        context.check_access(token, required)?;
        return Ok(Some(context.clone()));
    }
    let context = za_mutation_context(server, token, owner_id, calendar_id, required)
        .await?
        .map(Arc::new);
    if let Some(context) = &context {
        memo.insert((owner_id, calendar_id), context.clone());
    }
    Ok(context)
}

/// Spec 7.3 at 1.0: the read view of a stored event in `context`'s key
/// domain. `meta` is the stored metadata row and `content` the stored
/// content archive; the view's content keeps the stored archive's version,
/// so identity stays bound to the stored bytes. Read-only, never written
/// back.
pub fn za_event_view_in(
    context: &ZaCalendarContext,
    meta: &ArchivedCalendarEvent,
    content: &Archive<ArchiveBytes>,
    owner_id: u32,
    document_id: u32,
) -> trc::Result<ZaEventView> {
    let meta = rkyv_deserialize::<_, CalendarEvent>(meta).caused_by(trc::location!())?;
    event_view_in_domain(&meta, content, &context.domain)
        .map_err(|error| seal_error(error, owner_id, document_id))
}

/// The read context of an event's calendar, with `za_read_event_view`'s
/// error scopes: `Ok(Ok(None))` for a non-key account (upstream's path).
pub async fn za_read_event_context(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    document_id: u32,
    calendar_id: u32,
    memo: &mut ZaReadMemo,
) -> Result<trc::Result<Option<Arc<ZaCalendarContext>>>, ZaPipelineError> {
    match za_read_context(
        server,
        token,
        owner_id,
        calendar_id,
        Bitmap::new().with_item(Acl::ReadItems),
        memo,
    )
    .await
    {
        Ok(context) => Ok(Ok(context)),
        Err(error) => Ok(Err(za_read_refusal(error, owner_id, document_id)?)),
    }
}

/// The read view of a stored event (`meta` row, `content` archive) through a
/// fresh or memoized read context. The outer `Err` is a refusal that fails
/// the request; the inner one fails one item. `None` for a non-key account.
#[allow(clippy::too_many_arguments)]
pub async fn za_read_event_view(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    document_id: u32,
    calendar_id: u32,
    meta: &ArchivedCalendarEvent,
    content: &Archive<ArchiveBytes>,
    memo: &mut ZaReadMemo,
) -> Result<trc::Result<Option<ZaEventView>>, ZaPipelineError> {
    Ok(
        match za_read_event_context(server, token, owner_id, document_id, calendar_id, memo).await?
        {
            Ok(Some(context)) => {
                za_event_view_in(&context, meta, content, owner_id, document_id).map(Some)
            }
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        },
    )
}

/// A calendar's read view through a fresh or memoized read context: the
/// requested principal's personal bundle and the shared properties opened,
/// the stored version kept. Error scopes as `za_read_event_view`.
pub async fn za_read_calendar_view(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    memo: &mut ZaReadMemo,
) -> Result<trc::Result<Option<(Archive<ArchiveBytes>, Arc<ZaCalendarContext>)>>, ZaPipelineError> {
    Ok(
        match za_read_context(
            server,
            token,
            owner_id,
            calendar_id,
            Bitmap::new().with_item(Acl::Read),
            memo,
        )
        .await
        {
            Ok(Some(context)) => {
                calendar_view_in_domain(&context.stored, &context.domain, context.personal_id)
                    .map(|view| Some((view, context.clone())))
                    .map_err(|error| seal_error(error, owner_id, calendar_id))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(za_read_refusal(error, owner_id, calendar_id)?),
        },
    )
}

pub async fn za_write_contexts(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    event: &CalendarEvent,
    addressed: u32,
    required: Acl,
) -> Result<Vec<ZaCalendarContext>, ZaPipelineError> {
    let contexts = za_event_contexts(
        server,
        token,
        owner_id,
        event,
        addressed,
        Bitmap::new().with_item(required),
    )
    .await
    .map_err(|error| ZaPipelineError::from_calendar(error, owner_id, addressed))?;
    if let Some(keys) = token.session_keys() {
        za_repair_owned_contexts(server, keys, contexts)
            .await
            .map_err(|error| ZaPipelineError::from_calendar(error, owner_id, addressed))
    } else {
        Ok(contexts)
    }
}

/// Called once after an ambiguous assertion loss. Keep the original event
/// assertion and already-sealed payload; only its key envelope can change.
pub async fn za_refresh_event(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    addressed: u32,
    required: Acl,
    before: &[ZaCalendarContext],
    sealed: &mut ZaEvent,
) -> Result<Option<Vec<ZaCalendarContext>>, ZaPipelineError> {
    if before.is_empty() {
        return Ok(None);
    }
    let after =
        za_write_contexts(server, token, owner_id, &sealed.meta, addressed, required).await?;
    if !calendars_changed(before, &after) {
        return Ok(None);
    }
    rewrap_event(sealed, &before[0].domain, &after[0].domain)
        .map_err(|error| seal_error(error, owner_id, addressed))?;
    Ok(Some(after))
}

/// Resolve fresh own-id grants before any foreign path is emitted. Corrupt eligible
/// archives remain errors; only an actual access refusal or deleted calendar is omitted.
pub async fn za_foreign_calendar_contexts(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    calendar_ids: impl Iterator<Item = u32>,
    requested: Option<u32>,
) -> Result<BTreeMap<u32, Arc<ZaCalendarContext>>, ZaPipelineError> {
    let mut eligible = BTreeMap::new();
    for id in calendar_ids.filter(|id| requested.is_none_or(|requested| requested == *id)) {
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
            Err(error) => return Err(ZaPipelineError::from_calendar(error, owner_id, id)),
        }
    }
    Ok(eligible)
}
