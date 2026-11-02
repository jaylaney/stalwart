/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{loopback_redirect_parts, redirect_uri_matches};

#[test]
fn loopback_matches_any_port() {
    // Loopback redirects match regardless of port and of an empty versus "/" path (RFC 8252 §7.3)
    // (ruling 44) 1.0 matches loopback URIs across ports only when both
    // carry a path after the host or port.
    for (registered, presented) in [
        ("http://127.0.0.1/", "http://127.0.0.1:45678/"),
        ("http://127.0.0.1/cb", "http://127.0.0.1:45678/cb"),
        ("http://127.0.0.1/?a=1", "http://127.0.0.1:45678/?a=1"),
        ("http://[::1]/", "http://[::1]:8080/"),
    ] {
        assert!(
            redirect_uri_matches(registered, presented),
            "{registered} should match {presented}"
        );
    }

    // Ruling 44: 1.0 refuses a loopback URI without a path on either side
    // (`loopback_redirect_parts` needs a '/' after the host or port), which
    // RFC 8252 section 7.3 does not; 0.16 matched all of these. Documents
    // the 1.0 behaviour, recorded as an apparent upstream bug.
    for (registered, presented) in [
        ("http://127.0.0.1", "http://127.0.0.1:45678"),
        ("http://127.0.0.1/", "http://127.0.0.1:45678"),
        ("http://127.0.0.1", "http://127.0.0.1:45678/"),
        ("http://127.0.0.1/?a=1", "http://127.0.0.1:45678?a=1"),
        ("http://[::1]", "http://[::1]:8080"),
    ] {
        assert!(
            !redirect_uri_matches(registered, presented),
            "1.0 no longer matches {registered} with {presented}"
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
