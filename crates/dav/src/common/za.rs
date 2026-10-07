/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::DavError;
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
}

impl ZeroAccessGate for Server {
    async fn za_session_keys(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> crate::Result<Option<Arc<SessionKeys>>> {
        // An unknown account is not a key account: upstream's outcome stands.
        let Some(account) = self
            .try_account(account_id)
            .await
            .caused_by(trc::location!())?
        else {
            return Ok(None);
        };
        if !account.is_key_account() {
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
}
