/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 6.3, ruling R5: the grants a key account's mailboxes still
//! carry from before it was provisioned. They no longer route (the access
//! token leaves a key account's mail out), but a grantee's cached token
//! predates the provisioning; `setup-token` rebuilds the grantees' tokens
//! from this list.

use crate::mailbox::Mailbox;
use common::Server;
use trc::AddContext;
use types::collection::Collection;

/// The accounts named in the ACL grants of `account_id`'s mailboxes, each
/// once, read from the stored mailbox archives.
pub async fn za_mailbox_grantees(server: &Server, account_id: u32) -> trc::Result<Vec<u32>> {
    let mut grantees = Vec::new();
    server
        .archives(account_id, Collection::Mailbox, &(), |_, archive| {
            for grant in archive.unarchive::<Mailbox>()?.acls.iter() {
                let grantee = grant.account_id.to_native();
                if !grantees.contains(&grantee) {
                    grantees.push(grantee);
                }
            }
            Ok(true)
        })
        .await
        .caused_by(trc::location!())?;
    Ok(grantees)
}
