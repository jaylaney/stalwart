/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn rate_limit_headers_match_spec() {
    // Spec example: RateLimit-Policy: "burst";q=100;w=60,"daily";q=1000;w=86400
    let mut p1 = String::new();
    RateLimitPolicy::new("burst", 100)
        .with_window(60)
        .fmt_policy(&mut p1);
    assert_eq!(p1, r#""burst";q=100;w=60"#);

    // Spec example: RateLimit-Policy: "peruser";q=65535;qu="content-bytes";w=10
    let mut p2 = String::new();
    RateLimitPolicy::new("peruser", 65535)
        .with_window(10)
        .with_unit(RateLimitUnit::ContentBytes)
        .fmt_policy(&mut p2);
    assert_eq!(p2, r#""peruser";q=65535;w=10;qu="content-bytes""#);

    // Spec example: RateLimit: "default";r=50;t=30
    let mut s = String::new();
    RateLimitPolicy::new("default", 100)
        .with_remaining(50)
        .with_reset(30)
        .fmt_state(&mut s);
    assert_eq!(s, r#""default";r=50;t=30"#);

    // Two policies in one header
    let err = RequestError::too_many_requests()
        .with_rate_limit(
            RateLimitPolicy::new("burst", 100)
                .with_window(60)
                .with_reset(30),
        )
        .with_rate_limit(
            RateLimitPolicy::new("daily", 1000)
                .with_window(86400)
                .with_reset(3600),
        );
    assert_eq!(
        err.rate_limit_policy_header().as_deref(),
        Some(r#""burst";q=100;w=60, "daily";q=1000;w=86400"#),
    );
    assert_eq!(
        err.rate_limit_state_header().as_deref(),
        Some(r#""burst";r=0;t=30, "daily";r=0;t=3600"#),
    );
    assert_eq!(err.retry_after, Some(3600));
}
