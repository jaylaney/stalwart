/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{Email, Hostname};

#[test]
fn hostname_punycode_round_trip() {
    for (host, fqdn, sld) in [
        ("mail.example.com", "mail.example.com", Some("example.com")),
        (
            "MAIL.Example.CO.UK.",
            "mail.example.co.uk",
            Some("example.co.uk"),
        ),
        (
            "mail.xn--eebajf.xn--9dbq2a",
            "mail.\u{5de}\u{5d9}\u{5d9}\u{5dc}.\u{5e7}\u{5d5}\u{5dd}",
            Some("\u{5de}\u{5d9}\u{5d9}\u{5dc}.\u{5e7}\u{5d5}\u{5dd}"),
        ),
        ("xn--gmail-.com", "xn--gmail-.com", Some("xn--gmail-.com")),
        (
            "xn--example-.org",
            "xn--example-.org",
            Some("xn--example-.org"),
        ),
        ("xn--.com", "xn--.com", Some("xn--.com")),
        ("127.0.0.1", "127.0.0.1", None),
    ] {
        let parsed = Hostname::new(host);
        assert_eq!(parsed.fqdn, fqdn, "fqdn of {host:?}");
        assert_eq!(parsed.sld.as_deref(), sld, "sld of {host:?}");
    }
}

#[test]
fn email_a_label_and_u_label_are_equal() {
    assert_eq!(
        Email::new("bill@xn--eebajf.xn--9dbq2a"),
        Email::new("bill@\u{5de}\u{5d9}\u{5d9}\u{5dc}.\u{5e7}\u{5d5}\u{5dd}")
    );
    assert_ne!(
        Email::new("bill@example.com"),
        Email::new("bob@example.com")
    );
    assert_ne!(Email::new("postmaster"), Email::new("mailer-daemon"));
    assert_ne!(
        Email::new("victim@xn--gmail-.com"),
        Email::new("victim@gmail.com")
    );
}
