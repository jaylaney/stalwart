/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

#[cfg(test)]
#[path = "content_tests.rs"]
mod tests;

use super::FormatError;
use crate::search::{
    IndexDocument, MAX_STORED_DOCUMENT_SIZE, SearchFilter,
    codec::{Reader, read_document},
    leaf::{Leaf, LeafPlan},
    term::{AccountIndexer, deserialize_term_fields, matches_phrase},
};
use ahash::AHashMap;
use roaring::RoaringBitmap;
use std::{collections::BTreeMap, ops::Range, str};
use utils::cheeky_hash::CheekyHash;

const HAS_ATTACHMENT: u8 = 1;
const HAS_SENT_AT: u8 = 2;
const RECORD_OVERHEAD: usize = 4 + 1 + 8 + 3 + 4;
const MAX_SORT_KEY_LEN: usize = 40;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct SortKeys {
    pub from: String,
    pub to: String,
    pub subject: String,
}

#[derive(Clone, Copy)]
pub enum SortField {
    From,
    To,
    Subject,
    SentAt,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ContentRecord {
    document_id: u32,
    sent_at: Option<i64>,
    has_attachment: bool,
    sort_keys: SortKeys,
    terms: Vec<u8>,
}

impl ContentRecord {
    pub fn new(
        document: IndexDocument,
        document_id: u32,
        mut sort_keys: SortKeys,
        sent_at: Option<i64>,
        has_attachment: bool,
    ) -> Self {
        // Upstream produces keys of at most 40 bytes. Longer input still has to fit the
        // wire length without splitting a character. Ranking uses the first 40 bytes.
        for key in [
            &mut sort_keys.from,
            &mut sort_keys.to,
            &mut sort_keys.subject,
        ] {
            key.truncate(key.floor_char_boundary(u8::MAX as usize));
        }
        let terms = AccountIndexer::default()
            .index_document(document, document_id)
            .encode()
            .into_document();

        Self {
            document_id,
            sent_at,
            has_attachment,
            sort_keys,
            terms,
        }
    }

    pub fn document_id(&self) -> u32 {
        self.document_id
    }

    pub fn encoded_len(&self) -> usize {
        RECORD_OVERHEAD
            + self.sort_keys.from.len()
            + self.sort_keys.to.len()
            + self.sort_keys.subject.len()
            + self.terms.len()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.encoded_len());
        bytes.extend_from_slice(&self.document_id.to_le_bytes());
        bytes.push(
            u8::from(self.has_attachment) * HAS_ATTACHMENT
                | u8::from(self.sent_at.is_some()) * HAS_SENT_AT,
        );
        bytes.extend_from_slice(&self.sent_at.unwrap_or_default().to_le_bytes());
        for key in [
            &self.sort_keys.from,
            &self.sort_keys.to,
            &self.sort_keys.subject,
        ] {
            bytes.push(key.len() as u8);
            bytes.extend_from_slice(key.as_bytes());
        }
        bytes.extend_from_slice(&(self.terms.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.terms);
        bytes
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FormatError> {
        let mut reader = Reader::new(bytes);
        let document_id = u32::from_le_bytes(read_array(&mut reader)?);
        let flags = reader.u8().ok_or(FormatError::Truncated)?;
        let sent_at = i64::from_le_bytes(read_array(&mut reader)?);
        if flags & !(HAS_ATTACHMENT | HAS_SENT_AT) != 0 {
            return Err(FormatError::Kind(flags));
        }
        if flags & HAS_SENT_AT == 0 && sent_at != 0 {
            return Err(FormatError::Count);
        }

        let sort_keys = SortKeys {
            from: read_sort_key(&mut reader)?,
            to: read_sort_key(&mut reader)?,
            subject: read_sort_key(&mut reader)?,
        };
        let term_len = u32::from_le_bytes(read_array(&mut reader)?) as usize;
        let terms = reader.slice(term_len).ok_or(FormatError::Truncated)?;
        if term_len > MAX_STORED_DOCUMENT_SIZE || reader.u8().is_some() {
            return Err(FormatError::Count);
        }
        // Only validated term tables reach the infallible posting updates.
        deserialize_term_fields(terms, &mut Vec::new(), |_, _| {}).ok_or(FormatError::Count)?;

        Ok(Self {
            document_id,
            sent_at: (flags & HAS_SENT_AT != 0).then_some(sent_at),
            has_attachment: flags & HAS_ATTACHMENT != 0,
            sort_keys,
            terms: terms.to_vec(),
        })
    }
}

fn read_array<const N: usize>(reader: &mut Reader<'_>) -> Result<[u8; N], FormatError> {
    reader
        .slice(N)
        .ok_or(FormatError::Truncated)?
        .try_into()
        .map_err(|_| FormatError::Truncated)
}

fn read_sort_key(reader: &mut Reader<'_>) -> Result<String, FormatError> {
    let len = reader.u8().ok_or(FormatError::Truncated)? as usize;
    let bytes = reader.slice(len).ok_or(FormatError::Truncated)?;
    str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| FormatError::Count)
}

struct Stored {
    encoded: Box<[u8]>,
    term_range: Range<usize>,
    sent_at: Option<i64>,
    has_attachment: bool,
    sort: [Box<str>; 3],
}

impl Stored {
    fn terms(&self) -> &[u8] {
        &self.encoded[self.term_range.clone()]
    }
}

#[derive(Default)]
pub struct ContentPart {
    documents: AHashMap<u32, Stored>,
    document_ids: RoaringBitmap,
    postings: AHashMap<(u8, CheekyHash), RoaringBitmap>,
    verbatim: AHashMap<u8, BTreeMap<Vec<u8>, RoaringBitmap>>,
}

#[derive(Debug)]
pub enum ResolveError {
    Unresolvable(LeafKind),
    TooLarge(trc::Error),
    Corrupt(u32),
}

#[derive(Clone, Copy, Debug)]
pub enum LeafKind {
    Text,
    KeyValue,
    Integer,
}

impl ContentPart {
    pub fn insert(&mut self, record: ContentRecord) {
        let document_id = record.document_id();
        self.remove(document_id);
        for_each_posting(&record.terms, |field, term| {
            self.postings
                .entry((field, term))
                .or_default()
                .insert(document_id);
            if let Some(key) = verbatim_key(&term) {
                let terms = self.verbatim.entry(field).or_default();
                if let Some(posting) = terms.get_mut(key) {
                    posting.insert(document_id);
                } else {
                    terms.insert(key.to_vec(), RoaringBitmap::from([document_id]));
                }
            }
        });

        let encoded = record.encode().into_boxed_slice();
        let term_range = encoded.len() - record.terms.len()..encoded.len();
        self.documents.insert(
            document_id,
            Stored {
                encoded,
                term_range,
                sent_at: record.sent_at,
                has_attachment: record.has_attachment,
                sort: [
                    record.sort_keys.from.into_boxed_str(),
                    record.sort_keys.to.into_boxed_str(),
                    record.sort_keys.subject.into_boxed_str(),
                ],
            },
        );
        self.document_ids.insert(document_id);
    }

    pub fn remove(&mut self, document_id: u32) -> bool {
        let Some(record) = self.documents.remove(&document_id) else {
            return false;
        };
        for_each_posting(record.terms(), |field, term| {
            if let std::collections::hash_map::Entry::Occupied(mut entry) =
                self.postings.entry((field, term))
            {
                entry.get_mut().remove(document_id);
                if entry.get().is_empty() {
                    entry.remove();
                }
            }
            if let Some(key) = verbatim_key(&term)
                && let Some(terms) = self.verbatim.get_mut(&field)
            {
                if let Some(posting) = terms.get_mut(key) {
                    posting.remove(document_id);
                    if posting.is_empty() {
                        terms.remove(key);
                    }
                }
                if terms.is_empty() {
                    self.verbatim.remove(&field);
                }
            }
        });
        self.document_ids.remove(document_id);
        true
    }

    pub fn extend(&mut self, records: impl IntoIterator<Item = ContentRecord>) {
        for record in records {
            self.insert(record);
        }
    }

    pub fn contains(&self, document_id: u32) -> bool {
        self.document_ids.contains(document_id)
    }

    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    pub fn document_ids(&self) -> &RoaringBitmap {
        &self.document_ids
    }

    pub fn record_bytes(&self, document_id: u32) -> Option<&[u8]> {
        self.documents
            .get(&document_id)
            .map(|record| record.encoded.as_ref())
    }

    pub fn resolve(&self, filters: &[SearchFilter]) -> Result<Vec<SearchFilter>, ResolveError> {
        LeafPlan::new(filters).map_err(ResolveError::TooLarge)?;
        filters
            .iter()
            .map(|filter| match filter {
                SearchFilter::And
                | SearchFilter::Or
                | SearchFilter::Not
                | SearchFilter::End
                | SearchFilter::DocumentSet(_) => Ok(filter.clone()),
                SearchFilter::Integer { .. } => Err(ResolveError::Unresolvable(LeafKind::Integer)),
                SearchFilter::Text { .. } | SearchFilter::KeyValue { .. } => self
                    .resolve_leaf(Leaf::plan(filter))
                    .map(SearchFilter::DocumentSet),
            })
            .collect()
    }

    fn resolve_leaf(&self, leaf: Leaf) -> Result<RoaringBitmap, ResolveError> {
        Ok(match leaf {
            Leaf::Empty => RoaringBitmap::new(),
            Leaf::Resolved(documents) => documents,
            Leaf::Groups { field, groups } => {
                let mut result: Option<RoaringBitmap> = None;
                for group in groups {
                    let mut matches = RoaringBitmap::new();
                    for term in group.into_iter().flatten() {
                        if let Some(posting) = self.postings.get(&(field, term)) {
                            matches |= posting;
                        }
                    }
                    let intersection = match &mut result {
                        Some(result) => {
                            *result &= matches;
                            result
                        }
                        None => result.insert(matches),
                    };
                    if intersection.is_empty() {
                        break;
                    }
                }
                result.unwrap_or_default()
            }
            Leaf::Terms { field, terms } => self.intersect(field, &terms),
            Leaf::Phrase { field, words } => self.resolve_phrase(field, &words)?,
            Leaf::Prefix { field, prefix } => {
                let mut result = RoaringBitmap::new();
                if let Some(terms) = self.verbatim.get(&field) {
                    for (key, posting) in terms.range(prefix.to_vec()..) {
                        if !key.starts_with(&prefix) {
                            break;
                        }
                        result |= posting;
                    }
                }
                result
            }
            Leaf::UnsupportedRange => return Err(ResolveError::Unresolvable(LeafKind::Integer)),
        })
    }

    fn intersect(&self, field: u8, terms: &[CheekyHash]) -> RoaringBitmap {
        let mut postings = terms.iter().map(|term| self.postings.get(&(field, *term)));
        let Some(Some(first)) = postings.next() else {
            return RoaringBitmap::new();
        };
        let mut result = first.clone();
        for posting in postings {
            let Some(posting) = posting else {
                return RoaringBitmap::new();
            };
            result &= posting;
            if result.is_empty() {
                break;
            }
        }
        result
    }

    fn resolve_phrase(
        &self,
        field: u8,
        words: &[CheekyHash],
    ) -> Result<RoaringBitmap, ResolveError> {
        let mut scratch = Vec::new();
        let mut ordinals = Vec::new();
        let mut failure = Vec::new();
        let mut result = RoaringBitmap::new();
        for document_id in self.intersect(field, words) {
            let record = self
                .documents
                .get(&document_id)
                .ok_or(ResolveError::Corrupt(document_id))?;
            let document = read_document(record.terms(), &mut scratch)
                .ok_or(ResolveError::Corrupt(document_id))?;
            if matches_phrase(document, field, words, &mut ordinals, &mut failure)
                .ok_or(ResolveError::Corrupt(document_id))?
            {
                result.insert(document_id);
            }
        }
        Ok(result)
    }

    pub fn sort_ranks(&self, field: SortField, documents: &RoaringBitmap) -> AHashMap<u32, u32> {
        let records = documents
            .iter()
            .filter_map(|id| self.documents.get(&id).map(|record| (id, record)));
        let field_index = match field {
            SortField::From => 0,
            SortField::To => 1,
            SortField::Subject => 2,
            SortField::SentAt => {
                return dense_ranks(records.map(|(id, record)| {
                    // None follows every Some, including i64::MAX.
                    (
                        (record.sent_at.is_none(), record.sent_at.unwrap_or_default()),
                        id,
                    )
                }));
            }
        };

        dense_ranks(records.map(|(id, record)| {
            let key = record.sort[field_index].as_bytes();
            let key = &key[..key.len().min(MAX_SORT_KEY_LEN)];
            let mut padded = [0; MAX_SORT_KEY_LEN];
            padded[..key.len()].copy_from_slice(key);
            (padded, id)
        }))
    }

    pub fn sent_at(&self, document_id: u32) -> Option<i64> {
        self.documents
            .get(&document_id)
            .and_then(|record| record.sent_at)
    }

    pub fn has_attachment(&self, document_id: u32) -> bool {
        self.documents
            .get(&document_id)
            .is_some_and(|record| record.has_attachment)
    }

    pub fn loaded_bytes(&self) -> usize {
        self.documents
            .values()
            .map(|record| record.encoded.len())
            .sum::<usize>()
            + self
                .postings
                .values()
                .chain(self.verbatim.values().flat_map(BTreeMap::values))
                .map(|posting| 16 + posting.serialized_size())
                .sum::<usize>()
    }
}

fn for_each_posting(terms: &[u8], mut visit: impl FnMut(u8, CheekyHash)) {
    deserialize_term_fields(terms, &mut Vec::new(), |term, mut mask| {
        while mask != 0 {
            let field = mask.trailing_zeros();
            mask &= mask - 1;
            visit(field as u8, term);
        }
    })
    .expect("content record has a valid term table");
}

fn verbatim_key(term: &CheekyHash) -> Option<&[u8]> {
    let key = term.as_key();
    (term.len() <= CheekyHash::HASH_SIZE && key.last() != Some(&b'*')).then_some(key)
}

fn dense_ranks<K: Copy + Ord>(keys: impl Iterator<Item = (K, u32)>) -> AHashMap<u32, u32> {
    let mut keys: Vec<_> = keys.collect();
    keys.sort_unstable_by_key(|&(key, _)| key);

    let mut ranks = AHashMap::with_capacity(keys.len());
    let mut rank = 1;
    let mut previous = None;
    for (key, document_id) in keys {
        if previous.is_some_and(|previous| previous != key) {
            rank += 1;
        }
        ranks.insert(document_id, rank);
        previous = Some(key);
    }
    ranks
}
