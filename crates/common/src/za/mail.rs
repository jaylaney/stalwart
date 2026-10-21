/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 3.3 and 4: the IMAP and POP3 login gate. A mail session of a
//! key account exists only with the account's session keys. Also the error
//! a refused mail access answers with (spec 6.3, 10), the refusals of a
//! copy across accounts and of sharing a key account's mailbox, and the
//! rule that a key account's mail is reached only by its own sessions (spec
//! 6.3).

use super::{
    ZA_MAIL_KEYLESS, ZA_TRACE_MAIL_COPY, ZA_TRACE_MAIL_KEYLESS, ZA_TRACE_MAIL_LOGIN,
    ZA_TRACE_MAIL_SHARING,
};
use crate::{Server, auth::AccessToken};
use std::sync::Arc;
use types::acl::AclGrant;
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

/// Mail spec 6.3, ruling I3: whether `new` grants any account a right that
/// `current` does not, an addition or a widening, which a key account's
/// mailbox refuses; a removal or a narrowing only reduces access and passes.
pub fn za_acl_widens(new: &[AclGrant], current: &[AclGrant]) -> bool {
    new.iter().any(|grant| {
        let held = current
            .iter()
            .find(|held| held.account_id == grant.account_id)
            .map_or(0, |held| held.grants.bitmap);
        grant.grants.bitmap & !held != 0
    })
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

    /// Mail spec 6.3, 9: a copy or move of messages from `from_account_id`
    /// into another account is refused when either is a key account, before
    /// anything is read: traced as `Security(Unauthorized)` naming the
    /// source account, with `ZA_TRACE_MAIL_COPY`, and answered with the
    /// keyless refusal (IMAP `NO [NOPERM]`, JMAP `forbidden`). A copy within
    /// one account passes.
    pub async fn za_assert_mail_copy(
        &self,
        from_account_id: u32,
        to_account_id: u32,
        session_id: u64,
    ) -> trc::Result<()> {
        if from_account_id != to_account_id
            && (self.za_is_key_account(from_account_id).await?
                || self.za_is_key_account(to_account_id).await?)
        {
            trc::event!(
                Security(trc::SecurityEvent::Unauthorized),
                SpanId = session_id,
                AccountId = from_account_id,
                Details = ZA_TRACE_MAIL_COPY,
            );
            Err(za_keyless_refusal().account_id(from_account_id))
        } else {
            Ok(())
        }
    }

    /// Mail spec 6.3, ruling R34: whether `account_id` is a key account other
    /// than the token's own. Its mail is reached only by its own sessions,
    /// whatever route another token holds: a grant made before provisioning,
    /// a group's grant, impersonation, a token cached or kept by a WebSocket
    /// from before.
    pub async fn za_foreign_key_account(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> trc::Result<bool> {
        Ok(!access_token.is_account_id(account_id) && self.za_is_key_account(account_id).await?)
    }

    /// Mail spec 6.3, ruling R34: refuses a mail access to a foreign key
    /// account (`za_foreign_key_account`), traced as `Security(Unauthorized)`
    /// naming it, with `ZA_TRACE_MAIL_KEYLESS`, and answered with the keyless
    /// refusal. Any other account passes.
    pub async fn za_assert_mail_route(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> trc::Result<()> {
        if self
            .za_foreign_key_account(access_token, account_id)
            .await?
        {
            trc::event!(
                Security(trc::SecurityEvent::Unauthorized),
                AccountId = account_id,
                Details = ZA_TRACE_MAIL_KEYLESS,
            );
            Err(za_keyless_refusal().account_id(account_id))
        } else {
            Ok(())
        }
    }

    /// Mail spec 6.3, 9: whether a change to the sharing of `account_id`'s
    /// mailboxes is refused, `account_id` being a key account; traced as
    /// `Security(Unauthorized)` with `ZA_TRACE_MAIL_SHARING` when it is. The
    /// caller answers with its protocol's refusal.
    pub async fn za_mail_sharing_refused(&self, account_id: u32) -> trc::Result<bool> {
        if self.za_is_key_account(account_id).await? {
            trc::event!(
                Security(trc::SecurityEvent::Unauthorized),
                AccountId = account_id,
                Details = ZA_TRACE_MAIL_SHARING,
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
