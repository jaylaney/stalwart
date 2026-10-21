/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 6.3, ruling R34: a key account's mailboxes are listed only in
//! its own sessions. A token cached before the account was provisioned, or
//! one routed through a group's grant, still names the account among its
//! shared accounts; the session's list leaves it out at every sync.

use super::{MailboxSync, SessionData};
use common::{auth::AccessToken, network::SessionStream};
use trc::AddContext;

impl<T: SessionStream> SessionData<T> {
    /// Drops every foreign key account (`za_foreign_key_account`) from the
    /// session's list, its mailboxes reported as deleted in `changes`. The
    /// session's own account stays listed (the list's first entry), an
    /// impersonated key account included: every use of its mailboxes is
    /// refused (ruling R35).
    pub(crate) async fn za_unlist_key_accounts(
        &self,
        access_token: &AccessToken,
        changes: &mut Option<MailboxSync>,
    ) -> trc::Result<()> {
        let listed = self
            .mailboxes
            .lock()
            .iter()
            .map(|account| account.account_id)
            .filter(|account_id| *account_id != self.account_id)
            .collect::<Vec<_>>();
        for account_id in listed {
            if self
                .server
                .za_foreign_key_account(access_token, account_id)
                .await
                .caused_by(trc::location!())?
            {
                let mut mailboxes = self.mailboxes.lock();
                if let Some(pos) = mailboxes
                    .iter()
                    .position(|account| account.account_id == account_id)
                {
                    let account = mailboxes.remove(pos);
                    if let Some(changes) = changes {
                        changes.deleted.extend(account.mailbox_names.into_keys());
                    }
                }
            }
        }
        Ok(())
    }
}
