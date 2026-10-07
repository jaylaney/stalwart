/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{DavError, DavResourceName};
use common::{Server, auth::AccessToken};
use hyper::StatusCode;
use std::{future::Future, sync::Arc};
use trc::AddContext;
use vault::session::SessionKeys;

pub(crate) trait ZeroAccessGate: Sync + Send {
    /// `None` for a non-key account (unchanged upstream path). For a key
    /// account, the keys of that account held by this session, or 403.
    fn za_session_keys(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> impl Future<Output = crate::Result<Option<Arc<SessionKeys>>>> + Send;

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

async fn is_key_account(server: &Server, account_id: u32) -> crate::Result<bool> {
    Ok(server
        .try_account(account_id)
        .await
        .caused_by(trc::location!())?
        .is_some_and(|account| account.is_key_account()))
}
