/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    ALIAS_LEN, K, RECORD_MAX, ZaSearchKey, compaction_allowance, padded_len, reservation,
    thread::{Alias, RevisionRow, ThreadRecord, Tombstone},
};
use super::{
    BlobHeader, BlobKind, FormatError, PadTarget, SegmentId,
    blob::{
        HiddenSection, PublicSection, decode_content, decode_hidden, decode_public, encode_content,
        encode_hidden, encode_public, frame_and_pad, public_len, unframe,
    },
};
use crate::{
    search::{EmailSearchField, IndexDocument},
    write::SearchIndex,
    za_search::content::{ContentRecord, SortKeys},
};
use nlp::language::Language;
use types::blob_hash::BlobHash;

const M: usize = 65_536;
const ODD_M: usize = 123_413;

fn record(document_id: u32, hashes: usize) -> ThreadRecord {
    ThreadRecord {
        document_id,
        thread_id: document_id + 1000,
        revision: 1,
        created: 42,
        deleted: None,
        subject_hash: 123,
        hashes: (0..hashes as u128).collect(),
    }
}

fn section(records: u32, hashes: usize, aliases: u32) -> HiddenSection {
    HiddenSection {
        records: (0..records).map(|i| record(i, hashes)).collect(),
        aliases: (0..aliases)
            .map(|i| Alias {
                source: i + 5000,
                target: i / 2 + 1000,
                revision: 1,
            })
            .collect(),
    }
}

#[test]
fn r20_record_max_and_alias_len_are_the_encoded_maxima() {
    assert_eq!(RECORD_MAX, 304);
    assert_eq!(ALIAS_LEN, 15);
    assert_eq!(reservation(), 529);
    assert_eq!(encode_hidden(&section(1, K, 0)).len(), RECORD_MAX);
    assert_eq!(encode_hidden(&section(1, 0, 0)).len(), RECORD_MAX - 16 * K);
    assert_eq!(encode_hidden(&section(0, 0, 1)).len(), ALIAS_LEN);
}

#[test]
fn r15_padded_len_at_class_boundaries() {
    for (framed, expected) in [
        (100, M),
        (M - 1, M),
        (M, M),
        (M + 1, 2 * M),
        (2 * M, 2 * M),
        (2 * M + 1, 4 * M),
    ] {
        assert_eq!(padded_len(framed, M), expected);
    }

    for payload_len in [M - 5, M - 4] {
        let payload = vec![7; payload_len];
        let padded = frame_and_pad(&payload, PadTarget::Publishing, M).unwrap();
        assert_eq!(padded.len(), M);
        assert_eq!(unframe(&padded).unwrap(), payload);
        assert!(padded[4 + payload_len..].iter().all(|&byte| byte == 0));
    }
    for payload_len in [M - 3, 2 * M] {
        assert_eq!(
            frame_and_pad(&vec![7; payload_len], PadTarget::Publishing, M),
            Err(FormatError::AllowanceExceeded)
        );
    }
}

#[test]
fn r20_g1_maximal_pass_stays_under_m_and_every_pass_pads_to_m() {
    const H: usize = 32_768;
    assert!(H + reservation() + 4 <= M);
    assert!(61 * reservation() <= H);

    let largest = encode_hidden(&section(62, K, 62 * (K as u32 - 1)));
    assert!(largest.len() <= H + reservation());
    assert!(largest.len() + 4 < M);
    for payload in [
        encode_hidden(&section(1, 0, 0)),
        encode_hidden(&section(1, K, 15)),
        largest,
    ] {
        let padded = frame_and_pad(&payload, PadTarget::Publishing, M).unwrap();
        assert_eq!(padded.len(), M);
        assert_eq!(unframe(&padded).unwrap(), payload);
    }
}

#[test]
fn publishing_non_power_of_two_minimum_hides_alias_count() {
    const H: usize = 120 * 1024;
    const C: usize = 232;
    assert_eq!(200 * reservation(), 105_800);
    assert!(200 * reservation() < H);
    assert!(200 < C && C <= H / reservation());
    assert!(H + reservation() + 4 <= ODD_M);

    for (aliases, encoded_len) in [(0, 60_800), (400, 66_800)] {
        let section = section(200, K, aliases);
        let encoded = encode_hidden(&section);
        assert_eq!(encoded.len(), encoded_len);
        let padded = frame_and_pad(&encoded, PadTarget::Publishing, ODD_M).unwrap();
        assert_eq!(padded.len(), ODD_M);
        let unframed = unframe(&padded).unwrap();
        assert_eq!(unframed.len() + 4, encoded_len + 4);
        assert_eq!(decode_hidden(unframed).unwrap(), section);
    }

    let payload = vec![3; ODD_M - 4];
    let padded = frame_and_pad(&payload, PadTarget::Publishing, ODD_M).unwrap();
    assert_eq!(padded.len(), ODD_M);
    assert_eq!(unframe(&padded).unwrap(), payload);
    assert_eq!(
        frame_and_pad(&vec![3; ODD_M - 3], PadTarget::Publishing, ODD_M),
        Err(FormatError::AllowanceExceeded)
    );
}

#[test]
fn i2_compaction_allowance_is_a_function_of_counts() {
    for aliases in [0, 999] {
        let mut section = section(1000, K, aliases);
        for record in &mut section.records[500..] {
            record.deleted = Some(43);
            record.hashes.clear();
        }
        let encoded = encode_hidden(&section);
        for minimum in [M, ODD_M] {
            let padded =
                frame_and_pad(&encoded, PadTarget::Compaction { n_records: 1000 }, minimum)
                    .unwrap();
            assert_eq!(
                padded.len(),
                padded_len(compaction_allowance(1000), minimum)
            );
            assert_eq!(decode_hidden(unframe(&padded).unwrap()).unwrap(), section);
        }
    }

    assert_eq!(compaction_allowance(30_000), 30_000 * 319 + 4);
    assert_eq!(
        padded_len(compaction_allowance(30_000), M),
        16 * 1024 * 1024
    );
    assert_eq!(padded_len(compaction_allowance(0), M), M);
}

#[test]
fn r35_allowance_exceeded_is_an_error() {
    let encoded = encode_hidden(&section(10, K, 11));
    assert!(encoded.len() + 4 > compaction_allowance(10));
    for minimum in [M, ODD_M] {
        assert_eq!(
            frame_and_pad(&encoded, PadTarget::Compaction { n_records: 10 }, minimum),
            Err(FormatError::AllowanceExceeded)
        );
    }
    assert_eq!(
        frame_and_pad(
            &[],
            PadTarget::Compaction {
                n_records: usize::MAX
            },
            M
        ),
        Err(FormatError::AllowanceExceeded)
    );
    let unroundable = (usize::MAX / 2 + 1) / (RECORD_MAX + ALIAS_LEN) + 1;
    assert_eq!(
        frame_and_pad(
            &[],
            PadTarget::Compaction {
                n_records: unroundable
            },
            M
        ),
        Err(FormatError::AllowanceExceeded)
    );
    assert_eq!(
        frame_and_pad(&[], PadTarget::Publishing, u32::MAX as usize),
        Err(FormatError::AllowanceExceeded)
    );
}

#[test]
fn s42_hidden_section_round_trips_records_then_aliases() {
    let mut section = section(2, 2, 1);
    section.records[1].deleted = Some(43);
    section.records[1].hashes.clear();
    let mut encoded = encode_hidden(&section);
    let first_len = u16::from_le_bytes(encoded[..2].try_into().unwrap()) as usize;
    let second_len = u16::from_le_bytes(encoded[first_len..first_len + 2].try_into().unwrap());
    assert_eq!(encoded[2], 1);
    assert_eq!(encoded[first_len + 2], 1);
    assert_eq!(encoded[first_len + second_len as usize + 2], 2);
    assert_eq!(decode_hidden(&encoded), Ok(section));
    assert_eq!(
        decode_hidden(&encoded[..encoded.len() - 1]),
        Err(FormatError::Truncated)
    );
    encoded[2] = 9;
    assert_eq!(decode_hidden(&encoded), Err(FormatError::Kind(9)));
    assert_eq!(unframe(&[5, 0, 0, 0, 3]), Err(FormatError::Truncated));
}

#[test]
fn s42_public_section_is_fixed_width() {
    for t in [0, 1, 1000] {
        for m in [0, 1] {
            let section = PublicSection {
                tombstones: (0..t)
                    .map(|i| Tombstone {
                        document_id: i,
                        deleted: 42 + u64::from(i),
                    })
                    .collect(),
                rows: (0..m)
                    .map(|i| RevisionRow {
                        revision: 7 + i,
                        change_id: 43,
                    })
                    .collect(),
            };
            let mut encoded = encode_public(&section);
            assert_eq!(encoded.len(), 4 + 12 * t as usize + 4 + 12 * m as usize);
            assert_eq!(encoded.len(), public_len(t as usize, m as usize));
            assert_eq!(decode_public(&encoded), Ok(section));
            encoded.push(0);
            assert!(decode_public(&encoded).is_err());
        }
    }
    assert!(decode_public(&u32::MAX.to_le_bytes()).is_err());
}

#[test]
fn g5_content_section_is_length_prefixed_records() {
    let records = [1, 2, 3].map(|i| {
        let mut document = IndexDocument::new(SearchIndex::Email);
        document.index_text(
            EmailSearchField::Body,
            &"example text ".repeat(i),
            Language::English,
        );
        ContentRecord::new(document, i as u32, SortKeys::default(), Some(42), false)
    });
    let encoded_records = records.each_ref().map(ContentRecord::encode);
    let section = encode_content(&encoded_records.each_ref().map(Vec::as_slice));
    assert_eq!(
        section.len(),
        encoded_records.iter().map(Vec::len).sum::<usize>() + 4 + 4 * 3
    );
    assert!(decode_content(&section).unwrap() == records);
    assert!(decode_content(&section[..section.len() - 1]).is_err());
    assert!(decode_content(&u32::MAX.to_le_bytes()).is_err());
}

#[test]
fn s42_blob_header_round_trips_and_rejects_other_versions_and_kinds() {
    let mut header = BlobHeader {
        kind: BlobKind::Thread,
        tier: 2,
        segment: SegmentId([3; 16]),
        predecessor: None,
    };
    assert_eq!(BlobHeader::LEN, 67);
    let absent = header.encode();
    assert_eq!(absent[19..], [0; 48]);
    assert_eq!(BlobHeader::decode(&absent), Ok((header.clone(), &[][..])));

    header.kind = BlobKind::Content;
    header.predecessor = Some((BlobHash([4; 32]), SegmentId([5; 16])));
    let mut present = header.encode().to_vec();
    present.push(42);
    assert_eq!(BlobHeader::decode(&present), Ok((header, &[42][..])));
    assert_eq!(
        BlobHeader::decode(&present[..66]),
        Err(FormatError::Truncated)
    );
    present[0] = 2;
    assert_eq!(BlobHeader::decode(&present), Err(FormatError::Version(2)));
    present[0] = 1;
    present[1] = 3;
    assert_eq!(BlobHeader::decode(&present), Err(FormatError::Kind(3)));
}

#[test]
fn s42_zero_access_search_key_debug_is_silent() {
    assert_eq!(
        format!("{:?}", ZaSearchKey::new([7; 32])),
        "ZaSearchKey(..)"
    );
}
