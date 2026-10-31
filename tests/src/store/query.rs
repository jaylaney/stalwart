/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The search store's query test at 1.0 (upgrade ruling 76). 1.0's Email
//! index keeps text and key-value fields only (no `ReceivedAt`, `Size` or
//! `SentAt`, ruling 64), and the search store sorts through set
//! comparators, not indexed fields. So the artwork dataset's integer columns
//! (year, width, height) and the text range filters on the acquisition year
//! are applied as document sets computed from the dataset, as 1.0's callers
//! pass cache-side predicates (`SearchFilter::DocumentSet`), and every sort
//! is a `SearchComparator::SortedSet` ranking built from the same columns.
//! Integer filtering runs on the Tracing index, the one index with integer
//! fields (`QueueId`, `EventType`). The expected results are 0.16's.

use crate::{store::deflate_test_resource, utils::server::TestServer};
use ahash::AHashSet;
use nlp::language::Language;
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Instant,
};
use store::{
    SearchStore,
    ahash::AHashMap,
    rand::{self, RngExt, distr::Alphanumeric},
    roaring::RoaringBitmap,
    search::{
        EmailSearchField, IndexDocument, KeyValueMatch, SearchComparator, SearchField,
        SearchFilter, SearchQuery, SearchValue, TracingSearchField,
    },
    write::SearchIndex,
};

pub const FIELDS: [&str; 20] = [
    "id",
    "accession_number",
    "artist",
    "artistRole",
    "artistId",
    "title",
    "dateText",
    "medium",
    "creditLine",
    "year",
    "acquisitionYear",
    "dimensions",
    "width",
    "height",
    "depth",
    "units",
    "inscription",
    "thumbnailCopyright",
    "thumbnailUrl",
    "url",
];

/*
 "title", // Subject
 "year".   // document set (0.16: ReceivedAt)
 "width",  // document set (0.16: Size)
 "height", // document set (0.16: SentAt)
 "artist" // Headers
 "artistRole" // Cc
 "medium",  // From
 "creditLine" // Body
 "acquisitionYear" // Bcc
 "accession_number" // To
*/

/// The dataset columns a filter or a sort reads outside the index.
#[derive(Debug, Default, Clone)]
struct Row {
    year: u64,
    width: u64,
    height: u64,
    to: String,
    from: String,
    cc: String,
    bcc: String,
}

const ALL_IDS: &[&str] = &[
    "p11293", "p79426", "p79427", "p79428", "p79429", "p79430", "d05503", "d00399", "d05352",
    "p01764", "t05843", "n02478", "n02479", "n03568", "n03658", "n04327", "n04328", "n04721",
    "n04739", "n05095", "n05096", "n05145", "n05157", "n05158", "n05159", "n05298", "n05303",
    "n06070", "t01181", "t03571", "t05805", "t05806", "t12147", "t12154", "t12155", "ar00039",
    "t12600", "p80203", "t13209", "t13560", "t13561", "t13655", "t13811", "p13352", "p13351",
    "p13350", "p13349", "p13348", "p13347", "p13346", "p13345", "p13344", "p13342", "p13341",
    "p13340", "p13339", "p13338", "p13337", "p13336", "p13335", "p13334", "p13333", "p13332",
    "p13331", "p13330", "p13329", "p13328", "p13327", "p13326", "p13325", "p13324", "p13323",
    "t13786", "p13322", "p13321", "p13320", "p13319", "p13318", "p13317", "p13316", "p13315",
    "p13314", "t13588", "t13587", "t13586", "t13585", "t13584", "t13540", "t13444", "ar01154",
    "ar01153", "t03681", "t12601", "ar00166", "t12625", "t12915", "p04182", "t06483", "ar00703",
    "t07671", "ar00021", "t05557", "t07918", "p06298", "p05465", "p06640", "t12855", "t01355",
    "t12800", "t12557", "t02078", "ar00052", "ar00627", "t00352", "t07275", "t12318", "t04931",
    "t13683", "t13686", "t13687", "t13688", "t13689", "t13690", "t13691", "t13769", "t13773",
    "t07151", "t13684", "t07523", "t12369", "t12567", "ar00627", "ar00052", "t00352", "t07275",
    "t12318", "t04931", "t13683", "t13686", "t13687", "t13688", "t13689", "t13690", "t13691",
    "t07766", "t07918", "t12993", "ar00044", "t13326", "t07614", "t12414",
];

#[allow(clippy::mutex_atomic)]
pub async fn test(test: &TestServer) {
    let store = test.server.search_store().clone();
    println!("Running Store query tests...");

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .unwrap();
    let now = Instant::now();
    let documents = Arc::new(Mutex::new(Vec::new()));
    let mut mask = RoaringBitmap::new();
    let mut fields = AHashMap::new();
    let mut rows = AHashMap::new();

    // Global ids test
    println!("Running global id filtering tests...");
    test_global(store.clone()).await;

    // Large document insert test
    println!("Running large document insert tests...");
    let mut large_text = String::with_capacity(20 * 1024 * 1024);
    while large_text.len() < 20 * 1024 * 1024 {
        let word = rand::rng()
            .sample_iter(&Alphanumeric)
            .take(rand::rng().random_range(3..10))
            .map(char::from)
            .collect::<String>();
        large_text.push_str(&word);
        large_text.push(' ');
    }
    let mut document = IndexDocument::new(SearchIndex::Email)
        .with_account_id(1)
        .with_document_id(1);
    for field in [
        EmailSearchField::From,
        EmailSearchField::To,
        EmailSearchField::Cc,
        EmailSearchField::Bcc,
        EmailSearchField::Subject,
    ] {
        document.index_text(field, &large_text[..10 * 1024], Language::English);
    }
    for field in [EmailSearchField::Body, EmailSearchField::Attachment] {
        document.index_text(field, &large_text, Language::English);
    }
    index_documents(&store, SearchIndex::Email, 1, vec![document]).await;
    // Refresh
    if let SearchStore::ElasticSearch(store) = &store {
        store.refresh_index(SearchIndex::Email).await.unwrap();
    }

    println!("Running account filtering tests...");
    let filter_ids = std::env::var("QUICK_TEST").is_ok().then(|| {
        let mut ids = AHashSet::new();
        for &id in ALL_IDS {
            ids.insert(id.to_string());
            let id = id.as_bytes();
            if id.last().unwrap() > &b'0' {
                let mut alt_id = id.to_vec();
                *alt_id.last_mut().unwrap() -= 1;
                ids.insert(String::from_utf8(alt_id).unwrap());
            }
            if id.last().unwrap() < &b'9' {
                let mut alt_id = id.to_vec();
                *alt_id.last_mut().unwrap() += 1;
                ids.insert(String::from_utf8(alt_id).unwrap());
            }
        }

        ids
    });

    pool.scope_fifo(|s| {
        for (document_id, record) in csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(&deflate_test_resource("artwork_data.csv.gz")[..])
            .records()
            .enumerate()
        {
            let record = record.unwrap();
            let documents = documents.clone();

            if let Some(filter_ids) = &filter_ids {
                let id = record.get(1).unwrap().to_lowercase();
                if !filter_ids.contains(&id) {
                    continue;
                }
            }

            s.spawn_fifo(move |_| {
                let mut document = IndexDocument::new(SearchIndex::Email)
                    .with_account_id(0)
                    .with_document_id(document_id as u32);
                let mut row = Row::default();
                for (pos, field) in record.iter().enumerate() {
                    match FIELDS[pos] {
                        "accession_number" | "medium" | "artistRole" | "acquisitionYear" => {
                            let value = field.to_lowercase();
                            let search_field = match FIELDS[pos] {
                                "accession_number" => {
                                    row.to = value.clone();
                                    EmailSearchField::To
                                }
                                "medium" => {
                                    row.from = value.clone();
                                    EmailSearchField::From
                                }
                                "artistRole" => {
                                    row.cc = value.clone();
                                    EmailSearchField::Cc
                                }
                                _ => {
                                    row.bcc = value.clone();
                                    EmailSearchField::Bcc
                                }
                            };
                            document.index_text(search_field, &value, Language::None);
                        }
                        "title" | "creditLine" => {
                            document.index_text(
                                if FIELDS[pos] == "title" {
                                    EmailSearchField::Subject
                                } else {
                                    EmailSearchField::Body
                                },
                                &field
                                    .replace(|ch: char| !ch.is_alphanumeric(), " ")
                                    .to_lowercase(),
                                Language::English,
                            );
                        }
                        "artist" => {
                            document.insert_key_value(
                                EmailSearchField::Headers,
                                "artist",
                                field.to_lowercase(),
                            );
                        }
                        "year" => {
                            row.year = field.parse::<u64>().unwrap_or(0);
                        }
                        "width" => {
                            row.width = field.parse::<u64>().unwrap_or(0);
                        }
                        "height" => {
                            row.height = field.parse::<u64>().unwrap_or(0);
                        }
                        _ => {
                            continue;
                        }
                    };
                }

                documents
                    .lock()
                    .unwrap()
                    .push((document_id as u32, document, row));
            });
        }
    });

    println!(
        "Parsed {} entries in {} ms.",
        documents.lock().unwrap().len(),
        now.elapsed().as_millis()
    );

    let now = Instant::now();
    let batches = documents.lock().unwrap().drain(..).collect::<Vec<_>>();

    print!("Inserting... ",);
    let mut chunks = Vec::new();
    let mut chunk = Vec::new();
    for (document_id, document, row) in batches {
        let to_field = document
            .fields()
            .find_map(|(key, value)| match (key, value) {
                (SearchField::Email(EmailSearchField::To), SearchValue::Text { value, .. }) => {
                    Some(value.to_string())
                }
                _ => None,
            })
            .unwrap();
        mask.insert(document_id);
        fields.insert(document_id, to_field);
        rows.insert(document_id, row);

        chunk.push(document);
        if chunk.len() == 10 {
            chunks.push(chunk);
            chunk = Vec::new();
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }

    if test.is_reset() {
        let mut tasks = Vec::new();
        for chunk in chunks {
            let chunk_instance = Instant::now();
            tasks.push({
                let db = store.clone();
                tokio::spawn(
                    async move { index_documents(&db, SearchIndex::Email, 0, chunk).await },
                )
            });

            if tasks.len() == 100 {
                for handle in tasks {
                    handle.await.unwrap();
                }
                print!(" [{} ms]", chunk_instance.elapsed().as_millis());
                std::io::stdout().flush().unwrap();
                tasks = Vec::new();
            }
        }

        if !tasks.is_empty() {
            for handle in tasks {
                handle.await.unwrap();
            }
        }

        // Refresh
        if let SearchStore::ElasticSearch(store) = &store {
            store.refresh_index(SearchIndex::Email).await.unwrap();
        }

        println!("\nInsert took {} ms.", now.elapsed().as_millis());
    }

    if store.internal_fts().is_none() {
        let ids = store
            .query_account(
                SearchQuery::new(SearchIndex::Email)
                    .with_filters(vec![SearchFilter::integer_eq(SearchField::AccountId, 0u32)])
                    .with_comparator(SearchComparator::sorted_set(
                        rank(&rows, |row| row.year),
                        true,
                    ))
                    .with_mask(mask.clone()),
            )
            .await
            .unwrap()
            .into_iter()
            .collect::<RoaringBitmap>();
        assert_eq!(ids, mask);
        let ids = store
            .query_account(
                SearchQuery::new(SearchIndex::Email)
                    .with_filters(vec![
                        SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                        SearchFilter::integer_lt(SearchField::DocumentId, u32::MAX),
                    ])
                    .with_mask(mask.clone()),
            )
            .await
            .unwrap()
            .into_iter()
            .collect::<RoaringBitmap>();
        assert_eq!(ids, mask);
    }

    println!("Running account filter tests...");
    let now = Instant::now();
    test_filter(store.clone(), &fields, &rows, &mask).await;
    println!("Filtering took {} ms.", now.elapsed().as_millis());

    println!("Running account sort tests...");
    let now = Instant::now();
    test_sort(store.clone(), &fields, &rows, &mask).await;
    println!("Sorting took {} ms.", now.elapsed().as_millis());

    println!("Running unindex tests...");
    let now = Instant::now();
    test_unindex(store.clone(), &fields, &rows).await;
    println!("Unindexing took {} ms.", now.elapsed().as_millis());
}

/// Indexes `documents` of `account_id`: through the index worker's pass on
/// the internal search store (1.0's `SearchStore::index` serves external
/// stores only), through `SearchStore::index` elsewhere.
async fn index_documents(
    store: &SearchStore,
    index: SearchIndex,
    account_id: u32,
    documents: Vec<IndexDocument>,
) {
    if let Some(internal) = store.internal_fts() {
        internal
            .index_account_documents(account_id, index, documents, vec![])
            .await
            .unwrap();
    } else {
        store.index(documents).await.unwrap();
    }
}

/// The documents whose dataset row satisfies `predicate`, as a filter.
fn set(rows: &AHashMap<u32, Row>, predicate: impl Fn(&Row) -> bool) -> SearchFilter {
    SearchFilter::is_in_set(
        rows.iter()
            .filter(|(_, row)| predicate(row))
            .map(|(document_id, _)| *document_id)
            .collect(),
    )
}

/// Each document's rank by `key`, for a sorted-set comparator.
fn rank<K: Ord + Clone>(rows: &AHashMap<u32, Row>, key: impl Fn(&Row) -> K) -> AHashMap<u32, u32> {
    let mut keys = rows.values().map(&key).collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    rows.iter()
        .map(|(document_id, row)| (*document_id, keys.binary_search(&key(row)).unwrap() as u32))
        .collect()
}

fn artist_contains(value: &str) -> SearchFilter {
    SearchFilter::KeyValue {
        field: EmailSearchField::Headers.into(),
        key: "artist".to_string(),
        op: KeyValueMatch::Contains(value.to_string()),
    }
}

async fn test_filter(
    store: SearchStore,
    fields: &AHashMap<u32, String>,
    rows: &AHashMap<u32, Row>,
    mask: &RoaringBitmap,
) {
    let can_stem = !store.is_mysql();
    let can_negate_text = !store.is_meilisearch();

    let tests = [
        (
            vec![
                SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                SearchFilter::has_english_text(EmailSearchField::Subject, "water"),
                set(rows, |row| row.year == 1979),
            ],
            vec!["p11293"],
        ),
        (
            vec![
                SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                SearchFilter::has_keyword(EmailSearchField::From, "gelatin"),
                set(rows, |row| row.year > 2000),
                set(rows, |row| row.width < 180),
                set(rows, |row| row.width > 0),
            ],
            vec!["p79426", "p79427", "p79428", "p79429", "p79430"],
        ),
        (
            vec![
                SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                SearchFilter::has_english_text(EmailSearchField::Subject, "'rustic bridge'"),
            ],
            vec!["d05503"],
        ),
        (
            vec![
                SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                SearchFilter::has_english_text(EmailSearchField::Subject, "'rustic'"),
                SearchFilter::has_english_text(
                    EmailSearchField::Subject,
                    if can_stem { "study" } else { "studies" },
                ),
            ],
            vec!["d00399", "d05352"],
        ),
        (
            vec![
                SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                artist_contains("kunst, mauro"),
                SearchFilter::has_keyword(EmailSearchField::Cc, "artist"),
                SearchFilter::Or,
                set(rows, |row| row.year == 1969),
                set(rows, |row| row.year == 1971),
                SearchFilter::End,
            ],
            vec!["p01764", "t05843"],
        ),
        (
            vec![
                SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                SearchFilter::Not,
                SearchFilter::has_keyword(EmailSearchField::From, "oil"),
                SearchFilter::End,
                SearchFilter::has_english_text(
                    EmailSearchField::Body,
                    if can_stem { "bequeath" } else { "bequeathed" },
                ),
                SearchFilter::Or,
                SearchFilter::And,
                set(rows, |row| row.year >= 1900),
                set(rows, |row| row.year < 1910),
                SearchFilter::End,
                SearchFilter::And,
                set(rows, |row| row.year >= 2000),
                set(rows, |row| row.year < 2010),
                SearchFilter::End,
                SearchFilter::End,
            ],
            vec![
                "n02478", "n02479", "n03568", "n03658", "n04327", "n04328", "n04721", "n04739",
                "n05095", "n05096", "n05145", "n05157", "n05158", "n05159", "n05298", "n05303",
                "n06070", "t01181", "t03571", "t05805", "t05806", "t12147", "t12154", "t12155",
            ],
        ),
        (
            vec![
                SearchFilter::And,
                SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                artist_contains("warhol"),
                SearchFilter::Not,
                SearchFilter::has_english_text(EmailSearchField::Subject, "'campbell'"),
                SearchFilter::End,
                SearchFilter::Not,
                SearchFilter::Or,
                set(rows, |row| row.year > 1980),
                SearchFilter::And,
                set(rows, |row| row.width > 500),
                set(rows, |row| row.height > 500),
                SearchFilter::End,
                SearchFilter::End,
                SearchFilter::End,
                SearchFilter::text_eq(EmailSearchField::Bcc, "2008"),
                SearchFilter::End,
            ],
            vec!["ar00039", "t12600"],
        ),
        (
            if can_stem {
                vec![
                    SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                    SearchFilter::has_english_text(EmailSearchField::Subject, "study"),
                    SearchFilter::has_keyword(EmailSearchField::From, "paper"),
                    SearchFilter::has_english_text(EmailSearchField::Body, "'purchased'"),
                    SearchFilter::Not,
                    SearchFilter::Or,
                    SearchFilter::has_english_text(EmailSearchField::Subject, "'anatomical'"),
                    SearchFilter::has_english_text(EmailSearchField::Subject, "'discarded'"),
                    SearchFilter::has_english_text(EmailSearchField::Subject, "'untitled'"),
                    SearchFilter::has_english_text(EmailSearchField::Subject, "'girl'"),
                    SearchFilter::End,
                    SearchFilter::End,
                    set(rows, |row| row.year > 1900),
                    set(rows, |row| row.bcc.as_str() > "2008"),
                ]
            } else {
                vec![
                    SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                    SearchFilter::Or,
                    SearchFilter::has_english_text(EmailSearchField::Subject, "study"),
                    SearchFilter::has_english_text(EmailSearchField::Subject, "studies"),
                    SearchFilter::End,
                    SearchFilter::has_keyword(EmailSearchField::From, "paper"),
                    SearchFilter::has_english_text(EmailSearchField::Body, "'purchased'"),
                    SearchFilter::Not,
                    SearchFilter::Or,
                    SearchFilter::has_english_text(EmailSearchField::Subject, "'anatomical'"),
                    SearchFilter::has_english_text(EmailSearchField::Subject, "'discarded'"),
                    SearchFilter::has_english_text(EmailSearchField::Subject, "'untitled'"),
                    SearchFilter::has_english_text(EmailSearchField::Subject, "'girl'"),
                    SearchFilter::End,
                    SearchFilter::End,
                    set(rows, |row| row.year > 1900),
                    set(rows, |row| row.bcc.as_str() > "2008"),
                ]
            },
            vec!["p80203", "t13209", "t13560", "t13561"],
        ),
    ];

    for (filters, expected_results) in tests {
        if !can_negate_text && has_negated_text(&filters) {
            continue;
        }

        //println!("Running test: {:?}", filter);
        let ids = store
            .query_account(
                SearchQuery::new(SearchIndex::Email)
                    .with_filters(filters)
                    .with_comparator(SearchComparator::sorted_set(
                        rank(rows, |row| row.to.clone()),
                        true,
                    ))
                    .with_mask(mask.clone()),
            )
            .await
            .unwrap();

        let mut results = Vec::new();
        for document_id in ids {
            results.push(fields.get(&document_id).unwrap());
        }
        assert_eq!(results, expected_results);
    }
}

fn has_negated_text(filters: &[SearchFilter]) -> bool {
    let mut stack = Vec::new();
    let mut negated = 0;

    for filter in filters {
        match filter {
            SearchFilter::Not => {
                stack.push(true);
                negated += 1;
            }
            SearchFilter::And | SearchFilter::Or => {
                stack.push(false);
            }
            SearchFilter::End => {
                if stack.pop().unwrap_or(false) {
                    negated -= 1;
                }
            }
            SearchFilter::Text { .. } if negated > 0 => {
                return true;
            }
            _ => (),
        }
    }

    false
}

async fn test_sort(
    store: SearchStore,
    fields: &AHashMap<u32, String>,
    rows: &AHashMap<u32, Row>,
    mask: &RoaringBitmap,
) {
    // 1.0 sorts every search store's results locally (`QueryResults::into_sorted`),
    // so 0.16's PostgreSQL-specific tie order no longer applies.
    let tests = [
        (
            vec![
                SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                set(rows, |row| row.year > 0),
                set(rows, |row| row.bcc.as_str() > "0000"),
                set(rows, |row| row.width > 0),
            ],
            vec![
                SearchComparator::sorted_set(rank(rows, |row| row.year), false),
                SearchComparator::sorted_set(rank(rows, |row| row.bcc.clone()), true),
                SearchComparator::sorted_set(rank(rows, |row| row.width), true),
                SearchComparator::sorted_set(rank(rows, |row| row.to.clone()), false),
            ],
            vec![
                "t13655", "t13811", "p13352", "p13351", "p13350", "p13349", "p13348", "p13347",
                "p13346", "p13345", "p13344", "p13342", "p13341", "p13340", "p13339", "p13338",
                "p13337", "p13336", "p13335", "p13334", "p13333", "p13332", "p13331", "p13330",
                "p13329", "p13328", "p13327", "p13326", "p13325", "p13324", "p13323", "t13786",
                "p13322", "p13321", "p13320", "p13319", "p13318", "p13317", "p13316", "p13315",
                "p13314", "t13588", "t13587", "t13586", "t13585", "t13584", "t13540", "t13444",
                "ar01154", "ar01153",
            ],
        ),
        (
            vec![
                SearchFilter::integer_eq(SearchField::AccountId, 0u32),
                set(rows, |row| row.width > 0),
                set(rows, |row| row.height > 0),
            ],
            vec![
                SearchComparator::sorted_set(rank(rows, |row| row.width), false),
                SearchComparator::sorted_set(rank(rows, |row| row.height), true),
            ],
            vec![
                "t03681", "t12601", "ar00166", "t12625", "t12915", "p04182", "t06483", "ar00703",
                "t07671", "ar00021", "t05557", "t07918", "p06298", "p05465", "p06640", "t12855",
                "t01355", "t12800", "t12557", "t02078",
            ],
        ),
        (
            vec![SearchFilter::integer_eq(SearchField::AccountId, 0u32)],
            vec![
                SearchComparator::sorted_set(rank(rows, |row| row.from.clone()), false),
                SearchComparator::sorted_set(rank(rows, |row| row.cc.clone()), false),
                SearchComparator::sorted_set(rank(rows, |row| row.to.clone()), true),
            ],
            vec![
                "ar00627", "ar00052", "t00352", "t07275", "t12318", "t04931", "t13683", "t13686",
                "t13687", "t13688", "t13689", "t13690", "t13691", "t07766", "t07918", "t12993",
                "ar00044", "t13326", "t07614", "t12414",
            ],
        ),
    ];

    for (filters, comparators, expected_results) in tests {
        //println!("Running test: {:?}", sort);
        let ids = store
            .query_account(
                SearchQuery::new(SearchIndex::Email)
                    .with_filters(filters)
                    .with_comparators(comparators)
                    .with_mask(mask.clone()),
            )
            .await
            .unwrap();

        let mut results = Vec::new();
        for document_id in ids.into_iter().take(expected_results.len()) {
            results.push(fields.get(&document_id).unwrap());
        }
        assert_eq!(results, expected_results);
    }
}

async fn test_unindex(
    store: SearchStore,
    fields: &AHashMap<u32, String>,
    rows: &AHashMap<u32, Row>,
) {
    let filters = || {
        vec![
            SearchFilter::has_keyword(EmailSearchField::From, "gelatin"),
            set(rows, |row| row.year > 2000),
            set(rows, |row| row.width < 180),
            set(rows, |row| row.width > 0),
        ]
    };
    let ids = store
        .query_account(
            SearchQuery::new(SearchIndex::Email)
                .with_mask(RoaringBitmap::from_iter(fields.keys().copied()))
                .with_filters(filters())
                .with_account_id(0),
        )
        .await
        .unwrap();
    assert!(!ids.is_empty());
    let expected_count = ids.len().saturating_sub(10);
    let unindexed = ids.into_iter().take(10).collect::<Vec<_>>();

    if let Some(internal) = store.internal_fts() {
        // The index worker's deletion: each document and every term it lists.
        internal
            .index_account_documents(0, SearchIndex::Email, vec![], unindexed)
            .await
            .unwrap();
    } else {
        let mut query = SearchQuery::new(SearchIndex::Email)
            .with_account_id(0)
            .with_filter(SearchFilter::Or);
        for id in unindexed {
            query = query.with_filter(SearchFilter::integer_eq(SearchField::DocumentId, id));
        }
        query = query.with_filter(SearchFilter::End);

        store.unindex(query).await.unwrap();
    }

    // Refresh
    if let SearchStore::ElasticSearch(store) = &store {
        store.refresh_index(SearchIndex::Email).await.unwrap();
    }

    assert_eq!(
        store
            .query_account(
                SearchQuery::new(SearchIndex::Email)
                    .with_filters(filters())
                    .with_account_id(0)
                    .with_mask(RoaringBitmap::from_iter(fields.keys().copied())),
            )
            .await
            .unwrap()
            .len(),
        expected_count
    );
}

async fn test_global(store: SearchStore) {
    // 1.0's internal store deletes traces by id bucket (`unindex_traces`,
    // buckets of 2^48 ids), so the six documents sit in three buckets and
    // the deletion removes the first two, which is what 0.16's
    // "id below 3" deletion removed from its ids 0..=5.
    let bucket = |bucket: u64, n: u64| (bucket << 48) | n;
    let ids = [
        bucket(0, 0),
        bucket(0, 1),
        bucket(1, 0),
        bucket(1, 1),
        bucket(2, 0),
        bucket(2, 1),
    ];

    // Insert global ids
    let mut documents = Vec::new();
    for (id, queue_id, etyp, keywords) in [
        (ids[0], 1000u64, 1u64, "init start"),
        (ids[1], 1000u64, 2u64, "init complete"),
        (ids[2], 1001u64, 1u64, "process start"),
        (ids[3], 1001u64, 2u64, "process complete"),
        (ids[4], 1002u64, 1u64, "cleanup start"),
        (ids[5], 1002u64, 2u64, "cleanup complete"),
    ] {
        let mut document = IndexDocument::new(SearchIndex::Tracing).with_id(id);
        document.index_unsigned(TracingSearchField::QueueId, queue_id);
        document.index_unsigned(TracingSearchField::EventType, etyp);
        document.index_text(TracingSearchField::Keywords, keywords, Language::None);
        documents.push(document);
    }
    for document in documents {
        if let Some(internal) = store.internal_fts() {
            internal.index_trace(vec![document]).await.unwrap();
        } else {
            store.index(vec![document]).await.unwrap();
        }
    }

    // Refresh
    if let SearchStore::ElasticSearch(store) = &store {
        store.refresh_index(SearchIndex::Tracing).await.unwrap();
    }

    let query_global = |filters: Vec<SearchFilter>| {
        let store = store.clone();
        async move {
            store
                .query_global(SearchQuery::new(SearchIndex::Tracing).with_filters(filters))
                .await
                .unwrap()
                .into_iter()
                .collect::<AHashSet<_>>()
        }
    };

    // Query all
    assert_eq!(
        query_global(vec![SearchFilter::integer_lt(SearchField::Id, u64::MAX)]).await,
        AHashSet::from_iter(ids)
    );

    // Query with filter
    assert_eq!(
        query_global(vec![
            SearchFilter::integer_gt(SearchField::Id, ids[1]),
            SearchFilter::integer_lt(SearchField::Id, ids[5]),
            SearchFilter::has_keyword(TracingSearchField::Keywords, "start"),
        ])
        .await,
        AHashSet::from_iter([ids[2], ids[4]])
    );

    // Integer fields (1.0's Email index has none; the Tracing index keeps
    // `EventType` and `QueueId`)
    assert_eq!(
        query_global(vec![SearchFilter::integer_eq(
            TracingSearchField::EventType,
            1u64
        )])
        .await,
        AHashSet::from_iter([ids[0], ids[2], ids[4]])
    );
    assert_eq!(
        query_global(vec![
            SearchFilter::integer_gt(TracingSearchField::QueueId, 1000u64),
            SearchFilter::integer_lt(TracingSearchField::QueueId, 1002u64),
        ])
        .await,
        AHashSet::from_iter([ids[2], ids[3]])
    );
    assert_eq!(
        query_global(vec![
            SearchFilter::Or,
            SearchFilter::integer_eq(TracingSearchField::QueueId, 1000u64),
            SearchFilter::integer_eq(TracingSearchField::QueueId, 1002u64),
            SearchFilter::End,
            SearchFilter::integer_eq(TracingSearchField::EventType, 2u64),
        ])
        .await,
        AHashSet::from_iter([ids[1], ids[5]])
    );

    // Delete by id
    if let Some(internal) = store.internal_fts() {
        internal.unindex_traces(ids[4]).await.unwrap();
    } else {
        store
            .unindex(
                SearchQuery::new(SearchIndex::Tracing)
                    .with_filter(SearchFilter::integer_lt(SearchField::Id, ids[4])),
            )
            .await
            .unwrap();
    }

    // Refresh
    if let SearchStore::ElasticSearch(store) = &store {
        store.refresh_index(SearchIndex::Tracing).await.unwrap();
    }

    assert_eq!(
        query_global(vec![SearchFilter::integer_lt(SearchField::Id, u64::MAX)]).await,
        AHashSet::from_iter([ids[4], ids[5]])
    );
}
