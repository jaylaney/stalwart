/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{api::query::QueryResponseBuilder, changes::state::StateManager};
use common::{Server, auth::AccessToken};
use email::{
    message::za::search::snapshot::{ZaSnapshotFetch, za_no_map_error},
    submission::UndoStatus,
};
use jmap_proto::{
    method::query::{Filter, QueryRequest, QueryResponse},
    object::email_submission::{self, EmailSubmissionComparator, EmailSubmissionFilter},
    request::IntoValid,
};
use std::future::Future;
use store::{
    IterateParams, U32_LEN, U64_LEN, ValueKey,
    ahash::AHashSet,
    roaring::RoaringBitmap,
    search::{SearchFilter, SearchQuery},
    write::{IndexPropertyClass, QueueClass, SearchIndex, ValueClass, key::DeserializeBigEndian},
};
use trc::AddContext;
use types::{
    collection::{Collection, SyncCollection},
    field::EmailSubmissionField,
};

pub trait EmailSubmissionQuery: Sync + Send {
    fn email_submission_query(
        &self,
        request: QueryRequest<email_submission::EmailSubmission>,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<QueryResponse>> + Send;
}

struct Submission {
    document_id: u32,
    send_at: u64,
    email_id: u32,
    thread_id: u32,
    identity_id: u32,
    queue_id: u64,
    undo_status: u8,
}

impl EmailSubmissionQuery for Server {
    async fn email_submission_query(
        &self,
        mut request: QueryRequest<email_submission::EmailSubmission>,
        access_token: &AccessToken,
    ) -> trc::Result<QueryResponse> {
        let account_id = request.account_id.document_id();
        let thread_valued = request.filter.iter().any(|condition| {
            matches!(
                condition,
                Filter::Property(EmailSubmissionFilter::ThreadIds(_))
            )
        }) || request
            .sort
            .as_ref()
            .and_then(|sort| sort.first())
            .is_some_and(|sort| matches!(sort.property, EmailSubmissionComparator::ThreadId));
        let snapshot = if thread_valued && let Some(keys) = access_token.za_keys_for(account_id) {
            Some(
                self.za_snapshot(account_id, keys)
                    .await
                    .map_err(|reason| za_no_map_error(reason, account_id))?,
            )
        } else {
            None
        };
        let thread_of = |submission: &Submission| {
            snapshot.as_ref().map_or(submission.thread_id, |snapshot| {
                snapshot.thread_of(submission.email_id)
            })
        };

        let mut submissions = Vec::with_capacity(16);
        let mut document_ids = RoaringBitmap::new();

        self.store()
            .iterate(
                IterateParams::new(
                    ValueKey {
                        account_id,
                        collection: Collection::EmailSubmission.into(),
                        document_id: 0,
                        class: ValueClass::IndexProperty(IndexPropertyClass::Integer {
                            property: EmailSubmissionField::Metadata.into(),
                            value: 0,
                        }),
                    },
                    ValueKey {
                        account_id,
                        collection: Collection::EmailSubmission.into(),
                        document_id: 0,
                        class: ValueClass::IndexProperty(IndexPropertyClass::Integer {
                            property: EmailSubmissionField::Metadata.into(),
                            value: u64::MAX,
                        }),
                    },
                )
                .ascending(),
                |key, value| {
                    let document_id = key.deserialize_be_u32(key.len() - U32_LEN)?;

                    submissions.push(Submission {
                        document_id,
                        send_at: key.deserialize_be_u64(key.len() - U32_LEN - U64_LEN)?,
                        email_id: value.deserialize_be_u32(0)?,
                        thread_id: value.deserialize_be_u32(U32_LEN)?,
                        identity_id: value.deserialize_be_u32(U32_LEN + U32_LEN)?,
                        queue_id: value.deserialize_be_u64(U32_LEN * 3)?,
                        undo_status: value.get(U32_LEN * 3 + U64_LEN).copied().ok_or_else(
                            || trc::Error::corrupted_key(key, value.into(), trc::location!()),
                        )?,
                    });

                    document_ids.insert(document_id);

                    Ok(true)
                },
            )
            .await
            .caused_by(trc::location!())?;

        if request
            .filter
            .iter()
            .any(|cond| matches!(cond, Filter::Property(EmailSubmissionFilter::UndoStatus(_))))
        {
            let pending = UndoStatus::Pending.as_index();
            for submission in submissions
                .iter_mut()
                .filter(|s| s.undo_status == pending && s.queue_id != 0)
            {
                if !self
                    .store()
                    .key_exists(ValueKey::from(ValueClass::Queue(QueueClass::Message(
                        submission.queue_id,
                    ))))
                    .await
                    .caused_by(trc::location!())?
                {
                    submission.undo_status = UndoStatus::Final.as_index();
                }
            }
        }

        let mut filters = Vec::with_capacity(request.filter.len());
        for cond in std::mem::take(&mut request.filter) {
            match cond {
                Filter::Property(cond) => match cond {
                    EmailSubmissionFilter::IdentityIds(ids) => {
                        let ids = ids
                            .into_valid()
                            .map(|id| id.document_id())
                            .collect::<AHashSet<_>>();

                        filters.push(SearchFilter::is_in_set(RoaringBitmap::from_iter(
                            submissions
                                .iter()
                                .filter(|s| ids.contains(&s.identity_id))
                                .map(|s| s.document_id),
                        )));
                    }
                    EmailSubmissionFilter::EmailIds(ids) => {
                        let ids = ids
                            .into_valid()
                            .map(|id| id.document_id())
                            .collect::<AHashSet<_>>();

                        filters.push(SearchFilter::is_in_set(RoaringBitmap::from_iter(
                            submissions
                                .iter()
                                .filter(|s| ids.contains(&s.email_id))
                                .map(|s| s.document_id),
                        )));
                    }
                    EmailSubmissionFilter::ThreadIds(ids) => {
                        let ids = ids
                            .into_valid()
                            .map(|id| id.document_id())
                            .collect::<AHashSet<_>>();

                        filters.push(SearchFilter::is_in_set(RoaringBitmap::from_iter(
                            submissions
                                .iter()
                                .filter(|s| ids.contains(&thread_of(s)))
                                .map(|s| s.document_id),
                        )));
                    }
                    EmailSubmissionFilter::UndoStatus(undo_status) => {
                        let undo_status = match undo_status {
                            email_submission::UndoStatus::Pending => UndoStatus::Pending,
                            email_submission::UndoStatus::Final => UndoStatus::Final,
                            email_submission::UndoStatus::Canceled => UndoStatus::Canceled,
                        }
                        .as_index();

                        filters.push(SearchFilter::is_in_set(RoaringBitmap::from_iter(
                            submissions
                                .iter()
                                .filter(|s| s.undo_status == undo_status)
                                .map(|s| s.document_id),
                        )));
                    }
                    EmailSubmissionFilter::Before(before) => {
                        let before = before.timestamp() as u64;

                        filters.push(SearchFilter::is_in_set(RoaringBitmap::from_iter(
                            submissions
                                .iter()
                                .filter(|s| s.send_at < before)
                                .map(|s| s.document_id),
                        )));
                    }
                    EmailSubmissionFilter::After(after) => {
                        let after = after.timestamp() as u64;

                        filters.push(SearchFilter::is_in_set(RoaringBitmap::from_iter(
                            submissions
                                .iter()
                                .filter(|s| s.send_at > after)
                                .map(|s| s.document_id),
                        )));
                    }

                    EmailSubmissionFilter::_T(other) => {
                        return Err(trc::JmapEvent::UnsupportedFilter.into_err().details(other));
                    }
                },
                Filter::And => {
                    filters.push(SearchFilter::And);
                }
                Filter::Or => {
                    filters.push(SearchFilter::Or);
                }
                Filter::Not => {
                    filters.push(SearchFilter::Not);
                }
                Filter::Close => {
                    filters.push(SearchFilter::End);
                }
            }
        }

        let results = SearchQuery::new(SearchIndex::InMemory)
            .with_filters(filters)
            .with_mask(document_ids)
            .filter()
            .into_bitmap();

        let mut response = QueryResponseBuilder::new(
            results.len() as usize,
            self.core.jmap.query_max_results,
            self.get_state(account_id, SyncCollection::EmailSubmission)
                .await?,
            &request,
        );

        if !results.is_empty() {
            if let Some(comparator) = request.sort.take().unwrap_or_default().into_iter().next() {
                match comparator.property {
                    EmailSubmissionComparator::EmailId => {
                        if comparator.is_ascending {
                            submissions.sort_by_key(|s| s.email_id);
                        } else {
                            submissions.sort_by_key(|s| u32::MAX - s.email_id);
                        }
                    }
                    EmailSubmissionComparator::ThreadId => {
                        if comparator.is_ascending {
                            submissions.sort_by_key(thread_of);
                        } else {
                            submissions.sort_by_key(|s| u32::MAX - thread_of(s));
                        }
                    }
                    EmailSubmissionComparator::SentAt => {
                        if !comparator.is_ascending {
                            submissions.reverse();
                        }
                    }
                    EmailSubmissionComparator::_T(other) => {
                        return Err(trc::JmapEvent::UnsupportedSort.into_err().details(other));
                    }
                }
            }

            for submission in submissions {
                if results.contains(submission.document_id)
                    && !response.add(0, submission.document_id)
                {
                    break;
                }
            }
        }

        response.build()
    }
}
