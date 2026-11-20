/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 (spec 6.4, D12, the unexpanded form): the text of a key account's events, matched in
//! process memory over the unsealed views of one request that holds the keys (invariant 60). Each
//! event is the one document the Calendar index would hold for it, built by the index builder's
//! own `index_document`; its terms and the planning of each text condition are the internal
//! store's, through release 7's in-memory content part. Nothing is indexed, stored or cached.

use super::ArchivedCalendarEventContent;
use ahash::AHashSet;
use common::Server;
use nlp::language::Language;
use store::{
    search::{SearchField, SearchFilter},
    write::SearchIndex,
    za_search::content::{ContentPart, ContentRecord, ResolveError, SortKeys},
};

/// The text of a set of events as the internal search store would hold it, built in memory.
///
/// It holds plaintext terms for one request: it is neither cloned, serialized nor printed.
pub struct ZaEventTextIndex {
    content: ContentPart,
    index_fields: Option<AHashSet<SearchField>>,
    default_language: Language,
}

impl ZaEventTextIndex {
    /// Reads `core.email.index_fields[&SearchIndex::Calendar]` and `core.email.default_language`.
    /// Without index fields no document is indexed and every text condition matches nothing.
    pub fn new(server: &Server) -> Self {
        Self::with_fields(
            server
                .core
                .email
                .index_fields
                .get(&SearchIndex::Calendar)
                .cloned(),
            server.core.email.default_language,
        )
    }

    /// `None` is a configuration whose index fields have no Calendar entry.
    fn with_fields(
        index_fields: Option<AHashSet<SearchField>>,
        default_language: Language,
    ) -> Self {
        Self {
            content: ContentPart::default(),
            index_fields,
            default_language,
        }
    }

    /// Adds the whole event as one document, as the Calendar index builder would write it.
    pub fn insert(
        &mut self,
        account_id: u32,
        document_id: u32,
        view: &ArchivedCalendarEventContent,
    ) {
        if let Some(index_fields) = &self.index_fields {
            let document =
                view.index_document(account_id, document_id, index_fields, self.default_language);
            self.content.insert(ContentRecord::new(
                document,
                document_id,
                SortKeys::default(),
                None,
                false,
            ));
        }
    }

    /// Every `SearchFilter::Text` becomes the `DocumentSet` of the inserted events that match;
    /// operators, ends and sets pass through. A query past the store's leaf limits is the store's
    /// error. The caller passes no `Integer` leaf.
    pub fn resolve(&self, filters: &[SearchFilter]) -> trc::Result<Vec<SearchFilter>> {
        self.content.resolve(filters).map_err(|err| match err {
            ResolveError::TooLarge(err) => err,
            ResolveError::Unresolvable(_) | ResolveError::Corrupt(_) => {
                trc::StoreEvent::UnexpectedError
                    .into_err()
                    .details("zero-access: calendar text condition not resolved")
                    .caused_by(trc::location!())
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ZaEventTextIndex;
    use crate::calendar::{CalendarEventContent, CalendarEventData};
    use ahash::AHashSet;
    use calcard::{Entry, Parser, common::timezone::Tz};
    use nlp::language::Language;
    use store::{
        Serialize,
        search::{CalendarSearchField, MAX_QUERY_LEAVES, SearchField, SearchFilter},
        write::{Archive, ArchiveBytes, Archiver},
    };

    const EVENT_1: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n",
        "BEGIN:VEVENT\r\nUID:za-text-1\r\nDTSTAMP:20240101T000000Z\r\n",
        "DTSTART:20240102T090000Z\r\nDTEND:20240102T100000Z\r\n",
        "SUMMARY:quarterly budget review\r\n",
        "DESCRIPTION:the committee is reviewing the quarterly budgets for the marketing ",
        "department this afternoon\r\n",
        "LOCATION:room alpha\r\n",
        "ORGANIZER;CN=alice organizer:mailto:alice@example.com\r\n",
        "ATTENDEE;CN=bob guest:mailto:bob@example.com\r\n",
        "ATTENDEE;CN=carol guest:mailto:carol@example.com\r\n",
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    const EVENT_2: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n",
        "BEGIN:VEVENT\r\nUID:za-text-2\r\nDTSTAMP:20240101T000000Z\r\n",
        "DTSTART:20240102T090000Z\r\nDTEND:20240102T093000Z\r\nRRULE:FREQ=DAILY\r\n",
        "SUMMARY:team sync\r\n",
        "DESCRIPTION:the weekly meeting of the whole engineering team to talk about progress\r\n",
        "END:VEVENT\r\n",
        "BEGIN:VEVENT\r\nUID:za-text-2\r\nDTSTAMP:20240101T000000Z\r\n",
        "RECURRENCE-ID:20240104T090000Z\r\n",
        "DTSTART:20240104T090000Z\r\nDTEND:20240104T093000Z\r\n",
        "SUMMARY:offsite planning\r\n",
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    const EVENT_3: &str = concat!(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//t//EN\r\n",
        "BEGIN:VEVENT\r\nUID:za-text-3\r\nDTSTAMP:20240101T000000Z\r\n",
        "DTSTART:20240105T140000Z\r\nDTEND:20240105T150000Z\r\n",
        "SUMMARY:dentist appointment\r\n",
        "DESCRIPTION:a routine visit to the dentist for the yearly cleaning and a check ",
        "of the teeth\r\n",
        "LOCATION:clinic beta\r\n",
        "END:VEVENT\r\nEND:VCALENDAR\r\n"
    );

    /// The stored archive of an event's content, built as the seal tests build it.
    fn stored(ics: &str) -> Archive<ArchiveBytes> {
        let Entry::ICalendar(ical) = Parser::new(ics).entry() else {
            panic!("not an iCalendar object");
        };
        let content = CalendarEventContent {
            data: CalendarEventData::new(ical, Tz::Floating, 100),
            ..Default::default()
        };
        let bytes = Archiver::new(content).serialize().unwrap();
        <Archive<ArchiveBytes> as store::Deserialize>::deserialize(&bytes).unwrap()
    }

    fn all_events() -> Vec<(u32, Archive<ArchiveBytes>)> {
        vec![
            (1, stored(EVENT_1)),
            (2, stored(EVENT_2)),
            (3, stored(EVENT_3)),
        ]
    }

    fn first_event() -> Vec<(u32, Archive<ArchiveBytes>)> {
        vec![(1, stored(EVENT_1))]
    }

    fn index_of(
        events: &[(u32, Archive<ArchiveBytes>)],
        fields: Option<AHashSet<SearchField>>,
        language: Language,
    ) -> ZaEventTextIndex {
        let mut index = ZaEventTextIndex::with_fields(fields, language);
        for (id, stored) in events {
            index.insert(9, *id, stored.unarchive::<CalendarEventContent>().unwrap());
        }
        index
    }

    fn text(field: CalendarSearchField, text: &str, language: Language) -> SearchFilter {
        SearchFilter::has_text(field, text, language)
    }

    fn set(ids: &[u32]) -> SearchFilter {
        SearchFilter::DocumentSet(ids.iter().copied().collect())
    }

    #[test]
    fn text_index_cases() {
        use CalendarSearchField::{Attendee, Description, Location, Owner, Title};
        let (english, none) = (Language::English, Language::None);

        let index = index_of(&all_events(), Some(AHashSet::new()), english);
        for (field, value, language, ids) in [
            // A single word.
            (Title, "budget", english, &[1][..]),
            // Several words: each must match.
            (Title, "budget review", english, &[1]),
            (Title, "budget dentist", english, &[]),
            // One word in the master, one in the override.
            (Title, "team offsite", english, &[2]),
            // A phrase, in order only.
            (Title, "\"budget review\"", english, &[1]),
            (Title, "\"review budget\"", english, &[]),
            // The field is one string: master, then override.
            (Title, "\"sync offsite\"", english, &[2]),
            // A prefix.
            (Title, "quart*", english, &[1]),
            (Title, "udget*", english, &[]),
            // A stem.
            (Title, "appointments", english, &[3]),
            // "reviewing" by its stem.
            (Description, "review", english, &[1]),
            (Description, "marketing department", english, &[1]),
            (Description, "engineering", english, &[2]),
            (Location, "alpha", none, &[1]),
            (Location, "room alpha", none, &[1]),
            (Location, "clinic beta", none, &[3]),
            (Owner, "alice@example.com", none, &[1]),
            // The CN parameter is indexed.
            (Owner, "organizer", none, &[1]),
            (Attendee, "bob@example.com", none, &[1]),
            // A second value of one field.
            (Attendee, "carol@example.com", none, &[1]),
            (Attendee, "bob@example.com carol@example.com", none, &[1]),
            // The organizer is not an attendee.
            (Attendee, "alice@example.com", none, &[]),
        ] {
            assert_eq!(
                index.resolve(&[text(field, value, language)]).unwrap(),
                vec![set(ids)],
                "{field:?} {value:?} {language:?}"
            );
        }

        // Operators, ends and sets pass through, and each text leaf is replaced in place.
        assert_eq!(
            index
                .resolve(&[
                    SearchFilter::Or,
                    text(Title, "budget", english),
                    text(Title, "dentist", english),
                    SearchFilter::End,
                    set(&[7]),
                ])
                .unwrap(),
            vec![
                SearchFilter::Or,
                set(&[1]),
                set(&[3]),
                SearchFilter::End,
                set(&[7]),
            ]
        );
    }

    #[test]
    fn no_index_fields_matches_nothing() {
        let budget = text(CalendarSearchField::Title, "budget", Language::English);
        let review = text(
            CalendarSearchField::Description,
            "review",
            Language::English,
        );

        let index = index_of(&first_event(), None, Language::English);
        assert_eq!(
            index.resolve(std::slice::from_ref(&budget)).unwrap(),
            vec![set(&[])]
        );

        let index = index_of(
            &first_event(),
            Some(AHashSet::from_iter([SearchField::Calendar(
                CalendarSearchField::Title,
            )])),
            Language::English,
        );
        assert_eq!(
            index.resolve(std::slice::from_ref(&budget)).unwrap(),
            vec![set(&[1])]
        );
        assert_eq!(index.resolve(&[review]).unwrap(), vec![set(&[])]);
    }

    #[test]
    fn oversized_query_is_the_stores_error() {
        let index = index_of(&first_event(), Some(AHashSet::new()), Language::English);
        let leaf = text(CalendarSearchField::Title, "budget", Language::English);

        let mut filters = vec![leaf.clone(); MAX_QUERY_LEAVES];
        assert_eq!(
            index.resolve(&filters).unwrap(),
            vec![set(&[1]); MAX_QUERY_LEAVES]
        );

        filters.push(leaf);
        let err = index.resolve(&filters).unwrap_err();
        assert!(
            err.matches(trc::EventType::Limit(trc::LimitEvent::SizeRequest)),
            "{err:?}"
        );
    }
}
