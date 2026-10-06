/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::server::{TestServer, TestServerBuilder};
use registry::schema::enums::Permission;

pub mod setup;

pub const STRONG: &str = "correct horse battery staple 1";

#[tokio::test(flavor = "multi_thread")]
pub async fn za_tests() {
    let mut test = TestServerBuilder::new("za_tests")
        .await
        .with_default_listeners()
        .await
        .build()
        .await;

    let admin = test.create_admin_account("admin@example.com").await;
    // An ordinary account with a password, used as the ineligible case and
    // as the non-admin caller.
    let plain = admin
        .create_user_account(
            "plain@example.com",
            "plain secret with entropy 9",
            "Plain User",
            &[],
            vec![Permission::UnlimitedRequests, Permission::UnlimitedUploads],
        )
        .await;
    test.insert_account(plain);
    test.insert_account(admin);

    setup::test(&mut test).await;

    destroy_key_accounts(&test).await;
    test.assert_is_empty().await;

    if test.is_reset() {
        test.temp_dir.delete();
    }
}

pub fn user_permissions() -> Vec<Permission> {
    vec![
        Permission::UnlimitedRequests,
        Permission::UnlimitedUploads,
        Permission::DavPrincipalList,
        Permission::DavPrincipalSearch,
    ]
}

/// Vault records live in the Principal property subspace, which
/// `assert_is_empty` scans, so every key account is destroyed at the end.
pub async fn destroy_key_accounts(test: &TestServer) {
    let admin = test.account("admin@example.com");
    let accounts: Vec<_> = test
        .accounts
        .values()
        .filter(|a| a.recovery_key.is_some())
        .cloned()
        .collect();
    for account in accounts {
        admin.destroy_account(account).await;
    }
    test.wait_for_tasks().await;
}
