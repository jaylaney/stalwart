/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{BlobEncryption, BlobKey, BlobKeyring, BlobOpts};
use rand::{Rng, SeedableRng, rngs::StdRng};
use registry::schema::enums::{CompressionAlgo, SymmetricCipher};
use std::sync::Arc;
use trc::{EventType, StoreEvent};

const HASH: &[u8; 32] = &[1; 32];
const OTHER_HASH: &[u8; 32] = &[2; 32];
// Wire byte from email::message::za::MAIL_FORMAT_V1. The email crate depends on store.
const MAIL_FORMAT_V1: u8 = 1;

fn test_key(id: u32) -> Arc<BlobKey> {
    Arc::new(BlobKey::new(id, SymmetricCipher::Aes256Gcm, &[7; 32]))
}

#[test]
fn v101_markers_decode() {
    let keyring = BlobKeyring::default();
    let payload = b"old blob";
    let mut plain = payload.to_vec();
    plain.push(0x00);
    let mut lz4 = lz4_flex::compress_prepend_size(payload);
    lz4.push(0xa1);

    for encoded in [plain, lz4] {
        assert_eq!(keyring.decode(HASH, encoded).unwrap(), payload);
    }
}

#[test]
fn sealed_payload_round_trips_every_codec() {
    let mut payload = vec![MAIL_FORMAT_V1];
    payload.extend_from_slice(&[0x5a; 1024]);
    let key = test_key(7);
    let keyring = BlobKeyring::new(vec![key.clone()]);

    for (compression, trailer) in [
        (CompressionAlgo::None, 0x00),
        (CompressionAlgo::Lz4, 0xa1),
        (CompressionAlgo::Zstd, 0xa2),
    ] {
        for (encryption, trailer) in [
            (BlobEncryption::None, trailer),
            (BlobEncryption::Key(key.clone()), 0xb0 | (trailer & 0x0f)),
        ] {
            let encoded = BlobOpts::new(compression, encryption)
                .encode(HASH, &payload)
                .unwrap();
            assert_eq!(encoded.last(), Some(&trailer));
            assert_eq!(keyring.decode(HASH, encoded).unwrap(), payload);
        }
    }
}

#[test]
fn encryption_binds_the_storage_key() {
    let key = test_key(7);
    let encoded = BlobOpts::new(CompressionAlgo::None, BlobEncryption::Key(key.clone()))
        .encode(HASH, b"secret")
        .unwrap();
    let error = BlobKeyring::new(vec![key])
        .decode(OTHER_HASH, encoded)
        .unwrap_err();
    assert!(error.matches(EventType::Store(StoreEvent::DecryptError)));
}

#[test]
fn missing_key_fails_closed() {
    let encoded = BlobOpts::new(CompressionAlgo::None, BlobEncryption::Key(test_key(7)))
        .encode(HASH, b"secret")
        .unwrap();
    let error = BlobKeyring::new(vec![test_key(8)])
        .decode(HASH, encoded)
        .unwrap_err();
    assert!(error.matches(EventType::Store(StoreEvent::DecryptError)));
}

#[test]
fn incompressible_payload_stays_plain() {
    let mut payload = [0; 4096];
    StdRng::seed_from_u64(42).fill_bytes(&mut payload);
    for compression in [CompressionAlgo::Lz4, CompressionAlgo::Zstd] {
        let encoded = BlobOpts::compressed(compression)
            .encode(HASH, &payload)
            .unwrap();
        assert_eq!(encoded.last(), Some(&0x00));
        assert_eq!(&encoded[..encoded.len() - 1], &payload);
    }
}
