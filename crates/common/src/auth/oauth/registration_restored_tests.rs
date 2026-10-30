/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{loopback_redirect_parts, redirect_uri_matches};

#[test]
fn loopback_matches_any_port() {
    // Loopback redirects match regardless of port and of an empty versus "/" path (RFC 8252 §7.3)
    for (registered, presented) in [
        ("http://127.0.0.1", "http://127.0.0.1:45678"),
        ("http://127.0.0.1/", "http://127.0.0.1:45678"),
        ("http://127.0.0.1", "http://127.0.0.1:45678/"),
        ("http://127.0.0.1/cb", "http://127.0.0.1:45678/cb"),
        ("http://127.0.0.1/?a=1", "http://127.0.0.1:45678?a=1"),
        ("http://[::1]", "http://[::1]:8080"),
        ("http://[::1]/", "http://[::1]:8080/"),
    ] {
        assert!(
            redirect_uri_matches(registered, presented),
            "{registered} should match {presented}"
        );
    }

    // Different paths, hosts or schemes never match
    for (registered, presented) in [
        ("http://127.0.0.1/cb", "http://127.0.0.1:45678"),
        ("http://127.0.0.1", "http://127.0.0.1:45678/cb"),
        ("http://127.0.0.1", "http://[::1]:45678"),
        ("http://127.0.0.1", "https://127.0.0.1:45678"),
        ("http://127.0.0.1", "http://127.0.0.1.evil.com"),
        ("http://127.0.0.1", "http://127.0.0.1:80@evil.com"),
        ("http://127.0.0.1", "http://127.0.0.1:45678#frag"),
    ] {
        assert!(
            !redirect_uri_matches(registered, presented),
            "{registered} should not match {presented}"
        );
    }

    // Malformed loopback URIs are not recognised
    for uri in [
        "http://127.0.0.1:",
        "http://127.0.0.1:/cb",
        "http://127.0.0.1:abc/",
        "http://127.0.0.1.evil.com/",
        "http://127.0.0.1@evil.com/",
        "http://localhost:8080/",
    ] {
        assert_eq!(loopback_redirect_parts(uri), None, "{uri}");
    }
}
