/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{DavError, DavResourceName};
use common::{Server, auth::AccessToken};
use groupware::calendar::{
    CalendarEventNotification,
    seal::{
        SealError, archived_item_is_pending, calendar_view_in_domain, event_view_in_domain,
        seal_error, unseal_event_archive, unseal_item_archive,
    },
    sharing::resolve::{ZaCalendarContext, ZaCalendarError, za_calendar_context},
};
use hyper::StatusCode;
use std::{borrow::Cow, collections::HashMap, future::Future, sync::Arc};
use store::{
    ValueKey,
    write::{AlignedBytes, Archive},
};
use trc::AddContext;
use types::{acl::Acl, collection::Collection};
use utils::map::bitmap::Bitmap;
use vault::session::SessionKeys;

/// What a free-busy computation may read from an account (ruling R6).
pub(crate) enum ZaFreeBusy {
    /// Not a key account: the upstream path.
    Plain,
    /// A key account whose keys this session holds: read unsealed events.
    Unsealed(Arc<SessionKeys>),
    /// A key account whose keys this session does not hold: report no busy
    /// periods, so nothing of the account is disclosed.
    Withheld,
}

pub(crate) trait ZeroAccessGate: Sync + Send {
    /// `None` for a non-key account (unchanged upstream path). For a key
    /// account, the keys of that account held by this session, or 403.
    fn za_session_keys(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> impl Future<Output = crate::Result<Option<Arc<SessionKeys>>>> + Send;

    /// Ruling R6: like `za_session_keys`, but a key account without its
    /// keys yields `Withheld` instead of 403.
    fn za_freebusy_access(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> impl Future<Output = crate::Result<ZaFreeBusy>> + Send;

    /// Spec 8.1: a calendar COPY or MOVE across accounts is refused (403)
    /// when either side is a key account.
    fn za_refuse_cross_account(
        &self,
        from_account_id: u32,
        to_account_id: u32,
    ) -> impl Future<Output = crate::Result<()>> + Send;
}

impl ZeroAccessGate for Server {
    async fn za_session_keys(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> crate::Result<Option<Arc<SessionKeys>>> {
        // An unknown account is not a key account: upstream's outcome stands.
        if !is_key_account(self, account_id).await? {
            return Ok(None);
        }
        match access_token.za_keys_for(account_id) {
            Some(keys) => Ok(Some(keys.clone())),
            None => {
                trc::event!(
                    Security(trc::SecurityEvent::Unauthorized),
                    AccountId = account_id,
                    Details = "zero-access: calendar access without session keys",
                );
                Err(DavError::Code(StatusCode::FORBIDDEN))
            }
        }
    }

    async fn za_freebusy_access(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> crate::Result<ZaFreeBusy> {
        if !is_key_account(self, account_id).await? {
            return Ok(ZaFreeBusy::Plain);
        }
        Ok(match access_token.za_keys_for(account_id) {
            Some(keys) => ZaFreeBusy::Unsealed(keys.clone()),
            None => ZaFreeBusy::Withheld,
        })
    }

    async fn za_refuse_cross_account(
        &self,
        from_account_id: u32,
        to_account_id: u32,
    ) -> crate::Result<()> {
        if from_account_id != to_account_id
            && (is_key_account(self, from_account_id).await?
                || is_key_account(self, to_account_id).await?)
        {
            trc::event!(
                Security(trc::SecurityEvent::Unauthorized),
                AccountId = from_account_id,
                Details = "zero-access: calendar copy or move across accounts",
            );
            Err(DavError::Code(StatusCode::FORBIDDEN))
        } else {
            Ok(())
        }
    }
}

/// Calendar REPORTs (calendar-query, calendar-multiget, free-busy-query) read
/// the calendar store whatever the request URI's prefix, but the gate only
/// sees calendar and scheduling URIs: refuse every other prefix (405, as
/// upstream does for principal REPORTs sent elsewhere).
pub(crate) fn za_calendar_report_uri(resource: DavResourceName) -> crate::Result<()> {
    if matches!(resource, DavResourceName::Cal | DavResourceName::Scheduling) {
        Ok(())
    } else {
        Err(DavError::Code(StatusCode::METHOD_NOT_ALLOWED))
    }
}

/// Spec 7.3: the content view of a stored event. Without keys (a non-key
/// account) the stored archive is returned as is. The view keeps the stored
/// `version`, so ETags stay bound to the stored bytes; it is read-only and
/// never written back.
pub(crate) fn za_event_view<'x>(
    stored: &'x Archive<AlignedBytes>,
    keys: Option<&Arc<SessionKeys>>,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Cow<'x, Archive<AlignedBytes>>> {
    match keys {
        Some(keys) => unseal_event_archive(stored, keys, account_id)
            .map(Cow::Owned)
            .map_err(|err| seal_error(err, account_id, document_id)),
        None => Ok(Cow::Borrowed(stored)),
    }
}

/// Invitations spec 6.6: the inbox counterpart of [`za_event_view`], same
/// contract. A drained item is unsealed under the account's keys; a pending
/// (`pk`) item reached here is an unseal failure (spec 10): the caller
/// answers 500 for it and changes nothing.
pub(crate) fn za_item_view<'x>(
    stored: &'x Archive<AlignedBytes>,
    keys: Option<&Arc<SessionKeys>>,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Cow<'x, Archive<AlignedBytes>>> {
    match keys {
        Some(keys) => unseal_item_archive(stored, keys, account_id)
            .map(Cow::Owned)
            .map_err(|err| seal_error(err, account_id, document_id)),
        None => Ok(Cow::Borrowed(stored)),
    }
}

/// Invitations spec 6.6: whether a loaded archive is a key account's pending
/// inbox item, which clients do not see until the drain rewrites it. `false`
/// for every other collection and for non-key accounts (invariant 9). The
/// gate runs before the pending bit is read: a key account's item read
/// without its keys is the gate's 403 and trace (release 1 spec 10). The
/// resource cache knows no flags, so readers ask here after loading.
pub(crate) async fn za_item_hidden(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    collection: Collection,
    stored: &Archive<AlignedBytes>,
) -> crate::Result<bool> {
    if !za_reads_pending_bit(server, access_token, account_id, collection).await? {
        return Ok(false);
    }
    stored_item_is_pending(stored)
}

/// Invitations spec 6.6: [`za_item_hidden`] for a path taken from the
/// resource cache whose archive is not loaded, as the `propname` form of
/// PROPFIND has it: a listing (depth 1) and a request naming the item
/// (depth 0). A calendar-multiget never gets here for an inbox item, since
/// the guard in `multiget` reads hrefs only from the calendar collection.
/// Same gate, so other collections and non-key accounts answer `false` with
/// no store read; a missing archive is listed as upstream lists it.
pub(crate) async fn za_cached_item_hidden(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    collection: Collection,
    document_id: u32,
) -> crate::Result<bool> {
    if !za_reads_pending_bit(server, access_token, account_id, collection).await? {
        return Ok(false);
    }
    match server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(account_id, collection, document_id))
        .await
        .caused_by(trc::location!())?
    {
        Some(stored) => stored_item_is_pending(&stored),
        None => Ok(false),
    }
}

/// The gate both readers above run before the pending bit is read: `false`
/// outside the inbox (no lookup) and for a non-key account, the gate's 403
/// for a key account's item read without its keys.
async fn za_reads_pending_bit(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    collection: Collection,
) -> crate::Result<bool> {
    Ok(collection == Collection::CalendarEventNotification
        && server
            .za_session_keys(access_token, account_id)
            .await?
            .is_some())
}

fn stored_item_is_pending(stored: &Archive<AlignedBytes>) -> crate::Result<bool> {
    Ok(archived_item_is_pending(
        stored
            .unarchive::<CalendarEventNotification>()
            .caused_by(trc::location!())?,
    ))
}

/// This map belongs to one read request only. Writers never consume it.
pub(crate) type ZaReadMemo = HashMap<(u32, u32), Arc<ZaCalendarContext>>;

/// Explicit owner-session repair; the groupware resolver remains read-only.
pub(crate) async fn za_mutation_context(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    calendar_id: u32,
    required: Bitmap<Acl>,
) -> Result<Option<ZaCalendarContext>, ZaCalendarError> {
    let context = za_calendar_context(server, token, owner_id, calendar_id, required).await?;
    match (context, token.session_keys()) {
        (Some(context), Some(keys)) => {
            groupware::calendar::sharing::boundary::za_repair_owned_context(server, keys, context)
                .await
                .map(Some)
        }
        (context, _) => Ok(context),
    }
}

pub(crate) async fn za_read_context(
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

/// Refusals fail the request; corrupt content and general store errors fail an
/// individual listing item. Only the precise missing-resource error becomes 404.
fn calendar_item_error(
    error: ZaCalendarError,
    owner_id: u32,
    document_id: u32,
) -> crate::Result<trc::Error> {
    match error {
        ZaCalendarError::Forbidden => Err(DavError::Code(StatusCode::FORBIDDEN)),
        ZaCalendarError::LinkedEvent => Err(DavError::Code(StatusCode::CONFLICT)),
        ZaCalendarError::Seal(error) => Ok(seal_error(error, owner_id, document_id)),
        ZaCalendarError::Store(error)
            if error.matches(trc::EventType::Store(trc::StoreEvent::NotFound)) =>
        {
            Err(DavError::Code(StatusCode::NOT_FOUND))
        }
        ZaCalendarError::Store(error) => Ok(error),
    }
}

pub(crate) async fn za_read_event_view<'x>(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
    document_id: u32,
    calendar_id: u32,
    stored: &'x Archive<AlignedBytes>,
    memo: &mut ZaReadMemo,
) -> crate::Result<trc::Result<Cow<'x, Archive<AlignedBytes>>>> {
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
        Ok(Some(context)) => Ok(event_view_in_domain(stored, &context.domain)
            .map(Cow::Owned)
            .map_err(|error| seal_error(error, owner_id, document_id))),
        Ok(None) => Ok(Ok(Cow::Borrowed(stored))),
        Err(error) => Ok(Err(calendar_item_error(error, owner_id, document_id)?)),
    }
}

/// The PROPFIND loader's per-item view: only calendar collections, events
/// and inbox items consult the gate (outer `Err` is the 403 that fails the
/// request); an unseal failure (inner `Err`) fails one item. Other
/// collections borrow the stored archive with no lookup.
pub(crate) async fn za_archive_view<'x>(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    document_id: u32,
    collection: Collection,
    stored: &'x Archive<AlignedBytes>,
    addressed_calendar: Option<u32>,
    memo: &mut ZaReadMemo,
) -> crate::Result<trc::Result<Cow<'x, Archive<AlignedBytes>>>> {
    Ok(match collection {
        Collection::Calendar => {
            match za_read_context(
                server,
                access_token,
                account_id,
                document_id,
                Bitmap::new().with_item(Acl::Read),
                memo,
            )
            .await
            {
                Ok(Some(context)) => {
                    calendar_view_in_domain(&context.stored, &context.domain, context.personal_id)
                        .map(Cow::Owned)
                        .map_err(|error| seal_error(error, account_id, document_id))
                }
                Ok(None) => Ok(Cow::Borrowed(stored)),
                Err(error) => Err(calendar_item_error(error, account_id, document_id)?),
            }
        }
        Collection::CalendarEvent => {
            let calendar_id = match addressed_calendar {
                Some(calendar_id) => calendar_id,
                None => {
                    return Ok(Err(seal_error(
                        SealError::Structure("event calendar"),
                        account_id,
                        document_id,
                    )));
                }
            };
            return za_read_event_view(
                server,
                access_token,
                account_id,
                document_id,
                calendar_id,
                stored,
                memo,
            )
            .await;
        }
        Collection::CalendarEventNotification => {
            let keys = server.za_session_keys(access_token, account_id).await?;
            za_item_view(stored, keys.as_ref(), account_id, document_id)
        }
        _ => Ok(Cow::Borrowed(stored)),
    })
}

pub(crate) async fn is_key_account(server: &Server, account_id: u32) -> crate::Result<bool> {
    Ok(server
        .za_is_key_account(account_id)
        .await
        .caused_by(trc::location!())?)
}

#[cfg(test)]
mod sharing_read_tests {
    use super::*;
    use groupware::calendar::{seal::SealError, sharing::resolve::ZaCalendarError};

    #[test]
    fn read_errors_keep_their_scope() {
        assert!(matches!(
            calendar_item_error(ZaCalendarError::Forbidden, 1, 2),
            Err(DavError::Code(StatusCode::FORBIDDEN))
        ));
        assert!(matches!(
            calendar_item_error(ZaCalendarError::LinkedEvent, 1, 2),
            Err(DavError::Code(StatusCode::CONFLICT))
        ));
        assert!(calendar_item_error(ZaCalendarError::Seal(SealError::Aead), 1, 2).is_ok());
        assert!(matches!(
            calendar_item_error(ZaCalendarError::Store(trc::StoreEvent::DataCorruption.into_err()), 1, 2),
            Ok(error) if error.matches(trc::EventType::Store(trc::StoreEvent::DataCorruption))
        ));
        assert!(matches!(
            calendar_item_error(
                ZaCalendarError::Store(trc::StoreEvent::NotFound.into_err()),
                1,
                2
            ),
            Err(DavError::Code(StatusCode::NOT_FOUND))
        ));
    }
}
