/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

const KEY: &[u8] = b"a-test-encryption-key-of-some-length";
const NAME: &[u8] = b"user@example.org";

// 1.0 seals with a `SymmetricEncrypt` and a caller-chosen nonce; these
// wrappers keep 0.16's key-based calls, with a random nonce per token.
fn seal_token(key: &[u8], raw: &RawToken, footer: &[u8]) -> Result<String, String> {
    let nonce = store::rand::random::<[u8; SymmetricEncrypt::NONCE_LEN]>();
    super::seal_token(
        &SymmetricEncrypt::new(key, "restored-token-tests"),
        raw,
        footer,
        &nonce,
    )
}

fn open_token(key: &[u8], token: &str) -> Result<RawToken, ()> {
    super::open_token(&SymmetricEncrypt::new(key, "restored-token-tests"), token)
}

fn sample(grant_type: GrantType, claims: Option<&str>, cv: u64) -> RawToken {
    RawToken {
        grant_type,
        account_id: 42,
        claims: claims.map(|c| c.to_string()),
        issued_at: 1_000,
        expiry: 2_000,
        credential_version: cv,
        grant: None,
    }
}

fn assert_eq_fields(a: &RawToken, b: &RawToken) {
    assert_eq!(a.account_id, b.account_id);
    assert_eq!(a.grant_type, b.grant_type);
    assert_eq!(a.claims, b.claims);
    assert_eq!(a.issued_at, b.issued_at);
    assert_eq!(a.expiry, b.expiry);
    assert_eq!(a.credential_version, b.credential_version);
}

#[test]
fn round_trip_preserves_all_fields() {
    for (raw, footer) in [
        (sample(GrantType::AccessToken, None, 0), NAME),
        (
            sample(GrantType::RefreshToken, None, 0xdead_beef_cafe),
            NAME,
        ),
        (
            sample(GrantType::Rsvp, Some("attendee@x.org;7"), 0),
            b"owner@example.org",
        ),
        (sample(GrantType::AccessToken, None, 0), b""),
        (
            RawToken {
                account_id: u32::MAX,
                credential_version: u64::MAX,
                ..sample(GrantType::AccessToken, Some("名前;1"), 1)
            },
            "名字@example.org".as_bytes(),
        ),
    ] {
        let token = seal_token(KEY, &raw, footer).unwrap();
        assert!(token.starts_with(TOKEN_HEADER));
        let opened = open_token(KEY, &token).unwrap();
        assert_eq_fields(&raw, &opened);

        // The footer (account name) round-trips in clear text for proxies
        if footer.is_empty() {
            assert!(!token[TOKEN_HEADER.len()..].contains('.'));
        } else {
            let segment = token.rsplit_once('.').unwrap().1;
            assert_eq!(URL_SAFE_NO_PAD.decode(segment).unwrap(), footer);
        }
    }
}

#[test]
fn account_name_is_readable_in_clear_text_footer() {
    let token = seal_token(
        KEY,
        &sample(GrantType::AccessToken, None, 0),
        b"route-me@example.org",
    )
    .unwrap();
    let footer = token.rsplit_once('.').unwrap().1;
    let decoded = URL_SAFE_NO_PAD.decode(footer).unwrap();
    assert_eq!(decoded, b"route-me@example.org");
}

#[test]
fn wrong_key_is_rejected() {
    let token = seal_token(KEY, &sample(GrantType::AccessToken, None, 0), NAME).unwrap();
    assert!(open_token(b"a-different-encryption-key-entirely!", &token).is_err());
}

#[test]
fn tampering_with_ciphertext_is_rejected() {
    let raw = sample(GrantType::AccessToken, None, 0);
    let token = seal_token(KEY, &raw, NAME).unwrap();
    let (header, rest) = token.split_at(TOKEN_HEADER.len());
    let (body_b64, footer) = match rest.split_once('.') {
        Some((b, f)) => (b.to_string(), Some(f.to_string())),
        None => (rest.to_string(), None),
    };
    let mut body = URL_SAFE_NO_PAD.decode(&body_b64).unwrap();

    for idx in 0..body.len() {
        let mut tampered = body.clone();
        tampered[idx] ^= 0x01;
        let mut rebuilt = String::from(header);
        rebuilt.push_str(&URL_SAFE_NO_PAD.encode(&tampered));
        if let Some(footer) = &footer {
            rebuilt.push('.');
            rebuilt.push_str(footer);
        }
        assert!(
            open_token(KEY, &rebuilt).is_err(),
            "flipping byte {idx} of the body must invalidate the token"
        );
    }

    // Sanity: the untampered token still opens
    body[0] ^= 0x00;
    assert!(open_token(KEY, &token).is_ok());
}

#[test]
fn tampering_with_clear_text_footer_is_rejected() {
    let raw = sample(GrantType::AccessToken, None, 0);
    let token = seal_token(KEY, &raw, b"victim@example.org").unwrap();
    let (body, _) = token.rsplit_once('.').unwrap();

    // An attacker rewrites the clear-text account name to impersonate another account
    let forged_footer = URL_SAFE_NO_PAD.encode(b"attacker@example.org");
    let forged = format!("{body}.{forged_footer}");
    assert!(
        open_token(KEY, &forged).is_err(),
        "the footer is bound through the associated data and must be authenticated"
    );
}

#[test]
fn swapping_footers_between_tokens_is_rejected() {
    let a = seal_token(
        KEY,
        &sample(GrantType::AccessToken, None, 0),
        b"alice@example.org",
    )
    .unwrap();
    let b = seal_token(
        KEY,
        &sample(GrantType::AccessToken, None, 0),
        b"bob@example.org",
    )
    .unwrap();
    let a_body = a.rsplit_once('.').unwrap().0;
    let b_footer = b.rsplit_once('.').unwrap().1;
    let frankentoken = format!("{a_body}.{b_footer}");
    assert!(open_token(KEY, &frankentoken).is_err());
}

#[test]
fn malformed_input_never_panics_and_is_rejected() {
    let valid = seal_token(KEY, &sample(GrantType::AccessToken, None, 0), NAME).unwrap();
    let cases = [
        String::new(),
        "sw1.".to_string(),
        "sw1.!!!not-base64!!!".to_string(),
        "sw1...".to_string(),
        "wrong-prefix.".to_string(),
        "sw1.AAAA".to_string(),
        "sw1.AAAA.BBBB".to_string(),
        valid.replace("sw1.", "sw2."),
        valid[..valid.len() / 2].to_string(),
        format!("sw1.{}", "A".repeat(10_000)),
        "\u{0}\u{0}\u{0}".to_string(),
    ];
    for case in cases {
        assert!(open_token(KEY, &case).is_err(), "must reject {case:?}");
    }
}

#[test]
fn truncating_the_body_is_rejected() {
    let token = seal_token(KEY, &sample(GrantType::AccessToken, None, 0), NAME).unwrap();
    let (header, rest) = token.split_at(TOKEN_HEADER.len());
    let body_b64 = rest.split_once('.').map(|(b, _)| b).unwrap_or(rest);
    let body = URL_SAFE_NO_PAD.decode(body_b64).unwrap();
    for len in 0..body.len() {
        let mut rebuilt = String::from(header);
        rebuilt.push_str(&URL_SAFE_NO_PAD.encode(&body[..len]));
        assert!(
            open_token(KEY, &rebuilt).is_err(),
            "truncation to {len} must be rejected"
        );
    }
}

#[test]
fn identical_input_produces_distinct_tokens() {
    let raw = sample(GrantType::AccessToken, None, 7);
    let a = seal_token(KEY, &raw, NAME).unwrap();
    let b = seal_token(KEY, &raw, NAME).unwrap();
    assert_ne!(a, b, "a random nonce must make each token unique");
    assert_eq_fields(&open_token(KEY, &a).unwrap(), &open_token(KEY, &b).unwrap());
}

#[test]
fn claims_with_separators_round_trip_exactly() {
    let raw = sample(GrantType::Rsvp, Some("a;b;c;d@e.org;999"), 0);
    let token = seal_token(KEY, &raw, b"owner@example.org").unwrap();
    let opened = open_token(KEY, &token).unwrap();
    assert_eq!(opened.claims.as_deref(), Some("a;b;c;d@e.org;999"));
}
