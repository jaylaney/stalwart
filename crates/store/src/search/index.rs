/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    Deserialize, IterateParams, Store, U64_LEN, ValueKey,
    search::{
        IndexDocument, SearchField, SearchFilter, SearchOperator, SearchQuery, SearchValue,
        term::{TermIndex, TermIndexBuilder},
    },
    write::{
        AlignedBytes, Archive, Batch, BatchBuilder, Operation, SEARCH_INDEX_MAX_FIELD_LEN,
        SearchIndex, SearchIndexClass, SearchIndexField, SearchIndexId, SearchIndexType,
        ValueClass, ValueOp, assert::AssertValue, key::DeserializeBigEndian,
    },
};
use ahash::AHashMap;
use trc::AddContext;
use utils::cheeky_hash::CheekyHash;

impl Store {
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
    }

    pub(crate) async fn unindex(&self, query: SearchQuery) -> trc::Result<()> {
        let index = query.index;
        let mut account_documents: AHashMap<u32, Vec<u32>> = AHashMap::new();
        let mut ids = vec![];
        let mut to_id = None;
        let mut last_account_id = None;

        for filter in query.filters {
            match filter {
                SearchFilter::Operator { field, op, value } => match (field, value) {
                    (SearchField::AccountId, SearchValue::Uint(id))
                        if op == SearchOperator::Equal =>
                    {
                        last_account_id = Some(id as u32);
                        account_documents.entry(id as u32).or_default();
                    }
                    (SearchField::DocumentId, SearchValue::Uint(id))
                        if op == SearchOperator::Equal && last_account_id.is_some() =>
                    {
                        account_documents
                            .get_mut(&last_account_id.unwrap())
                            .unwrap()
                            .push(id as u32);
                    }
                    (SearchField::Id, SearchValue::Uint(id)) => match op {
                        SearchOperator::LowerThan => {
                            to_id = Some(id.saturating_sub(1));
                        }
                        SearchOperator::LowerEqualThan => {
                            to_id = Some(id);
                        }
                        SearchOperator::Equal => {
                            ids.push(id);
                        }
                        _ => {
                            return Err(trc::StoreEvent::UnexpectedError
                                .into_err()
                                .reason("Unsupported operator for Id field"));
                        }
                    },
                    filter => {
                        return Err(trc::StoreEvent::UnexpectedError
                            .into_err()
                            .details(format!("Unsupported unindex filter {filter:?}")));
                    }
                },
                SearchFilter::And | SearchFilter::Or | SearchFilter::End => {}
                SearchFilter::Not | SearchFilter::DocumentSet(_) => {
                    return Err(trc::StoreEvent::UnexpectedError
                        .into_err()
                        .details(format!("Unsupported unindex filter {filter:?}")));
                }
            }
        }

        // Delete by account and document ids
        for (account_id, document_ids) in account_documents {
            if !document_ids.is_empty() {
                for document_id in document_ids {
                    let Some(archive) = self
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
                    else {
                        continue;
                    };
                    let term_index = archive
                        .unarchive::<TermIndex>()
                        .caused_by(trc::location!())?;
                    let mut batch = BatchBuilder::new();
                    term_index.delete_index(
                        &mut batch,
                        index,
                        SearchIndexId::Account {
                            account_id,
                            document_id,
                        },
                    );
                    self.write(batch.build_all())
                        .await
                        .caused_by(trc::location!())?;
                }
            } else {
                // Delete all documents for the account
                self.delete_range(
                    ValueKey::from(ValueClass::SearchIndex(SearchIndexClass {
                        index,
                        id: SearchIndexId::Account {
                            account_id,
                            document_id: 0,
                        },
                        typ: SearchIndexType::Document,
                    })),
                    ValueKey::from(ValueClass::SearchIndex(SearchIndexClass {
                        index,
                        id: SearchIndexId::Account {
                            account_id,
                            document_id: u32::MAX,
                        },
                        typ: SearchIndexType::Document,
                    })),
                )
                .await
                .caused_by(trc::location!())?;

                self.delete_range(
                    ValueKey::from(ValueClass::SearchIndex(SearchIndexClass {
                        index,
                        id: SearchIndexId::Account {
                            account_id,
                            document_id: 0,
                        },
                        typ: SearchIndexType::Index {
                            field: SearchIndexField {
                                field_id: 0,
                                data: vec![0u8],
                            },
                        },
                    })),
                    ValueKey::from(ValueClass::SearchIndex(SearchIndexClass {
                        index,
                        id: SearchIndexId::Account {
                            account_id,
                            document_id: u32::MAX,
                        },
                        typ: SearchIndexType::Index {
                            field: SearchIndexField {
                                field_id: u8::MAX,
                                data: vec![u8::MAX; SEARCH_INDEX_MAX_FIELD_LEN],
                            },
                        },
                    })),
                )
                .await
                .caused_by(trc::location!())?;

                self.delete_range(
                    ValueKey::from(ValueClass::SearchIndex(SearchIndexClass {
                        index,
                        id: SearchIndexId::Account {
                            account_id,
                            document_id: 0,
                        },
                        typ: SearchIndexType::Term {
                            hash: CheekyHash::NULL,
                            field: 0,
                        },
                    })),
                    ValueKey::from(ValueClass::SearchIndex(SearchIndexClass {
                        index,
                        id: SearchIndexId::Account {
                            account_id,
                            document_id: u32::MAX,
                        },
                        typ: SearchIndexType::Term {
                            hash: CheekyHash::FULL,
                            field: u8::MAX,
                        },
                    })),
                )
                .await
                .caused_by(trc::location!())?;
            }
        }

        // Delete by global ids
        for id in ids {
            let Some(archive) = self
                .get_value::<Archive<AlignedBytes>>(ValueKey::from(ValueClass::SearchIndex(
                    SearchIndexClass {
                        index,
                        id: SearchIndexId::Global { id },
                        typ: SearchIndexType::Document,
                    },
                )))
                .await
                .caused_by(trc::location!())?
            else {
                continue;
            };
            let term_index = archive
                .unarchive::<TermIndex>()
                .caused_by(trc::location!())?;
            let mut batch = BatchBuilder::new();
            term_index.delete_index(&mut batch, index, SearchIndexId::Global { id });
            self.write(batch.build_all())
                .await
                .caused_by(trc::location!())?;
        }

        // Delete ranges
        if let Some(to_id) = to_id {
            let mut batches = Vec::new();
            self.iterate(
                IterateParams::new(
                    ValueKey::from(ValueClass::SearchIndex(SearchIndexClass {
                        index,
                        id: SearchIndexId::Global { id: 0 },
                        typ: SearchIndexType::Document,
                    })),
                    ValueKey::from(ValueClass::SearchIndex(SearchIndexClass {
                        index,
                        id: SearchIndexId::Global { id: to_id },
                        typ: SearchIndexType::Document,
                    })),
                ),
                |key, value| {
                    let archive = <Archive<AlignedBytes> as Deserialize>::deserialize(value)?;
                    let term_index = archive.unarchive::<TermIndex>()?;
                    let mut batch = BatchBuilder::new();
                    term_index.delete_index(
                        &mut batch,
                        index,
                        SearchIndexId::Global {
                            id: key.deserialize_be_u64(key.len() - U64_LEN)?,
                        },
                    );
                    batches.push(batch);

                    Ok(true)
                },
            )
            .await
            .caused_by(trc::location!())?;

            for mut batch in batches {
                self.write(batch.build_all())
                    .await
                    .caused_by(trc::location!())?;
            }
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
