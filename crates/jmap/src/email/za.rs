/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 8: the filters and sorts of `Email/query` that need a message's
//! content, which a key account's search index does not hold (spec 5.5; the
//! stub carries the arrival time and the size only). `email_query` refuses
//! them up front with `unsupportedFilter` or `unsupportedSort` rather than
//! leaving them to the store's silence on a missing field. The matches carry
//! no wildcard arm, so a variant added upstream fails to compile here until
//! it is classified.

use jmap_proto::{
    method::query::{Comparator, Filter},
    object::email::{EmailComparator, EmailFilter},
};

/// True when answering the filter needs a message's sealed content (header
/// values, body text, attachment presence, the sent date) or is unknown, in
/// which case it is refused as well.
pub fn filter_needs_content(filter: &EmailFilter) -> bool {
    match filter {
        EmailFilter::Text(_)
        | EmailFilter::From(_)
        | EmailFilter::To(_)
        | EmailFilter::Cc(_)
        | EmailFilter::Bcc(_)
        | EmailFilter::Subject(_)
        | EmailFilter::Body(_)
        | EmailFilter::Header(_)
        | EmailFilter::HasAttachment(_)
        | EmailFilter::SentBefore(_)
        | EmailFilter::SentAfter(_)
        | EmailFilter::_T(_) => true,
        EmailFilter::InMailbox(_)
        | EmailFilter::InMailboxOtherThan(_)
        | EmailFilter::Before(_)
        | EmailFilter::After(_)
        | EmailFilter::MinSize(_)
        | EmailFilter::MaxSize(_)
        | EmailFilter::AllInThreadHaveKeyword(_)
        | EmailFilter::SomeInThreadHaveKeyword(_)
        | EmailFilter::NoneInThreadHaveKeyword(_)
        | EmailFilter::HasKeyword(_)
        | EmailFilter::NotKeyword(_)
        | EmailFilter::InThread(_)
        | EmailFilter::Id(_) => false,
    }
}

/// True when ordering by the property needs a message's sealed content.
pub fn sort_needs_content(comparator: &EmailComparator) -> bool {
    match comparator {
        EmailComparator::From
        | EmailComparator::To
        | EmailComparator::Cc
        | EmailComparator::Subject
        | EmailComparator::SentAt => true,
        EmailComparator::ReceivedAt
        | EmailComparator::Size
        | EmailComparator::HasKeyword(_)
        | EmailComparator::AllInThreadHaveKeyword(_)
        | EmailComparator::SomeInThreadHaveKeyword(_)
        | EmailComparator::ThreadId
        | EmailComparator::_T(_) => false,
    }
}

/// The first filter condition of the request that needs content. Operators
/// (`AND`, `OR`, `NOT`, the closing mark) are not conditions; a refused
/// condition anywhere in the tree refuses the whole request.
pub fn first_refused_filter(filters: &[Filter<EmailFilter>]) -> Option<&EmailFilter> {
    filters.iter().find_map(|filter| match filter {
        Filter::Property(condition) if filter_needs_content(condition) => Some(condition),
        _ => None,
    })
}

/// The first sort property of the request that needs content.
pub fn first_refused_sort(sorts: &[Comparator<EmailComparator>]) -> Option<&EmailComparator> {
    sorts
        .iter()
        .map(|sort| &sort.property)
        .find(|property| sort_needs_content(property))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jmap_proto::types::date::UTCDate;
    use types::{id::Id, keyword::Keyword};

    fn keyword() -> Keyword {
        Keyword::Seen
    }

    fn sort(property: EmailComparator) -> Comparator<EmailComparator> {
        Comparator {
            is_ascending: true,
            collation: None,
            property,
        }
    }

    /// Every `EmailFilter` variant with its verdict (spec 8). The match in
    /// `filter_needs_content` has no wildcard, so a new variant fails to
    /// compile there; this table fails when one is added without a row.
    fn filters() -> Vec<(EmailFilter, bool)> {
        vec![
            (EmailFilter::InMailbox(Id::default()), false),
            (EmailFilter::InMailboxOtherThan(vec![Id::default()]), false),
            (EmailFilter::Before(UTCDate::default()), false),
            (EmailFilter::After(UTCDate::default()), false),
            (EmailFilter::MinSize(1), false),
            (EmailFilter::MaxSize(1), false),
            (EmailFilter::AllInThreadHaveKeyword(keyword()), false),
            (EmailFilter::SomeInThreadHaveKeyword(keyword()), false),
            (EmailFilter::NoneInThreadHaveKeyword(keyword()), false),
            (EmailFilter::HasKeyword(keyword()), false),
            (EmailFilter::NotKeyword(keyword()), false),
            (EmailFilter::HasAttachment(true), true),
            (EmailFilter::From("x".into()), true),
            (EmailFilter::To("x".into()), true),
            (EmailFilter::Cc("x".into()), true),
            (EmailFilter::Bcc("x".into()), true),
            (EmailFilter::Subject("x".into()), true),
            (EmailFilter::Body("x".into()), true),
            (EmailFilter::Header(vec!["X-Private".into()]), true),
            (EmailFilter::Text("x".into()), true),
            (EmailFilter::SentBefore(UTCDate::default()), true),
            (EmailFilter::SentAfter(UTCDate::default()), true),
            (EmailFilter::InThread(Id::default()), false),
            (EmailFilter::Id(vec![Id::default()]), false),
            (EmailFilter::_T("x-unknown".into()), true),
        ]
    }

    fn sorts() -> Vec<(EmailComparator, bool)> {
        vec![
            (EmailComparator::ReceivedAt, false),
            (EmailComparator::Size, false),
            (EmailComparator::From, true),
            (EmailComparator::To, true),
            (EmailComparator::Subject, true),
            (EmailComparator::Cc, true),
            (EmailComparator::SentAt, true),
            (EmailComparator::ThreadId, false),
            (EmailComparator::HasKeyword(keyword()), false),
            (EmailComparator::AllInThreadHaveKeyword(keyword()), false),
            (EmailComparator::SomeInThreadHaveKeyword(keyword()), false),
            (EmailComparator::_T("x-unknown".into()), false),
        ]
    }

    #[test]
    fn filter_table() {
        for (filter, needs_content) in filters() {
            assert_eq!(filter_needs_content(&filter), needs_content, "{filter}");
        }
    }

    #[test]
    fn sort_table() {
        for (property, needs_content) in sorts() {
            assert_eq!(sort_needs_content(&property), needs_content, "{property}");
        }
    }

    #[test]
    fn first_refused_walks_the_filter_tree() {
        let tree = vec![
            Filter::And,
            Filter::Property(EmailFilter::InMailbox(Id::default())),
            Filter::Not,
            Filter::Property(EmailFilter::Subject("x".into())),
            Filter::Close,
            Filter::Property(EmailFilter::From("y".into())),
            Filter::Close,
        ];
        assert_eq!(
            first_refused_filter(&tree),
            Some(&EmailFilter::Subject("x".into()))
        );
        assert_eq!(first_refused_filter(&[]), None);
        assert_eq!(
            first_refused_filter(&[
                Filter::Or,
                Filter::Property(EmailFilter::MinSize(1)),
                Filter::Property(EmailFilter::HasKeyword(keyword())),
                Filter::Close,
            ]),
            None
        );
    }

    #[test]
    fn first_refused_sort_names_the_first_content_sort() {
        let sorts = vec![
            sort(EmailComparator::ReceivedAt),
            sort(EmailComparator::Subject),
            sort(EmailComparator::From),
        ];
        assert_eq!(first_refused_sort(&sorts), Some(&EmailComparator::Subject));
        assert_eq!(first_refused_sort(&[]), None);
        assert_eq!(
            first_refused_sort(&[sort(EmailComparator::Size), sort(EmailComparator::ThreadId)]),
            None
        );
    }
}
