/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{DavError, DavResourceName};
use common::{Server, auth::AccessToken};
use groupware::calendar::seal::{seal_error, unseal_calendar_archive, unseal_event_archive};
use hyper::StatusCode;
use std::{borrow::Cow, future::Future, sync::Arc};
use store::write::{AlignedBytes, Archive};
use trc::AddContext;
use types::collection::Collection;
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

/// The collection counterpart of [`za_event_view`], same contract.
pub(crate) fn za_calendar_view<'x>(
    stored: &'x Archive<AlignedBytes>,
    keys: Option<&Arc<SessionKeys>>,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Cow<'x, Archive<AlignedBytes>>> {
    match keys {
        Some(keys) => unseal_calendar_archive(stored, keys, account_id)
            .map(Cow::Owned)
            .map_err(|err| seal_error(err, account_id, document_id)),
        None => Ok(Cow::Borrowed(stored)),
    }
}

/// The PROPFIND loader's per-item view: only calendar collections and events
/// consult the gate (outer `Err` is the 403 that fails the request); an
/// unseal failure (inner `Err`) fails one item. Other collections borrow the
/// stored archive with no lookup.
pub(crate) async fn za_archive_view<'x>(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    document_id: u32,
    collection: Collection,
    stored: &'x Archive<AlignedBytes>,
) -> crate::Result<trc::Result<Cow<'x, Archive<AlignedBytes>>>> {
    Ok(match collection {
        Collection::Calendar => {
            let keys = server.za_session_keys(access_token, account_id).await?;
            za_calendar_view(stored, keys.as_ref(), account_id, document_id)
        }
        Collection::CalendarEvent => {
            let keys = server.za_session_keys(access_token, account_id).await?;
            za_event_view(stored, keys.as_ref(), account_id, document_id)
        }
        _ => Ok(Cow::Borrowed(stored)),
    })
}

async fn is_key_account(server: &Server, account_id: u32) -> crate::Result<bool> {
    Ok(server
        .try_account(account_id)
        .await
        .caused_by(trc::location!())?
        .is_some_and(|account| account.is_key_account()))
}
