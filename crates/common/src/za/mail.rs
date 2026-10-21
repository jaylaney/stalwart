/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 3.3 and 4: the IMAP and POP3 login gate. A mail session of a
//! key account exists only with the account's session keys. Also the error
//! a refused mail access answers with (spec 6.3, 10).

use super::{ZA_MAIL_KEYLESS, ZA_TRACE_MAIL_LOGIN};
use crate::{Server, auth::AccessToken};
use std::sync::Arc;
use vault::session::SessionKeys;

/// Mail spec 6.3, 10: the error of a mail write or read refused for a key
/// account, one every caller writes to its client: IMAP answers
/// `NO [NOPERM]` and JMAP `forbidden`, with `ZA_MAIL_KEYLESS`. A
/// `SecurityEvent` error is never written, which would leave an IMAP client
/// waiting, so the caller traces its refusal as its own event.
pub fn za_keyless_refusal() -> trc::Error {
    trc::JmapEvent::Forbidden
        .into_err()
        .details(ZA_MAIL_KEYLESS)
        .code("NOPERM")
}

/// Whether `err` is `za_keyless_refusal`'s error, for a caller that answers
/// it outside its protocol's error mapping (`/jmap/download`, ruling R30).
pub fn za_is_keyless_refusal(err: &trc::Error) -> bool {
    err.matches(trc::EventType::Jmap(trc::JmapEvent::Forbidden))
        && err.value_as_str(trc::Key::Details) == Some(ZA_MAIL_KEYLESS)
}

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
