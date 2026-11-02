/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::chain_matches;
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_ECDSA_P256_SHA256};

fn self_signed_pem(common_name: &str) -> String {
    let mut params = CertificateParams::new(vec!["host.example".to_string()]).unwrap();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    params.distinguished_name = dn;
    let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    params.self_signed(&key_pair).unwrap().pem()
}

#[test]
fn matches_top_certificate_issuer() {
    let chain = self_signed_pem("ISRG Root X1");
    assert!(chain_matches(&chain, "ISRG Root X1"));
}

#[test]
fn match_is_case_sensitive() {
    let chain = self_signed_pem("ISRG Root X1");
    assert!(!chain_matches(&chain, "isrg root x1"));
}

#[test]
fn match_is_exact_not_substring() {
    let chain = self_signed_pem("ISRG Root X10");
    assert!(!chain_matches(&chain, "ISRG Root X1"));
}

#[test]
fn does_not_match_unrelated_issuer() {
    let chain = self_signed_pem("ISRG Root X2");
    assert!(!chain_matches(&chain, "ISRG Root X1"));
}

#[test]
fn uses_topmost_certificate_not_leaf() {
    let leaf = self_signed_pem("Leaf Issuer");
    let top = self_signed_pem("ISRG Root X1");
    let chain = format!("{leaf}{top}");
    assert!(chain_matches(&chain, "ISRG Root X1"));
    assert!(!chain_matches(&chain, "Leaf Issuer"));
}

#[test]
fn rejects_unparseable_chain() {
    assert!(!chain_matches("not a pem", "ISRG Root X1"));
}
