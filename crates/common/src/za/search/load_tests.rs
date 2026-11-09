/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::za::search::seal::{build_content_blob, build_thread_blob};
use ahash::{AHashMap, AHashSet};
use std::{
    pin::pin,
    sync::{Arc, Barrier, Mutex, atomic::Ordering},
    time::Duration,
};
use store::za_search::Checkpoint;
use store::{
    search::{EmailSearchField, IndexDocument},
    write::SearchIndex,
    za_search::{
        PadTarget,
        blob::{HiddenSection, PublicSection},
        content::SortKeys,
        manifest::Inventory,
        thread::{RevisionRow, ThreadRecord, Tombstone},
    },
};
use tokio::{
    sync::{Semaphore, mpsc},
    time::{Instant, timeout_at},
};

const ACCOUNT: u32 = 1;
const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

fn segment(n: u8) -> SegmentId {
    SegmentId([n; 16])
}

fn thread_header(segment: SegmentId) -> BlobHeader {
    BlobHeader {
        kind: BlobKind::Thread,
        tier: 0,
        segment,
        predecessor: None,
    }
}

fn thread_record(document_id: u32, revision: u32, created: u64) -> ThreadRecord {
    ThreadRecord {
        document_id,
        thread_id: 1,
        revision,
        created,
        deleted: None,
        subject_hash: 7,
        hashes: vec![9],
    }
}

fn thread_blob(
    key: &Secret,
    map: &mut AHashMap<BlobHash, Vec<u8>>,
    segment: SegmentId,
    records: Vec<ThreadRecord>,
    public: PublicSection,
) -> ThreadBlobEntry {
    let hidden = HiddenSection {
        records,
        aliases: Vec::new(),
    };
    let bytes = build_thread_blob(
        key,
        ACCOUNT,
        &thread_header(segment),
        (!hidden.records.is_empty()).then_some((&hidden, PadTarget::Publishing)),
        &public,
        4096,
    )
    .unwrap();
    let hash = BlobHash::generate(&bytes);
    let size = bytes.len() as u32;
    map.insert(hash.clone(), bytes);
    ThreadBlobEntry {
        segment,
        hash,
        size,
    }
}

fn document(id: u32) -> IndexDocument {
    IndexDocument::new(SearchIndex::Email)
        .with_account_id(ACCOUNT)
        .with_document_id(id)
}

fn record(id: u32) -> ContentRecord {
    ContentRecord::new(document(id), id, SortKeys::default(), None, false)
}

fn content_blob(
    key: &Secret,
    map: &mut AHashMap<BlobHash, Vec<u8>>,
    segment: SegmentId,
    previous: Option<&ContentHead>,
    ids: &[u32],
) -> ContentHead {
    let header = BlobHeader {
        kind: BlobKind::Content,
        tier: 0,
        segment,
        predecessor: previous.map(|head| (head.hash.clone(), head.segment)),
    };
    let encoded = ids
        .iter()
        .map(|id| record(*id).encode())
        .collect::<Vec<_>>();
    let records = encoded.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let bytes = build_content_blob(key, ACCOUNT, &header, &records);
    let hash = BlobHash::generate(&bytes);
    let size = bytes.len() as u64 + previous.map_or(0, |head| head.size);
    let length = 1 + previous.map_or(0, |head| head.length);
    map.insert(hash.clone(), bytes);
    ContentHead {
        hash,
        segment,
        length,
        size,
    }
}

fn encode(key: &Secret, manifest: &Manifest) -> Vec<u8> {
    ManifestValue::Index(manifest.clone()).encode(&za_search_key(key))
}

type Fixture = (Secret, u32, AHashMap<BlobHash, Vec<u8>>, Vec<u8>, Manifest);

fn fixture() -> Fixture {
    let key = Secret::from_bytes([7; 32]);
    let mut map = AHashMap::new();
    let row1 = RevisionRow {
        revision: 1,
        change_id: 100,
    };
    let row2 = RevisionRow {
        revision: 2,
        change_id: 200,
    };
    let thread1 = thread_blob(
        &key,
        &mut map,
        segment(1),
        (1..=3).map(|id| thread_record(id, 1, 100)).collect(),
        PublicSection::default(),
    );
    let thread2 = thread_blob(
        &key,
        &mut map,
        segment(2),
        vec![thread_record(4, 2, 200)],
        PublicSection {
            tombstones: vec![Tombstone {
                document_id: 2,
                deleted: 150,
            }],
            rows: vec![row1],
        },
    );
    let content1 = content_blob(&key, &mut map, segment(11), None, &[1, 2, 3]);
    let content2 = content_blob(&key, &mut map, segment(12), Some(&content1), &[4]);
    let manifest = Manifest {
        revision: 2,
        newest_row: row2,
        epoch: 42,
        indexed_to: Checkpoint {
            change_id: 180,
            position: 0,
            complete: true,
        },
        thread_blobs: vec![thread1, thread2],
        content: Some(content2),
        ..Manifest::default()
    };
    let bytes = encode(&key, &manifest);
    (key, ACCOUNT, map, bytes, manifest)
}

fn append(key: &Secret, map: &mut AHashMap<BlobHash, Vec<u8>>, old: &Manifest, id: u8) -> Manifest {
    let mut manifest = old.clone();
    manifest.revision += 1;
    manifest.newest_row = RevisionRow {
        revision: manifest.revision,
        change_id: old.newest_row.change_id + 100,
    };
    manifest.thread_blobs.push(thread_blob(
        key,
        map,
        segment(id),
        vec![thread_record(
            id as u32,
            manifest.revision,
            manifest.newest_row.change_id,
        )],
        PublicSection {
            tombstones: vec![],
            rows: vec![old.newest_row],
        },
    ));
    append_content(key, map, &manifest, id)
}

fn append_content(
    key: &Secret,
    map: &mut AHashMap<BlobHash, Vec<u8>>,
    old: &Manifest,
    id: u8,
) -> Manifest {
    let mut manifest = old.clone();
    manifest.content = Some(content_blob(
        key,
        map,
        segment(id + 10),
        old.content.as_ref(),
        &[id as u32],
    ));
    manifest
}

struct CountingSource {
    map: AHashMap<BlobHash, Vec<u8>>,
    reads: Mutex<Vec<BlobHash>>,
    delay_first: bool,
}

fn counting(map: AHashMap<BlobHash, Vec<u8>>) -> CountingSource {
    CountingSource {
        map,
        reads: Mutex::new(Vec::new()),
        delay_first: false,
    }
}

impl CountingSource {
    fn reads(&self) -> Vec<BlobHash> {
        std::mem::take(&mut *self.reads.lock().unwrap())
    }
}

impl BlobSource for CountingSource {
    async fn fetch(&self, hash: &BlobHash) -> trc::Result<Option<Vec<u8>>> {
        let first = {
            let mut reads = self.reads.lock().unwrap();
            reads.push(hash.clone());
            reads.len() == 1
        };
        if self.delay_first && first {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.map.fetch(hash).await
    }
}

impl BlobSource for AHashMap<BlobHash, Vec<u8>> {
    async fn fetch(&self, hash: &BlobHash) -> trc::Result<Option<Vec<u8>>> {
        Ok(self.get(hash).cloned())
    }
}

fn cache() -> ZaSearchCache {
    ZaSearchCache::new(Ok(ZaSearchConfig {
        mem_mb: 1,
        thread_mem_mb: 1,
        account_max_mb: 1,
        ..ZaSearchConfig::default()
    }))
}

async fn loaded(
    source: &impl BlobSource,
    cache: &ZaSearchCache,
    key: &Secret,
    bytes: &[u8],
    manifest: Manifest,
    content: bool,
) -> Arc<IndexEntry> {
    let entry = cache.entry(ACCOUNT);
    load_entry(source, &entry, key, cache.config(), async {
        Ok((bytes.to_vec(), manifest))
    })
    .await
    .unwrap();
    if content {
        load_current_content(source, &entry, key, cache.config())
            .await
            .unwrap();
    }
    entry
}

fn predecessor_of_head(map: &AHashMap<BlobHash, Vec<u8>>, manifest: &Manifest) -> BlobHash {
    let head = manifest.content.as_ref().unwrap();
    BlobHeader::decode(&map[&head.hash])
        .unwrap()
        .0
        .predecessor
        .unwrap()
        .0
}

#[tokio::test]
async fn s43_round_trip_a_populated_index() {
    let (key, _, map, bytes, manifest) = fixture();
    let cache = cache();
    let entry = loaded(&map, &cache, &key, &bytes, manifest.clone(), false).await;
    let content = load_content(&map, &entry, &key, &manifest, None, 10, 100)
        .await
        .unwrap();
    let index = entry.read_loaded().await.unwrap();
    assert_eq!(index.thread.present_members(1), [1, 3, 4]);
    assert_eq!(index.thread.record(2).unwrap().deleted, Some(150));
    assert_eq!(
        index
            .thread
            .rows()
            .iter()
            .map(|r| (r.revision, r.change_id))
            .collect::<Vec<_>>(),
        [(1, 100), (2, 200)]
    );
    assert_eq!(index.scanned_to, 180);
    assert!(content.part.contains(1));
    assert!(content.part.contains(4));
    assert!(!content.part.contains(2));
    assert_eq!(content.segments, [segment(12), segment(11)]);
}

#[test]
fn pl4_partial_checkpoint_does_not_claim_the_record() {
    let checkpoint = Checkpoint {
        change_id: 180,
        position: 1,
        complete: false,
    };
    assert_eq!(scanned_seed(&checkpoint), 179);
}

#[tokio::test]
async fn r9_thread_load_fetches_no_content_blob() {
    let (key, account, map, bytes, manifest) = fixture();
    let expected = manifest
        .thread_blobs
        .iter()
        .map(|blob| blob.hash.clone())
        .collect::<Vec<_>>();
    let source = counting(map);
    load_thread_part(
        &source,
        &cache().entry(account),
        &key,
        &bytes,
        manifest,
        10,
        100,
    )
    .await
    .unwrap();
    assert_eq!(source.reads(), expected);
}

#[tokio::test]
async fn r11_chain_is_walked_from_the_head_descriptor() {
    let (key, account, map, _, mut manifest) = fixture();
    let cache = cache();
    let entry = cache.entry(account);
    let old_head = manifest.content.clone().unwrap();
    let first = predecessor_of_head(&map, &manifest);
    let mut source = counting(map);
    load_content(&source, &entry, &key, &manifest, None, 10, 100)
        .await
        .unwrap();
    assert_eq!(source.reads(), [old_head.hash, first.clone()]);

    let wrong = content_blob(&key, &mut source.map, segment(40), None, &[1, 2, 3]);
    source.map.insert(first, source.map[&wrong.hash].clone());
    assert!(matches!(
        load_content(&source, &entry, &key, &manifest, None, 10, 100).await,
        Err(LoadFailure::Loss(LossClass::Hash))
    ));

    let wrong_descriptor = ContentHead {
        segment: segment(11),
        ..wrong
    };
    manifest.content = Some(content_blob(
        &key,
        &mut source.map,
        segment(12),
        Some(&wrong_descriptor),
        &[4],
    ));
    assert!(matches!(
        load_content(&source, &entry, &key, &manifest, None, 10, 100).await,
        Err(LoadFailure::Loss(LossClass::Parse))
    ));
}

#[tokio::test]
async fn pl1_a_stripped_hidden_section_is_loss() {
    let (key, account, map, manifest_bytes, manifest) = fixture();
    let cache = cache();
    let entry = cache.entry(account);
    load_thread_part(
        &map,
        &entry,
        &key,
        &manifest_bytes,
        manifest.clone(),
        10,
        100,
    )
    .await
    .unwrap();
    load_content(&map, &entry, &key, &manifest, None, 10, 100)
        .await
        .unwrap();

    let mut altered = map;
    let descriptor = &manifest.thread_blobs[0];
    let bytes = &altered[&descriptor.hash];
    let offset = BlobHeader::LEN;
    let length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    let mut stripped = bytes[..offset].to_vec();
    stripped.extend_from_slice(&0u32.to_le_bytes());
    stripped.extend_from_slice(&bytes[offset + 4 + length..]);
    assert!(
        parse_thread_blob(&key, account, &thread_header(descriptor.segment), &stripped)
            .unwrap()
            .0
            .is_none()
    );
    altered.insert(descriptor.hash.clone(), stripped);
    let Err(ObservedLoadError::MapLoss(loss)) = load_thread_part(
        &altered,
        &entry,
        &key,
        &manifest_bytes,
        manifest.clone(),
        10,
        100,
    )
    .await
    else {
        panic!("the failed named thread blob must carry map loss evidence");
    };
    assert_eq!(loss.class(), LossClass::Hash);
    assert_eq!(loss.raw(), Some(manifest_bytes.as_slice()));
    assert_eq!(loss.failed_thread(), Some(&descriptor.hash));

    let first = predecessor_of_head(&altered, &manifest);
    let (header, _) = BlobHeader::decode(&altered[&first]).unwrap();
    let subset = build_content_blob(
        &key,
        account,
        &header,
        &[&record(1).encode(), &record(2).encode()],
    );
    assert_eq!(
        parse_content_blob(&key, account, Some(header.segment), None, &subset)
            .unwrap()
            .1
            .len(),
        2
    );
    altered.insert(first, subset);
    assert!(matches!(
        load_content(&altered, &entry, &key, &manifest, None, 10, 100).await,
        Err(LoadFailure::Loss(LossClass::Hash))
    ));
}

fn assert_same_index(left: &LoadedIndex, right: &LoadedIndex) {
    let mut left_records = left.thread.records().collect::<Vec<_>>();
    let mut right_records = right.thread.records().collect::<Vec<_>>();
    left_records.sort_by_key(|record| record.document_id);
    right_records.sort_by_key(|record| record.document_id);
    assert_eq!(left_records, right_records);
    let mut left_aliases = left.thread.aliases().collect::<Vec<_>>();
    let mut right_aliases = right.thread.aliases().collect::<Vec<_>>();
    left_aliases.sort_by_key(|alias| alias.source);
    right_aliases.sort_by_key(|alias| alias.source);
    assert_eq!(left_aliases, right_aliases);
    assert_eq!(left.thread.rows(), right.thread.rows());
    let left_content = left.content.as_ref().unwrap();
    let right_content = right.content.as_ref().unwrap();
    assert_eq!(left_content.segments, right_content.segments);
    assert_eq!(
        left_content.part.document_ids(),
        right_content.part.document_ids()
    );
    assert_eq!(left.manifest_bytes_hash, right.manifest_bytes_hash);
}

#[tokio::test]
async fn f20_incremental_reload_equals_a_fresh_load() {
    let (key, _, mut map, bytes, v1) = fixture();
    let v2 = append(&key, &mut map, &v1, 5);
    let v2_bytes = encode(&key, &v2);
    let source = counting(map);
    let cache1 = cache();
    let old = loaded(&source, &cache1, &key, &bytes, v1, true).await;
    let new_thread = v2.thread_blobs[2].clone();
    assert_eq!(
        plan_reload(&*old.read_loaded().await.unwrap(), &v2),
        ReloadPlan::Append {
            thread: vec![new_thread.clone()],
            content_head_moved: true
        }
    );
    source.reads();
    apply_reload(
        &source,
        &old,
        &key,
        cache1.config(),
        &v2_bytes,
        v2.clone(),
        100,
    )
    .await
    .unwrap();
    assert_eq!(
        source.reads(),
        [new_thread.hash, v2.content.as_ref().unwrap().hash.clone()]
    );
    let cache2 = cache();
    let fresh = loaded(&source, &cache2, &key, &v2_bytes, v2, true).await;
    assert_same_index(
        &*old.read_loaded().await.unwrap(),
        &*fresh.read_loaded().await.unwrap(),
    );
}

#[tokio::test]
async fn f20_compaction_reloads_whole() {
    let (key, _, mut map, _, v1) = fixture();
    let v2 = append(&key, &mut map, &v1, 5);
    let v2_bytes = encode(&key, &v2);
    let cache1 = cache();
    let old = loaded(&map, &cache1, &key, &v2_bytes, v2.clone(), true).await;
    let mut v3 = v2.clone();
    let guard = old.read_loaded().await.unwrap();
    let retained = guard.thread.records().cloned().collect();
    v3.thread_blobs = vec![thread_blob(
        &key,
        &mut map,
        segment(50),
        retained,
        PublicSection {
            tombstones: vec![],
            rows: guard.thread.rows()[..2].to_vec(),
        },
    )];
    v3.content = Some(content_blob(
        &key,
        &mut map,
        segment(51),
        None,
        &[1, 3, 4, 5],
    ));
    assert_eq!(plan_reload(&guard, &v3), ReloadPlan::Whole);
    drop(guard);

    let v3_bytes = encode(&key, &v3);
    apply_reload(
        &map,
        &old,
        &key,
        cache1.config(),
        &v3_bytes,
        v3.clone(),
        100,
    )
    .await
    .unwrap();
    let cache2 = cache();
    let fresh = loaded(&map, &cache2, &key, &v3_bytes, v3, true).await;
    let guard = old.read_loaded().await.unwrap();
    assert_eq!(guard.thread_segments, [segment(50)]);
    assert_same_index(&guard, &*fresh.read_loaded().await.unwrap());
}

#[tokio::test]
async fn r18_a_deletion_only_change_moves_the_hash_and_reloads() {
    let (key, _, mut map, bytes, mut manifest) = fixture();
    let cache = cache();
    let entry = loaded(&map, &cache, &key, &bytes, manifest.clone(), true).await;
    manifest.thread_blobs.push(thread_blob(
        &key,
        &mut map,
        segment(5),
        vec![],
        PublicSection {
            tombstones: vec![Tombstone {
                document_id: 3,
                deleted: 250,
            }],
            rows: vec![],
        },
    ));
    let moved = encode(&key, &manifest);
    let old = entry.read_loaded().await.unwrap();
    assert_eq!(old.identity_version(), manifest.identity_version());
    assert_ne!(old.manifest_bytes_hash, manifest_hash(&moved));
    assert!(matches!(
        plan_reload(&old, &manifest),
        ReloadPlan::Append { .. }
    ));
    drop(old);

    apply_reload(&map, &entry, &key, cache.config(), &moved, manifest, 100)
        .await
        .unwrap();
    let index = entry.read_loaded().await.unwrap();
    assert!(!index.thread.is_present(3));
    assert!(!index.content.as_ref().unwrap().part.contains(3));
}

#[test]
fn r2_identity_version_distinguishes_epochs() {
    let mut manifest = Manifest {
        revision: 5,
        epoch: 100,
        ..Manifest::default()
    };
    let old = LoadedIndex::empty(manifest.clone());
    assert_eq!(old.identity_version(), (100, 5));
    assert_eq!(
        old.identity_version(),
        LoadedIndex::empty(manifest.clone()).identity_version()
    );
    manifest.epoch = 900;
    assert_ne!(
        old.identity_version(),
        LoadedIndex::empty(manifest).identity_version()
    );
}

#[tokio::test]
async fn f3_h6_error_classes() {
    use trc::{Key, StoreEvent};
    assert_eq!(
        classify_blob_error(&StoreEvent::NotFound.into_err()),
        LoadFailure::Transient(TransientClass::BlobStore)
    );
    assert_eq!(
        classify_blob_error(&StoreEvent::DecryptError.into_err().ctx(Key::Id, 7u64)),
        LoadFailure::Transient(TransientClass::CodecKey)
    );
    assert_eq!(
        classify_blob_error(
            &StoreEvent::DecryptError
                .into_err()
                .ctx(Key::Details, "truncated")
        ),
        LoadFailure::Loss(LossClass::Parse)
    );
    assert_eq!(
        classify_blob_error(&StoreEvent::DecompressError.into_err()),
        LoadFailure::Loss(LossClass::Parse)
    );
    assert_eq!(
        absence_class(100, 105, 10),
        LoadFailure::Transient(TransientClass::AbsentWithinWindow)
    );
    assert_eq!(
        absence_class(100, 110, 10),
        LoadFailure::Loss(LossClass::BlobAbsent)
    );

    let (key, account, mut map, bytes, manifest) = fixture();
    let cache = cache();
    let entry = cache.entry(account);
    let hash = manifest.thread_blobs[0].hash.clone();
    let missing = map.remove(&hash).unwrap();
    assert!(matches!(
        load_thread_part(&map, &entry, &key, &bytes, manifest.clone(), 10, 100)
            .await
            .map_err(|failure| failure.classification()),
        Err(LoadFailure::Transient(TransientClass::AbsentWithinWindow))
    ));
    assert!(matches!(
        load_thread_part(&map, &entry, &key, &bytes, manifest.clone(), 10, 111)
            .await
            .map_err(|failure| failure.classification()),
        Err(LoadFailure::Loss(LossClass::BlobAbsent))
    ));
    map.insert(hash.clone(), missing);
    load_thread_part(&map, &entry, &key, &bytes, manifest, 10, 112)
        .await
        .unwrap();
    assert_eq!(entry.absent_since(&hash, 113), 113);
}

#[test]
fn h11_f7_g12_manifest_outcomes() {
    assert_eq!(
        classify_manifest(Err(ManifestError::NewerFormat(2))),
        Err(LoadFailure::NoMap(NoMapReason::NewerFormat))
    );
    assert_eq!(
        classify_manifest(Err(ManifestError::OlderFormat(0))),
        Err(LoadFailure::Loss(LossClass::OlderFormat))
    );
    assert_eq!(
        classify_manifest(Err(ManifestError::Tag)),
        Err(LoadFailure::Loss(LossClass::Tag))
    );
    assert_eq!(
        classify_manifest(Err(ManifestError::Parse)),
        Err(LoadFailure::Loss(LossClass::Parse))
    );
    let value = ManifestValue::Destroying(Inventory::default());
    assert_eq!(classify_manifest(Ok(value.clone())), Ok(value.clone()));
    assert_eq!(
        entry_outcome(Some(value)),
        Err(LoadFailure::NoMap(NoMapReason::Destroying))
    );
    assert_eq!(
        entry_outcome(None),
        Err(LoadFailure::Loss(LossClass::ManifestMissing))
    );
}

#[cfg(feature = "test_mode")]
#[tokio::test]
async fn f18_budget_counts_the_transient_and_evicts_idle_content_first() {
    let cache = cache();
    cache.set_content_bytes_for_test(1, 300 * KIB);
    let charge = cache.admit(PartKind::Content, 900 * KIB, 2).unwrap();
    assert!(
        cache
            .get(1)
            .unwrap()
            .read_loaded()
            .await
            .unwrap()
            .content
            .is_none()
    );
    assert_eq!(cache.content_used(), 900 * KIB);
    drop(charge);

    cache.set_content_bytes_for_test(1, 300 * KIB);
    let held = cache.get(1).unwrap().read_loaded().await.unwrap();
    assert!(cache.admit(PartKind::Content, 900 * KIB, 3).is_none());
    assert!(held.content.is_some());
}

#[cfg(feature = "test_mode")]
#[tokio::test]
async fn pl11_an_evicted_generation_stays_charged_until_its_last_reader_drops() {
    let cache = cache();
    cache.set_content_bytes_for_test(1, 300 * KIB);
    let first = cache.get(1).unwrap();
    let first_reader = first.read_loaded().await.unwrap();
    cache.remove_account(1, EvictReason::Invalidation);
    assert_eq!(cache.content_used(), 300 * KIB);

    cache.set_content_bytes_for_test(1, 300 * KIB);
    let second = cache.get(1).unwrap();
    let second_reader = second.read_loaded().await.unwrap();
    assert_eq!(cache.content_used(), 600 * KIB);
    assert!(cache.admit(PartKind::Content, 600 * KIB, 2).is_none());
    drop(first);
    drop(first_reader);
    assert_eq!(cache.content_used(), 300 * KIB);
    assert!(cache.admit(PartKind::Content, 600 * KIB, 2).is_some());
    cache.remove_account(1, EvictReason::Invalidation);
    drop(second);
    drop(second_reader);
    assert_eq!(cache.content_used(), 0);
}

// Fill an exact number of accounted bytes, including a new posting key in every batch.
fn add_content_bytes(part: &mut ContentPart, next_id: &mut u32, bytes: u64) {
    let previous = part.loaded_bytes();
    let mut with_posting = document(*next_id);
    with_posting.insert_key_value(
        EmailSearchField::Headers,
        "x-budget-test",
        &next_id.to_string(),
    );
    part.insert(ContentRecord::new(
        with_posting,
        *next_id,
        SortKeys::default(),
        None,
        false,
    ));
    *next_id += 1;

    let base = record(0).encoded_len();
    let mut remaining = bytes as usize - (part.loaded_bytes() - previous);
    while remaining != 0 {
        let mut len = remaining.min(base + 3 * 255);
        if remaining - len < base && remaining != len {
            len = remaining - base;
        }
        let mut padding = len - base;
        let mut key = || {
            let count = padding.min(255);
            padding -= count;
            "x".repeat(count)
        };
        part.insert(ContentRecord::new(
            document(*next_id),
            *next_id,
            SortKeys {
                from: key(),
                to: key(),
                subject: key(),
            },
            None,
            false,
        ));
        *next_id += 1;
        remaining -= len;
    }
    assert_eq!(part.loaded_bytes() - previous, bytes as usize);
}

fn charged_content(
    cache: &ZaSearchCache,
    account: u32,
    bytes: u64,
    next_id: &mut u32,
) -> LoadedIndex {
    let mut index = LoadedIndex::empty(Manifest::default());
    let charge = cache.admit(PartKind::Content, bytes, account).unwrap();
    let mut part = ContentPart::default();
    add_content_bytes(&mut part, next_id, bytes);
    index.content = Some(LoadedContent {
        part,
        segments: vec![],
        bytes,
        charge,
    });
    index
}

#[tokio::test]
async fn pl11_growth_through_local_passes_is_charged() {
    let cache = cache();
    let entry = cache.entry(1);
    let mut id = 1;
    let mut guard = entry.index.write().await;
    let index = guard.insert(charged_content(&cache, 1, 100 * KIB, &mut id));
    for step in 1..=5 {
        add_content_bytes(&mut index.content.as_mut().unwrap().part, &mut id, 40 * KIB);
        assert!(cache.admit_content_growth(index, 1));
        assert_eq!(cache.content_used(), (100 + step * 40) * KIB);
    }
    drop(guard);
    let charge = cache.admit(PartKind::Content, 800 * KIB, 2).unwrap();
    assert!(entry.read_loaded().await.unwrap().content.is_none());
    assert_eq!(cache.content_used(), 800 * KIB);
    drop(charge);
}

#[tokio::test]
async fn pl11_growth_across_accounts_is_admitted_against_the_process_budget() {
    for pin_other_accounts in [false, true] {
        let cache = cache();
        for account in 1..=4 {
            let index = charged_content(&cache, account, 200 * KIB, &mut 1);
            *cache.entry(account).index.write().await = Some(index);
        }
        let mut readers = Vec::new();
        for account in 2..=4 {
            let entry = cache.get(account).unwrap();
            entry
                .last_used
                .store(100 + account as u64, Ordering::Relaxed);
            if pin_other_accounts {
                readers.push(entry.read_loaded().await.unwrap());
            }
        }
        let entry = cache.get(1).unwrap();
        let mut index = entry.index.write().await;
        let index = index.as_mut().unwrap();
        add_content_bytes(
            &mut index.content.as_mut().unwrap().part,
            &mut 1000,
            300 * KIB,
        );
        assert_eq!(cache.admit_content_growth(index, 1), !pin_other_accounts);
        if pin_other_accounts {
            assert!(index.content.is_none());
            assert_eq!(cache.content_used(), 600 * KIB);
        } else {
            assert!(
                cache
                    .get(2)
                    .unwrap()
                    .read_loaded()
                    .await
                    .unwrap()
                    .content
                    .is_none()
            );
            assert_eq!(cache.content_used(), 900 * KIB);
        }
    }
}

#[test]
fn pl11_concurrent_growth_is_admitted_atomically() {
    for _ in 0..200 {
        let cache = cache();
        let entries = [cache.entry(1), cache.entry(2)];
        let _held = [
            entries[0].index.try_read().unwrap(),
            entries[1].index.try_read().unwrap(),
        ];
        let indices = [
            charged_content(&cache, 1, 400 * KIB, &mut 1),
            charged_content(&cache, 2, 400 * KIB, &mut 1),
        ];
        let barrier = Barrier::new(2);
        std::thread::scope(|scope| {
            let [first, second] = indices;
            let grow = |mut index: LoadedIndex, account| {
                add_content_bytes(
                    &mut index.content.as_mut().unwrap().part,
                    &mut 1000,
                    150 * KIB,
                );
                barrier.wait();
                let admitted = cache.admit_content_growth(&mut index, account);
                assert!(cache.content_used() <= MIB);
                (admitted, index)
            };
            let first = scope.spawn(move || grow(first, 1));
            let second = scope.spawn(move || grow(second, 2));
            let (first_admitted, first) = first.join().unwrap();
            let (second_admitted, second) = second.join().unwrap();
            assert_ne!(first_admitted, second_admitted);
            assert_eq!(first.content.is_some(), first_admitted);
            assert_eq!(second.content.is_some(), second_admitted);
            assert_eq!(cache.content_used(), 550 * KIB);
        });
        assert_eq!(cache.content_used(), 0);
    }
}

#[tokio::test]
async fn fx1_a_reload_to_a_manifest_without_content_drops_the_part() {
    let (key, _, map, bytes, mut manifest) = fixture();
    let cache = cache();
    let entry = loaded(&map, &cache, &key, &bytes, manifest.clone(), true).await;
    assert!(cache.content_used() > 0);
    manifest.content = None;
    assert_eq!(
        plan_reload(&*entry.read_loaded().await.unwrap(), &manifest),
        ReloadPlan::Append {
            thread: vec![],
            content_head_moved: true
        }
    );
    apply_reload(
        &map,
        &entry,
        &key,
        cache.config(),
        &encode(&key, &manifest),
        manifest,
        100,
    )
    .await
    .unwrap();
    assert!(entry.read_loaded().await.unwrap().content.is_none());
    assert_eq!(cache.content_used(), 0);
}

#[tokio::test]
async fn content_failure_during_reload_does_not_turn_into_thread_loss() {
    let (key, _, mut map, bytes, v1) = fixture();
    let cache = cache();
    let entry = loaded(&map, &cache, &key, &bytes, v1.clone(), true).await;
    let v2 = append(&key, &mut map, &v1, 5);
    let head = v2.content.as_ref().unwrap();
    map.insert(head.hash.clone(), b"wrong blob".to_vec());
    let v2_bytes = encode(&key, &v2);
    apply_reload(&map, &entry, &key, cache.config(), &v2_bytes, v2, 100)
        .await
        .unwrap();
    let index = entry.read_loaded().await.unwrap();
    assert!(index.thread.is_present(5));
    assert_eq!(index.manifest_bytes_hash, manifest_hash(&v2_bytes));
    assert!(index.content.is_none());
    let current = index.manifest.clone();
    drop(index);
    assert_eq!(cache.content_used(), 0);
    let Err(ObservedLoadError::ContentLoss(loss)) =
        load_current_content(&map, &entry, &key, cache.config()).await
    else {
        panic!("the failed content walk must carry content loss evidence");
    };
    assert_eq!(loss.class(), LossClass::Hash);
    assert!(loss.matches(&v2_bytes, &current));
    assert!(!loss.matches(&bytes, &current));
    assert!(!loss.matches(&v2_bytes, &v1));
}

struct GatedSource<'a> {
    map: &'a AHashMap<BlobHash, Vec<u8>>,
    gated: AHashSet<BlobHash>,
    fetching: mpsc::UnboundedSender<BlobHash>,
    release: Semaphore,
}

impl BlobSource for GatedSource<'_> {
    async fn fetch(&self, hash: &BlobHash) -> trc::Result<Option<Vec<u8>>> {
        if self.gated.contains(hash) {
            self.fetching.send(hash.clone()).unwrap();
            self.release.acquire().await.unwrap().forget();
        }
        self.map.fetch(hash).await
    }
}

async fn swap_head(entry: &IndexEntry, key: &Secret, manifest: &Manifest) {
    let mut index = entry.index.write().await;
    let index = index.as_mut().unwrap();
    index.manifest = manifest.clone();
    index.manifest_bytes_hash = manifest_hash(&encode(key, manifest));
}

#[tokio::test]
async fn fx3_a_load_racing_a_swap_installs_the_current_head() {
    let (key, _, mut map, bytes, v1) = fixture();
    let v2 = append_content(&key, &mut map, &v1, 5);
    let cache = cache();
    let entry = loaded(&map, &cache, &key, &bytes, v1.clone(), false).await;
    let first = v1.content.unwrap().hash;
    let (fetching, mut fetches) = mpsc::unbounded_channel();
    let source = GatedSource {
        map: &map,
        gated: [first.clone()].into_iter().collect(),
        fetching,
        release: Semaphore::new(0),
    };
    let mut load = pin!(timeout_at(
        Instant::now() + Duration::from_secs(2),
        load_current_content(&source, &entry, &key, cache.config())
    ));
    tokio::select! {
        result = &mut load => panic!("load completed before the swap: {result:?}"),
        hash = fetches.recv() => assert_eq!(hash, Some(first)),
    }
    swap_head(&entry, &key, &v2).await;
    source.release.add_permits(1);
    load.await.unwrap().unwrap();

    let index = entry.read_loaded().await.unwrap();
    let part = index.content.as_ref().unwrap();
    assert_eq!(part.segments, [segment(15), segment(12), segment(11)]);
    assert_eq!(
        part.segments.first(),
        index.manifest.content.as_ref().map(|head| &head.segment)
    );
    assert!(part.part.contains(5));
    assert_eq!(part.bytes, cache.content_used());
}

#[tokio::test]
async fn fx3_repeated_head_moves_respect_the_callers_single_deadline() {
    let (key, _, mut map, bytes, v1) = fixture();
    let v2 = append_content(&key, &mut map, &v1, 5);
    let v3 = append_content(&key, &mut map, &v2, 6);
    let v4 = append_content(&key, &mut map, &v3, 7);
    let cache = cache();
    let entry = loaded(&map, &cache, &key, &bytes, v1.clone(), false).await;
    let heads = [&v1, &v2, &v3, &v4].map(|m| m.content.as_ref().unwrap().hash.clone());
    let (fetching, mut fetches) = mpsc::unbounded_channel();
    let source = GatedSource {
        map: &map,
        gated: heads.iter().cloned().collect(),
        fetching,
        release: Semaphore::new(0),
    };
    let deadline = Instant::now() + Duration::from_millis(500);
    let mut load = Box::pin(timeout_at(
        deadline,
        load_current_content(&source, &entry, &key, cache.config()),
    ));
    for (expected, next) in heads.iter().zip([Some(&v2), Some(&v3), Some(&v4), None]) {
        tokio::select! {
            result = &mut load => panic!("load completed before the deadline: {result:?}"),
            hash = fetches.recv() => assert_eq!(hash.as_ref(), Some(expected)),
        }
        assert!(entry.read_loaded().await.unwrap().content.is_none());
        assert!(cache.content_used() > 0);
        if let Some(next) = next {
            swap_head(&entry, &key, next).await;
            source.release.add_permits(1);
        }
    }
    assert!(load.await.is_err());
    assert_eq!(cache.content_used(), 0);
    assert!(entry.load_flight.try_lock().is_ok());
    let index = entry.index.try_read().unwrap();
    let index = index.as_ref().unwrap();
    assert!(index.content.is_none());
    assert_eq!(index.manifest, v4);
    assert!(index.thread.is_present(4));
}

#[tokio::test]
async fn h8_thread_parts_are_never_refused_or_evicted_for_content() {
    let cache = cache();
    let mut thread = cache.admit(PartKind::Thread, 2 * MIB, 1).unwrap();
    assert_eq!(cache.thread_used(), 2 * MIB);
    assert!(cache.grow(&mut thread, PartKind::Thread, 3 * MIB, 1));
    assert_eq!(cache.thread_used(), 3 * MIB);
    let entry = cache.entry(1);
    *entry.index.write().await = Some(LoadedIndex {
        thread_bytes: 3 * MIB,
        thread_charge: thread,
        ..LoadedIndex::empty(Manifest::default())
    });
    assert_eq!(thread_budget_outcome(false), Some(ZA_SEARCH_OVER_BUDGET));
    assert_eq!(thread_budget_outcome(true), None);
    assert!(cache.admit(PartKind::Content, MIB + 1, 2).is_none());
    assert!(cache.get(1).unwrap().read_loaded().await.is_some());
    assert_eq!(cache.thread_used(), 3 * MIB);
}

#[tokio::test]
async fn g6_loads_are_single_flight() {
    let (key, account, map, bytes, manifest) = fixture();
    let cache = Arc::new(cache());
    let mut source = counting(map);
    source.delay_first = true;
    let source = Arc::new(source);
    let head = manifest.thread_blobs[0].hash.clone();
    let expected = manifest_hash(&bytes);
    let run = || {
        let (source, cache, key, bytes, manifest) = (
            source.clone(),
            cache.clone(),
            key.clone(),
            bytes.clone(),
            manifest.clone(),
        );
        tokio::spawn(async move {
            let entry = cache.entry(account);
            load_entry(&*source, &entry, &key, cache.config(), async {
                Ok((bytes, manifest))
            })
            .await
            .unwrap();
            entry.read_loaded().await.unwrap().manifest_bytes_hash
        })
    };
    let (first, second) = tokio::join!(run(), run());
    assert_eq!(first.unwrap(), expected);
    assert_eq!(second.unwrap(), expected);
    assert_eq!(
        source.reads().iter().filter(|hash| **hash == head).count(),
        1
    );
}

#[cfg(feature = "test_mode")]
#[tokio::test]
async fn s43_idle_sweep_and_invalidation_drop_entries() {
    let cache = ZaSearchCache::new(Ok(ZaSearchConfig {
        idle_secs: 1,
        ..ZaSearchConfig::default()
    }));
    cache.set_content_bytes_for_test(1, 300 * KIB);
    cache
        .get(1)
        .unwrap()
        .last_used
        .store(store::write::now() - 2, Ordering::Relaxed);
    cache.sweep_idle();
    assert!(cache.get(1).is_none());
    assert_eq!(cache.content_used(), 0);
    cache.set_content_bytes_for_test(1, 300 * KIB);
    cache.remove_account(1, EvictReason::Invalidation);
    assert!(cache.get(1).is_none());
    assert_eq!(cache.content_used(), 0);
}

#[test]
fn f16_never_swapped_is_pinned_by_the_type() {
    let swap = include_str!("../../cache/swap/evict.rs");
    assert_eq!(swap.matches("impl SwapCached for").count(), 2);
    assert!(swap.contains("impl SwapCached for MessageStoreCache"));
    assert!(swap.contains("impl SwapCached for GroupwareResources"));
}

struct FaultSource<'a> {
    map: &'a AHashMap<BlobHash, Vec<u8>>,
    hash: &'a BlobHash,
    error: Option<trc::StoreEvent>,
}

impl BlobSource for FaultSource<'_> {
    async fn fetch(&self, hash: &BlobHash) -> trc::Result<Option<Vec<u8>>> {
        if hash != self.hash {
            return self.map.fetch(hash).await;
        }
        match self.error {
            Some(trc::StoreEvent::DecryptError) => Err(trc::StoreEvent::DecryptError
                .into_err()
                .ctx(trc::Key::Id, 7u64)),
            Some(error) => Err(error.into_err()),
            None => Ok(None),
        }
    }
}

#[tokio::test]
async fn content_transient_during_append_preserves_the_installed_index() {
    let (key, _, mut map, bytes, v1) = fixture();
    let v2 = append(&key, &mut map, &v1, 5);
    let v2_bytes = encode(&key, &v2);
    for (error, expected) in [
        (
            Some(trc::StoreEvent::DecryptError),
            TransientClass::CodecKey,
        ),
        (Some(trc::StoreEvent::NotFound), TransientClass::BlobStore),
        (None, TransientClass::AbsentWithinWindow),
    ] {
        let cache = cache();
        let entry = loaded(&map, &cache, &key, &bytes, v1.clone(), true).await;
        let before = (cache.thread_used(), cache.content_used());
        let source = FaultSource {
            map: &map,
            hash: &v2.content.as_ref().unwrap().hash,
            error,
        };
        assert_eq!(
            apply_reload(
                &source,
                &entry,
                &key,
                cache.config(),
                &v2_bytes,
                v2.clone(),
                100
            )
            .await,
            Err(LoadFailure::Transient(expected))
        );
        assert_eq!((cache.thread_used(), cache.content_used()), before);
        let index = entry.read_loaded().await.unwrap();
        assert_eq!(index.manifest, v1);
        assert_eq!(index.manifest_bytes_hash, manifest_hash(&bytes));
        assert!(!index.thread.is_present(5));
        assert_eq!(
            index.content.as_ref().unwrap().segments,
            [segment(12), segment(11)]
        );
    }
}

#[tokio::test]
async fn g6_content_loads_share_one_chain_walk() {
    let (key, _, map, bytes, manifest) = fixture();
    let cache = cache();
    let first = manifest.content.as_ref().unwrap().hash.clone();
    let last = predecessor_of_head(&map, &manifest);
    let entry = loaded(&map, &cache, &key, &bytes, manifest, false).await;
    let mut source = counting(map);
    source.delay_first = true;

    let load = || load_current_content(&source, &entry, &key, cache.config());
    let (left, right) = tokio::join!(load(), load());
    assert_eq!(left, Ok(()));
    assert_eq!(right, Ok(()));
    assert_eq!(source.reads(), [first, last]);
}
