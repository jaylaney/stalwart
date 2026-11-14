/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    AAD_SEARCH, LossClass, build_content_blob, build_thread_blob, open_section, parse_content_blob,
    parse_thread_blob, seal_section, section_aad, za_search_key,
};
use store::{
    search::IndexDocument,
    write::SearchIndex,
    za_search::{
        BlobHeader, BlobKind, FormatError, K, PadTarget, SectionKind, SegmentId,
        blob::{HiddenSection, PublicSection, SEAL_OVERHEAD, encode_hidden, public_len},
        content::{ContentRecord, SortKeys},
        thread::{Alias, RevisionRow, ThreadRecord, Tombstone},
    },
};
use types::blob_hash::BlobHash;
use vault::keys::Secret;

fn thread_header() -> BlobHeader {
    BlobHeader {
        kind: BlobKind::Thread,
        tier: 0,
        segment: SegmentId([1; 16]),
        predecessor: None,
    }
}

fn hidden(records: u32, aliases: u32) -> HiddenSection {
    HiddenSection {
        records: (0..records)
            .map(|i| ThreadRecord {
                document_id: i,
                thread_id: i + 1000,
                revision: 7,
                created: 42,
                deleted: None,
                subject_hash: 42,
                hashes: (0..K).map(|hash| hash as u128).collect(),
            })
            .collect(),
        aliases: (0..aliases)
            .map(|i| Alias {
                source: i + 5000,
                target: i / 2 + 1000,
                revision: 7,
            })
            .collect(),
    }
}

#[test]
fn s31_a_section_opens_only_under_its_account_header_and_kind() {
    let key = Secret::from_bytes([7; 32]);
    let other_key = Secret::from_bytes([8; 32]);
    let header = thread_header();
    let kind = SectionKind::Hidden;
    let bytes = seal_section(&key, 1, &header, kind, b"hidden");
    assert_eq!(
        &**open_section(&key, 1, &header, kind, &bytes).unwrap(),
        b"hidden"
    );
    assert_eq!(za_search_key(&key).as_bytes(), key.as_bytes());

    let mut expected_aad = vault::keys::aad(AAD_SEARCH, 1);
    assert_eq!(AAD_SEARCH, "search");
    expected_aad.push(b'|');
    expected_aad.extend_from_slice(&header.encode());
    expected_aad.push(1);
    assert_eq!(section_aad(1, &header, kind), expected_aad);

    let different_segment = BlobHeader {
        segment: SegmentId([2; 16]),
        ..header.clone()
    };
    let different_predecessor = BlobHeader {
        predecessor: Some((BlobHash([3; 32]), SegmentId([4; 16]))),
        ..header.clone()
    };
    let different_tier = BlobHeader {
        tier: 2,
        ..header.clone()
    };
    for (key, account, header, kind) in [
        (&key, 2, &header, kind),
        (&key, 1, &different_segment, kind),
        (&key, 1, &different_predecessor, kind),
        (&key, 1, &different_tier, kind),
        (&key, 1, &header, SectionKind::Public),
        (&other_key, 1, &header, kind),
    ] {
        assert_eq!(
            open_section(key, account, header, kind, &bytes),
            Err(LossClass::Aead)
        );
    }
    assert_eq!(
        open_section(&key, 1, &header, kind, &[]),
        Err(LossClass::Aead)
    );
}

#[test]
fn r9_h13_thread_blob_parses_with_and_without_a_hidden_section() {
    const M: usize = 65_536;
    const ODD_M: usize = 123_413;
    let key = Secret::from_bytes([7; 32]);
    let header = thread_header();
    let public = PublicSection {
        tombstones: vec![Tombstone {
            document_id: 3,
            deleted: 55,
        }],
        rows: vec![RevisionRow {
            revision: 7,
            change_id: 56,
        }],
    };
    let fixed_len = BlobHeader::LEN + 4 + public_len(1, 1) + SEAL_OVERHEAD;
    let blob = build_thread_blob(&key, 1, &header, None, &public, M).unwrap();
    assert_eq!(blob.len(), fixed_len);
    assert_eq!(
        parse_thread_blob(&key, 1, &header, &blob),
        Ok((None, public.clone()))
    );

    let mut odd_lengths = Vec::new();
    for (minimum, hidden) in [
        (M, hidden(1, 0)),
        (ODD_M, hidden(200, 0)),
        (ODD_M, hidden(200, 400)),
    ] {
        let blob = build_thread_blob(
            &key,
            1,
            &header,
            Some((&hidden, PadTarget::Publishing)),
            &public,
            minimum,
        )
        .unwrap();
        assert_eq!(blob.len(), fixed_len + minimum + SEAL_OVERHEAD);
        assert_eq!(
            parse_thread_blob(&key, 1, &header, &blob),
            Ok((Some(hidden), public.clone()))
        );
        if minimum == ODD_M {
            odd_lengths.push(blob.len());
        }
    }
    assert_eq!(odd_lengths, [fixed_len + ODD_M + SEAL_OVERHEAD; 2]);

    let over_m = hidden(200, 400);
    assert_eq!(
        build_thread_blob(
            &key,
            1,
            &header,
            Some((&over_m, PadTarget::Publishing)),
            &public,
            M
        ),
        Err(FormatError::AllowanceExceeded)
    );
}

#[test]
fn s42_thread_blob_rejects_another_position() {
    let key = Secret::from_bytes([7; 32]);
    let header = thread_header();
    let blob =
        build_thread_blob(&key, 1, &header, None, &PublicSection::default(), 65_536).unwrap();
    let wrong_header = BlobHeader {
        segment: SegmentId([2; 16]),
        ..header.clone()
    };
    assert_eq!(
        parse_thread_blob(&key, 1, &wrong_header, &blob),
        Err(LossClass::Parse)
    );
    assert_eq!(
        parse_thread_blob(&key, 1, &header, &blob[..BlobHeader::LEN + 3]),
        Err(LossClass::Parse)
    );
    let mut corrupt_blob = blob;
    *corrupt_blob.last_mut().unwrap() ^= 1;
    assert_eq!(
        parse_thread_blob(&key, 1, &header, &corrupt_blob),
        Err(LossClass::Aead)
    );
}

#[test]
fn r11_content_blob_names_its_predecessor() {
    let key = Secret::from_bytes([7; 32]);
    let first = BlobHeader {
        kind: BlobKind::Content,
        ..thread_header()
    };
    let doc_id = 3;
    let record = ContentRecord::new(
        IndexDocument::new(SearchIndex::Email).with_document_id(doc_id),
        doc_id,
        SortKeys::default(),
        Some(42),
        true,
    );
    let first_blob = build_content_blob(&key, 1, &first, &[&record.encode()]);
    let first_hash = BlobHash::generate(&first_blob);
    let first_descriptor = (first_hash.clone(), first.segment);
    let second = BlobHeader {
        segment: SegmentId([2; 16]),
        predecessor: Some(first_descriptor.clone()),
        ..first.clone()
    };
    let second_blob = build_content_blob(&key, 1, &second, &[]);
    let (header, records) =
        parse_content_blob(&key, 1, Some(second.segment), None, &second_blob).unwrap();
    assert_eq!(header.predecessor, Some(first_descriptor.clone()));
    assert!(records.is_empty());

    let (header, records) =
        parse_content_blob(&key, 1, None, Some(&first_descriptor), &first_blob).unwrap();
    assert_eq!(header, first);
    assert!(records == [record]);

    let wrong_segment = (first_hash, second.segment);
    let wrong_hash = (BlobHash([4; 32]), first.segment);
    for expected in [&wrong_segment, &wrong_hash] {
        assert!(matches!(
            parse_content_blob(&key, 1, None, Some(expected), &first_blob),
            Err(LossClass::Parse)
        ));
    }
    assert!(matches!(
        parse_content_blob(&key, 1, Some(first.segment), None, &second_blob),
        Err(LossClass::Parse)
    ));
}

#[test]
fn s31_sealed_bytes_hold_no_plaintext() {
    let key = Secret::from_bytes([7; 32]);
    let header = thread_header();
    let mut hidden = hidden(1, 0);
    let pattern = *b"ZA-SEARCH-CANARY";
    hidden.records[0].subject_hash = u128::from_le_bytes(pattern);
    hidden.records[0].hashes = vec![u128::from_le_bytes(pattern); 4];
    let canary = pattern.repeat(4);
    assert!(
        encode_hidden(&hidden)
            .windows(canary.len())
            .any(|window| window == canary)
    );
    let blob = build_thread_blob(
        &key,
        1,
        &header,
        Some((&hidden, PadTarget::Publishing)),
        &PublicSection::default(),
        65_536,
    )
    .unwrap();
    assert!(!blob.windows(canary.len()).any(|window| window == canary));
}
