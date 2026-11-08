/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    Checkpoint, SegmentId, ZaSearchKey,
    manifest::{
        ContentCursor, ContentHead, ContentTo, FailingDocument, Manifest, ManifestError,
        ManifestFill, ManifestValue, ManifestWrite, RebuildCursor, ReconciliationMarker,
        ThreadBlobEntry, manifest_assertion, manifest_hash,
    },
    thread::RevisionRow,
};
use crate::write::{AssignedIds, SerializeWithIds, assert::AssertValue};
use types::blob_hash::BlobHash;
use xxhash_rust::xxh3::xxh3_64;

fn manifest() -> Manifest {
    Manifest {
        revision: 3,
        newest_row: RevisionRow {
            revision: 3,
            change_id: 150,
        },
        indexed_to: Checkpoint {
            change_id: 149,
            position: 11,
            complete: false,
        },
        epoch: 30,
        thread_blobs: (1..=3)
            .map(|i| ThreadBlobEntry {
                segment: SegmentId([i; 16]),
                hash: BlobHash([i + 10; 32]),
                size: 60_000 + u32::from(i),
            })
            .collect(),
        content: Some(ContentHead {
            hash: BlobHash([42; 32]),
            segment: SegmentId([44; 16]),
            length: 17,
            size: 750_000,
        }),
        content_to: ContentTo {
            checkpoint: Checkpoint {
                change_id: 143,
                position: 12,
                complete: true,
            },
            suspended: true,
            cursor: Some(ContentCursor { next_below: 11 }),
        },
        cursor: Some(RebuildCursor { next: 19, end: 31 }),
        reconciliation: Some(ReconciliationMarker {
            bound: 122,
            gap_next: 13,
        }),
        failing: Some(FailingDocument {
            document_id: 81,
            attempts: 2,
        }),
    }
}

#[test]
fn s42_manifest_round_trips_every_field() {
    let key = ZaSearchKey::new([7; 32]);
    for manifest in [Manifest::default(), manifest()] {
        let version = (manifest.epoch, manifest.revision);
        assert_eq!(manifest.identity_version(), version);
        let value = ManifestValue::Index(manifest);
        assert_eq!(ManifestValue::decode(&value.encode(&key), &key), Ok(value));
    }
}

#[test]
fn h9_r33_tag_is_computed_at_commit_over_the_filled_change_id() {
    let key = ZaSearchKey::new([7; 32]);
    let mut m = manifest();
    let mut write = ManifestWrite::new(
        ManifestValue::Index(m.clone()),
        key.clone(),
        ManifestFill {
            newest_row: true,
            epoch: true,
        },
    );
    let ids = AssignedIds::with_current_change_id(4242);
    let (mut bytes, changed) = write.serialize_with_ids(&ids).unwrap();
    assert_eq!(changed, None);
    assert_eq!(write.size_hint(), bytes.len());
    m.newest_row.change_id = 4242;
    m.epoch = 4242;
    assert_eq!(
        ManifestValue::decode(&bytes, &key),
        Ok(ManifestValue::Index(m.clone()))
    );
    assert_eq!(
        ManifestValue::decode(&bytes, &ZaSearchKey::new([9; 32])),
        Err(ManifestError::Tag)
    );
    assert_eq!(
        write.serialize_with_ids(&ids).unwrap(),
        (bytes.clone(), None)
    );
    bytes[10] ^= 1;
    assert_eq!(ManifestValue::decode(&bytes, &key), Err(ManifestError::Tag));

    let new_ids = AssignedIds::with_current_change_id(4243);
    m.newest_row.change_id = 4243;
    m.epoch = 4243;
    let (new_bytes, _) = write.serialize_with_ids(&new_ids).unwrap();
    assert_eq!(
        ManifestValue::decode(&new_bytes, &key),
        Ok(ManifestValue::Index(m))
    );
}

#[test]
fn h9_a_manifest_without_fill_keeps_its_change_ids() {
    let key = ZaSearchKey::new([7; 32]);
    let value = ManifestValue::Index(manifest());
    let mut write = ManifestWrite::new(value.clone(), key.clone(), ManifestFill::default());
    let (bytes, changed) = write.serialize_with_ids(&AssignedIds::default()).unwrap();
    assert_eq!(changed, None);
    assert_eq!(ManifestValue::decode(&bytes, &key), Ok(value));
}

#[test]
fn f7_newer_format_is_no_map_older_is_loss() {
    let key = ZaSearchKey::new([7; 32]);
    for (version, error) in [
        (2, ManifestError::NewerFormat(2)),
        (0, ManifestError::OlderFormat(0)),
    ] {
        let mut bytes = ManifestValue::Index(manifest()).encode(&key);
        bytes[0] = version;
        assert_eq!(ManifestValue::decode(&bytes, &key), Err(error));
        assert_eq!(ManifestValue::decode_unverified(&bytes), Err(error));
        assert_eq!(ManifestValue::decode_inventory(&bytes), Err(error));
    }
}

#[test]
fn r8_manifest_stays_small_at_segments_entries() {
    let mut m = manifest();
    m.thread_blobs.resize(16, ThreadBlobEntry::default());
    m.content.as_mut().unwrap().length = 10_000;
    let key = ZaSearchKey::new([7; 32]);
    let value = ManifestValue::Index(m);
    let bytes = value.encode(&key);
    assert!(bytes.len() < 2048);
    assert_eq!(ManifestValue::decode(&bytes, &key), Ok(value));

    let mut m = manifest();
    m.thread_blobs.resize(255, ThreadBlobEntry::default());
    let value = ManifestValue::Index(m);
    let bytes = value.encode(&key);
    assert_eq!(bytes[39], 255); // the thread count is one byte in v1
    assert_eq!(ManifestValue::decode(&bytes, &key), Ok(value));
}

#[test]
fn g12_destroying_marker_and_inventory_decode_without_a_key() {
    let m = manifest();
    let inv = m.inventory();
    let key = ZaSearchKey::new([7; 32]);
    let value = ManifestValue::Destroying(inv.clone());
    let bytes = ManifestValue::encode_destroying(&inv);
    assert_eq!(bytes, value.encode(&key));
    assert_eq!(bytes.len(), 2 + 1 + 52 * 3 + 1 + 60);
    assert_eq!(ManifestValue::decode_inventory(&bytes), Ok(inv.clone()));
    assert_eq!(ManifestValue::decode(&bytes, &key), Ok(value));

    let value = ManifestValue::Index(m);
    let mut bytes = value.encode(&key);
    *bytes.last_mut().unwrap() ^= 1;
    assert_eq!(ManifestValue::decode_inventory(&bytes), Ok(inv));
    assert_eq!(ManifestValue::decode_unverified(&bytes), Ok(value));
    assert_eq!(ManifestValue::decode(&bytes, &key), Err(ManifestError::Tag));
}

#[test]
fn r18_manifest_hash_changes_on_any_byte() {
    let key = ZaSearchKey::new([7; 32]);
    let bytes = ManifestValue::Index(manifest()).encode(&key);
    let mut changed = bytes.clone();
    changed[18] ^= 1; // byte in indexed_to
    assert_ne!(manifest_hash(&bytes), manifest_hash(&changed));
    assert_eq!(
        manifest_assertion(Some(&bytes)),
        AssertValue::Hash(xxh3_64(&bytes))
    );
    assert_eq!(manifest_assertion(None), AssertValue::None);
}

#[test]
fn malformed_manifest_is_refused_even_without_verifying_the_tag() {
    let key = ZaSearchKey::new([7; 32]);
    let bytes = ManifestValue::Index(Manifest::default()).encode(&key);
    for end in 0..bytes.len() {
        assert_eq!(
            ManifestValue::decode_unverified(&bytes[..end]),
            Err(ManifestError::Parse)
        );
    }

    let mut non_boolean = bytes;
    non_boolean[30] = 2; // indexed_to.complete
    let (body, tag) = non_boolean.split_last_chunk_mut::<32>().unwrap();
    tag.copy_from_slice(blake3::keyed_hash(key.as_bytes(), body).as_bytes());
    assert_eq!(
        ManifestValue::decode(&non_boolean, &key),
        Err(ManifestError::Parse)
    );
}
