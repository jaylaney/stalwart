/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

const KEY: &[u8] = b"a-test-encryption-key-of-some-length";

fn sample() -> ClientMeta {
    ClientMeta {
        redirect_uris: vec![
            "http://127.0.0.1/cb".to_string(),
            "com.example.app:/oauth".to_string(),
        ],
        scope_mask: scopes_to_mask(&format!("{SCOPE_OFFLINE_ACCESS} {SCOPE_MAIL}")),
        client_name: Some("Example Client".to_string()),
    }
}

#[test]
fn round_trip_preserves_all_fields() {
    for meta in [
        sample(),
        ClientMeta {
            redirect_uris: vec!["http://[::1]/".to_string()],
            scope_mask: 0,
            client_name: None,
        },
        ClientMeta::default(),
    ] {
        let client_id = encode_client_id(KEY, &meta).unwrap();
        assert!(client_id.starts_with(CLIENT_ID_HEADER));
        assert_eq!(decode_client_id(KEY, &client_id), Some(meta));
    }
}

#[test]
fn scope_mask_is_order_independent_and_drops_unknown() {
    assert_eq!(
        scopes_to_mask(&format!("{SCOPE_MAIL} {SCOPE_OFFLINE_ACCESS}")),
        scopes_to_mask(&format!("{SCOPE_OFFLINE_ACCESS} {SCOPE_MAIL}"))
    );
    assert_eq!(
        scopes_to_mask(&format!("{SCOPE_MAIL} custom:unknown")),
        scopes_to_mask(SCOPE_MAIL)
    );
    assert_eq!(scopes_to_mask("totally unknown"), 0);
}

#[test]
fn identical_input_is_deterministic() {
    let meta = sample();
    assert_eq!(
        encode_client_id(KEY, &meta).unwrap(),
        encode_client_id(KEY, &meta).unwrap()
    );
}

#[test]
fn wrong_key_is_rejected() {
    let client_id = encode_client_id(KEY, &sample()).unwrap();
    assert_eq!(
        decode_client_id(b"a-completely-different-key-value!", &client_id),
        None
    );
}

#[test]
fn tampering_is_rejected() {
    let client_id = encode_client_id(KEY, &sample()).unwrap();
    let (header, body_b64) = client_id.split_at(CLIENT_ID_HEADER.len());
    let mut body = general_purpose::URL_SAFE_NO_PAD.decode(body_b64).unwrap();
    for idx in 0..body.len() {
        let mut tampered = body.clone();
        tampered[idx] ^= 0x01;
        let forged = format!(
            "{header}{}",
            general_purpose::URL_SAFE_NO_PAD.encode(&tampered)
        );
        assert_eq!(decode_client_id(KEY, &forged), None, "byte {idx}");
    }
    body[0] ^= 0x00;
    assert!(decode_client_id(KEY, &client_id).is_some());
}

#[test]
fn malformed_input_never_panics() {
    for case in [
        "",
        "swc1.",
        "swc1.!!!",
        "swc1.AAAA",
        "wrong.AAAA",
        "swc1.AAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    ] {
        assert_eq!(decode_client_id(KEY, case), None, "{case:?}");
    }
}
