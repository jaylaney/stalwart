/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::get::IntermediateChangesResponse;
use common::Server;
use email::{
    cache::email::MessageCacheAccess,
    message::za::search::{
        changes::{
            RangeView, StreamKind, ZaRetainedBound, za_email_stream_with_presence,
            za_epoch_refuses, za_initial_stream, za_initial_thread_stream, za_present_at_end,
            za_thread_stream_with_presence,
        },
        snapshot::ZaSnapshot,
    },
};
use jmap_proto::{
    error::method::ChangeHistoryRefusal, method::changes::ChangesResponse,
    request::method::MethodObject, types::state::State,
};
use store::{query::log::ZaChangeRecords, roaring::RoaringBitmap};
use types::{collection::SyncCollection, id::Id};

#[derive(Clone, Copy)]
struct Range {
    since: u64,
    pinned: Option<(u64, usize)>,
}

pub(crate) struct ZaChangesSince {
    state: State,
    range: Range,
}

impl ZaChangesSince {
    pub(crate) fn new(state: State) -> Self {
        let range = match &state {
            State::Initial => Range {
                since: 0,
                pinned: None,
            },
            State::Exact(since) => Range {
                since: *since,
                pinned: None,
            },
            State::Intermediate(page) => Range {
                since: page.from_id,
                pinned: Some((page.to_id, page.items_sent)),
            },
        };
        Self { state, range }
    }

    pub(crate) fn minimum(&self) -> Option<u64> {
        if let Some((end, _)) = self.range.pinned {
            Some(end)
        } else {
            Some(self.range.since).filter(|since| *since > 0)
        }
    }
}

pub(crate) struct ZaChangesEndpoint {
    pub(crate) since: u64,
    pub(crate) end: u64,
    revision: u32,
    // None uses the admitted cache's present set.
    historical_presence: Option<RoaringBitmap>,
}

impl ZaChangesEndpoint {
    pub(crate) fn thread_of(&self, view: &ZaSnapshot, document_id: u32) -> u32 {
        view.index
            .thread
            .thread_of_revision(document_id, self.revision)
    }

    pub(crate) fn documents<'a>(&'a self, view: &'a ZaSnapshot) -> impl Iterator<Item = u32> + 'a {
        self.historical_presence
            .as_ref()
            .into_iter()
            .flat_map(|ids| ids.iter())
            .chain(
                self.historical_presence
                    .is_none()
                    .then_some(&view.cache.emails)
                    .into_iter()
                    .flat_map(|emails| emails.iter().map(|email| email.document_id())),
            )
    }

    pub(crate) fn contains(&self, view: &ZaSnapshot, document_id: u32) -> bool {
        self.historical_presence.as_ref().map_or_else(
            || view.cache.has_email_id(&document_id),
            |ids| ids.contains(document_id),
        )
    }
}

pub(crate) struct ZaChangesPage {
    pub(crate) changes: IntermediateChangesResponse,
    pub(crate) endpoint: ZaChangesEndpoint,
}

fn truncated() -> trc::Error {
    ChangeHistoryRefusal::Truncated.into_err()
}

fn predates_epoch() -> trc::Error {
    ChangeHistoryRefusal::PredatesEpoch.into_err()
}

fn invalid() -> trc::Error {
    trc::JmapEvent::CannotCalculateChanges
        .into_err()
        .details("Since state is invalid")
}

pub(crate) async fn za_changes_page(
    server: &Server,
    account: Id,
    object: MethodObject,
    parsed: ZaChangesSince,
    max_changes: usize,
    view: &ZaSnapshot,
) -> trc::Result<ZaChangesPage> {
    let account_id = account.document_id();
    let group_end = view.cache.last_change_id;
    let is_thread = object == MethodObject::Thread;
    let (current_end, collection) = if is_thread {
        (group_end, SyncCollection::Thread)
    } else {
        (view.cache.emails.change_id, SyncCollection::Email)
    };
    let mut range = parsed.range;

    loop {
        let (end, skip) = range.pinned.unwrap_or((current_end, 0));
        let since = range.since;
        if since > end || end > group_end {
            return Err(invalid());
        }

        let retained_endpoint = if since > 0 {
            Some(since)
        } else {
            range.pinned.map(|(end, _)| end)
        };
        if let Some(endpoint) = retained_endpoint {
            if endpoint < server.za_retained_bound(account_id).await? {
                return Err(truncated());
            }
            #[cfg(feature = "test_mode")]
            common::za::search_page_pause_point(account_id).await;
        }
        let empty_exact = range.pinned.is_none() && since == end;
        let store = server.store();
        let epoch_records = if since > 0 && since < view.epoch {
            store
                .za_changes_records_range(account_id, collection, since, view.epoch)
                .await?
        } else {
            ZaChangeRecords::default()
        };
        let records = if empty_exact {
            ZaChangeRecords::default()
        } else {
            store
                .za_changes_records_range(account_id, SyncCollection::Email, since, end)
                .await?
        };
        let later = if range.pinned.is_some() {
            store
                .za_changes_records_range(account_id, SyncCollection::Email, end, group_end)
                .await?
        } else {
            ZaChangeRecords::default()
        };

        // A truncation marker can move beyond group_end and erase records
        // before either bounded read can encounter it.
        if let Some(endpoint) = retained_endpoint
            && (endpoint < server.za_retained_bound(account_id).await?
                || later.truncated
                || (since > 0 && (records.truncated || epoch_records.truncated)))
        {
            return Err(truncated());
        }
        if (since == 0 && range.pinned.is_some() && end < view.epoch)
            || za_epoch_refuses(&epoch_records, is_thread, since, view.epoch)
        {
            return Err(predates_epoch());
        }

        let historical_presence = (range.pinned.is_some() && end < group_end).then(|| {
            za_present_at_end(
                view.cache.emails.iter().map(|email| email.document_id()),
                &later,
            )
        });
        let thread = &view.index.thread;
        let endpoint = ZaChangesEndpoint {
            since,
            end,
            revision: thread.revision_at(end),
            historical_presence,
        };
        let range_view = RangeView {
            records: &records,
            later: &later,
            present: &|id| view.cache.has_email_id(&id),
            end_is_current: endpoint.historical_presence.is_none(),
        };

        let items = if empty_exact {
            Vec::new()
        } else if is_thread {
            let stream = if since == 0 {
                za_initial_thread_stream(thread, endpoint.documents(view), end)
            } else {
                za_thread_stream_with_presence(
                    thread,
                    &range_view,
                    since,
                    end,
                    endpoint.historical_presence.as_ref(),
                )
            };
            stream
                .into_iter()
                .map(|item| (item.kind, Id::from(item.thread_id)))
                .collect()
        } else {
            let stream = if since == 0 {
                za_initial_stream(thread, endpoint.documents(view), end)
            } else {
                za_email_stream_with_presence(
                    thread,
                    &range_view,
                    since,
                    end,
                    endpoint.historical_presence.as_ref(),
                )
            };
            stream
                .into_iter()
                .filter_map(|item| item.new.or(item.old).map(|id| (item.kind, id)))
                .collect()
        };

        if range.pinned.is_some() && skip >= items.len() {
            range = Range {
                since: end,
                pinned: None,
            };
            continue;
        }

        let has_more_changes = items.len().saturating_sub(skip) > max_changes;
        let mut response = ChangesResponse {
            account_id: account,
            old_state: parsed.state,
            new_state: if has_more_changes {
                State::new_intermediate(since, end, skip + max_changes)
            } else {
                State::new_exact(end)
            },
            has_more_changes,
            created: Vec::new(),
            updated: Vec::new(),
            destroyed: Vec::new(),
            updated_properties: None,
        };
        for (kind, id) in items.into_iter().skip(skip).take(max_changes) {
            match kind {
                StreamKind::Created => response.created.push(id),
                StreamKind::Updated => response.updated.push(id),
                StreamKind::Destroyed => response.destroyed.push(id),
            }
        }
        return Ok(ZaChangesPage {
            changes: IntermediateChangesResponse {
                response,
                object,
                updated_properties: None,
            },
            endpoint,
        });
    }
}
