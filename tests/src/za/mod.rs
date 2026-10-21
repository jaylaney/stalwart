/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::server::{TestServer, TestServerBuilder};
use ::registry::schema::{
    enums::{NetworkListenerProtocol, Permission},
    prelude::Property,
    structs::{Expression, Http, Imap, MtaStageAuth, Rate},
};
use ::registry::types::duration::Duration;
use common::za::test_hooks;

pub mod app_password;
pub mod caches;
pub mod cors;
pub mod dav_gate;
pub mod dav_seal;
pub mod disabled;
pub mod drain;
pub mod drain_limits;
pub mod expansion;
pub mod fanout;
pub mod gating;
pub mod inbox;
pub mod index_task;
pub mod leak;
pub mod mail;
pub mod pairs;
pub mod password;
pub mod pinning;
pub mod raw_input;
pub mod registry;
pub mod reply;
pub mod setup;
pub mod sweep;
pub mod totp;
pub mod tracing;
pub mod uid_lease;
pub mod websocket;

pub const STRONG: &str = "correct horse battery staple 1";

/// Origin of the account page, allowed by CORS on `/api/vault/*`.
pub const ACCOUNT_PAGE_ORIGIN: &str = "https://account.example.com";

#[tokio::test(flavor = "multi_thread")]
pub async fn za_tests() {
    let mut test = TestServerBuilder::new("za_tests")
        .await
        .with_default_listeners()
        .await
        // Submission over implicit TLS, for the raw-input trace tests.
        .with_listener(
            NetworkListenerProtocol::Smtp,
            "submissions",
            raw_input::SUBMISSION_PORT,
            true,
        )
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
    // The mail modules deliver over the LMTP listener without authenticating
    // (`mail::deliver`); every other listener keeps upstream's requirement.
    admin
        .registry_update_setting(
            MtaStageAuth {
                require: Expression {
                    else_: "local_port != 25 && local_port != 11200".to_string(),
                    ..Default::default()
                },
                ..Default::default()
            },
            &[Property::Require],
        )
        .await;
    admin.reload_settings().await;
    test.insert_account(plain);
    test.insert_account(admin);
    // Plan 10: the drain runs on every DAV request of a key account. The
    // plan 8 and 9 modules read pending items at store level, so the suite
    // skips every account's drain; the drain modules turn it on (`DrainOn`).
    test_hooks::set_drain_skip(test_hooks::DRAIN_SKIP_ALL);

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
    dav_seal::test_conditional(&mut test).await;
    dav_seal::test_legacy(&mut test).await;
    fanout::test_task_queue(&mut test).await;
    fanout::test(&mut test).await;
    fanout::test_failures(&mut test).await;
    fanout::test_plain_owner(&mut test).await;
    reply::test(&mut test).await;
    uid_lease::test(&mut test).await;
    drain::test(&mut test).await;
    drain::test_reply(&mut test).await;
    drain::test_adoption(&mut test).await;
    drain::test_lease(&mut test).await;
    drain::test_cursor(&mut test).await;
    drain::test_errors(&mut test).await;
    drain_limits::test(&mut test).await;
    pinning::test(&mut test).await;
    sweep::test(&mut test).await;
    tracing::test(&mut test).await;
    raw_input::test(&mut test).await;
    expansion::test(&mut test).await;
    gating::test(&mut test).await;
    gating::test_scheduling(&mut test).await;
    websocket::test(&mut test).await;
    pairs::test(&mut test).await;
    inbox::test(&mut test).await;
    inbox::test_sync(&mut test).await;
    inbox::test_expiry(&mut test).await;
    leak::test_drain(&mut test).await;
    leak::test_pins(&mut test).await;
    leak::test(&mut test).await;
    leak::test_index_control(&mut test).await;
    index_task::test(&mut test).await;
    index_task::test_sealed_archive(&mut test).await;
    mail::login::test(&mut test).await;
    mail::delivery::test(&mut test).await;
    mail::writes::test(&mut test).await;
    mail::fence::test(&mut test).await;
    mail::reads::test(&mut test).await;

    destroy_key_accounts(&test).await;
    test.assert_is_empty().await;
    test_hooks::clear_drain_skip(test_hooks::DRAIN_SKIP_ALL);

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
