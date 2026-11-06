/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! A recipient feeds upstream's key harvest only when its delivery
//! committed a message as an ordinary account (mail spec 12 and the
//! v1.0.3 merge's decision 2).

use crate::message::ingest::IngestedEmail;
use common::Server;

pub async fn za_harvest_allowed(
    server: &Server,
    account_id: u32,
    outcome: &trc::Result<IngestedEmail>,
) -> bool {
    outcome
        .as_ref()
        .is_ok_and(|message| message.change_id != u64::MAX)
        && matches!(server.try_account(account_id).await, Ok(Some(account)) if !account.is_key_account())
}
