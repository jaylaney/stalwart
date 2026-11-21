/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::{Server, auth::AccessToken, za::tokens::za_split_bearer};
use std::sync::Arc;
use vault::{session::SessionKeys, tokens::ZaSuffix};

pub(super) const TOKEN_KEYS_UNAVAILABLE: &str = "zero-access: token keys unavailable";

pub(super) fn za_bearer_refusal(access_token: &AccessToken, details: &'static str) -> trc::Error {
    trc::AuthEvent::Failed
        .into_err()
        .details(details)
        .account_id(access_token.account_id())
        .ctx_opt(trc::Key::Id, access_token.grant_id())
        .caused_by(trc::location!())
}

/// Only HTTP opens token keys after upstream has authenticated the Bearer
/// token. Spec 4.6 (D27): the refusal goes straight to the HTTP handler,
/// outside the authentication delay and ban accounting.
pub(crate) async fn za_bearer_keys(
    server: &Server,
    token: &str,
    access_token: &AccessToken,
) -> trc::Result<Option<Arc<SessionKeys>>> {
    let refuse = |details| za_bearer_refusal(access_token, details);
    let secret = match za_split_bearer(token).1 {
        ZaSuffix::None => return Ok(None),
        ZaSuffix::Malformed => return Err(refuse("zero-access: malformed token secret")),
        ZaSuffix::Secret(secret) => secret,
    };
    let account_id = access_token.account_id();
    let account = server
        .account(account_id)
        .await
        .map_err(|_| refuse(TOKEN_KEYS_UNAVAILABLE))?;
    if !account.is_key_account() {
        return Err(refuse(
            "zero-access: token secret on an ordinary account's token",
        ));
    }
    let grant_id = access_token
        .grant_id()
        .ok_or_else(|| refuse("zero-access: token secret on a token without a grant"))?;
    match server
        .za_token_open_access(account_id, grant_id, &secret)
        .await
    {
        Ok(Some(keys)) if keys.account_id == account_id => Ok(Some(keys)),
        _ => Err(refuse(TOKEN_KEYS_UNAVAILABLE)),
    }
}
