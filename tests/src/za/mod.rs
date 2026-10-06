/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::server::{TestServer, TestServerBuilder};
use ::registry::schema::{enums::Permission, prelude::Property, structs::Http};

pub mod app_password;
pub mod cors;
pub mod password;
pub mod registry;
pub mod setup;
pub mod totp;

pub const STRONG: &str = "correct horse battery staple 1";

/// Origin of the account page, allowed by CORS on `/api/vault/*`.
pub const ACCOUNT_PAGE_ORIGIN: &str = "https://account.example.com";

#[tokio::test(flavor = "multi_thread")]
pub async fn za_tests() {
    // Read once when the server starts. SAFETY: set before this test builds
    // its server and starts its runtime work; the variable is only read by
    // server startup, and no test in this binary writes the environment.
    unsafe { std::env::set_var("ZA_ACCOUNT_PAGE_ORIGIN", ACCOUNT_PAGE_ORIGIN) };
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
    // Forwarded addresses let the fail2ban test use its own client address.
    admin
        .registry_update_setting(
            Http {
                use_x_forwarded: true,
                ..Default::default()
            },
            &[Property::UseXForwarded],
        )
        .await;
    admin.reload_settings().await;
    test.insert_account(plain);
    test.insert_account(admin);

    setup::test(&mut test).await;
    password::test(&mut test).await;
    app_password::test(&mut test).await;
    totp::test(&mut test).await;
    registry::test(&mut test).await;
    cors::test(&mut test).await;

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
