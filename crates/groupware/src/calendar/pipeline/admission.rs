/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::error::{ZaPipelineError, ZaRefusal};
use common::{
    Server,
    auth::{AccessToken, AccountInfo},
};
use registry::schema::enums::Permission;
use std::sync::Arc;
use trc::AddContext;
use types::collection::Collection;
use vault::session::SessionKeys;

/// Calendar admission before the per-calendar resolver checks operation rights.
pub async fn za_calendar_admission(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
) -> Result<(), ZaPipelineError> {
    if !server
        .za_is_key_account(owner_id)
        .await
        .caused_by(trc::location!())?
        || token.za_keys_for(owner_id).is_some()
    {
        return Ok(());
    }
    if token
        .session_keys()
        .is_some_and(|keys| keys.account_id == token.account_id())
        && !token.za_is_impersonated()
        && !token.has_permission(Permission::Impersonate)
        && token.has_shared_access(owner_id, Collection::Calendar)
    {
        return Ok(());
    }
    trc::event!(
        Security(trc::SecurityEvent::Unauthorized),
        AccountId = owner_id,
        Details = "zero-access: calendar access without session keys",
    );
    Err(ZaPipelineError::Forbidden(ZaRefusal::NoKeys))
}

/// Event writers keep the authenticated writer's keys. The fresh calendar
/// resolver below decides whether those keys authorize this owner's calendar.
pub async fn za_writer_keys(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
) -> Result<Option<Arc<SessionKeys>>, ZaPipelineError> {
    if !server.za_is_key_account(owner_id).await? {
        return Ok(None);
    }
    token
        .session_keys()
        .filter(|keys| keys.account_id == token.account_id() && !token.za_is_impersonated())
        .cloned()
        .map(Some)
        .ok_or(ZaPipelineError::Forbidden(ZaRefusal::NoKeys))
}

/// The scheduling roles of a key account's write (release 4 delegated
/// scheduling): the sender is the writer's account info with the owner's
/// addresses added (0.16's `scheduling_account_info`, which 1.0 removed; its
/// handlers now pass the owner's), the writer's and the owner's own infos.
pub struct ZaRoles {
    pub sender: AccountInfo,
    pub writer: AccountInfo,
    pub owner: AccountInfo,
}

pub async fn za_roles(
    server: &Server,
    token: &AccessToken,
    owner_id: u32,
) -> Result<ZaRoles, ZaPipelineError> {
    let writer = server
        .account_info(token.account_id())
        .await
        .caused_by(trc::location!())?;
    let owner = if owner_id == token.account_id() {
        writer.clone()
    } else {
        server
            .account_info(owner_id)
            .await
            .caused_by(trc::location!())?
    };
    let mut sender = writer.clone();
    if owner_id != token.account_id() {
        for address in &owner.addresses {
            if !sender.addresses.contains(address) {
                sender.addresses.push(address.clone());
            }
        }
    }
    Ok(ZaRoles {
        sender,
        writer,
        owner,
    })
}
