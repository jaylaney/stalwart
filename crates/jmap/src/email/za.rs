/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Classifiers for the content view required by an Email/query.

use jmap_proto::{
    method::query::{Comparator, Filter},
    object::email::{EmailComparator, EmailFilter, EmailQueryFilter},
};
use types::keyword::Keyword;

/// True when answering the filter needs a message's sealed content (header
/// values, body text, attachment presence, the attachment keywords, the sent
/// date). An unknown
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
        // Ruling 55: 1.0 sets `$hasattachment` and `$hasnoattachment` from
        // the parsed message; a key account's message carries neither, only
        // what its client stored, so a condition on them would answer only
        // the client's flags. Refused as `hasAttachment` and IMAP's
        // `KEYWORD`/`UNKEYWORD` on the same flags are.
        EmailFilter::AllInThreadHaveKeyword(keyword)
        | EmailFilter::SomeInThreadHaveKeyword(keyword)
        | EmailFilter::NoneInThreadHaveKeyword(keyword)
        | EmailFilter::HasKeyword(keyword)
        | EmailFilter::NotKeyword(keyword) => is_attachment_keyword(keyword),
        EmailFilter::InMailbox(_)
        | EmailFilter::InMailboxOtherThan(_)
        | EmailFilter::Before(_)
        | EmailFilter::After(_)
        | EmailFilter::MinSize(_)
        | EmailFilter::MaxSize(_)
        | EmailFilter::InThread(_)
        | EmailFilter::Id(_)
        | EmailFilter::_T(_) => false,
    }
}

/// True when answering an `Email/query` condition needs what a key
/// account's message does not hold in the clear: an `EmailFilter` that
/// needs content (`filter_needs_content`), or, new at 1.0, any condition on
/// the message's object metadata (`metadataExists`, `metadataTextContains`,
/// `metadataTextEquals` and their private forms). A key account's message
/// is ingested without object metadata (`za_metadata_refusal`), so such a
/// filter would answer only silently; it is refused as the attachment
/// keywords are (ruling 55).
pub fn query_filter_needs_content(filter: &EmailQueryFilter) -> bool {
    match filter {
        EmailQueryFilter::Email(filter) => filter_needs_content(filter),
        EmailQueryFilter::Metadata(_) => true,
    }
}

/// True when ordering by the property needs a message's sealed content (the
/// sender, recipients, subject and sent date, and the attachment keywords).
pub fn sort_needs_content(comparator: &EmailComparator) -> bool {
    match comparator {
        EmailComparator::From
        | EmailComparator::To
        | EmailComparator::Cc
        | EmailComparator::Subject
        | EmailComparator::SentAt => true,
        // Ruling 55, as for the keyword filters above.
        EmailComparator::HasKeyword(keyword)
        | EmailComparator::AllInThreadHaveKeyword(keyword)
        | EmailComparator::SomeInThreadHaveKeyword(keyword) => is_attachment_keyword(keyword),
        EmailComparator::ReceivedAt
        | EmailComparator::Size
        | EmailComparator::ThreadId
        | EmailComparator::_T(_) => false,
    }
}

/// Ruling 55: whether `keyword` is one of the attachment keywords the server
/// sets at ingest (`$hasattachment`, `$hasnoattachment`), in any case. The
/// parser maps either name to its variant whatever its case, and keeps any
/// other keyword lowercase; an `Other` holding one of the names (built
/// without the parser) is matched the same way.
fn is_attachment_keyword(keyword: &Keyword) -> bool {
    attachment_keyword(keyword).is_some()
}

pub(super) fn attachment_keyword(keyword: &Keyword) -> Option<bool> {
    let name = keyword.as_str();
    if name.eq_ignore_ascii_case(Keyword::HasAttachment.as_str()) {
        Some(true)
    } else if name.eq_ignore_ascii_case(Keyword::HasNoAttachment.as_str()) {
        Some(false)
    } else {
        None
    }
}

/// The first filter condition of the request that needs content. Operators
/// (`AND`, `OR`, `NOT`, the closing mark) are not conditions; a refused
/// condition anywhere in the tree refuses the whole request.
pub fn first_refused_filter(filters: &[Filter<EmailQueryFilter>]) -> Option<&EmailQueryFilter> {
    filters.iter().find_map(|filter| match filter {
        Filter::Property(condition) if query_filter_needs_content(condition) => Some(condition),
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
    use jmap_proto::{
        object::metadata::{MetadataFilter, MetadataRoot},
        types::date::UTCDate,
    };
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
        .into_iter()
        .chain(attachment_keywords().into_iter().flat_map(|keyword| {
            [
                EmailFilter::HasKeyword(keyword.clone()),
                EmailFilter::NotKeyword(keyword.clone()),
                EmailFilter::AllInThreadHaveKeyword(keyword.clone()),
                EmailFilter::SomeInThreadHaveKeyword(keyword.clone()),
                EmailFilter::NoneInThreadHaveKeyword(keyword),
            ]
            .map(|filter| (filter, true))
        }))
        .collect()
    }

    /// Ruling 55: the attachment keywords in the forms a request can carry
    /// (parsed in any case, and an `Other` holding a name in mixed case).
    fn attachment_keywords() -> Vec<Keyword> {
        vec![
            Keyword::HasAttachment,
            Keyword::HasNoAttachment,
            Keyword::parse("$HasAttachment"),
            Keyword::parse("$HASNOATTACHMENT"),
            Keyword::Other("$HasAttachment".into()),
            Keyword::Other("$hasnoattachment".into()),
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
        .into_iter()
        .chain(attachment_keywords().into_iter().flat_map(|keyword| {
            [
                EmailComparator::HasKeyword(keyword.clone()),
                EmailComparator::AllInThreadHaveKeyword(keyword.clone()),
                EmailComparator::SomeInThreadHaveKeyword(keyword),
            ]
            .map(|property| (property, true))
        }))
        .collect()
    }

    #[test]
    fn attachment_keywords_parse_to_their_variants() {
        // The parser ignores case for both names, so a request's
        // `$HasAttachment` reaches the classification as the variant.
        assert_eq!(Keyword::parse("$HasAttachment"), Keyword::HasAttachment);
        assert_eq!(Keyword::parse("$HASNOATTACHMENT"), Keyword::HasNoAttachment);
        // Neighbouring keywords stay ordinary.
        for other in [
            Keyword::Other("$hasattachments".into()),
            Keyword::HasMemo,
            Keyword::parse("$attachment"),
        ] {
            assert!(!is_attachment_keyword(&other), "{other:?}");
        }
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
    fn query_filter_table() {
        for (filter, needs_content) in filters() {
            assert_eq!(
                query_filter_needs_content(&EmailQueryFilter::Email(filter.clone())),
                needs_content,
                "{filter}"
            );
        }
        // Every object-metadata condition is refused for a key account.
        let metadata = MetadataFilter::Invalid {
            root: MetadataRoot::Shared,
            name: "metadataExists",
            reason: "test",
        };
        assert!(query_filter_needs_content(&EmailQueryFilter::Metadata(
            metadata
        )));
    }

    fn email(filter: EmailFilter) -> Filter<EmailQueryFilter> {
        Filter::Property(EmailQueryFilter::Email(filter))
    }

    #[test]
    fn first_refused_walks_the_filter_tree() {
        let tree = vec![
            Filter::And,
            email(EmailFilter::InMailbox(Id::default())),
            Filter::Not,
            email(EmailFilter::Subject("x".into())),
            Filter::Close,
            email(EmailFilter::From("y".into())),
            Filter::Close,
        ];
        assert_eq!(
            first_refused_filter(&tree),
            Some(&EmailQueryFilter::Email(EmailFilter::Subject("x".into())))
        );
        assert_eq!(first_refused_filter(&[]), None);
        assert_eq!(
            first_refused_filter(&[
                Filter::Or,
                email(EmailFilter::MinSize(1)),
                email(EmailFilter::HasKeyword(keyword())),
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
