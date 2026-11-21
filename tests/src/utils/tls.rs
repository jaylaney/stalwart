/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use rustls::{crypto::aws_lc_rs, pki_types::PrivatePkcs8KeyDer, sign::CertifiedKey};
use std::sync::{Arc, LazyLock};

static FIXTURE_TLS: LazyLock<FixtureTls> = LazyLock::new(FixtureTls::generate);

pub(crate) struct FixtureTls {
    pub(crate) ca: reqwest::Certificate,
    pub(crate) server_key: Arc<CertifiedKey>,
}

impl FixtureTls {
    pub(crate) fn generate() -> Self {
        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, "Stalwart Integration Test CA");
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let issuer = CertifiedIssuer::self_signed(
            params,
            KeyPair::generate().expect("generate fixture CA key"),
        )
        .expect("generate fixture CA certificate");

        let mut params = CertificateParams::new(vec!["127.0.0.1".into(), "localhost".into()])
            .expect("parse fixture certificate names");
        params
            .distinguished_name
            .push(DnType::CommonName, "localhost");
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let key = KeyPair::generate().expect("generate fixture server key");
        let leaf = params
            .signed_by(&key, &issuer)
            .expect("sign fixture server certificate");
        let server_key = CertifiedKey::from_der(
            vec![leaf.der().clone()],
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            &aws_lc_rs::default_provider(),
        )
        .expect("load fixture server certificate and key");

        Self {
            ca: reqwest::Certificate::from_der(issuer.der().as_ref())
                .expect("load fixture CA certificate"),
            server_key: Arc::new(server_key),
        }
    }
}

pub(crate) fn fixture_tls_server_key() -> Arc<CertifiedKey> {
    FIXTURE_TLS.server_key.clone()
}

/// For direct HTTPS requests to the loopback test server's fallback certificate.
pub(crate) fn fixture_https_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .tls_certs_only([FIXTURE_TLS.ca.clone()])
        .no_proxy()
}
