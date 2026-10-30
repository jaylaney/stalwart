/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

// 1.0 formats dates with jiff; chrono is no longer a dependency.
fn utc_today(fmt: &str) -> String {
    strtime::format(fmt, &Timestamp::now().to_zoned(TimeZone::UTC)).unwrap()
}

#[test]
fn basic_algorithm_date() {
    let sel = generate_dkim_selector(
        "{algorithm}-{date-%Y%m%d}",
        DkimSignatureType::Dkim1RsaSha256,
    )
    .unwrap();
    let today = utc_today("%Y%m%d");
    assert_eq!(sel, format!("rsa-{today}"));
}

#[test]
fn all_variables() {
    let sel = generate_dkim_selector(
        "v{version}-{algorithm}-{hash}-{epoch}",
        DkimSignatureType::Dkim1Ed25519Sha256,
    )
    .unwrap();
    assert!(sel.starts_with("v1-ed25519-sha256-"));
}

#[test]
fn literal_only() {
    let sel = generate_dkim_selector("my-static-selector", DkimSignatureType::default()).unwrap();
    assert_eq!(sel, "my-static-selector");
}

#[test]
fn invalid_chars_stripped() {
    let sel =
        generate_dkim_selector("{algorithm} {hash}", DkimSignatureType::Dkim1RsaSha256).unwrap();
    assert_eq!(sel, "rsasha256");
}

#[test]
fn unrecognized_variable_errors() {
    let err = generate_dkim_selector("{bogus}", DkimSignatureType::default()).unwrap_err();
    assert!(err.contains("unrecognized variable"));
}

#[test]
fn unclosed_brace_errors() {
    let err = generate_dkim_selector("{algorithm", DkimSignatureType::default()).unwrap_err();
    assert!(err.contains("unclosed"));
}

#[test]
fn empty_after_sanitization_errors() {
    let err = generate_dkim_selector("   ", DkimSignatureType::default()).unwrap_err();
    assert!(err.contains("empty"));
}

#[test]
fn empty_date_format_errors() {
    let err = generate_dkim_selector("{date-}", DkimSignatureType::default()).unwrap_err();
    assert!(err.contains("empty strftime"));
}

#[test]
fn date_month_only() {
    let sel = generate_dkim_selector("{date-%Y%m}", DkimSignatureType::Dkim1RsaSha256).unwrap();
    let expected = utc_today("%Y%m");
    assert_eq!(sel, expected);
}
