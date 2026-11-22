/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

pub mod antispam;
pub mod authentication;
pub mod authorization;
pub mod crypto;
pub mod delivery;
pub mod directory;
pub mod oidc;
pub mod purge;
pub mod quota;
pub mod security;
pub mod task;

use crate::utils::{
    server::{TestServer, TestServerBuilder},
    tls::{FixtureTls, fixture_https_client_builder},
};
use registry::schema::{
    enums::NetworkListenerProtocol,
    structs::{Expression, Imap, MtaStageAuth},
};
use rustls::CertificateError;
use std::{error::Error, io, net::Ipv4Addr, sync::Arc, time::Duration};

/// Submission over implicit TLS: 1.0 offers the bearer SASL mechanisms
/// only over TLS, so `oidc::test` authenticates with OAUTHBEARER here.
pub const SUBMISSION_PORT: u16 = 4467;

fn tls_certificate_error(error: &reqwest::Error) -> Option<&CertificateError> {
    let mut cause: &(dyn Error + 'static) = error;
    loop {
        if let Some(rustls::Error::InvalidCertificate(reason)) =
            cause.downcast_ref::<rustls::Error>()
        {
            return Some(reason);
        }

        // An io::Error's source can skip the stored error itself.
        cause = if let Some(io_error) = cause.downcast_ref::<io::Error>() {
            io_error.get_ref()?
        } else {
            cause.source()?
        };
    }
}

async fn assert_fixture_tls(test: &TestServer, port: u16) {
    let loopback = (Ipv4Addr::LOCALHOST, port).into();
    let timeout = Duration::from_secs(5);
    let client = fixture_https_client_builder()
        .timeout(timeout)
        .resolve("localhost", loopback)
        .resolve("wrong-host.invalid", loopback)
        .build()
        .unwrap();

    for host in ["127.0.0.1", "localhost"] {
        assert_eq!(
            client
                .get(format!("https://{host}:{port}/healthz/live"))
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::OK
        );
    }

    let wrong_name = client
        .get(format!("https://wrong-host.invalid:{port}/healthz/live"))
        .send()
        .await
        .expect_err("fixture HTTPS client accepted a mismatched hostname");
    assert!(
        matches!(
            tls_certificate_error(&wrong_name),
            Some(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. }
            )
        ),
        "expected a TLS hostname mismatch, got {wrong_name:?}"
    );

    // A distinct issuer name exercises issuer lookup. The alternate certificate
    // must take precedence over the fixture fallback on a fresh connection.
    let alternate = FixtureTls::generate("Stalwart Alternate Test CA");
    let certificates = &test.server.inner.data.tls_certificates;
    let original = certificates.swap(Arc::new(
        [("*".into(), alternate.server_key)].into_iter().collect(),
    ));
    let url = format!("https://127.0.0.1:{port}/healthz/live");
    let untrusted = fixture_https_client_builder()
        .timeout(timeout)
        .build()
        .unwrap()
        .get(&url)
        .send()
        .await;
    let trusted = reqwest::Client::builder()
        .tls_certs_only([alternate.ca])
        .no_proxy()
        .timeout(timeout)
        .build()
        .unwrap()
        .get(&url)
        .send()
        .await;
    certificates.store(original);

    let untrusted = untrusted.expect_err("fixture HTTPS client trusted a different CA");
    assert_eq!(
        tls_certificate_error(&untrusted),
        Some(&CertificateError::UnknownIssuer),
        "{untrusted:?}"
    );
    assert_eq!(trusted.unwrap().status(), reqwest::StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread")]
pub async fn system_tests() {
    let builder = TestServerBuilder::new("system_tests")
        .await
        .with_default_listeners()
        .await
        .with_listener(
            NetworkListenerProtocol::Smtp,
            "submissions",
            SUBMISSION_PORT,
            true,
        )
        .await
        .with_object(Imap {
            allow_plain_text_auth: true,
            ..Default::default()
        })
        .await
        .with_object(MtaStageAuth {
            require: Expression {
                else_: "false".to_string(),
                ..Default::default()
            },
            ..Default::default()
        })
        .await;
    builder.assert_listeners_bind_to_loopback().await;
    let http_port = builder.http_listener_port();
    let mut test = builder.build().await;
    assert_fixture_tls(&test, http_port).await;

    // Create admin account
    let admin = test
        .create_user_account(
            "admin",
            "admin@example.org",
            "these_pretzels_are_making_me_thirsty",
            &[],
            "Admin",
        )
        .await;
    test.account("admin")
        .assign_roles_to_account(admin.id(), &["user", "system"])
        .await;
    test.insert_account(admin);

    directory::test(&test).await;
    authentication::test(&test).await;
    oidc::test(&mut test).await;
    authorization::test(&mut test).await;
    security::test(&mut test).await;
    quota::test(&mut test).await;
    purge::test(&mut test).await;
    delivery::test(&mut test).await;
    crypto::test(&mut test).await;
    antispam::test(&mut test).await;
    task::test(&mut test).await;

    if test.is_reset() {
        test.temp_dir.delete();
    }
}
