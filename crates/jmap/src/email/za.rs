/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 8: the filters and sorts of `Email/query` that need a message's
//! content, which a key account's search index does not hold (spec 5.5: the
//! index holds the arrival time and the size only). `email_query` and
//! `Email/queryChanges` refuse them up front with `unsupportedFilter` or
//! `unsupportedSort` rather than leaving them to the store's silence on a
//! missing field. The matches carry no wildcard arm, so a variant added
//! upstream fails to compile here until it is classified.

use common::{Server, za::ZA_MAIL_SEARCH_REFUSED};
use jmap_proto::{
    method::query::{Comparator, Filter},
    object::email::{EmailComparator, EmailFilter},
};

/// True when answering the filter needs a message's sealed content (header
/// values, body text, attachment presence, the sent date). An unknown
/// property is left to upstream, which names it in its own
/// `unsupportedFilter`.
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
        | EmailFilter::SentAfter(_) => true,
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
        | EmailFilter::Id(_)
        | EmailFilter::_T(_) => false,
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

/// The first `text`, `subject` or `body` condition of a `SearchSnippet/get`
/// filter, wherever it sits in the tree (under `NOT` as well): the
/// properties whose terms the snippets mark.
pub fn first_snippet_term(filters: &[Filter<EmailFilter>]) -> Option<&EmailFilter> {
    filters.iter().find_map(|filter| match filter {
        Filter::Property(
            condition @ (EmailFilter::Text(_) | EmailFilter::Subject(_) | EmailFilter::Body(_)),
        ) => Some(condition),
        _ => None,
    })
}

/// Refuses an `Email/query` or `Email/queryChanges` of a key account whose
/// filter or sort needs content: `unsupportedFilter` or `unsupportedSort`,
/// naming the property and carrying `ZA_MAIL_SEARCH_REFUSED`. The request is
/// read first, so an account whose request needs no content is not looked up.
pub async fn za_refuse_content_query(
    server: &Server,
    account_id: u32,
    filters: &[Filter<EmailFilter>],
    sorts: &[Comparator<EmailComparator>],
) -> trc::Result<()> {
    let filter = first_refused_filter(filters);
    let sort = first_refused_sort(sorts);
    if (filter.is_some() || sort.is_some()) && server.za_is_key_account(account_id).await? {
        if let Some(filter) = filter {
            return Err(trc::JmapEvent::UnsupportedFilter
                .into_err()
                .details(format!("{filter}: {ZA_MAIL_SEARCH_REFUSED}")));
        }
        if let Some(sort) = sort {
            return Err(trc::JmapEvent::UnsupportedSort
                .into_err()
                .details(format!("{sort}: {ZA_MAIL_SEARCH_REFUSED}")));
        }
    }
    Ok(())
}

/// Refuses a `SearchSnippet/get` of a key account whose filter carries a
/// `text`, `subject` or `body` property, even one that tokenises to nothing
/// or sits under `NOT`: `unsupportedFilter`, naming the property. Snippets
/// mark terms in sealed content, which the server does not search.
pub async fn za_refuse_snippet_terms(
    server: &Server,
    account_id: u32,
    filters: &[Filter<EmailFilter>],
) -> trc::Result<()> {
    if let Some(term) = first_snippet_term(filters)
        && server.za_is_key_account(account_id).await?
    {
        return Err(trc::JmapEvent::UnsupportedFilter
            .into_err()
            .details(format!("{term}: {ZA_MAIL_SEARCH_REFUSED}")));
    }
    Ok(())
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

    /// Every `EmailFilter` variant with its verdict (mail spec 8). The table
    /// pins today's verdicts; the compile-time guard against a new variant
    /// is the exhaustive match in `filter_needs_content`.
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
            (EmailFilter::_T("x-unknown".into()), false),
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
    fn first_snippet_term_finds_the_marked_properties_anywhere() {
        let under_not = vec![
            Filter::And,
            Filter::Property(EmailFilter::InMailbox(Id::default())),
            Filter::Not,
            Filter::Property(EmailFilter::Body("x".into())),
            Filter::Close,
            Filter::Close,
        ];
        assert_eq!(
            first_snippet_term(&under_not),
            Some(&EmailFilter::Body("x".into()))
        );
        for term in [
            EmailFilter::Text("!!!".into()),
            EmailFilter::Subject("x".into()),
            EmailFilter::Body("x".into()),
        ] {
            assert_eq!(
                first_snippet_term(&[Filter::Property(term.clone())]),
                Some(&term)
            );
        }
        // The other content properties are not marked in a snippet.
        assert_eq!(
            first_snippet_term(&[
                Filter::Property(EmailFilter::From("x".into())),
                Filter::Property(EmailFilter::HasAttachment(true)),
                Filter::Property(EmailFilter::InMailbox(Id::default())),
            ]),
            None
        );
        assert_eq!(first_snippet_term(&[]), None);
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
