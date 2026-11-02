/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::ParsePolicy;
use crate::outbound::mta_sts::verify::VerifyPolicy;
use common::config::smtp::resolver::Policy;

#[test]
fn mx_patterns_are_a_labels() {
    let policy = Policy::parse(
        concat!(
            "version: STSv1\n",
            "mode: enforce\n",
            "mx: *.\u{5de}\u{5d9}\u{5d9}\u{5dc}.\u{5e7}\u{5d5}\u{5dd}\n",
            "mx: MAIL.\u{5de}\u{5d9}\u{5d9}\u{5dc}.\u{5e7}\u{5d5}\u{5dd}\n",
            "max_age: 604800\n"
        ),
        "test".to_string(),
    )
    .unwrap();

    assert!(policy.verify("mx.xn--eebajf.xn--9dbq2a"));
    assert!(policy.verify("mail.xn--eebajf.xn--9dbq2a"));
    assert!(!policy.verify("mx.example.org"));
}
