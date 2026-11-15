/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{ErrorType, OAuthResponse};
use common::{
    Server, auth::oauth::grant::GrantDeletion, cache::invalidate::CacheInvalidationBuilder,
    za::tokens::ZaTokenIssue,
};
use store::write::now;
use trc::AddContext;
use vault::{
    keys::Secret,
    tokens::{ZaSuffix, append_secret},
};

pub(crate) struct ZaGrantKeys {
    pub mk: Secret,
    pub binding: u64,
}

/// Only called after upstream has consumed the authorization code.
pub(crate) async fn za_code_handoff(
    server: &Server,
    account_id: u32,
    code: &str,
    suffix: ZaSuffix,
) -> trc::Result<Result<Option<ZaGrantKeys>, ErrorType>> {
    let account = match server.account(account_id).await.caused_by(trc::location!()) {
        Ok(account) => account,
        Err(err) => {
            // The code is spent, so its hand-off goes too. A failed cleanup
            // never replaces the classification error.
            let _ = server.za_handoff_delete(code).await;
            return Err(err);
        }
    };
    if !account.is_key_account() {
        return Ok(match suffix {
            ZaSuffix::None => Ok(None),
            _ => Err(ErrorType::InvalidGrant),
        });
    }

    let ZaSuffix::Secret(secret) = suffix else {
        server.za_handoff_delete(code).await?;
        return Ok(Err(ErrorType::InvalidGrant));
    };
    let Some(handoff) = server
        .za_handoff_take(code, &secret)
        .await?
        .filter(|handoff| handoff.account_id == account_id)
    else {
        return Ok(Err(ErrorType::InvalidGrant));
    };
    Ok(Ok(Some(ZaGrantKeys {
        mk: handoff.mk,
        binding: handoff.binding,
    })))
}

pub(crate) async fn za_issue_keyed(
    server: &Server,
    account_id: u32,
    grant_id: u64,
    keys: &ZaGrantKeys,
    response: &mut OAuthResponse,
) -> trc::Result<Result<(), ErrorType>> {
    let now = now();
    let failure = match server
        .za_token_issue(
            account_id,
            grant_id,
            &keys.mk,
            keys.binding,
            now + server.core.oauth.oauth_expiry_token,
            now + server.core.oauth.oauth_expiry_refresh_token,
        )
        .await
    {
        Ok(ZaTokenIssue::Issued(issued)) => {
            append_secret(&mut response.access_token, &issued.access);
            if let Some(refresh) = &mut response.refresh_token {
                append_secret(refresh, &issued.refresh);
            }
            return Ok(Ok(()));
        }
        Ok(ZaTokenIssue::VaultChanged) => Ok(Err(ErrorType::InvalidGrant)),
        Err(err) => Err(err),
    };
    if delete_unissued_grant(server, account_id, grant_id)
        .await
        .is_err()
    {
        trc::error!(
            trc::StoreEvent::UnexpectedError
                .into_err()
                .account_id(account_id)
                .id(grant_id)
                .details("zero-access: unissued OAuth grant not deleted")
        );
    }
    failure
}

async fn delete_unissued_grant(server: &Server, account_id: u32, grant_id: u64) -> trc::Result<()> {
    if let Some(stored) = server
        .oauth_grant(grant_id)
        .await?
        .filter(|stored| stored.account_id() == account_id)
    {
        let mut invalidator = CacheInvalidationBuilder::default();
        server
            .delete_oauth_grant(stored, GrantDeletion::Unconditional, &mut invalidator)
            .await?;
        server.invalidate_caches(invalidator).await?;
    }
    Ok(())
}
