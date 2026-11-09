/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::search::{za_filter_needs_content, za_sort_needs_content};
use crate::core::{Row, SelectedMailbox, SessionData};
use common::{
    network::SessionStream,
    za::{
        ZA_MAIL_SEARCH_REFUSED, ZA_MAIL_SEARCH_UNAVAILABLE,
        search::trace::{ZA_SEARCH_UNAVAILABLE, za_search_trace},
    },
};
use email::message::{
    sortkeys::MessageComparator,
    za::{
        search::{
            snapshot::{ZaSnapshotFetch, za_no_map_error},
            view::{ViewNeed, ZaView, za_view},
        },
        view::ZaMailView,
    },
};
use imap_proto::{
    ResponseCode,
    protocol::{
        Flag,
        search::{Comparator, Filter, Sort},
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use store::{
    roaring::RoaringBitmap,
    search::SearchComparator,
    za_search::content::{ContentPart, SortField},
};

pub async fn za_imap_content_guard<T: SessionStream>(
    data: &SessionData<T>,
    mailbox: &SelectedMailbox,
    filters: &[Filter],
    sorts: &[Comparator],
    saved_search: &mut Option<Option<Arc<Vec<Row>>>>,
) -> trc::Result<Option<ZaView>> {
    // The ordinary comparator path rejects CC independently of availability.
    if sorts
        .iter()
        .any(|comparator| matches!(comparator.sort, Sort::Cc))
        || !(filters.iter().any(za_filter_needs_content)
            || sorts
                .iter()
                .any(|comparator| za_sort_needs_content(&comparator.sort)))
    {
        return Ok(None);
    }
    let server = &data.server;
    let account_id = mailbox.id.account_id;
    let keys = match data.access_token.za_keys_for(account_id) {
        Some(keys) => keys,
        None if server.za_is_key_account(account_id).await? => {
            return Err(trc::ImapEvent::Error
                .into_err()
                .details(ZA_MAIL_SEARCH_REFUSED)
                .code(ResponseCode::Cannot));
        }
        None => return Ok(None),
    };
    capture_saved_search(mailbox, filters, saved_search).await;
    let deadline = Instant::now() + Duration::from_secs(server.za_search_config().wait_secs);
    za_view(server, account_id, keys, ViewNeed::Content { deadline })
        .await
        .map(Some)
        .map_err(|error| {
            za_search_trace!(
                ZA_SEARCH_UNAVAILABLE,
                AccountId = account_id,
                Reason = error.into_unavailable().as_str()
            );
            za_imap_unavailable()
        })
}

/// Map fallback after Content has been ruled out. THREAD needs this
/// for identity filters now. Task 12 also uses the map for its grouping.
pub(super) async fn za_imap_map_guard<T: SessionStream>(
    data: &SessionData<T>,
    mailbox: &SelectedMailbox,
    filters: &[Filter],
    saved_search: &mut Option<Option<Arc<Vec<Row>>>>,
    map_metadata: bool,
) -> trc::Result<Option<ZaView>> {
    let account_id = mailbox.id.account_id;
    let keys = if filters
        .iter()
        .any(|filter| matches!(filter, Filter::EmailId(_) | Filter::ThreadId(_)))
    {
        // A selected owner connection authenticated before provisioning
        // can remain keyless. Check the selected account before using IDs.
        data.server
            .za_mail_keys(&data.access_token, account_id)
            .await?
    } else if map_metadata {
        data.access_token.za_keys_for(account_id).cloned()
    } else {
        None
    };
    let Some(keys) = keys else {
        return Ok(None);
    };
    capture_saved_search(mailbox, filters, saved_search).await;
    data.server
        .za_snapshot(account_id, &keys)
        .await
        .map(Some)
        .map_err(|reason| za_no_map_error(reason, account_id).code(ResponseCode::Unavailable))
}

async fn capture_saved_search(
    mailbox: &SelectedMailbox,
    filters: &[Filter],
    saved_search: &mut Option<Option<Arc<Vec<Row>>>>,
) {
    // A SAVE producer may need the index write lock before sending its
    // rows. Capture before acquiring either view mode. SAVE has already
    // captured its prior result, even if that result was missing.
    if saved_search.is_none()
        && filters.iter().any(
            |filter| matches!(filter, Filter::Sequence(sequence, _) if sequence.is_saved_search()),
        )
    {
        *saved_search = Some(mailbox.get_saved_search().await);
    }
}

pub(super) fn za_imap_unavailable() -> trc::Error {
    trc::ImapEvent::Error
        .into_err()
        .details(ZA_MAIL_SEARCH_UNAVAILABLE)
        .code(ResponseCode::Cannot)
}

pub enum SentOp {
    Before,
    On,
    Since,
}

pub fn za_imap_sent(content: &ContentPart, op: SentOp, date: i64) -> RoaringBitmap {
    let end = date.saturating_add(86400);
    content
        .document_ids()
        .iter()
        .filter(|&id| {
            content.sent_at(id).is_some_and(|sent_at| match op {
                SentOp::Before => sent_at < date,
                SentOp::On => date <= sent_at && sent_at < end,
                SentOp::Since => sent_at >= date,
            })
        })
        .collect()
}

fn attachment_flag(flag: &Flag) -> Option<bool> {
    match flag {
        Flag::HasAttachment => Some(true),
        Flag::HasNoAttachment => Some(false),
        _ => None,
    }
}

pub fn za_imap_attachment(content: &ContentPart, has: bool) -> RoaringBitmap {
    content
        .document_ids()
        .iter()
        .filter(|&id| content.has_attachment(id) == has)
        .collect()
}

pub(super) fn za_imap_condition(filter: &Filter, content: &ContentPart) -> Option<RoaringBitmap> {
    Some(match filter {
        Filter::Keyword(flag) => za_imap_attachment(content, attachment_flag(flag)?),
        Filter::Unkeyword(flag) => za_imap_attachment(content, !attachment_flag(flag)?),
        Filter::SentBefore(date) => za_imap_sent(content, SentOp::Before, *date),
        Filter::SentOn(date) => za_imap_sent(content, SentOp::On, *date),
        Filter::SentSince(date) => za_imap_sent(content, SentOp::Since, *date),
        _ => return None,
    })
}

pub(super) fn za_imap_sort(
    content: &ContentPart,
    comparator: &Comparator,
    documents: &RoaringBitmap,
) -> Option<MessageComparator> {
    let field = match comparator.sort {
        Sort::Date => SortField::SentAt,
        Sort::From | Sort::DisplayFrom => SortField::From,
        Sort::Subject => SortField::Subject,
        Sort::To | Sort::DisplayTo => SortField::To,
        _ => return None,
    };
    Some(MessageComparator::Search(SearchComparator::sorted_set(
        content.sort_ranks(field, documents),
        comparator.ascending,
    )))
}

#[cfg(test)]
mod tests {
    use super::{SentOp, za_imap_attachment, za_imap_sent};
    use store::{
        roaring::RoaringBitmap,
        search::IndexDocument,
        write::SearchIndex,
        za_search::content::{ContentPart, ContentRecord, SortKeys},
    };

    fn fixture_content() -> ContentPart {
        let mut content = ContentPart::default();
        for (id, date) in [(1, 86_400), (2, 172_799), (3, 172_800)] {
            content.insert(ContentRecord::new(
                IndexDocument::new(SearchIndex::Email)
                    .with_account_id(1)
                    .with_document_id(id),
                id,
                SortKeys::default(),
                Some(date),
                id == 2,
            ));
        }
        content
    }

    #[test]
    fn senton_is_a_half_open_day_window() {
        let content = fixture_content();
        assert_eq!(
            za_imap_sent(&content, SentOp::On, 86_400),
            RoaringBitmap::from([1, 2])
        );
        assert_eq!(
            za_imap_sent(&content, SentOp::Since, 172_800),
            RoaringBitmap::from([3])
        );
        assert_eq!(
            za_imap_sent(&content, SentOp::Before, 172_800),
            RoaringBitmap::from([1, 2])
        );
    }

    #[test]
    fn both_attachment_polarities_use_the_index() {
        let content = fixture_content();
        assert_eq!(za_imap_attachment(&content, true), RoaringBitmap::from([2]));
        assert_eq!(
            za_imap_attachment(&content, false),
            RoaringBitmap::from([1, 3])
        );
    }
}
