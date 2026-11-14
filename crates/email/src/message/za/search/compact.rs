/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{changes::ZaRetainedBound, pass::Staging};
use common::{
    Server,
    za::search::{
        ZaSearchConfig,
        cache::{BudgetCharge, IndexEntry, LoadedIndex, PartKind},
        error::TransientClass,
        load::{ContentChainReader, ObservedLoadError, VerifiedContentNode, content_frame_bytes},
        seal::{build_thread_blob, seal_content_blob},
        trace::ZA_SEARCH_COMPACTION_SKIPPED,
    },
    za_search_trace,
};
use std::collections::BTreeMap;
use store::{
    write::Operation,
    za_search::{
        BlobHeader, BlobKind, FormatError, PadTarget, SegmentId,
        blob::{HiddenSection, PublicSection, SEAL_OVERHEAD, encode_content},
        compaction_allowance,
        content::OwnedContentFrame,
        thread::{ThreadPart, Tombstone},
    },
};
use types::{blob::BlobClass, blob_hash::BlobHash, collection::Collection};
use vault::{Zeroizing, session::SessionKeys};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TierOverflow;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MergeStep {
    pub count: usize,
    pub into_tier: u8,
}

// After each completed run, its output is the first input to the next
// candidate run. Older nodes can only extend that run until the first mismatch.
struct TierPlan {
    segments: usize,
    tier: u8,
    count: usize,
    steps: Vec<MergeStep>,
}

impl TierPlan {
    fn new(segments: usize) -> Self {
        debug_assert!(segments >= 2);
        Self {
            segments,
            tier: 0,
            count: 0,
            steps: Vec::new(),
        }
    }

    fn push(&mut self, tier: u8) -> Result<bool, TierOverflow> {
        if self.count == 0 {
            self.tier = tier;
        }
        if tier != self.tier {
            return Ok(false);
        }
        self.count += 1;
        if self.count == self.segments {
            self.tier = self.tier.checked_add(1).ok_or(TierOverflow)?;
            self.steps.push(MergeStep {
                count: self.segments,
                into_tier: self.tier,
            });
            self.count = 1;
        }
        Ok(true)
    }
}

#[cfg(test)]
fn tier_merge_plan(
    tiers_young_first: &[u8],
    segments: usize,
) -> Result<Vec<MergeStep>, TierOverflow> {
    let mut plan = TierPlan::new(segments);
    for &tier in tiers_young_first {
        if !plan.push(tier)? {
            break;
        }
    }
    Ok(plan.steps)
}

pub(super) struct CompactedThreads {
    pub part: ThreadPart,
    pub charge: BudgetCharge,
}

pub(super) async fn compact_threads(
    server: &Server,
    account_id: u32,
    keys: &SessionKeys,
    thread: &ThreadPart,
    config: &ZaSearchConfig,
) -> Result<Option<(BlobHeader, Vec<u8>, CompactedThreads)>, TransientClass> {
    let bound = server
        .za_retained_bound(account_id)
        .await
        .map_err(|_| TransientClass::Store)?;
    let cache = &server.inner.cache.za_search;
    let mut charge = cache
        .admit(PartKind::Thread, thread.loaded_bytes() as u64, account_id)
        .expect("a thread part is always admitted");
    let compacted = thread.compact(bound);
    let encoding_bytes = compacted
        .part
        .loaded_bytes()
        .saturating_add(compaction_allowance(compacted.n_records).saturating_mul(4))
        .saturating_add(config.pad_min.saturating_mul(2));
    let _encoding = cache
        .admit(PartKind::Thread, encoding_bytes as u64, account_id)
        .expect("a thread part is always admitted");
    let hidden = HiddenSection {
        records: compacted.part.records().cloned().collect(),
        aliases: compacted.part.aliases().copied().collect(),
    };
    let public = PublicSection {
        tombstones: compacted
            .part
            .records()
            .filter_map(|record| {
                record.deleted.map(|deleted| Tombstone {
                    document_id: record.document_id,
                    deleted,
                })
            })
            .collect(),
        rows: compacted.part.rows().to_vec(),
    };
    let header = BlobHeader {
        kind: BlobKind::Thread,
        tier: 0,
        segment: SegmentId::random(),
        predecessor: None,
    };
    let n_records = compacted.n_records;
    #[cfg(feature = "test_mode")]
    let n_records = if common::za::test_hooks::search_compaction_force_allowance(account_id) {
        0
    } else {
        n_records
    };
    let bytes = match build_thread_blob(
        keys.swk(),
        account_id,
        &header,
        Some((&hidden, PadTarget::Compaction { n_records })),
        &public,
        config.pad_min,
    ) {
        Ok(bytes) => bytes,
        Err(FormatError::AllowanceExceeded) => {
            za_search_trace!(
                ZA_SEARCH_COMPACTION_SKIPPED,
                AccountId = account_id,
                Reason = "allowance"
            );
            return Ok(None);
        }
        Err(_) => return Err(TransientClass::Validation),
    };
    cache.grow(
        &mut charge,
        PartKind::Thread,
        (compacted.part.loaded_bytes() as u64).saturating_add(bytes.len() as u64),
        account_id,
    );
    Ok(Some((
        header,
        bytes,
        CompactedThreads {
            part: compacted.part,
            charge,
        },
    )))
}

pub(super) enum CompactError {
    Read(ObservedLoadError),
    Transient(TransientClass),
    TierOverflow,
    ThreadInventory,
}

impl From<TierOverflow> for CompactError {
    fn from(_: TierOverflow) -> Self {
        Self::TierOverflow
    }
}

pub(super) struct CompactedContent {
    pub header: BlobHeader,
    pub bytes: Vec<u8>,
    pub replaced: Vec<BlobHash>,
    pub replaced_size: u64,
    pub rewritten: u64,
    pub full: bool,
    pub removed_missing: bool,
    pub charge: BudgetCharge,
    pub staged_bytes: u64,
    pub retirement_bytes: u64,
}

pub(super) async fn compact_content(
    server: &Server,
    entry: &IndexEntry,
    keys: &SessionKeys,
    base: &LoadedIndex,
    staging: &Staging,
    cursor_pending: bool,
    config: &ZaSearchConfig,
) -> Result<Option<CompactedContent>, CompactError> {
    let Some(head) = &base.manifest.content else {
        return Ok(None);
    };
    if staging.content.is_empty() || u64::from(head.length) + 1 < config.segments as u64 {
        return Ok(None);
    }

    let cache = &server.inner.cache.za_search;
    let account_id = entry.account_id;
    let staged_size = staging
        .content
        .iter()
        .map(|(_, record)| (size_of::<u32>() + record.encoded_len()) as u64)
        .sum::<u64>()
        .saturating_add((BlobHeader::LEN + SEAL_OVERHEAD + size_of::<u32>()) as u64);
    // Compressed terms can retain much more capacity than their encoded length.
    let staged_bytes = staging.content_owned_bytes();
    let mut charge = cache
        .admit(PartKind::Content, staged_bytes, account_id)
        .ok_or(CompactError::Transient(TransientClass::Budget))?;
    let link = BlobClass::Linked {
        account_id,
        collection: Collection::Principal.into(),
        document_id: 0,
    };
    let mut reader = ContentChainReader::new(base).expect("a named chain");
    let mut tiered = TierPlan::new(config.segments);
    tiered.push(0)?;
    let mut inputs = Vec::new();
    let mut max_tier = 0;
    let mut descriptor_bytes = 0u64;
    // Reserve spare container capacity and relocation while the descriptor
    // vector, visited set, promotion plan and replacement list can coexist.
    const DESCRIPTOR_BYTES: u64 = (4
        * (size_of::<VerifiedContentNode>()
            + size_of::<SegmentId>()
            + 1
            + size_of::<MergeStep>()
            + size_of::<BlobHash>())) as u64;
    while let Some((hash, remaining)) = reader.next_input() {
        let hint = server
            .store()
            .blob_link_size(hash, &link)
            .await
            .map_err(|_| CompactError::Transient(TransientClass::Store))?;
        let size = hint
            .filter(|&size| size != 0 && size <= remaining)
            .unwrap_or(remaining);
        // This read checks the digest and header only. It does not retain
        // ciphertext or decrypt record frames during overflow preflight.
        descriptor_bytes = descriptor_bytes.saturating_add(DESCRIPTOR_BYTES);
        let retained_bytes = staged_bytes.saturating_add(descriptor_bytes);
        let read_bytes = retained_bytes.saturating_add(size).saturating_add(128);
        if !cache.grow(&mut charge, PartKind::Content, read_bytes, account_id) {
            return Err(CompactError::Transient(TransientClass::Budget));
        }
        let node = reader
            .next(server, entry, config.absent_retry_secs)
            .await
            .map_err(CompactError::Read)?
            .expect("the reader has a next input");
        cache.grow(&mut charge, PartKind::Content, retained_bytes, account_id);
        if node.size() > size {
            // A stale or damaged link-size hint must not authorize recovery.
            return Err(CompactError::Transient(TransientClass::Validation));
        }
        if cursor_pending && !tiered.push(node.header().tier)? {
            break;
        }
        max_tier = max_tier.max(node.header().tier);
        inputs.push(node);
    }
    let steps = if cursor_pending {
        tiered.steps
    } else {
        vec![MergeStep {
            count: inputs.len() + 1,
            into_tier: max_tier.saturating_add(1),
        }]
    };
    if steps.is_empty() {
        return Ok(None);
    }

    let selected = steps.iter().map(|step| step.count - 1).sum::<usize>();
    inputs.truncate(selected);
    let replaced_size = inputs.iter().map(|node| node.size()).sum::<u64>();
    // The retired-hash vectors, clear operations and serialized transaction
    // keys can coexist. Keep this estimate through disposal of the batch.
    let retirement_bytes = (selected as u64)
        .saturating_mul((4 * (size_of::<BlobHash>() + size_of::<Operation>()) + 128) as u64);
    let retained = staged_bytes
        .saturating_add(descriptor_bytes)
        .saturating_add(retirement_bytes);
    // Sealing the final root can hold plaintext, ciphertext and its sealed
    // copy together. That peak is separate from frame parsing and assembly.
    let reserve = replaced_size
        .saturating_add(staged_size)
        .saturating_mul(3)
        .saturating_add(retained);
    // After overflow preflight, an allocation may fit cold even when the
    // selected account's pinned content occupies the remaining room.
    let mut grow_work = |needed| {
        cache.grow(&mut charge, PartKind::Content, needed, account_id)
            || (needed <= cache.content_budget()
                && entry.try_evict_content_for_compaction(base.manifest_bytes_hash)
                && cache.grow(&mut charge, PartKind::Content, needed, account_id))
    };
    if !grow_work(reserve) {
        return Err(CompactError::Transient(TransientClass::Budget));
    }

    #[cfg(feature = "test_mode")]
    common::za::test_hooks::note_search_content_write(
        account_id,
        common::za::test_hooks::SearchContentWrite::Original(staged_size),
    );
    let mut reserve_work = |bytes: u64| grow_work(reserve.max(retained.saturating_add(bytes)));
    let mut frames = BTreeMap::new();
    let mut frame_bytes = 0u64;
    let mut removed_missing = false;
    let budget = CompactError::Transient(TransientClass::Budget);
    if !reserve_work(staged_size.saturating_add(frame_slots(staging.content.len()))) {
        return Err(budget);
    }
    for (_, record) in staging.content.iter().rev() {
        let (missing, inserted) = insert_frame(
            &mut frames,
            OwnedContentFrame::from_record(record),
            &base.thread,
        );
        removed_missing |= missing;
        frame_bytes = frame_bytes.saturating_add(inserted);
    }
    let mut inputs = inputs.into_iter();
    let mut replaced = Vec::new();
    let mut rewritten = 0u64;
    let mut steps = steps.into_iter().peekable();
    let (header, bytes) = loop {
        let step = steps.next().expect("a nonempty plan");
        let mut predecessor = None;
        for _ in 1..step.count {
            let input = inputs.next().expect("the preflight verified this input");
            let working = frame_bytes.saturating_add(frame_slots(frames.len()));
            let records = reader
                .records(
                    server,
                    entry,
                    keys.swk(),
                    config.absent_retry_secs,
                    &input,
                    |needed| reserve_work(working.saturating_add(needed)),
                )
                .await
                .map_err(CompactError::Read)?;
            let assembly = working
                .saturating_add(content_frame_bytes(&records))
                .saturating_add((records.capacity() * size_of::<OwnedContentFrame>()) as u64)
                .saturating_add(frame_slots(records.len()));
            if !reserve_work(assembly) {
                return Err(budget);
            }
            for record in records.into_iter().rev() {
                let (missing, inserted) = insert_frame(&mut frames, record, &base.thread);
                removed_missing |= missing;
                frame_bytes = frame_bytes.saturating_add(inserted);
            }
            replaced.push(input.hash().clone());
            predecessor = input.header().predecessor.clone();
        }
        let header = BlobHeader {
            kind: BlobKind::Content,
            tier: step.into_tier,
            segment: SegmentId::random(),
            predecessor,
        };
        let count = frames.len() as u64;
        let output =
            frame_bytes.saturating_add((count + 1).saturating_mul(size_of::<u32>() as u64));
        let assembly = frame_bytes
            .saturating_add(frame_slots(frames.len()))
            .saturating_add(count.saturating_mul(size_of::<&[u8]>() as u64))
            .saturating_add(output);
        if !reserve_work(assembly) {
            return Err(budget);
        }
        let slices = frames
            .values()
            .map(OwnedContentFrame::bytes)
            .collect::<Vec<_>>();
        let plaintext = Zeroizing::new(encode_content(&slices));
        let output_size = (BlobHeader::LEN + SEAL_OVERHEAD + plaintext.len()) as u64;
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::note_search_content_write(
            account_id,
            common::za::test_hooks::SearchContentWrite::Encoded {
                tier: header.tier,
                bytes: output_size,
            },
        );
        rewritten = rewritten
            .checked_add(output_size)
            .ok_or(CompactError::Transient(TransientClass::Validation))?;
        if steps.peek().is_none() {
            drop(slices);
            drop(frames);
            if !reserve_work(output_size.saturating_mul(3)) {
                return Err(budget);
            }
            let bytes = seal_content_blob(keys.swk(), account_id, &header, plaintext);
            break (header, bytes);
        }
        // Intermediate frames are reused for the next step without sealing
        // or uploading. Count their exact sealed-equivalent rewrite volume.
    };
    Ok(Some(CompactedContent {
        header,
        bytes,
        replaced,
        replaced_size,
        rewritten,
        full: !cursor_pending,
        removed_missing,
        charge,
        staged_bytes,
        retirement_bytes,
    }))
}

// Include spare B-tree nodes and their keys while constructing the map.
fn frame_slots(count: usize) -> u64 {
    (count as u64).saturating_mul((4 * size_of::<(u32, OwnedContentFrame)>()) as u64)
}

fn insert_frame(
    frames: &mut BTreeMap<u32, OwnedContentFrame>,
    record: OwnedContentFrame,
    thread: &ThreadPart,
) -> (bool, u64) {
    let id = record.document_id();
    if thread.is_present(id)
        && let std::collections::btree_map::Entry::Vacant(entry) = frames.entry(id)
    {
        // Youngest blobs are visited first, and frames in each blob in reverse.
        let bytes = record.bytes().len() as u64;
        entry.insert(record);
        (false, bytes)
    } else {
        (thread.record(id).is_none(), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use store::{
        search::IndexDocument,
        write::SearchIndex,
        za_search::{
            ThreadFieldsK,
            content::{ContentRecord, SortKeys},
        },
    };

    #[test]
    fn tier_merge_plan_cascades() {
        assert_eq!(
            tier_merge_plan(&[0, 0, 0, 0, 1, 1, 1, 2], 4),
            Ok(vec![
                MergeStep {
                    count: 4,
                    into_tier: 1
                },
                MergeStep {
                    count: 4,
                    into_tier: 2
                }
            ])
        );
    }

    #[test]
    fn tier_merge_plan_stops_at_a_mixed_run() {
        assert_eq!(tier_merge_plan(&[0, 0, 1], 3), Ok(vec![]));
        assert_eq!(
            tier_merge_plan(&[0, 0, 255], 2),
            Ok(vec![MergeStep {
                count: 2,
                into_tier: 1
            }])
        );
    }

    #[test]
    fn tier_merge_plan_rejects_the_entire_cascade_on_overflow() {
        let tiers = [0].into_iter().chain(0..=255).collect::<Vec<_>>();
        assert_eq!(tier_merge_plan(&tiers, 2), Err(TierOverflow));
    }

    #[test]
    fn chain_bound_holds() {
        let mut chain: Vec<(u8, usize)> = Vec::new();
        let mut rewritten = 0;
        for _ in 0..4096 {
            chain.insert(0, (0, 1));
            let tiers = chain.iter().map(|&(tier, _)| tier).collect::<Vec<_>>();
            for step in tier_merge_plan(&tiers, 16).unwrap() {
                let bytes = chain.drain(..step.count).map(|(_, bytes)| bytes).sum();
                rewritten += bytes;
                chain.insert(0, (step.into_tier, bytes));
            }
            assert!(chain.len() <= 46);
        }
        assert!(rewritten <= 3 * 4096);
    }

    fn record(id: u32) -> ContentRecord {
        ContentRecord::new(
            IndexDocument::new(SearchIndex::Email),
            id,
            SortKeys::default(),
            None,
            false,
        )
    }

    #[test]
    fn merge_drops_tombstoned_content() {
        let mut thread = ThreadPart::default();
        for id in 1..=3 {
            thread.assign(
                &ThreadFieldsK {
                    hashes: vec![id.into()],
                    subject_hash: id.into(),
                    sent_at: None,
                },
                id,
                1,
                1,
            );
        }
        thread.tombstone(2, 2);
        let mut merged = BTreeMap::new();
        for id in 1..=3 {
            assert!(
                !insert_frame(
                    &mut merged,
                    OwnedContentFrame::from_record(&record(id)),
                    &thread
                )
                .0
            );
        }
        assert!(
            insert_frame(
                &mut merged,
                OwnedContentFrame::from_record(&record(4)),
                &thread
            )
            .0
        );
        assert_eq!(merged.keys().copied().collect::<Vec<_>>(), vec![1, 3]);
    }
}
