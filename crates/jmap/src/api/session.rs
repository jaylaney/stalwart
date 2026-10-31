/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::{
    Server,
    auth::{AccessToken, oauth::scope::OAuthScopes},
};
use jmap_proto::request::capability::{
    Account, BaseCapabilities, Capabilities, Capability, CapabilityIds, EmptyCapabilities,
    PrincipalOwnerCapabilities, Session,
};
use registry::schema::enums::Permission;
use std::future::Future;
use trc::AddContext;
use types::id::Id;
use utils::map::vec_map::VecMap;

pub trait SessionHandler: Sync + Send {
    fn handle_session_resource(
        &self,
        base_url: String,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<Session>> + Send;
}

pub trait JmapAccount {
    fn jmap_account(&self, access_token: &AccessToken, account_id: u32, name: String) -> Account;

    fn jmap_account_capabilities(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> impl Iterator<Item = (Capability, Capabilities)>;
}

impl JmapAccount for Server {
    fn jmap_account(&self, access_token: &AccessToken, account_id: u32, name: String) -> Account {
        let mut account_capabilities =
            VecMap::with_capacity(self.core.jmap.capabilities.account.len() + 1);
        account_capabilities.extend(self.jmap_account_capabilities(access_token, account_id));
        account_capabilities.append(
            Capability::PrincipalsOwner,
            Capabilities::PrincipalsOwner(PrincipalOwnerCapabilities {
                account_id_for_principal: Id::from(access_token.account_id()),
                principal_id: Id::from(account_id),
            }),
        );
        Account {
            name,
            is_personal: account_id == access_token.account_id(),
            is_read_only: access_token.is_read_only(account_id),
            account_capabilities,
        }
    }

    fn jmap_account_capabilities(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> impl Iterator<Item = (Capability, Capabilities)> {
        let base_capabilities = &self.core.jmap.capabilities;
        let is_owner = access_token.is_member(account_id);
        let current_user_principal_id = Some(Id::from(access_token.account_id()));
        let supports_private = access_token.has_permission(Permission::JmapMetadataPrivate);
        let capability_ids = access_token
            .account_capabilities(base_capabilities)
            .filter(|capability| {
                is_owner
                    || !matches!(
                        capability,
                        Capability::Submission
                            | Capability::VacationResponse
                            | Capability::Sieve
                            | Capability::Quota
                            | Capability::EmailPush
                    )
            })
            .collect::<CapabilityIds>();

        Capability::all_capabilities()
            .iter()
            .copied()
            .filter(move |capability| capability_ids.contains(*capability))
            .filter_map(move |capability| {
                let capabilities = match base_capabilities.account.get(&capability) {
                    Some(Capabilities::Metadata(metadata)) => Capabilities::Metadata(
                        metadata.to_account_capabilities(capability_ids, supports_private)?,
                    ),
                    Some(capabilities) => {
                        capabilities.to_account_capabilities(current_user_principal_id, is_owner)
                    }
                    None => Capabilities::Empty(EmptyCapabilities::default()),
                };
                Some((capability, capabilities))
            })
    }
}

impl SessionHandler for Server {
    async fn handle_session_resource(
        &self,
        base_url: String,
        access_token: &AccessToken,
    ) -> trc::Result<Session> {
        let mut session = Session::new(base_url, &self.core.jmap.capabilities);
        session.set_state(access_token.state());
        if access_token.oauth_scopes().is_some() {
            for capability in Capability::all_capabilities() {
                if !access_token.allows_oauth_scopes(capability_scopes(*capability)) {
                    session.capabilities.remove(capability);
                }
            }
        }

        // Set primary account
        let account = self
            .account(access_token.account_id())
            .await
            .caused_by(trc::location!())?;
        session.username = account.name().to_string();
        let account_id = Id::from(access_token.account_id());
        // Spec 9 and Task 6: no calendars or object metadata for key
        // accounts (`za_hidden_capability`).
        let primary_is_key = account.is_key_account();
        let mut primary_account = self.jmap_account(
            access_token,
            access_token.account_id(),
            account.name().to_string(),
        );
        za_trim_account(&mut primary_account, primary_is_key, primary_is_key);
        for capability in access_token
            .account_capabilities(&self.core.jmap.capabilities)
            .filter(|capability| !za_hidden_capability(primary_is_key, primary_is_key, *capability))
        {
            if capability != Capability::Metadata
                || primary_account
                    .account_capabilities
                    .contains_key(&Capability::Metadata)
            {
                session.primary_accounts.append(capability, account_id);
            }
        }
        session.accounts.append(account_id, primary_account);

        // Add secondary accounts
        for &account_id in access_token.secondary_ids() {
            let Some(account) = self
                .try_account(account_id)
                .await
                .caused_by(trc::location!())?
            else {
                trc::event!(
                    Auth(trc::AuthEvent::Warning),
                    AccountId = account_id,
                    Reason = "Skipping orphan secondary account id in session",
                );
                continue;
            };
            let mut secondary =
                self.jmap_account(access_token, account_id, account.name().to_string());
            za_trim_account(&mut secondary, account.is_key_account(), primary_is_key);
            session.accounts.append(Id::from(account_id), secondary);
        }

        if !session.accounts.values().any(|account| {
            account
                .account_capabilities
                .contains_key(&Capability::Metadata)
        }) {
            session.capabilities.remove(&Capability::Metadata);
        }

        Ok(session)
    }
}

trait AccountCapabilities {
    fn account_capabilities(
        &self,
        base_capabilities: &BaseCapabilities,
    ) -> impl Iterator<Item = Capability>;
}

impl AccountCapabilities for AccessToken {
    fn account_capabilities(
        &self,
        base_capabilities: &BaseCapabilities,
    ) -> impl Iterator<Item = Capability> {
        Capability::all_capabilities()
            .iter()
            .filter(move |capability| {
                let permission = match capability {
                    Capability::Mail | Capability::MailShare | Capability::EmailPush => {
                        Permission::JmapEmailGet
                    }
                    Capability::Submission => Permission::JmapEmailSubmissionCreate,
                    Capability::VacationResponse => Permission::JmapVacationResponseGet,
                    Capability::Contacts => Permission::JmapContactCardGet,
                    Capability::ContactsParse => Permission::JmapContactCardParse,
                    Capability::Calendars => Permission::JmapCalendarEventGet,
                    Capability::CalendarsParse => Permission::JmapCalendarEventParse,
                    Capability::Sieve => Permission::JmapSieveScriptGet,
                    Capability::Blob => Permission::JmapBlobGet,
                    Capability::Quota => Permission::JmapQuotaGet,
                    Capability::FileNode => Permission::JmapFileNodeGet,
                    Capability::Metadata
                        if base_capabilities
                            .account
                            .contains_key(&Capability::Metadata) =>
                    {
                        Permission::JmapMetadataGet
                    }
                    Capability::WebSocket
                    | Capability::Principals
                    | Capability::PrincipalsAvailability
                    | Capability::Stalwart => return true,
                    Capability::Core
                    | Capability::PrincipalsOwner
                    | Capability::WebPushVapid
                    | Capability::Metadata => {
                        return false;
                    }
                };
                self.has_permission(permission)
            })
            .filter(|capability| self.allows_oauth_scopes(capability_scopes(**capability)))
            .copied()
    }
}

pub fn capability_scopes(capability: Capability) -> OAuthScopes {
    match capability {
        Capability::Core
        | Capability::Blob
        | Capability::Quota
        | Capability::Principals
        | Capability::PrincipalsOwner
        | Capability::WebSocket
        | Capability::WebPushVapid
        | Capability::Metadata => OAuthScopes::INTEROPERABLE,
        Capability::Mail
        | Capability::Submission
        | Capability::VacationResponse
        | Capability::Sieve
        | Capability::MailShare
        | Capability::EmailPush => OAuthScopes::MAIL,
        Capability::Contacts | Capability::ContactsParse => OAuthScopes::CONTACTS,
        Capability::Calendars | Capability::CalendarsParse | Capability::PrincipalsAvailability => {
            OAuthScopes::CALENDARS
        }
        Capability::FileNode => OAuthScopes::FILES,
        Capability::Stalwart => OAuthScopes::NONE,
    }
}

pub fn allowed_capabilities(access_token: &AccessToken, using: CapabilityIds) -> CapabilityIds {
    if access_token.oauth_scopes().is_none() {
        return using;
    }
    Capability::all_capabilities()
        .iter()
        .copied()
        .filter(|capability| {
            using.contains(*capability)
                && access_token.allows_oauth_scopes(capability_scopes(*capability))
        })
        .collect()
}

/// Whether an account object leaves `capability` out, the account being a
/// key account (`account_is_key`) or the session's or principal's caller
/// being one (`caller_is_key`):
/// - spec 9: JMAP calendars are not offered for key accounts;
/// - Task 6: object metadata is refused for key accounts, as owner or as
///   caller (`api::metadata::za_metadata_refused`), so it is not offered
///   either; when no account offers it, upstream's rule drops the session's
///   own metadata capability.
pub(crate) fn za_hidden_capability(
    account_is_key: bool,
    caller_is_key: bool,
    capability: Capability,
) -> bool {
    match capability {
        Capability::Calendars | Capability::CalendarsParse => account_is_key,
        Capability::Metadata => account_is_key || caller_is_key,
        _ => false,
    }
}

/// Removes the capabilities `za_hidden_capability` leaves out from a
/// session's account object.
fn za_trim_account(account: &mut Account, account_is_key: bool, caller_is_key: bool) {
    for capability in Capability::all_capabilities() {
        if za_hidden_capability(account_is_key, caller_is_key, *capability) {
            account.account_capabilities.remove(capability);
        }
    }
}
