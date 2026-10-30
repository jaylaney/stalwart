/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

fn encrypt_len(plaintext_len: usize) -> usize {
    let secret = EphemeralSecret::random(&mut OsRng);
    let p256dh = secret.public_key().to_encoded_point(false);

    ece_encrypt(p256dh.as_bytes(), &[0u8; 16], &vec![b'a'; plaintext_len])
        .expect("encryption failed")
        .len()
}

#[test]
fn max_plaintext_stays_within_a_single_record() {
    let header = ECE_AES128GCM_HEADER_LENGTH + ECE_WEBPUSH_PUBLIC_KEY_LENGTH;
    let limit = WEBPUSH_MAX_BODY_SIZE;

    let padded = (ECE_WEBPUSH_MAX_PLAINTEXT_SIZE + ECE_AES128GCM_PAD_SIZE)
        .next_multiple_of(ECE_WEBPUSH_DEFAULT_PADDING_BLOCK_SIZE);

    let len = encrypt_len(ECE_WEBPUSH_MAX_PLAINTEXT_SIZE);
    assert!(
        len <= limit,
        "{len} exceeds the {limit} octet payload limit"
    );
    assert_eq!(
        len,
        header + padded + ECE_TAG_LENGTH,
        "expected exactly one authentication tag, i.e. a single record"
    );

    let over = encrypt_len(ECE_WEBPUSH_MAX_PLAINTEXT_SIZE + 1);
    assert!(
        over > limit,
        "{ECE_WEBPUSH_MAX_PLAINTEXT_SIZE} is not the largest single-record plaintext"
    );
}
