/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Upgrade design, "Dead properties and object metadata": IMAP METADATA is
//! refused for key accounts. 1.0 keeps annotations in `Collection::Metadata`
//! containers, in the clear, so a key account's session answers
//! `GETMETADATA`, `SETMETADATA` and `LIST … RETURN (METADATA …)` with
//! `NO [NOPERM]`, and its `CAPABILITY` names neither `METADATA` nor
//! `LIST-METADATA` (`Session::capabilities`). A mailbox of a key account
//! named by another session is refused too (`metadata_mailbox`), although
//! such a session does not list it (ruling R34). Ordinary sessions take
//! upstream's paths.

use crate::{core::SessionData, op::ImapContext};
use common::network::SessionStream;
use compact_str::CompactString;
use imap_proto::ResponseCode;

/// The client text of a refused metadata command.
pub(crate) const ZA_IMAP_METADATA_REFUSED: &str =
    "Annotations are not available for zero-access accounts.";

impl<T: SessionStream> SessionData<T> {
    /// Refuses a metadata command of a key account's session, or one naming
    /// a key account's mailbox (`account_id`). The accounts are read with
    /// `account()`, which fails closed, so a session that logged in before
    /// its account was provisioned, which holds no keys, is refused too.
    pub(crate) async fn za_assert_metadata_allowed(
        &self,
        tag: &str,
        account_id: Option<u32>,
    ) -> trc::Result<()> {
        for account_id in [Some(self.account_id), account_id].into_iter().flatten() {
            if self
                .server
                .account(account_id)
                .await
                .imap_ctx(tag, trc::location!())?
                .is_key_account()
            {
                return Err(trc::ImapEvent::Error
                    .into_err()
                    .details(ZA_IMAP_METADATA_REFUSED)
                    .code(ResponseCode::NoPerm)
                    .id(CompactString::from(tag)));
            }
        }
        Ok(())
    }
}
