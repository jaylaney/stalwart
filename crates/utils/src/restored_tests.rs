/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::DomainPart;

use super::{sanitize_domain, sanitize_email};

#[test]
fn idn_domains_canonicalize_to_a_label() {
    assert_eq!(
        sanitize_domain("straß6.de").as_deref(),
        Some("xn--stra6-oqa.de")
    );
    assert_eq!(
        sanitize_domain("STRASS.straß6.DE").as_deref(),
        Some("strass.xn--stra6-oqa.de")
    );
    assert_eq!(
        sanitize_domain("münchen.de").as_deref(),
        Some("xn--mnchen-3ya.de")
    );
}

#[test]
fn a_label_and_ascii_domains_are_idempotent() {
    assert_eq!(
        sanitize_domain("xn--stra6-oqa.de").as_deref(),
        Some("xn--stra6-oqa.de")
    );
    assert_eq!(
        sanitize_domain(&sanitize_domain("straß6.de").unwrap()).as_deref(),
        Some("xn--stra6-oqa.de")
    );
    assert_eq!(
        sanitize_domain("Example.COM").as_deref(),
        Some("example.com")
    );
}

#[test]
fn email_domain_part_canonicalizes_local_part_preserved() {
    assert_eq!(
        sanitize_email("cornelius_strauss@straß6.de").as_deref(),
        Some("cornelius_strauss@xn--stra6-oqa.de")
    );
    assert_eq!(
        sanitize_email("Foo.Bar@münchen.de").as_deref(),
        Some("foo.bar@xn--mnchen-3ya.de")
    );
    assert_eq!(
        sanitize_email("user@example.com").as_deref(),
        Some("user@example.com")
    );
}

#[test]
fn bare_public_suffix_domains_are_accepted() {
    assert_eq!(
        sanitize_email("user@gov.in").as_deref(),
        Some("user@gov.in")
    );
    assert_eq!(sanitize_email("user@co.uk").as_deref(), Some("user@co.uk"));
    assert_eq!(sanitize_email("user@com"), None);
    assert_eq!(sanitize_email("user@example.invalidtld"), None);
}

#[test]
fn a_label_email_domains_are_accepted_and_idempotent() {
    assert_eq!(
        sanitize_email("user@xn--fsqu00a.com").as_deref(),
        Some("user@xn--fsqu00a.com")
    );
    assert_eq!(
        sanitize_email("User@例子.com").as_deref(),
        sanitize_email("user@xn--fsqu00a.com").as_deref()
    );
}

#[test]
fn to_ascii_domain_borrows_ascii_owns_idn() {
    assert!(matches!(
        "example.com".to_ascii_domain(),
        Some(std::borrow::Cow::Borrowed(_))
    ));
    assert!(matches!(
        "straß6.de".to_ascii_domain(),
        Some(std::borrow::Cow::Owned(_))
    ));
}
