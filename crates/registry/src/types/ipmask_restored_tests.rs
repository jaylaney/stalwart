/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn test_ipaddrmask() {
    for (mask, ip) in [
        ("10.0.0.0/8", "10.30.20.11"),
        ("10.0.0.0/8", "10.0.13.73"),
        ("192.168.1.1", "192.168.1.1"),
    ] {
        let mask = IpAddrOrMask::from_str(mask).unwrap();
        let ip = ip.parse::<IpAddr>().unwrap();
        assert!(mask.matches(&ip));
    }

    for (mask, ip) in [
        ("10.0.0.0/8", "11.30.20.11"),
        ("192.168.1.1", "193.168.1.1"),
    ] {
        let mask = IpAddrOrMask::from_str(mask).unwrap();
        let ip = ip.parse::<IpAddr>().unwrap();
        assert!(!mask.matches(&ip));
    }
}
