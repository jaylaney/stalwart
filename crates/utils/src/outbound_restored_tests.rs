/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

// Two 0.16 modules whose subjects 1.0 moved into this file: the test of
// `is_global_ip` from `common::network` and the test of
// `is_same_organization` from `services::state_manager::http`.

use super::{is_global_ip, is_same_organization};
use reqwest::Url;
use std::net::IpAddr;

#[test]
fn global_ip_classification() {
    for ip in [
        "8.8.8.8",
        "1.1.1.1",
        "93.184.216.34",
        "172.32.0.1",
        "100.63.255.255",
        "100.128.0.1",
        "198.20.0.1",
        "192.0.3.1",
        "2606:4700::1111",
        "2a00:1450:4001::200e",
        "::ffff:8.8.8.8",
        "64:ff9b::808:808",
        "2002:0808:0808::",
        "2001:db9::1",
    ] {
        assert!(
            is_global_ip(&ip.parse::<IpAddr>().unwrap()),
            "expected {ip} to be global"
        );
    }

    for ip in [
        "127.0.0.1",
        "127.1.2.3",
        "10.0.0.1",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "100.127.255.255",
        "198.18.0.1",
        "198.19.255.255",
        "192.0.0.1",
        "192.0.2.5",
        "192.88.99.1",
        "240.0.0.1",
        "255.255.255.255",
        "224.0.0.1",
        "0.0.0.0",
        "0.1.2.3",
        "::1",
        "::",
        "fc00::1",
        "fd12:3456::1",
        "fe80::1",
        "febf::1",
        "fec0::1",
        "2001:db8::1",
        "ff02::1",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
        "::127.0.0.1",
        "64:ff9b::7f00:1",
        "64:ff9b::a00:1",
        "64:ff9b:1::7f00:1",
        "2002:7f00:1::",
        "2002:c0a8:101::",
        "100::1",
        "2001::1",
        "2001:20::1",
    ] {
        assert!(
            !is_global_ip(&ip.parse::<IpAddr>().unwrap()),
            "expected {ip} to be rejected"
        );
    }
}

#[test]
fn same_organization_redirects() {
    for (previous, next, expected) in [
        (
            "https://push.example.org/a",
            "https://push.example.org/b",
            true,
        ),
        (
            "https://push.example.org/a",
            "https://push2.example.org/b",
            true,
        ),
        ("https://push.example.org/a", "https://example.org/b", true),
        (
            "https://push.example.org/a",
            "https://push.example.org:8443/b",
            true,
        ),
        (
            "https://push.example.org/a",
            "https://push.evil.org/b",
            false,
        ),
        (
            "https://push.example.org/a",
            "http://push.example.org/b",
            false,
        ),
        (
            "https://push.example.co.uk/a",
            "https://evil.co.uk/b",
            false,
        ),
        ("https://1.2.3.4/a", "https://1.2.3.4/b", true),
        ("https://1.2.3.4/a", "https://5.6.7.8/b", false),
        ("https://1.2.3.4/a", "https://5.6.3.4/b", false),
        ("https://1.2.3.4/a", "https://127.0.0.1/b", false),
        ("https://[2606:4700::1111]/a", "https://[::1]/b", false),
        (
            "https://[2606:4700::1111]/a",
            "https://[2606:4700::1111]/b",
            true,
        ),
    ] {
        let previous = Url::parse(previous).unwrap();
        let next = Url::parse(next).unwrap();

        assert_eq!(
            is_same_organization(&previous, &next),
            expected,
            "{previous} -> {next}"
        );
    }
}
