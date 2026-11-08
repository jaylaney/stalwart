/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{ContentPart, ContentRecord, LeafKind, ResolveError, SortField, SortKeys};
use crate::{
    search::{
        EmailSearchField, IndexDocument, KeyValueMatch, MAX_PLANNED_TOKENS, MAX_QUERY_LEAVES,
        MAX_QUERY_PLANNED_TOKENS, MAX_STORED_DOCUMENT_SIZE, SearchField, SearchFilter, SearchQuery,
        codec::read_document, term::AccountIndexer,
    },
    write::SearchIndex,
    za_search::FormatError,
};
use ahash::AHashMap;
use nlp::language::Language;
use roaring::RoaringBitmap;
use std::fmt::Write;

fn doc(document_id: u32) -> IndexDocument {
    IndexDocument::new(SearchIndex::Email)
        .with_account_id(1)
        .with_document_id(document_id)
}

fn record(document_id: u32, build: impl FnOnce(&mut IndexDocument)) -> ContentRecord {
    let mut document = doc(document_id);
    build(&mut document);
    ContentRecord::new(
        document,
        document_id,
        SortKeys::default(),
        Some(1_700_000_000),
        false,
    )
}

fn body(text: &str) -> impl FnOnce(&mut IndexDocument) + '_ {
    |document| document.index_text(EmailSearchField::Body, text, Language::English)
}

fn part(records: impl IntoIterator<Item = ContentRecord>) -> ContentPart {
    let mut part = ContentPart::default();
    part.extend(records);
    part
}

fn resolve_one(part: &ContentPart, filter: SearchFilter) -> RoaringBitmap {
    let resolved = part.resolve(&[filter]).unwrap();
    let [SearchFilter::DocumentSet(documents)] = resolved.as_slice() else {
        panic!("expected exactly one resolved document set");
    };
    documents.clone()
}

#[test]
fn s41_record_round_trips_through_its_encoding() {
    let mut document = doc(7);
    body("the quick brown fox")(&mut document);
    document.index_text(EmailSearchField::Subject, "Quick", Language::English);
    document.insert_key_value(EmailSearchField::Headers, "x-custom", "Hello World");
    let record = ContentRecord::new(
        document,
        7,
        SortKeys {
            from: "ALICE".into(),
            to: "BOB".into(),
            subject: "QUICK".into(),
        },
        Some(42),
        true,
    );

    let encoded = record.encode();
    assert_eq!(record.encoded_len(), encoded.len());
    assert_eq!(record.document_id(), 7);
    assert!(ContentRecord::decode(&encoded).unwrap() == record);
    assert!(matches!(
        ContentRecord::decode(&encoded[..encoded.len() - 1]),
        Err(FormatError::Truncated)
    ));

    let long_key = ContentRecord::new(
        doc(8),
        8,
        SortKeys {
            from: format!("{}é!", "a".repeat(254)),
            ..SortKeys::default()
        },
        None,
        false,
    );
    let encoded = long_key.encode();
    assert_eq!(encoded[13], 254); // The wire length cannot split the final character.
    assert!(ContentRecord::decode(&encoded).unwrap() == long_key);
}

#[test]
fn s41_term_bytes_are_upstreams_stored_document() {
    let text = "the quick brown fox";
    let record = record(7, body(text));
    let mut upstream_document = doc(7);
    body(text)(&mut upstream_document);

    let expected = AccountIndexer::default()
        .index_document(upstream_document, 7)
        .encode()
        .into_document();
    assert_eq!(record.terms, expected);
}

#[test]
fn g5_records_are_encoded_independently() {
    let a = |id| {
        let mut document = doc(id);
        body(&"alpha beta gamma delta epsilon ".repeat(256))(&mut document);
        document
    };
    let a_record = ContentRecord::new(a(1), 1, SortKeys::default(), None, false);
    let alone = AccountIndexer::default().index_document(a(1), 1);
    let mut indexer = AccountIndexer::default();
    let before_b = indexer.index_document(a(1), 1);
    let _b = indexer.index_document(a(2), 2);
    let after_b = indexer.index_document(a(1), 1);

    let stored = alone.encode().into_document();
    assert_eq!(before_b.encode().into_document(), stored);
    assert_eq!(after_b.encode().into_document(), stored);
    let decoded = ContentRecord::decode(&a_record.encode()).unwrap();
    assert_eq!(decoded.terms, stored);
    assert!(decoded.terms.starts_with(b"\x28\xb5\x2f\xfd"));

    let mut scratch = Vec::new();
    let plaintext = read_document(&decoded.terms, &mut scratch).unwrap();
    assert!(plaintext.len() >= crate::write::compress::COMPRESS_WATERMARK_DICTIONARY);
    #[cfg(feature = "test_mode")]
    assert_eq!(plaintext, alone.encode().into_raw_bytes());
    #[cfg(not(feature = "test_mode"))]
    assert_eq!(plaintext, read_document(&stored, &mut Vec::new()).unwrap());
}

#[test]
fn s71_standard_text_matches_terms_and_stems() {
    let part = part([record(1, body("the connections dropped"))]);
    let query = |text| {
        resolve_one(
            &part,
            SearchFilter::has_english_text(EmailSearchField::Body, text),
        )
    };
    assert_eq!(query("connection"), RoaringBitmap::from([1]));
    assert_eq!(query("dropping"), RoaringBitmap::from([1]));
    assert!(query("elephant").is_empty());
}

#[test]
fn s71_groups_and_across_tokens() {
    let part = part([record(1, body("alpha beta gamma"))]);
    let query = |text| {
        resolve_one(
            &part,
            SearchFilter::has_english_text(EmailSearchField::Body, text),
        )
    };
    assert_eq!(query("alpha gamma"), RoaringBitmap::from([1]));
    assert!(query("alpha delta").is_empty());
}

#[test]
fn s71_exact_phrase_uses_positions() {
    let part = part([record(1, body("alpha beta gamma"))]);
    let query = |text| {
        resolve_one(
            &part,
            SearchFilter::has_english_text(EmailSearchField::Body, text),
        )
    };
    assert_eq!(query("\"alpha beta\""), RoaringBitmap::from([1]));
    assert!(query("\"beta alpha\"").is_empty());
    assert!(query("\"alpha gamma\"").is_empty());
}

#[test]
fn s71_phrase_on_a_language_none_field_is_co_occurrence() {
    let part = part([record(1, |document| {
        document.index_text(EmailSearchField::From, "alice example", Language::None);
    })]);
    assert_eq!(
        resolve_one(
            &part,
            SearchFilter::has_keyword(EmailSearchField::From, "\"example alice\"")
        ),
        RoaringBitmap::from([1])
    );
}

#[test]
fn s71_prefix_matches_verbatim_short_terms_only() {
    let part = part([
        record(1, |doc| {
            doc.index_text(EmailSearchField::Body, "alphabet", Language::None)
        }),
        record(2, |doc| {
            doc.index_text(
                EmailSearchField::Body,
                "alphabeticalsequenceofterms",
                Language::None,
            )
        }),
        record(3, body("connections")),
    ]);
    let query = |text| {
        resolve_one(
            &part,
            SearchFilter::has_keyword(EmailSearchField::Body, text),
        )
    };
    assert_eq!(query("alpha*"), RoaringBitmap::from([1]));
    assert!(query("alphabetical*").is_empty());
    // "connect*" is indexed as a stem and only the whole word can satisfy a prefix.
    assert!(query("connect**").is_empty());
}

#[test]
fn s71_key_value_equals_contains_exists() {
    let part = part([record(1, |document| {
        document.insert_key_value(EmailSearchField::Headers, "x-custom", "Hello World");
    })]);
    let query = |key: &str, op| {
        resolve_one(
            &part,
            SearchFilter::KeyValue {
                field: EmailSearchField::Headers.into(),
                key: key.into(),
                op,
            },
        )
    };
    assert_eq!(
        query("X-Custom", KeyValueMatch::Equals("hello world".into())),
        RoaringBitmap::from([1])
    );
    assert_eq!(
        query("x-custom", KeyValueMatch::Contains("world".into())),
        RoaringBitmap::from([1])
    );
    assert_eq!(
        query("x-custom", KeyValueMatch::Exists),
        RoaringBitmap::from([1])
    );
    assert!(query("x-other", KeyValueMatch::Exists).is_empty());
}

#[test]
fn s71_integer_leaf_is_unresolvable() {
    let part = ContentPart::default();
    for field in [SearchField::Id, SearchField::AccountId] {
        assert!(matches!(
            part.resolve(&[SearchFilter::integer_eq(field, 10u64)]),
            Err(ResolveError::Unresolvable(LeafKind::Integer))
        ));
    }
}

#[test]
fn s71_resolve_keeps_operators_and_replaces_leaves() {
    let part = part([
        record(1, body("alpha")),
        record(2, |document| {
            body("alpha")(document);
            document.index_text(EmailSearchField::From, "alice", Language::None);
        }),
        record(3, body("beta")),
    ]);
    let filters = [
        SearchFilter::And,
        SearchFilter::has_english_text(EmailSearchField::Body, "alpha"),
        SearchFilter::Not,
        SearchFilter::has_keyword(EmailSearchField::From, "alice"),
        SearchFilter::End,
        SearchFilter::End,
    ];
    let resolved = part.resolve(&filters).unwrap();
    assert!(matches!(
        resolved.as_slice(),
        [
            SearchFilter::And,
            SearchFilter::DocumentSet(_),
            SearchFilter::Not,
            SearchFilter::DocumentSet(_),
            SearchFilter::End,
            SearchFilter::End
        ]
    ));
    assert_eq!(
        SearchQuery::new(SearchIndex::Email)
            .with_filters(resolved)
            .with_mask(RoaringBitmap::from([1, 2, 3]))
            .filter()
            .into_bitmap(),
        RoaringBitmap::from([1])
    );
    let resolved_set = SearchFilter::DocumentSet(RoaringBitmap::from([1, 3]));
    let filters = [SearchFilter::Or, resolved_set, SearchFilter::End];
    assert_eq!(part.resolve(&filters).unwrap(), filters);
}

#[test]
fn s71_too_many_leaves_is_too_large() {
    let part = ContentPart::default();
    let filters =
        vec![SearchFilter::has_english_text(EmailSearchField::Body, "alpha"); MAX_QUERY_LEAVES + 1];
    assert!(matches!(
        part.resolve(&filters),
        Err(ResolveError::TooLarge(_))
    ));
}

#[test]
fn s71_too_many_planned_tokens_is_too_large() {
    let part = ContentPart::default();
    let text = (0..MAX_PLANNED_TOKENS)
        .map(|i| format!("term{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let filter = SearchFilter::has_keyword(EmailSearchField::Body, text);
    let filters = vec![filter; MAX_QUERY_PLANNED_TOKENS / MAX_PLANNED_TOKENS + 1];
    assert!(matches!(
        part.resolve(&filters),
        Err(ResolveError::TooLarge(_))
    ));
}

#[test]
fn s71_ranks_mirror_message_sort_ranks() {
    let mut part = ContentPart::default();
    for (id, from) in [(1, "B"), (2, "a"), (3, "B"), (4, "")] {
        part.insert(ContentRecord::new(
            doc(id),
            id,
            SortKeys {
                from: from.into(),
                ..SortKeys::default()
            },
            None,
            false,
        ));
    }
    let all = RoaringBitmap::from([1, 2, 3, 4]);
    assert_eq!(
        part.sort_ranks(SortField::From, &all),
        AHashMap::from([(4, 1), (1, 2), (3, 2), (2, 3)])
    );

    for (id, suffix) in [(5, "A"), (6, "Z")] {
        part.insert(ContentRecord::new(
            doc(id),
            id,
            SortKeys {
                from: format!("{}{suffix}", "a".repeat(40)),
                ..SortKeys::default()
            },
            None,
            false,
        ));
    }
    assert_eq!(
        part.sort_ranks(SortField::From, &RoaringBitmap::from([1, 5, 6, 99])),
        AHashMap::from([(1, 1), (5, 2), (6, 2)])
    );
}

#[test]
fn s71_sent_at_and_attachment_bit_per_document() {
    let part = part([
        ContentRecord::new(doc(1), 1, SortKeys::default(), Some(100), true),
        ContentRecord::new(doc(2), 2, SortKeys::default(), None, false),
        ContentRecord::new(doc(3), 3, SortKeys::default(), Some(100), false),
        ContentRecord::new(doc(4), 4, SortKeys::default(), Some(-100), false),
    ]);
    assert_eq!(part.sent_at(1), Some(100));
    assert_eq!(part.sent_at(2), None);
    assert!(part.has_attachment(1));
    assert!(!part.has_attachment(2));
    assert!(!part.has_attachment(9));
    assert_eq!(
        part.sort_ranks(SortField::SentAt, &RoaringBitmap::from([1, 2, 3, 4])),
        AHashMap::from([(4, 1), (1, 2), (3, 2), (2, 3)])
    );
}

#[test]
fn s71_remove_drops_postings_and_the_document() {
    let original = record(1, body("alpha beta"));
    let mut part = part([original.clone()]);
    let query = |text| SearchFilter::has_keyword(EmailSearchField::Body, text);
    for text in ["alpha", "alp*", "\"alpha beta\""] {
        assert_eq!(resolve_one(&part, query(text)), RoaringBitmap::from([1]));
    }
    assert_eq!(part.document_ids(), &RoaringBitmap::from([1]));
    assert_eq!(part.record_bytes(1), Some(original.encode().as_slice()));
    assert!(part.remove(1));
    assert!(!part.remove(1));
    assert!(!part.contains(1));
    assert!(part.is_empty());
    assert!(part.document_ids().is_empty());
    assert!(part.record_bytes(1).is_none());
    assert_eq!(part.loaded_bytes(), 0);
    for text in ["alpha", "alp*", "\"alpha beta\""] {
        assert!(resolve_one(&part, query(text)).is_empty());
    }
}

#[test]
fn s71_replacement_keeps_other_documents_and_removes_stale_postings() {
    let mut part = part([record(1, body("alpha beta")), record(2, body("alpha"))]);
    let replacement = record(1, body("gamma"));
    part.insert(replacement.clone());
    let query = |text| {
        resolve_one(
            &part,
            SearchFilter::has_keyword(EmailSearchField::Body, text),
        )
    };
    assert_eq!(query("alpha"), RoaringBitmap::from([2]));
    assert_eq!(query("alp*"), RoaringBitmap::from([2]));
    assert!(query("\"alpha beta\"").is_empty());
    assert_eq!(query("gamma"), RoaringBitmap::from([1]));
    assert_eq!(part.len(), 2);
    assert_eq!(part.document_ids(), &RoaringBitmap::from([1, 2]));
    assert_eq!(part.record_bytes(1), Some(replacement.encode().as_slice()));
}

#[test]
fn s42_document_cap_is_upstreams() {
    let mut text = String::new();
    for i in 0..2_000_000 {
        write!(&mut text, "word{i} ").unwrap();
    }
    let record = record(1, body(&text));
    let mut upstream_document = doc(1);
    body(&text)(&mut upstream_document);

    assert_eq!(
        record.terms,
        AccountIndexer::default()
            .index_document(upstream_document, 1)
            .encode()
            .into_document()
    );
    assert!(record.encoded_len() <= MAX_STORED_DOCUMENT_SIZE + 256);
}
