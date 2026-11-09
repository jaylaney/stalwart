/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

mod filter;
mod text;
mod window;

#[cfg(all(test, stalwart_tests))]
#[rustfmt::skip]
#[path = "../../../../../stalwart-tests/unit/jmap/src/email/query/tests.rs"]
mod tests;

use crate::{
    api::{
        metadata::{MetadataType, ObjectMetadata},
        query::QueryResponseBuilder,
    },
    changes::state::JmapCacheState,
    email::{
        za::sort_needs_content,
        za_search::{za_content_query, za_sort_comparator, za_unavailable},
    },
};
use common::{
    MessageStoreCache, MessagesCache, Server, auth::AccessToken, cache::email::MessageRef,
};
use compact_str::ToCompactString;
use email::message::za::search::{
    snapshot::{ZaSnapshotFetch, za_no_map_error},
    threads::za_thread_keywords,
};
use email::{
    cache::{
        MessageCacheFetch,
        email::{MessageAccess, MessageCacheAccess, thread_keywords},
        mailbox::MailboxCacheAccess,
    },
    message::{
        sortkeys::{
            CacheSorter, EmailSortKeys, MessageCacheField, MessageComparator, MessageSortField,
        },
        za::search::{query::za_resolve_residual, view::ZaView},
    },
};
use filter::{QueryFilter, RowFilter};
use jmap_proto::{
    method::query::{Comparator, Filter, QueryRequest, QueryResponse},
    object::email::{Email, EmailComparator, EmailQueryFilter},
    request::capability::CapabilityIds,
    types::state::State,
};
use std::future::Future;
use store::{
    roaring::RoaringBitmap,
    search::{SearchComparator, SearchQuery},
    write::SearchIndex,
};
use trc::AddContext;
use types::acl::Acl;
use window::{TextSource, TextWindow, WindowedWalk};

const SPARSE_RESIDUAL_RATIO: u64 = 8;

pub trait EmailQuery: Sync + Send {
    fn email_query(
        &self,
        request: QueryRequest<Email>,
        access_token: &AccessToken,
        using: CapabilityIds,
    ) -> impl Future<Output = trc::Result<QueryResponse>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WalkOrder {
    Ascending,
    Descending,
}

enum Plan {
    Walk(WalkOrder),
    TopK(CacheSorter),
    Sorted(Vec<MessageComparator>),
}

enum Candidates {
    All,
    Positions(Vec<u32>),
}

enum Pending<'a> {
    Ready(Page<'a>),
    Sort {
        page: Page<'a>,
        matches: RoaringBitmap,
        comparators: Vec<MessageComparator>,
    },
    Window {
        page: Page<'a>,
        candidates: Candidates,
        order: WalkOrder,
        text: TextWindow,
    },
}

struct Page<'a> {
    snapshot: Option<&'a ZaView>,
    builder: QueryResponseBuilder,
    threads: Option<RoaringBitmap>,
    total: usize,
}

impl EmailQuery for Server {
    async fn email_query(
        &self,
        request: QueryRequest<Email>,
        access_token: &AccessToken,
        using: CapabilityIds,
    ) -> trc::Result<QueryResponse> {
        let (view, mode) = email_query_view(
            self,
            access_token,
            request.account_id.document_id(),
            &request.filter,
            request.sort.as_deref().unwrap_or_default(),
        )
        .await?;
        email_query_with_view(self, request, access_token, using, view.as_ref(), mode).await
    }
}

#[derive(Clone, Copy)]
pub enum EmailQueryViewMode {
    Map,
    Content,
}

pub(crate) async fn email_query_view(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    filters: &[Filter<EmailQueryFilter>],
    sorts: &[Comparator<EmailComparator>],
) -> trc::Result<(Option<ZaView>, Option<EmailQueryViewMode>)> {
    if let Some(view) = za_content_query(server, access_token, account_id, filters, sorts).await? {
        Ok((Some(view), Some(EmailQueryViewMode::Content)))
    } else if let Some(keys) = access_token.za_keys_for(account_id) {
        let view = server
            .za_snapshot(account_id, keys)
            .await
            .map_err(|reason| za_no_map_error(reason, account_id))?;
        Ok((Some(view), Some(EmailQueryViewMode::Map)))
    } else {
        Ok((None, None))
    }
}

/// Evaluate using the caller's already acquired view and selected mode.
pub(crate) async fn email_query_with_view(
    server: &Server,
    mut request: QueryRequest<Email>,
    access_token: &AccessToken,
    using: CapabilityIds,
    view: Option<&ZaView>,
    mode: Option<EmailQueryViewMode>,
) -> trc::Result<QueryResponse> {
    let account_id = request.account_id.document_id();
    let content = match mode {
        Some(EmailQueryViewMode::Content) => Some(view.expect("a selected content view").content()),
        _ => None,
    };
    let object_metadata = ObjectMetadata::new(server, access_token, using, MetadataType::Email);
    let sampled = object_metadata.viewer_change_id(server, account_id).await?;
    let cache = match view {
        Some(view) => view.cache.clone(),
        None => server
            .get_cached_messages(account_id)
            .await
            .caused_by(trc::location!())?,
    };
    let access = if access_token.is_shared(account_id) {
        MessageAccess::Mailboxes(cache.shared_mailboxes(access_token, Acl::ReadItems))
    } else {
        MessageAccess::All
    };
    let metadata =
        object_metadata.query(request.filter.iter().filter_map(|filter| match filter {
            Filter::Property(EmailQueryFilter::Metadata(filter)) => Some(filter),
            _ => None,
        }))?;
    let metadata_matches = object_metadata
        .evaluate(server, account_id, &metadata, || {
            cache
                .with_metadata()
                .filter(|message| access.allows(*message))
                .map(|message| message.document_id())
                .collect::<RoaringBitmap>()
        })
        .await?;
    let mut filter = QueryFilter::parse(
        std::mem::take(&mut request.filter),
        &cache,
        access,
        metadata_matches.into_iter(),
        server.core.email.default_language,
        view,
        content,
    )?;
    let sort = request.sort.take();
    let documents = content
        .filter(|_| {
            sort.as_ref()
                .is_some_and(|sort| sort.iter().any(|sort| sort_needs_content(&sort.property)))
        })
        .map(|_| {
            filter
                .as_mut()
                .map(|filter| filter.readable(&cache))
                .unwrap_or_default()
        });
    let mut comparators = sort_comparators(sort, view.zip(documents.as_ref()))?;
    let query_state = if view.is_some() {
        cache.get_state(false)
    } else {
        sampled.state(cache.get_state(false))
    };
    let max_results = server.core.jmap.query_max_results;

    let Some(mut filter) = filter.filter(|query| !query.row.access.is_empty()) else {
        return Page::new(&request, max_results, query_state, view).finish();
    };

    restrict_thread_keywords(&mut comparators, &cache, &mut filter, view);
    let windowed = if view.is_none() && server.search_store().supports_windowed_walk() {
        let resolve_rows = server
            .search_store()
            .account_query_rows(cache.emails.len() as u64);
        WindowedWalk::of(
            &request,
            &comparators,
            &mut filter,
            account_id,
            resolve_rows,
        )
    } else {
        None
    };
    let residual_filters = filter.residual_filters();
    if !residual_filters.is_empty() {
        let residual = if view.is_some() {
            let filters = match content {
                Some(content) => za_resolve_residual(account_id, residual_filters, content)
                    .map_err(|reason| za_unavailable(&"residual", false, &reason))?,
                // Map mode only admits metadata conditions.
                None => residual_filters,
            };
            SearchQuery::new(SearchIndex::Email)
                .with_filters(filters)
                .with_mask(filter.readable(&cache))
                .filter()
                .into_bitmap()
        } else {
            server
                .search_store()
                .filter_account(
                    SearchQuery::new(SearchIndex::Email)
                        .with_filters(residual_filters)
                        .with_account_id(account_id)
                        .with_mask(filter.readable(&cache)),
                )
                .await
                .caused_by(trc::location!())?
        };
        if residual.is_empty() {
            return Page::new(&request, max_results, query_state, view).finish();
        }
        filter.row.residual = Some(residual);
    }

    let mut row = filter.into_row();
    match Pending::new(
        &cache,
        &mut row,
        comparators,
        request,
        max_results,
        query_state,
        windowed,
        view,
    ) {
        Pending::Ready(page) => page.finish(),
        Pending::Window {
            mut page,
            candidates,
            order,
            text,
        } => {
            page.walk_windowed(
                TextSource::Store(server),
                &cache,
                &mut row,
                &candidates,
                order,
                text,
            )
            .await?;
            page.finish()
        }
        Pending::Sort {
            mut page,
            matches,
            comparators,
        } => {
            let results = if view.is_some() {
                server
                    .sort_email_matches(account_id, &cache, matches, comparators)
                    .await?
            } else {
                server
                    .query_emails(
                        account_id,
                        &cache,
                        SearchQuery::new(SearchIndex::Email)
                            .with_account_id(account_id)
                            .with_mask(matches),
                        comparators,
                    )
                    .await?
            };
            page.feed(&cache, results);
            page.finish()
        }
    }
}

impl<'a> Pending<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        cache: &MessageStoreCache,
        filter: &mut RowFilter,
        comparators: Vec<MessageComparator>,
        mut request: QueryRequest<Email>,
        max_results: usize,
        query_state: State,
        windowed: Option<WindowedWalk>,
        view: Option<&'a ZaView>,
    ) -> Self {
        let candidates = Candidates::new(&cache.emails, filter);
        if let Some(WindowedWalk {
            order,
            query,
            resolve_rows,
        }) = windowed
        {
            let page = Page::new(&request, max_results, query_state, view);
            let wanted = usize::try_from(request.position.unwrap_or(0))
                .unwrap_or(0)
                .saturating_add(page.builder.limit);
            if wanted == 0 {
                return Pending::Ready(page);
            }
            let count = match (&candidates, &filter.residual) {
                (Candidates::All, Some(residual)) => {
                    usize::try_from(residual.len()).unwrap_or(usize::MAX)
                }
                (candidates, _) => candidates.len(&cache.emails),
            };
            return Pending::Window {
                text: TextWindow::new(query, wanted, count, resolve_rows),
                page,
                candidates,
                order,
            };
        }
        let plan = Plan::new(&request, cache, comparators);
        if request.anchor.is_none()
            && let Some(position) = request.position.filter(|position| *position < 0)
            && !matches!(plan, Plan::Sorted(_))
        {
            let total = candidates.count(&cache.emails, filter, collapse_threads(&request), view);
            let start = i64::try_from(total)
                .unwrap_or(i64::MAX)
                .saturating_add(i64::from(position))
                .max(0);
            request.position = Some(i32::try_from(start).unwrap_or(i32::MAX));
        }

        let mut page = Page::new(&request, max_results, query_state, view);
        match plan {
            Plan::Walk(order) => {
                page.walk(&cache.emails, filter, &candidates, order);
                Pending::Ready(page)
            }
            Plan::TopK(sorter) => {
                let start = usize::try_from(request.position.unwrap_or(0)).unwrap_or(0);
                page.top(cache, filter, &candidates, &sorter, start);
                Pending::Ready(page)
            }
            Plan::Sorted(comparators) => Pending::Sort {
                page,
                matches: candidates.matches(&cache.emails, filter),
                comparators,
            },
        }
    }
}

fn restrict_thread_keywords(
    comparators: &mut [MessageComparator],
    cache: &MessageStoreCache,
    filter: &mut QueryFilter,
    snapshot: Option<&ZaView>,
) {
    for comparator in comparators {
        if let MessageComparator::Cache {
            field: MessageCacheField::ThreadKeyword { keyword, match_all },
            ascending,
        } = comparator
        {
            let visible = filter.visible(cache);
            let documents = if let Some(snapshot) = snapshot {
                za_thread_keywords(snapshot, keyword, *match_all, None, visible)
            } else if let Some(visible) = visible {
                thread_keywords(cache, keyword, *match_all, None, Some(visible))
            } else {
                continue;
            };
            *comparator = MessageComparator::Search(SearchComparator::set(documents, *ascending));
        }
    }
}

fn collapse_threads(request: &QueryRequest<Email>) -> bool {
    request.arguments.collapse_threads.unwrap_or(false)
}

impl Plan {
    fn walk_order(comparators: &[MessageComparator]) -> Option<WalkOrder> {
        match comparators {
            [] => Some(WalkOrder::Ascending),
            [
                MessageComparator::Cache {
                    field: MessageCacheField::ReceivedAt,
                    ascending,
                },
            ] => Some(if *ascending {
                WalkOrder::Ascending
            } else {
                WalkOrder::Descending
            }),
            _ => None,
        }
    }

    fn new(
        request: &QueryRequest<Email>,
        cache: &MessageStoreCache,
        comparators: Vec<MessageComparator>,
    ) -> Self {
        if let Some(order) = Plan::walk_order(&comparators) {
            return Plan::Walk(order);
        }
        match comparators.as_slice() {
            _ if !collapse_threads(request) && request.anchor.is_none() => {
                match CacheSorter::new(cache, comparators) {
                    Ok(sorter) => Plan::TopK(sorter),
                    Err(comparators) => Plan::Sorted(comparators),
                }
            }
            _ => Plan::Sorted(comparators),
        }
    }
}

impl WindowedWalk {
    fn of(
        request: &QueryRequest<Email>,
        comparators: &[MessageComparator],
        filter: &mut QueryFilter,
        account_id: u32,
        resolve_rows: Option<f64>,
    ) -> Option<Self> {
        let order = Plan::walk_order(comparators)?;
        if request.anchor.is_some()
            || request.calculate_total.unwrap_or(false)
            || request.position.is_some_and(|position| position < 0)
        {
            return None;
        }
        filter.take_text().map(|filters| WindowedWalk {
            order,
            query: SearchQuery::new(SearchIndex::Email)
                .with_filters(filters)
                .with_account_id(account_id),
            resolve_rows,
        })
    }
}

impl Candidates {
    fn new(cache: &MessagesCache, filter: &mut RowFilter) -> Self {
        match filter.residual.take() {
            Some(residual)
                if residual.len().saturating_mul(SPARSE_RESIDUAL_RATIO) < cache.len() as u64 =>
            {
                let mut positions = residual
                    .iter()
                    .filter_map(|document_id| cache.position(document_id))
                    .collect::<Vec<_>>();
                positions.sort_unstable();
                Candidates::Positions(positions)
            }
            residual => {
                filter.residual = residual;
                Candidates::All
            }
        }
    }

    fn len(&self, cache: &MessagesCache) -> usize {
        match self {
            Candidates::All => cache.len(),
            Candidates::Positions(positions) => positions.len(),
        }
    }

    fn count(
        &self,
        cache: &MessagesCache,
        filter: &RowFilter,
        collapse_threads: bool,
        snapshot: Option<&ZaView>,
    ) -> usize {
        match self {
            Candidates::All => count_matches(cache.iter(), filter, collapse_threads, snapshot),
            Candidates::Positions(positions) => count_matches(
                positions.iter().filter_map(|position| cache.at(*position)),
                filter,
                collapse_threads,
                snapshot,
            ),
        }
    }

    fn matches(&self, cache: &MessagesCache, filter: &RowFilter) -> RoaringBitmap {
        match self {
            Candidates::All => matching_ids(cache.iter(), filter),
            Candidates::Positions(positions) => matching_ids(
                positions.iter().filter_map(|position| cache.at(*position)),
                filter,
            ),
        }
    }
}

fn count_matches<'x>(
    messages: impl Iterator<Item = MessageRef<'x>>,
    filter: &RowFilter,
    collapse_threads: bool,
    snapshot: Option<&ZaView>,
) -> usize {
    if collapse_threads {
        let mut threads = RoaringBitmap::new();
        messages
            .filter(|message| {
                filter.matches(*message) && threads.insert(thread_prefix(snapshot, *message))
            })
            .count()
    } else {
        messages.filter(|message| filter.matches(*message)).count()
    }
}

fn matching_ids<'x>(
    messages: impl Iterator<Item = MessageRef<'x>>,
    filter: &RowFilter,
) -> RoaringBitmap {
    messages
        .filter(|message| filter.matches(*message))
        .map(|message| message.document_id())
        .collect()
}

fn thread_prefix(snapshot: Option<&ZaView>, message: MessageRef<'_>) -> u32 {
    snapshot.map_or_else(
        || message.thread_id(),
        |snapshot| snapshot.thread_of(message.document_id()),
    )
}

impl<'a> Page<'a> {
    fn new(
        request: &QueryRequest<Email>,
        max_results: usize,
        query_state: State,
        snapshot: Option<&'a ZaView>,
    ) -> Self {
        Page {
            snapshot,
            builder: QueryResponseBuilder::new_streaming(max_results, query_state, request),
            threads: collapse_threads(request).then(RoaringBitmap::new),
            total: 0,
        }
    }

    fn emit(&mut self, message: MessageRef<'_>) -> bool {
        let thread_id = thread_prefix(self.snapshot, message);
        if self
            .threads
            .as_mut()
            .is_some_and(|threads| !threads.insert(thread_id))
        {
            return true;
        }
        self.total += 1;
        self.builder.add(thread_id, message.document_id())
    }

    fn count(&mut self, message: MessageRef<'_>) {
        let thread_id = thread_prefix(self.snapshot, message);
        if self
            .threads
            .as_mut()
            .is_none_or(|threads| threads.insert(thread_id))
        {
            self.total += 1;
        }
    }

    fn fill<'x>(
        &mut self,
        filter: &RowFilter,
        messages: &mut impl Iterator<Item = MessageRef<'x>>,
    ) -> bool {
        for message in messages {
            if filter.matches(message) && !self.emit(message) {
                return true;
            }
        }
        false
    }

    fn count_rest<'x>(
        &mut self,
        filter: &RowFilter,
        messages: impl Iterator<Item = MessageRef<'x>>,
    ) {
        for message in messages {
            if filter.matches(message) {
                self.count(message);
            }
        }
    }

    fn walk(
        &mut self,
        cache: &MessagesCache,
        filter: &RowFilter,
        candidates: &Candidates,
        order: WalkOrder,
    ) {
        let counts = self.builder.wants_total();
        match (candidates, order) {
            (Candidates::All, WalkOrder::Ascending) => {
                let mut messages = cache.iter();
                if self.fill(filter, &mut messages) && counts {
                    self.count_rest(filter, messages);
                }
            }
            (Candidates::All, WalkOrder::Descending) => {
                let mut messages = cache.iter().rev();
                if self.fill(filter, &mut messages) && counts {
                    self.count_rest(filter, messages);
                }
            }
            (Candidates::Positions(positions), WalkOrder::Ascending) => {
                let mut messages = positions.iter().filter_map(|position| cache.at(*position));
                if self.fill(filter, &mut messages) && counts {
                    self.count_rest(filter, messages);
                }
            }
            (Candidates::Positions(positions), WalkOrder::Descending) => {
                let mut messages = positions
                    .iter()
                    .rev()
                    .filter_map(|position| cache.at(*position));
                if self.fill(filter, &mut messages) && counts {
                    self.count_rest(filter, messages);
                }
            }
        }
    }

    fn top(
        &mut self,
        cache: &MessageStoreCache,
        filter: &RowFilter,
        candidates: &Candidates,
        sorter: &CacheSorter,
        start: usize,
    ) {
        let count = start.saturating_add(self.builder.limit);
        let (messages, total) = match candidates {
            Candidates::All => sorter.top(
                cache,
                cache
                    .emails
                    .iter()
                    .filter(|message| filter.matches(*message)),
                count,
            ),
            Candidates::Positions(positions) => sorter.top(
                cache,
                positions
                    .iter()
                    .filter_map(|position| cache.emails.at(*position))
                    .filter(|message| filter.matches(*message)),
                count,
            ),
        };
        self.total = total;
        for (document_id, thread_id) in messages {
            let thread_id = self
                .snapshot
                .map_or(thread_id, |snapshot| snapshot.thread_of(document_id));
            if !self.builder.add(thread_id, document_id) {
                break;
            }
        }
    }

    fn feed(&mut self, cache: &MessageStoreCache, results: Vec<u32>) {
        let counts = self.builder.wants_total();
        let mut results = results
            .into_iter()
            .filter_map(|document_id| cache.email_by_id(&document_id));
        for message in results.by_ref() {
            if !self.emit(message) {
                break;
            }
        }
        if counts {
            for message in results {
                self.count(message);
            }
        }
    }

    fn finish(self) -> trc::Result<QueryResponse> {
        let total = self.builder.wants_total().then_some(self.total);
        self.builder.finish(total)
    }
}

fn sort_comparators(
    sort: Option<Vec<Comparator<EmailComparator>>>,
    content_sort: Option<(&ZaView, &RoaringBitmap)>,
) -> trc::Result<Vec<MessageComparator>> {
    let sort = sort.filter(|sort| !sort.is_empty()).unwrap_or_default();
    let mut comparators = Vec::with_capacity(sort.len());
    for comparator in sort {
        let ascending = comparator.is_ascending;
        if let Some((view, documents)) = content_sort
            && let Some(comparator) = za_sort_comparator(
                &comparator.property,
                ascending,
                view.content(),
                &view.cache,
                documents,
                |message| view.thread_of(message.document_id()),
            )
        {
            comparators.push(comparator);
            continue;
        }
        comparators.push(match comparator.property {
            EmailComparator::ReceivedAt => MessageComparator::Cache {
                field: MessageCacheField::ReceivedAt,
                ascending,
            },
            EmailComparator::Size => MessageComparator::Cache {
                field: MessageCacheField::Size,
                ascending,
            },
            EmailComparator::From => MessageComparator::SortKey {
                field: MessageSortField::From,
                ascending,
            },
            EmailComparator::To => MessageComparator::SortKey {
                field: MessageSortField::To,
                ascending,
            },
            EmailComparator::Subject => MessageComparator::SortKey {
                field: MessageSortField::Subject,
                ascending,
            },
            EmailComparator::SentAt => MessageComparator::Cache {
                field: MessageCacheField::SentAt,
                ascending,
            },
            EmailComparator::HasKeyword(keyword) => MessageComparator::Cache {
                field: MessageCacheField::Keyword(keyword),
                ascending,
            },
            EmailComparator::AllInThreadHaveKeyword(keyword) => MessageComparator::Cache {
                field: MessageCacheField::ThreadKeyword {
                    keyword,
                    match_all: true,
                },
                ascending,
            },
            EmailComparator::SomeInThreadHaveKeyword(keyword) => MessageComparator::Cache {
                field: MessageCacheField::ThreadKeyword {
                    keyword,
                    match_all: false,
                },
                ascending,
            },
            other => {
                return Err(trc::JmapEvent::UnsupportedSort
                    .into_err()
                    .details(other.to_compact_string()));
            }
        });
    }
    Ok(comparators)
}
