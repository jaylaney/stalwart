/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access fork: the share-revocation cleanup leaves a key account's
//! calendars, events and inbox items to the fork's keyed revoke
//! (`groupware::calendar::sharing`), which removes the grantee's entries
//! with the owner's keys and keeps the grant ring in line with the ACLs.
//! The cleanup runs in the background without keys, and staging one of
//! those objects would rewrite a key account's sealed calendar data.

use super::cleanup::GranteeCleanup;

impl GranteeCleanup<'_> {
    /// Whether the staging of a calendar, an event or an inbox item is
    /// skipped: the owner is a key account. An unknown owner is not one
    /// (`za_is_key_account`), and a lookup error is returned, so the task
    /// retries rather than staging unclassified.
    pub(super) async fn za_skips_owner(&self) -> trc::Result<bool> {
        self.server.za_is_key_account(self.owner_id).await
    }
}
