/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

fn jwt(payload: &str) -> String {
    format!(
        "eyJhbGciOiJSUzI1NiJ9.{}.c2lnbmF0dXJl",
        URL_SAFE_NO_PAD.encode(payload)
    )
}

fn hints(token: &str) -> Option<(Option<String>, Option<String>)> {
    let payload = JwtClaims::decode_payload(token)?;
    let claims = JwtClaims::parse(&payload)?;

    Some((
        claims.domain().map(str::to_string),
        claims.iss.as_deref().map(str::to_string),
    ))
}

#[test]
fn jwt_claims_are_extracted() {
    for (payload, domain, issuer) in [
        (
            r#"{"iss":"https://idp.example.org","email":"John@Example.ORG"}"#,
            Some("Example.ORG"),
            Some("https://idp.example.org"),
        ),
        (
            r#"{"preferred_username":"jane@example.net","upn":"jane@example.com"}"#,
            Some("example.net"),
            None,
        ),
        (
            r#"{"email":"broken@","upn":"jane@example.com"}"#,
            Some("example.com"),
            None,
        ),
        (
            r#"{"iss":"https://idp.example.org","sub":"5db2d1b6","aud":["a","b"],"scope":"openid"}"#,
            None,
            Some("https://idp.example.org"),
        ),
        (r#"{"sub":"5db2d1b6"}"#, None, None),
        (r#"{"email":"jane@example.net"}"#, Some("example.net"), None),
    ] {
        assert_eq!(
            hints(&jwt(payload)),
            Some((domain.map(str::to_string), issuer.map(str::to_string))),
            "Unexpected claims for {payload}"
        );
    }
}

#[test]
fn non_jwt_tokens_are_ignored() {
    for token in [
        "sw1.eyJhbGciOiJSUzI1NiJ9.eyJpc3MiOiJodHRwczovL2lkcC5leGFtcGxlLm9yZyJ9",
        "sw1.eyJhbGciOiJSUzI1NiJ9",
        "opaque-token",
        "one.two",
        "one.two.three.four",
        "",
    ] {
        assert!(
            JwtClaims::decode_payload(token).is_none(),
            "Token {token:?} was parsed as a JWT"
        );
    }
}
