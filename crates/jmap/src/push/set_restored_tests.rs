/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::validate_push_url;

#[test]
fn push_url_validation() {
    for url in [
        "https://push.example.org/subscription/123",
        "https://push.example.org:8443/subscription/123",
        "HTTPS://push.example.org/subscription/123",
        "https://8.8.8.8/push",
        "https://[2606:4700::1111]/push",
        "https://[64:ff9b::808:808]/push",
    ] {
        assert!(validate_push_url(url).is_ok(), "expected {url} to be valid");
    }

    for url in [
        "http://push.example.org/push",
        "ftp://push.example.org/push",
        "https://user:pass@push.example.org/push",
        "not a url",
        "https://",
        "https://10.0.0.1/push",
        "https://192.168.1.1/push",
        "https://169.254.169.254/latest/meta-data/",
        "https://100.100.100.200/push",
        "https://[fd00::1]/push",
        "https://[fe80::1]/push",
        "https://[::ffff:127.0.0.1]/push",
        "https://[64:ff9b::7f00:1]/push",
    ] {
        assert!(
            validate_push_url(url).is_err(),
            "expected {url} to be rejected"
        );
    }

    for url in [
        "https://127.0.0.1/push",
        "https://0177.0.0.1/push",
        "https://2130706433/push",
        "https://0x7f000001/push",
        "https://[::1]/push",
    ] {
        let result = validate_push_url(url);

        if cfg!(feature = "test_mode") {
            assert!(
                result.is_ok(),
                "expected {url} to be allowed under test_mode"
            );
        } else {
            assert!(result.is_err(), "expected {url} to be rejected");
        }
    }
}
