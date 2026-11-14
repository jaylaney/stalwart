/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Focused whole-suite runner for PR 84 regressions. The canonical ZA suite
//! also calls every fixture; run this suite alone because it shares ports.

use super::{
    case_mapped_get_and_query, imap_guards, key_mail_account, protocol, trusted_references,
};
use crate::{
    utils::{server::TestServerBuilder, za::expire_and_purge_blobs},
    za::{STRONG, destroy_key_accounts, mail::cache_idle},
};
use registry::{
    schema::{
        prelude::Property,
        structs::{Expression, Http, Imap, MtaStageAuth, Rate},
    },
    types::duration::Duration,
};

#[tokio::test(flavor = "multi_thread")]
async fn pr84_regression_tests() {
    let mut test = TestServerBuilder::new("za_pr84_regression_tests")
        .await
        .with_default_listeners()
        .await
        .build()
        .await;
    let admin = test.create_admin_account("admin@example.com").await;
    admin
        .registry_update_setting(
            Http {
                rate_limit_anonymous: Some(Rate {
                    count: 10000,
                    period: Duration::from_millis(60000),
                }),
                ..Default::default()
            },
            &[Property::RateLimitAnonymous],
        )
        .await;
    admin
        .registry_update_setting(
            Imap {
                allow_plain_text_auth: true,
                ..Default::default()
            },
            &[Property::AllowPlainTextAuth],
        )
        .await;
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
    test.insert_account(admin);

    crate::za::mail::index::pr84_retry_followup_tests(&mut test).await;

    let key = key_mail_account(&mut test, "keymail-pr84-references@example.com", STRONG).await;
    let (root, root_id, _) = case_mapped_get_and_query(&test, &key).await;
    protocol::test(&test, &key, &root, root_id).await;
    imap_guards::test(&mut test).await;
    trusted_references::test(&test, &key, &root, root_id).await;
    test.destroy_all_mailboxes(&key).await;

    cache_idle::test(&mut test).await;

    destroy_key_accounts(&test).await;
    expire_and_purge_blobs(&test, test.account("admin@example.com")).await;
    test.assert_is_empty().await;
    test.shutdown();
    if test.is_reset() {
        test.temp_dir.delete();
    }
}
