/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    IterateParams, Store, U32_LEN, ValueKey,
    search::{
<<<<<<< HEAD
        IndexDocument, SearchField, SearchFilter, SearchOperator, SearchQuery, SearchValue,
        term::{TermIndex, TermIndexBuilder},
    },
    write::{
        AlignedBytes, Archive, Batch, BatchBuilder, Operation, SEARCH_INDEX_MAX_FIELD_LEN,
        SearchIndex, SearchIndexClass, SearchIndexField, SearchIndexId, SearchIndexType,
        ValueClass, ValueOp, assert::AssertValue, key::DeserializeBigEndian,
=======
        GLOBAL_BUCKET_SHIFT, IndexDocument, MAX_DOCUMENT_SIZE, SearchField, SearchValue, codec,
        term::{AccountIndexer, GlobalIndexer, deserialize_term_fields},
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
    },
    write::{BatchBuilder, SearchIndex, SearchIndexClass, ValueClass},
};
use ahash::AHashSet;
use trc::AddContext;

const STORED_DOCUMENT_BATCH: usize = 1024;

impl Store {
<<<<<<< HEAD
    pub(crate) async fn index(&self, documents: Vec<IndexDocument>) -> trc::Result<()> {
        let truncate_at = if self.is_foundationdb() { 1_048_576 } else { 0 };
        let mut fenced_out = false;

        for mut document in documents {
            let mut batch = BatchBuilder::new();
            let index = document.index;
            let mut old_term_index = None;

            // Zero-access fork (mail ruling R58): the absent-key assertion
            // opens the batch, so it lands in the first commit chunk, with the
            // `Document` key that `write_index` and `merge_index` set first.
            let assert_absent = document.assert_absent.take();
            if let Some((account_id, collection, document_id, class)) = assert_absent.clone() {
                batch
                    .with_account_id(account_id)
                    .with_collection(collection)
                    .with_document(document_id)
                    .assert_value(class, AssertValue::None);
            }

            if matches!(index, SearchIndex::Calendar | SearchIndex::Contacts) {
                let mut account_id = None;
                let mut document_id = None;
                for (field, value) in &document.fields {
                    if let SearchValue::Uint(id) = value {
                        match field {
                            SearchField::AccountId => {
                                account_id = Some(*id as u32);
                            }
                            SearchField::DocumentId => {
                                document_id = Some(*id as u32);
                            }
                            _ => {}
                        }
                    }
                }

                if let (Some(account_id), Some(document_id)) = (account_id, document_id)
                    && let Some(archive) = self
                        .get_value::<Archive<AlignedBytes>>(ValueKey::from(
                            ValueClass::SearchIndex(SearchIndexClass {
                                index,
                                id: SearchIndexId::Account {
                                    account_id,
                                    document_id,
                                },
                                typ: SearchIndexType::Document,
                            }),
                        ))
                        .await
                        .caused_by(trc::location!())?
                {
                    old_term_index = Some(archive);
                }
            }

            let term_index_builder = TermIndexBuilder::build(document, truncate_at);
            // Zero-access fork (mail ruling R62): the `Document` key chunk 0
            // writes, which every later chunk of a fenced document asserts.
            let document_guard = assert_absent.is_some().then(|| {
                ValueClass::SearchIndex(SearchIndexClass {
                    index,
                    id: term_index_builder.id,
                    typ: SearchIndexType::Document,
                })
            });
            if let Some(old_term_index) = old_term_index {
                let old_term_index = old_term_index
                    .unarchive::<TermIndex>()
                    .caused_by(trc::location!())?;
                term_index_builder
                    .index
                    .merge_index(&mut batch, index, term_index_builder.id, old_term_index)
                    .caused_by(trc::location!())?;
            } else {
                term_index_builder
                    .index
                    .write_index(&mut batch, index, term_index_builder.id)
                    .caused_by(trc::location!())?;
            }

            let mut commit_points = batch.commit_points();
            // R63: set once a later chunk's failed guard is confirmed; the
            // document's remaining chunks then write their clears only.
            let mut stopped = false;
            for (chunk, commit_point) in commit_points.iter().enumerate() {
                if stopped {
                    let built = batch.build_one(commit_point);
                    if let Some(mut clears) = take_clears(built.ops) {
                        self.write(Batch {
                            changes: built.changes,
                            ops: &mut clears,
                        })
                        .await
                        .caused_by(trc::location!())?;
                    }
                    continue;
                }

                // Mail ruling R62's test: chunk 0 is written, the rest is not.
                #[cfg(feature = "test_mode")]
                if chunk == 1
                    && let SearchIndexId::Account { account_id, .. } = term_index_builder.id
                {
                    crate::za_test::index_chunk_pause_point(account_id).await;
                }

                match &document_guard {
                    // R62: a later chunk is written only while chunk 0's
                    // `Document` key is stored; a concurrent unindex deleted
                    // it and every key it lists, so the rest of this
                    // document is not written. The assertion goes first; a
                    // `SearchIndex` class serializes its own ids, so the
                    // context ops after it do not matter to it.
                    Some(document_key) if chunk > 0 => {
                        let built = batch.build_one(commit_point);
                        let changes = built.changes;
                        let mut guarded = Vec::with_capacity(built.ops.len() + 1);
                        guarded.push(Operation::AssertValue {
                            class: document_key.clone(),
                            assert_value: AssertValue::Some,
                        });
                        guarded.extend(built.ops.iter_mut().map(|op| {
                            std::mem::replace(op, Operation::DocumentId { document_id: 0 })
                        }));
                        // Mail ruling R63's test: the guard fails unwritten,
                        // as FoundationDB reports a read error at the
                        // assertion.
                        #[cfg(feature = "test_mode")]
                        let result = if let SearchIndexId::Account { account_id, .. } =
                            term_index_builder.id
                            && crate::za_test::take_index_guard_fail(account_id)
                        {
                            Err(trc::StoreEvent::AssertValueFailed.into_err())
                        } else {
                            self.write(Batch {
                                changes,
                                ops: &mut guarded,
                            })
                            .await
                        };
                        #[cfg(not(feature = "test_mode"))]
                        let result = self
                            .write(Batch {
                                changes,
                                ops: &mut guarded,
                            })
                            .await;
                        match result {
                            Ok(_) => {}
                            // R63: FoundationDB reports a read error at the
                            // assertion as a mismatch, so the failure is
                            // confirmed by a read outside the transaction.
                            // Absent: the document stops without an error
                            // (the unindex is the later intent and already
                            // removed what chunk 0 wrote), its remaining
                            // clears are written, its sets dropped, and the
                            // batch's other documents still are written.
                            // Stored, or the read failing: the error is
                            // returned and the caller defers the batch.
                            Err(err) if err.is_assertion_failure() => {
                                match self.key_exists(ValueKey::from(document_key.clone())).await {
                                    Ok(false) => {
                                        stopped = true;
                                        if let Some(mut clears) = take_clears(&mut guarded[1..]) {
                                            self.write(Batch {
                                                changes,
                                                ops: &mut clears,
                                            })
                                            .await
                                            .caused_by(trc::location!())?;
                                        }
                                    }
                                    Ok(true) => {
                                        return Err(err.caused_by(trc::location!()).details(
                                            "Search document's later chunk lost its guard \
                                             while its Document key is stored",
                                        ));
                                    }
                                    Err(read_err) => {
                                        return Err(read_err.caused_by(trc::location!()).details(
                                            "Failed to confirm a search document's lost guard",
                                        ));
                                    }
                                }
                            }
                            Err(err) => return Err(err.caused_by(trc::location!())),
                        }
                    }
                    _ => match self.write(batch.build_one(commit_point)).await {
                        Ok(_) => {}
                        // R58: a fenced document whose first chunk lost its
                        // assertion is not written at all; the batch's other
                        // documents still are, and the caller gets the
                        // failure. A fenced document's later chunks take the
                        // arm above.
                        Err(err) if assert_absent.is_some() && err.is_assertion_failure() => {
                            fenced_out = true;
                            break;
                        }
                        Err(err) => return Err(err.caused_by(trc::location!())),
                    },
                }
            }
        }

        if fenced_out {
            Err(trc::StoreEvent::AssertValueFailed
                .into_err()
                .details("Search document fenced out by its absent-key assertion"))
        } else {
            Ok(())
        }
=======
    pub async fn index_account_documents(
        &self,
        account_id: u32,
        index: SearchIndex,
        index_documents: Vec<IndexDocument>,
        unindex_document_ids: Vec<u32>,
    ) -> trc::Result<()> {
        let diffs = matches!(
            index,
            SearchIndex::Email | SearchIndex::Calendar | SearchIndex::Contacts | SearchIndex::File
        );
        let documents = numbered_documents(index_documents)?;

        let mut read_ids = unindex_document_ids.clone();
        if diffs {
            read_ids.extend(documents.iter().map(|(document_id, _)| *document_id));
        }
        let stored = self
            .stored_documents(index, account_id, read_ids)
            .await
            .caused_by(trc::location!())?;

        let mut batch = account_batch(
            index,
            account_id,
            documents,
            &unindex_document_ids,
            &stored,
            diffs,
        )?;
        self.write_batch(&mut batch)
            .await
            .caused_by(trc::location!())?;

        Ok(())
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
    }

    async fn stored_documents(
        &self,
        index: SearchIndex,
        account_id: u32,
        mut document_ids: Vec<u32>,
    ) -> trc::Result<Vec<(u32, Vec<u8>)>> {
        document_ids.sort_unstable();
        document_ids.dedup();
        let mut stored = Vec::with_capacity(document_ids.len());
        for chunk in document_ids.chunks(STORED_DOCUMENT_BATCH) {
            let ranges = chunk
                .iter()
                .map(|document_id| {
                    let key = codec::account_document_key(index, account_id, *document_id);
                    IterateParams::new(codec::document_key(key.clone()), codec::document_key(key))
                })
                .collect::<Vec<_>>();
            let mut corrupted = None;
            self.iterate_many(ranges, |key, value| {
                match key
                    .get(key.len().wrapping_sub(U32_LEN)..)
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(u32::from_be_bytes)
                {
                    Some(document_id) => {
                        stored.push((document_id, value.to_vec()));
                        Ok(true)
                    }
                    None => {
                        corrupted = Some(trc::Error::corrupted_key(
                            key,
                            Some(value),
                            trc::location!(),
                        ));
                        Ok(false)
                    }
                }
            })
            .await?;
            if let Some(err) = corrupted {
                return Err(err);
            }
        }
        Ok(stored)
    }

    pub async fn index_trace(&self, documents: Vec<IndexDocument>) -> trc::Result<()> {
        let mut global_indexer = GlobalIndexer::default();

        for document in documents.into_iter().rev() {
            let document_id = document
                .fields
                .get(&SearchField::Id)
                .and_then(|v| match v {
                    SearchValue::Uint(v) => Some(*v),
                    _ => None,
                })
                .ok_or_else(|| {
                    trc::StoreEvent::UnexpectedError
                        .into_err()
                        .details("Missing id for document")
                })?;

            if global_indexer.documents.contains_key(&document_id) {
                continue;
            }

            if let Some(size) = global_indexer.insert(document, document_id) {
                trc::event!(
                    SearchIndex(trc::SearchIndexEvent::DocumentTruncated),
                    DocumentId = document_id,
                    Size = size,
                    Limit = MAX_DOCUMENT_SIZE,
                );
            }
        }

        let mut batch = global_indexer.build_batch(SearchIndex::Tracing);
        self.write_batch(&mut batch)
            .await
            .caused_by(trc::location!())?;

        Ok(())
    }

    pub async fn unindex_account(&self, index: SearchIndex, account_id: u32) -> trc::Result<()> {
        let (begin, end) = codec::account_region_range(index, account_id);

        self.delete_range(codec::term_key(begin.clone()), codec::term_key(end.clone()))
            .await
            .caused_by(trc::location!())?;

        self.delete_range(codec::document_key(begin), codec::document_key(end))
            .await
            .caused_by(trc::location!())?;

        Ok(())
    }

    pub async fn clear_search_index_queue(
        &self,
        index: SearchIndex,
        partition: u32,
    ) -> trc::Result<()> {
        let (from_class, to_class) = SearchIndexClass::queue_range(index, partition);
        self.delete_range(ValueKey::from(from_class), ValueKey::from(to_class))
            .await
            .caused_by(trc::location!())?;

        let mut batch = BatchBuilder::new();
        batch
            .clear(SearchIndexClass::QueueIndex { index, partition })
            .clear(SearchIndexClass::QueueStatus { index, partition });
        self.write_batch(&mut batch)
            .await
            .caused_by(trc::location!())
            .map(|_| ())
    }

    pub async fn unindex_traces(&self, before_id: u64) -> trc::Result<()> {
        let last_bucket = (before_id >> GLOBAL_BUCKET_SHIFT)
            .checked_sub(1)
            .ok_or_else(|| {
                trc::StoreEvent::UnexpectedError
                    .into_err()
                    .details("Invalid id cutoff for global unindex")
            })? as u16;
        let last_id =
            ((last_bucket as u64) << GLOBAL_BUCKET_SHIFT) | ((1 << GLOBAL_BUCKET_SHIFT) - 1);
        let mut delete_terms = AHashSet::new();
        let mut has_documents = false;
        let mut scratch = Vec::new();

        self.iterate(
            IterateParams::new(
                ValueKey::from(ValueClass::SearchIndex(SearchIndexClass::GlobalDocument {
                    index: SearchIndex::Tracing,
                    document_id: 0,
                })),
                ValueKey::from(ValueClass::SearchIndex(SearchIndexClass::GlobalDocument {
                    index: SearchIndex::Tracing,
                    document_id: last_id + 1,
                })),
            ),
            |key, value| {
                has_documents = true;
                deserialize_term_fields(value, &mut scratch, |term_hash, mut field_mask| {
                    while field_mask != 0 {
                        let item = 31 - field_mask.leading_zeros();
                        field_mask ^= 1 << item;
                        delete_terms.insert((term_hash, item as u8));
                    }
                })
                .ok_or_else(|| trc::Error::corrupted_key(key, value.into(), trc::location!()))?;

                Ok(true)
            },
        )
        .await
        .caused_by(trc::location!())?;

        if has_documents {
            // Delete terms
            for (term_hash, field) in delete_terms {
                self.delete_range(
                    ValueKey::from(ValueClass::SearchIndex(SearchIndexClass::GlobalTerm {
                        index: SearchIndex::Tracing,
                        field,
                        term: term_hash,
                        block_id: 0,
                    })),
                    ValueKey::from(ValueClass::SearchIndex(SearchIndexClass::GlobalTerm {
                        index: SearchIndex::Tracing,
                        field,
                        term: term_hash,
                        block_id: last_bucket + 1,
                    })),
                )
                .await
                .caused_by(trc::location!())?;
            }

            // Delete global documents
            self.delete_range(
                ValueKey::from(ValueClass::SearchIndex(SearchIndexClass::GlobalDocument {
                    index: SearchIndex::Tracing,
                    document_id: 0,
                })),
                ValueKey::from(ValueClass::SearchIndex(SearchIndexClass::GlobalDocument {
                    index: SearchIndex::Tracing,
                    document_id: last_id + 1,
                })),
            )
            .await
            .caused_by(trc::location!())?;

            // Delete global document ids
            self.delete_range(
                ValueKey::from(ValueClass::SearchIndex(
                    SearchIndexClass::GlobalDocumentId {
                        index: SearchIndex::Tracing,
                        block_id: 0,
                    },
                )),
                ValueKey::from(ValueClass::SearchIndex(
                    SearchIndexClass::GlobalDocumentId {
                        index: SearchIndex::Tracing,
                        block_id: last_bucket + 1,
                    },
                )),
            )
            .await
            .caused_by(trc::location!())?;
        }

        Ok(())
    }

    /// Zero-access fork (mail ruling R62): whether any `Term` or `Index` key
    /// of the account is stored in `index`, with the ranges of `unindex`'s
    /// account-wide delete.
    pub async fn has_account_search_terms(
        &self,
        index: SearchIndex,
        account_id: u32,
    ) -> trc::Result<bool> {
        let key = |document_id, typ| {
            ValueKey::from(ValueClass::SearchIndex(SearchIndexClass {
                index,
                id: SearchIndexId::Account {
                    account_id,
                    document_id,
                },
                typ,
            }))
        };
        for (from, to) in [
            (
                key(
                    0,
                    SearchIndexType::Term {
                        hash: CheekyHash::NULL,
                        field: 0,
                    },
                ),
                key(
                    u32::MAX,
                    SearchIndexType::Term {
                        hash: CheekyHash::FULL,
                        field: u8::MAX,
                    },
                ),
            ),
            (
                key(
                    0,
                    SearchIndexType::Index {
                        field: SearchIndexField {
                            field_id: 0,
                            data: vec![0u8],
                        },
                    },
                ),
                key(
                    u32::MAX,
                    SearchIndexType::Index {
                        field: SearchIndexField {
                            field_id: u8::MAX,
                            data: vec![u8::MAX; SEARCH_INDEX_MAX_FIELD_LEN],
                        },
                    },
                ),
            ),
        ] {
            let mut found = false;
            self.iterate(
                IterateParams::new(from, to).no_values().only_first(),
                |_, _| {
                    found = true;
                    Ok(false)
                },
            )
            .await
            .caused_by(trc::location!())?;
            if found {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// Zero-access fork (mail ruling R63): the `Clear` operations of a stopped
/// document's chunk, with the context operations of the chunk, moved out of
/// `ops`; `None` when the chunk holds no clear. Only `merge_index` emits
/// clears, and it emits them last.
fn take_clears(ops: &mut [Operation]) -> Option<Vec<Operation>> {
    let mut clears = Vec::new();
    let mut has_clear = false;
    for op in ops {
        match op {
            Operation::AccountId { .. }
            | Operation::Collection { .. }
            | Operation::DocumentId { .. } => {}
            Operation::Value {
                op: ValueOp::Clear, ..
            } => has_clear = true,
            _ => continue,
        }
        clears.push(std::mem::replace(
            op,
            Operation::DocumentId { document_id: 0 },
        ));
    }
    has_clear.then_some(clears)
}

pub(crate) fn numbered_documents(
    index_documents: Vec<IndexDocument>,
) -> trc::Result<Vec<(u32, IndexDocument)>> {
    let mut documents = Vec::with_capacity(index_documents.len());
    for document in index_documents.into_iter().rev() {
        let document_id = document
            .fields
            .get(&SearchField::DocumentId)
            .and_then(|v| match v {
                SearchValue::Uint(v) => Some(*v as u32),
                _ => None,
            })
            .ok_or_else(|| {
                trc::StoreEvent::UnexpectedError
                    .into_err()
                    .details("Missing documentId for document")
            })?;
        documents.push((document_id, document));
    }
    Ok(documents)
}

pub(crate) fn account_batch(
    index: SearchIndex,
    account_id: u32,
    documents: Vec<(u32, IndexDocument)>,
    unindex_document_ids: &[u32],
    stored: &[(u32, Vec<u8>)],
    diffs: bool,
) -> trc::Result<BatchBuilder> {
    let mut account_indexer = AccountIndexer::default();
    for document_id in unindex_document_ids {
        if let Some(document) = stored_document(stored, *document_id) {
            account_indexer
                .remove(document, *document_id)
                .caused_by(trc::location!())?;
        }
    }

    for (document_id, document) in documents {
        if account_indexer.documents.contains_key(&document_id) {
            continue;
        }

        if let Some(size) = account_indexer.insert(document, document_id) {
            trc::event!(
                SearchIndex(trc::SearchIndexEvent::DocumentTruncated),
                AccountId = account_id,
                DocumentId = document_id,
                Size = size,
                Limit = MAX_DOCUMENT_SIZE,
            );
        }

        if diffs && let Some(current_document) = stored_document(stored, document_id) {
            account_indexer
                .diff(current_document, document_id)
                .caused_by(trc::location!())?;
        }
    }

    account_indexer.build_batch(index, account_id)
}

pub(crate) fn stored_document(stored: &[(u32, Vec<u8>)], document_id: u32) -> Option<&[u8]> {
    stored
        .binary_search_by_key(&document_id, |(id, _)| *id)
        .ok()
        .and_then(|position| stored.get(position))
        .map(|(_, document)| document.as_slice())
}

#[cfg(stalwart_tests)]
#[rustfmt::skip]
#[path = "../../../../../stalwart-tests/unit/store/src/search/index/test_support.rs"]
mod test_support;
