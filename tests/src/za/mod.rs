/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::server::{TestServer, TestServerBuilder};
use ::registry::schema::{
    enums::Permission,
    prelude::Property,
    structs::{Http, Imap, Rate},
};
use ::registry::types::duration::Duration;

pub mod app_password;
pub mod caches;
pub mod cors;
pub mod dav_gate;
pub mod dav_seal;
pub mod disabled;
pub mod gating;
pub mod password;
pub mod registry;
pub mod setup;
pub mod totp;

pub const STRONG: &str = "correct horse battery staple 1";

/// Origin of the account page, allowed by CORS on `/api/vault/*`.
pub const ACCOUNT_PAGE_ORIGIN: &str = "https://account.example.com";

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
    // Forwarded addresses let the fail2ban test use its own client address.
    // The vault modules send well over upstream's default of 100 anonymous
    // requests per minute from one address; raise the limit for the suite.
    admin
        .registry_update_setting(
            Http {
                use_x_forwarded: true,
                rate_limit_anonymous: Some(Rate {
                    count: 10000,
                    period: Duration::from_millis(60000),
                }),
                ..Default::default()
            },
            &[Property::UseXForwarded, Property::RateLimitAnonymous],
        )
        .await;
    // The cache module logs a key account in over plain-text IMAP.
    admin
        .registry_update_setting(
            Imap {
                allow_plain_text_auth: true,
                ..Default::default()
            },
            &[Property::AllowPlainTextAuth],
        )
        .await;
    admin.reload_settings().await;
    test.insert_account(plain);
    test.insert_account(admin);

    setup::test(&mut test).await;
    setup::test_data_check(&mut test).await;
    disabled::test(&mut test).await;
    password::test(&mut test).await;
    app_password::test(&mut test).await;
    totp::test(&mut test).await;
    registry::test(&mut test).await;
    cors::test(&mut test).await;
    caches::test(&mut test).await;
    dav_gate::test(&mut test).await;
    dav_seal::test(&mut test).await;
    dav_seal::test_reports(&mut test).await;
    dav_seal::test_collections(&mut test).await;
    gating::test(&mut test).await;

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

/// `assert_is_empty` allows a vault record only while its account exists;
/// destroying every key account at the end checks that none outlives it.
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
