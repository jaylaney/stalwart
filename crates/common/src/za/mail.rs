/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 3.3 and 4: the IMAP and POP3 login gate. A mail session of a
//! key account exists only with the account's session keys.

use super::ZA_TRACE_MAIL_LOGIN;
use crate::{Server, auth::AccessToken};
use std::sync::Arc;
use vault::session::SessionKeys;

impl Server {
    /// The token an IMAP or POP3 session keeps after a successful login:
    /// with `session_keys` attached when the login derived them, unchanged
    /// for an account that is not a key account, and `None` for a key
    /// account whose login yields no keys (OAuth Bearer, API key, master
    /// user), traced as `SecurityEvent::Unauthorized`. The caller answers
    /// `None` with its protocol's refusal and leaves its session state as it
    /// is; the keys go only to the session's own token, never to a cache.
    pub async fn za_mail_login(
        &self,
        access_token: AccessToken,
        session_keys: Option<Arc<SessionKeys>>,
        session_id: u64,
    ) -> trc::Result<Option<AccessToken>> {
        if let Some(keys) = session_keys {
            Ok(Some(access_token.with_session_keys(keys)))
        } else if self.za_is_key_account(access_token.account_id()).await? {
            trc::event!(
                Security(trc::SecurityEvent::Unauthorized),
                SpanId = session_id,
                AccountId = access_token.account_id(),
                Details = ZA_TRACE_MAIL_LOGIN,
            );
            Ok(None)
        } else {
            Ok(Some(access_token))
        }
    }
}
