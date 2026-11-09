/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::za::{attachment_keyword, first_refused_filter, first_refused_sort};
use common::{
    MessageStoreCache, Server,
    auth::AccessToken,
    za::{
        ZA_MAIL_SEARCH_REFUSED, ZA_MAIL_SEARCH_UNAVAILABLE,
        search::trace::{ZA_SEARCH_UNAVAILABLE, za_search_trace},
    },
};
use email::{
    cache::email::MessageAccess,
    message::{
        sortkeys::MessageComparator,
        za::search::{
            ensure::Unavailable,
            view::{ViewNeed, ZaView, za_view},
        },
    },
};
use jmap_proto::{
    method::query::{Comparator, Filter},
    object::email::{EmailComparator, EmailFilter, EmailQueryFilter},
};
use std::{
    fmt::Display,
    time::{Duration, Instant},
};
use store::{
    roaring::RoaringBitmap,
    search::SearchComparator,
    za_search::content::{ContentPart, SortField},
};

pub use email::message::za::search::ensure::is_current as za_content_current;

pub async fn za_content_query(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    filters: &[Filter<EmailQueryFilter>],
    sorts: &[Comparator<EmailComparator>],
) -> trc::Result<Option<ZaView>> {
    let first_filter = first_refused_filter(filters);
    let first_sort = first_refused_sort(sorts);
    let (property, is_sort): (&(dyn Display + Sync), bool) = match (first_filter, first_sort) {
        (Some(filter), _) => (filter, false),
        (None, Some(sort)) => (sort, true),
        (None, None) => return Ok(None),
    };

    let keys = access_token.za_keys_for(account_id);
    if keys.is_none() && !server.za_is_key_account(account_id).await? {
        return Ok(None);
    }
    let Some(keys) = keys else {
        return Err(refusal(property, is_sort, ZA_MAIL_SEARCH_REFUSED));
    };
    // Object metadata is unavailable for a key account even in a tree
    // that begins with a supported content condition.
    if let Some(metadata) = filters.iter().find_map(|filter| match filter {
        Filter::Property(condition @ EmailQueryFilter::Metadata(_)) => Some(condition),
        _ => None,
    }) {
        return Err(refusal(metadata, false, ZA_MAIL_SEARCH_REFUSED));
    }
    if sorts
        .iter()
        .any(|sort| matches!(sort.property, EmailComparator::Cc))
    {
        return Err(trc::JmapEvent::UnsupportedSort
            .into_err()
            .details(EmailComparator::Cc.to_string()));
    }

    let deadline = Instant::now() + Duration::from_secs(server.za_search_config().wait_secs);
    za_view(server, account_id, keys, ViewNeed::Content { deadline })
        .await
        .map(Some)
        .map_err(|error| {
            let reason = error.into_unavailable();
            za_search_trace!(
                ZA_SEARCH_UNAVAILABLE,
                AccountId = account_id,
                Reason = reason.as_str()
            );
            za_unavailable(property, is_sort, &reason)
        })
}

fn refusal(property: &dyn Display, is_sort: bool, text: &str) -> trc::Error {
    let event = if is_sort {
        trc::JmapEvent::UnsupportedSort
    } else {
        trc::JmapEvent::UnsupportedFilter
    };
    event.into_err().details(format!("{property}: {text}"))
}

pub fn za_unavailable(property: &dyn Display, is_sort: bool, _reason: &Unavailable) -> trc::Error {
    refusal(property, is_sort, ZA_MAIL_SEARCH_UNAVAILABLE)
}

pub fn za_resolve_condition(
    condition: &EmailFilter,
    content: &ContentPart,
    cache: &MessageStoreCache,
    access: &MessageAccess,
) -> Option<RoaringBitmap> {
    let documents = content.document_ids();
    let attachment = |wanted| {
        documents
            .iter()
            .filter(|&id| content.has_attachment(id) == wanted)
            .collect()
    };
    Some(match condition {
        EmailFilter::SentBefore(date) => {
            let date = date.timestamp();
            documents
                .iter()
                .filter(|&id| content.sent_at(id).is_some_and(|s| s < date))
                .collect()
        }
        EmailFilter::SentAfter(date) => {
            let date = date.timestamp();
            documents
                .iter()
                .filter(|&id| content.sent_at(id).is_some_and(|s| s > date))
                .collect()
        }
        EmailFilter::HasAttachment(wanted) => attachment(*wanted),
        EmailFilter::HasKeyword(keyword) => attachment(attachment_keyword(keyword)?),
        EmailFilter::NotKeyword(keyword) => attachment(!attachment_keyword(keyword)?),
        EmailFilter::AllInThreadHaveKeyword(keyword)
        | EmailFilter::SomeInThreadHaveKeyword(keyword)
        | EmailFilter::NoneInThreadHaveKeyword(keyword) => {
            let wanted = attachment_keyword(keyword)?;
            let all = matches!(condition, EmailFilter::AllInThreadHaveKeyword(_));
            let none = matches!(condition, EmailFilter::NoneInThreadHaveKeyword(_));
            let visible = || {
                cache
                    .emails
                    .iter()
                    .filter(|message| access.allows(*message))
            };
            let threads = attachment_threads(content, wanted, all, visible());
            visible()
                .filter(|message| threads.contains(message.thread_id()) != none)
                .map(|message| message.document_id())
                .collect()
        }
        _ => return None,
    })
}

pub fn za_sort_comparator(
    property: &EmailComparator,
    ascending: bool,
    content: &ContentPart,
    cache: &MessageStoreCache,
    documents: &RoaringBitmap,
) -> Option<MessageComparator> {
    let field = match property {
        EmailComparator::From => SortField::From,
        EmailComparator::To => SortField::To,
        EmailComparator::Subject => SortField::Subject,
        EmailComparator::SentAt => SortField::SentAt,
        EmailComparator::HasKeyword(keyword) => {
            let wanted = attachment_keyword(keyword)?;
            let set = (documents & content.document_ids())
                .iter()
                .filter(|&id| content.has_attachment(id) == wanted)
                .collect();
            return Some(MessageComparator::Search(SearchComparator::set(
                set, ascending,
            )));
        }
        EmailComparator::AllInThreadHaveKeyword(keyword)
        | EmailComparator::SomeInThreadHaveKeyword(keyword) => {
            let wanted = attachment_keyword(keyword)?;
            let all = matches!(property, EmailComparator::AllInThreadHaveKeyword(_));
            let visible = || {
                cache
                    .emails
                    .iter()
                    .filter(|message| documents.contains(message.document_id()))
            };
            let threads = attachment_threads(content, wanted, all, visible());
            let set = visible()
                .filter(|message| threads.contains(message.thread_id()))
                .map(|message| message.document_id())
                .collect();
            return Some(MessageComparator::Search(SearchComparator::set(
                set, ascending,
            )));
        }
        _ => return None,
    };
    Some(MessageComparator::Search(SearchComparator::sorted_set(
        content.sort_ranks(field, documents),
        ascending,
    )))
}

// Task 12 switches these stored thread ids to the validated map.
fn attachment_threads<'a>(
    content: &ContentPart,
    wanted: bool,
    all: bool,
    visible: impl Iterator<Item = common::cache::email::MessageRef<'a>>,
) -> RoaringBitmap {
    let mut tagged = RoaringBitmap::new();
    let mut untagged = RoaringBitmap::new();
    for message in visible {
        let id = message.document_id();
        if content.contains(id) && content.has_attachment(id) == wanted {
            tagged.insert(message.thread_id());
        } else if all {
            untagged.insert(message.thread_id());
        }
    }
    tagged - untagged
}

#[cfg(test)]
mod tests {
    use super::{MessageAccess, za_content_current, za_resolve_condition, za_sort_comparator};
    use common::{
        MailboxesCache, MessageStoreCache, MessagesCache, za::search::cache::LoadedIndex,
    };
    use email::message::{
        sortkeys::MessageComparator,
        za::search::{ensure::Unavailable, query::za_resolve_residual},
    };
    use jmap_proto::{
        object::email::{EmailComparator, EmailFilter},
        types::date::UTCDate,
    };
    use nlp::language::Language;
    use store::{
        roaring::RoaringBitmap,
        search::{
            EmailSearchField, IndexDocument, KeyValueMatch, SearchComparator, SearchField,
            SearchFilter, SearchQuery,
        },
        write::SearchIndex,
        za_search::{
            Checkpoint,
            content::{ContentPart, ContentRecord, SortKeys},
            manifest::{ContentTo, Manifest},
        },
    };
    use types::{id::Id, keyword::Keyword};

    fn fixture_content() -> ContentPart {
        let mut content = ContentPart::default();
        for (id, subject, sent_at) in [(1, "bravo", 100), (2, "alpha", 200), (3, "alpha", 300)] {
            let mut document = IndexDocument::new(SearchIndex::Email)
                .with_account_id(1)
                .with_document_id(id);
            document.index_text(EmailSearchField::Subject, subject, Language::English);
            if id == 1 {
                document.insert_key_value(EmailSearchField::Headers, "x-private", "yes");
            }
            content.insert(ContentRecord::new(
                document,
                id,
                SortKeys {
                    subject: subject.to_string(),
                    ..Default::default()
                },
                Some(sent_at),
                id == 2,
            ));
        }
        content
    }

    fn empty_cache() -> MessageStoreCache {
        MessageStoreCache {
            emails: MessagesCache::new(0, Vec::new(), Vec::new()).into(),
            mailboxes: MailboxesCache {
                change_id: 0,
                index: Default::default(),
                items: Box::new([]),
                size: 0,
            }
            .into(),
            update_lock: Default::default(),
            refresh: Default::default(),
            last_change_id: 0,
            size: 0,
            verification: Default::default(),
        }
    }

    #[test]
    fn every_content_leaf_becomes_a_document_set() {
        let filters = vec![
            SearchFilter::And,
            SearchFilter::has_english_text(EmailSearchField::Subject, "alpha"),
            SearchFilter::KeyValue {
                field: EmailSearchField::Headers.into(),
                key: "x-private".into(),
                op: KeyValueMatch::Exists,
            },
            SearchFilter::End,
            SearchFilter::integer_eq(SearchField::AccountId, 1u32),
        ];
        let resolved = za_resolve_residual(1, filters, &fixture_content()).unwrap();
        assert_eq!(
            resolved.len(),
            4,
            "account id is already scoped by the view"
        );
        assert!(matches!(&resolved[1], SearchFilter::DocumentSet(set) if set.iter().eq([2, 3])));
        assert!(matches!(&resolved[2], SearchFilter::DocumentSet(set) if set.iter().eq([1])));
        let result = SearchQuery::new(SearchIndex::Email)
            .with_filters(resolved)
            .with_mask(RoaringBitmap::from([1, 2, 3]))
            .filter()
            .into_bitmap();
        assert!(result.is_empty(), "{result:?}");
    }

    #[test]
    fn unresolved_leaf_refuses_the_entire_tree() {
        let filters = vec![
            SearchFilter::Or,
            SearchFilter::has_english_text(EmailSearchField::Subject, "alpha"),
            SearchFilter::Not,
            SearchFilter::integer_eq(SearchField::Id, 7u32),
            SearchFilter::End,
            SearchFilter::End,
        ];
        assert!(matches!(
            za_resolve_residual(1, filters, &fixture_content()),
            Err(Unavailable::Unresolved)
        ));
    }

    #[test]
    fn sent_and_attachment_conditions_read_content() {
        let content = fixture_content();
        let cache = empty_cache();
        for (condition, expected) in [
            (
                EmailFilter::SentBefore(UTCDate::from_timestamp(200)),
                vec![1],
            ),
            (
                EmailFilter::SentAfter(UTCDate::from_timestamp(200)),
                vec![3],
            ),
            (EmailFilter::HasAttachment(true), vec![2]),
            (EmailFilter::HasAttachment(false), vec![1, 3]),
            (EmailFilter::NotKeyword(Keyword::HasAttachment), vec![1, 3]),
            (EmailFilter::NotKeyword(Keyword::HasNoAttachment), vec![2]),
        ] {
            assert_eq!(
                za_resolve_condition(&condition, &content, &cache, &MessageAccess::All)
                    .unwrap()
                    .iter()
                    .collect::<Vec<_>>(),
                expected
            );
        }
        assert!(
            za_resolve_condition(
                &EmailFilter::InMailbox(Id::from(1u32)),
                &content,
                &cache,
                &MessageAccess::All
            )
            .is_none()
        );
    }

    #[test]
    fn subject_sort_uses_dense_ranks_and_keeps_ties() {
        let content = fixture_content();
        let cache = empty_cache();
        let documents = RoaringBitmap::from([1, 2, 3]);
        let Some(MessageComparator::Search(SearchComparator::SortedSet { set, ascending })) =
            za_sort_comparator(
                &EmailComparator::Subject,
                true,
                &content,
                &cache,
                &documents,
            )
        else {
            panic!("missing subject ranks");
        };
        assert!(ascending);
        assert_eq!((set[&2], set[&3], set[&1]), (1, 1, 2));
    }

    #[test]
    fn current_content_covers_the_same_counter_and_checkpoint() {
        let checkpoint = Checkpoint {
            change_id: 40,
            position: 0,
            complete: true,
        };
        let mut index = LoadedIndex::empty(Manifest {
            indexed_to: checkpoint,
            content_to: ContentTo {
                checkpoint,
                ..Default::default()
            },
            ..Default::default()
        });
        index.scanned_to = 45;
        assert!(za_content_current(&index, 45));
        assert!(!za_content_current(&index, 46));
        index.manifest.content_to.suspended = true;
        assert!(!za_content_current(&index, 45));
    }
}
