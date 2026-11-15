/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::server::TestServer;
use common::auth::vault::RawValue;
use store::{ValueKey, write::BatchBuilder};
use types::{collection::Collection, field::PrincipalField};
use vault::tokens::ZaTokenRecord;

/// The stored bytes of the account's token record (field 153).
pub async fn raw_token_record(test: &TestServer, account_id: u32) -> Option<Vec<u8>> {
    test.server
        .store()
        .get_value::<RawValue>(ValueKey::property(
            account_id,
            Collection::Principal,
            0,
            PrincipalField::ZeroAccessTokens,
        ))
        .await
        .unwrap()
        .map(|raw| raw.0)
}

/// An unconditional set of field 153 to `raw`, or its removal with `None`.
pub async fn set_raw_token_record(test: &TestServer, account_id: u32, raw: Option<&[u8]>) {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0);
    match raw {
        Some(raw) => batch.set(PrincipalField::ZeroAccessTokens, raw.to_vec()),
        None => batch.clear(PrincipalField::ZeroAccessTokens),
    };
    test.server.store().write_batch(&mut batch).await.unwrap();
}

pub async fn token_record(test: &TestServer, account_id: u32) -> Option<ZaTokenRecord> {
    test.server.za_token_read(account_id).await.unwrap().record
}
