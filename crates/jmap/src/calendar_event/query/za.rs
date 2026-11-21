/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 6 (spec 6.4, D12): the text leaves of a key account's `CalendarEvent/query`,
//! resolved in process memory over the unsealed views of one request that holds the keys
//! (invariant 60). After the call the filter list holds sets only, so no text leaf of a key
//! account reaches the search store. Nothing is indexed, and the query charges no memory
//! budget for the views it opens.

use crate::calendar_event::za_read::ZaEventReader;
use calcard::icalendar::ICalendar;
use common::{GroupwareResources, Server, auth::AccessToken};
use groupware::calendar::{
    CalendarEvent, CalendarEventContent, privacy::ICalendarPrivacy, za_text::ZaEventTextIndex,
};
use store::{
    ValueKey,
    roaring::RoaringBitmap,
    search::SearchFilter,
    write::{Archive, ArchiveBytes, serialize::rkyv_deserialize},
};
use trc::AddContext;
use types::{collection::Collection, field::CalendarEventField};

/// Opens every candidate of `mask`, inserts it in a `ZaEventTextIndex`, and replaces the
/// text leaves of `filters`. Without a text leaf nothing is opened. For a caller that is
/// not a member of the account, an event whose unsealed tree is not public is left out of
/// the index, so it matches no text leaf, as `CalendarEvent/get` shows it (spec 6.3) and as
/// an ordinary account's flagged event matches none (D12; final review FC2-1).
#[allow(clippy::too_many_arguments)]
pub(super) async fn za_resolve_text_leaves(
    server: &Server,
    access_token: &AccessToken,
    reader: &mut ZaEventReader,
    cache: &GroupwareResources,
    account_id: u32,
    is_member: bool,
    mask: &RoaringBitmap,
    filters: &mut Vec<SearchFilter>,
) -> trc::Result<()> {
    if !filters
        .iter()
        .any(|filter| matches!(filter, SearchFilter::Text { .. }))
    {
        return Ok(());
    }

    let mut index = ZaEventTextIndex::new(server);
    for document_id in za_text_candidates(mask, filters) {
        // A missing resource, row or content, or a view this caller cannot read: the
        // event matches no text condition.
        let Some(resource) = cache.item_by_id(document_id) else {
            continue;
        };
        let Some(row) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::archive(
                account_id,
                Collection::CalendarEvent,
                document_id,
            ))
            .await?
        else {
            continue;
        };
        let Some(content) = server
            .store()
            .get_value::<Archive<ArchiveBytes>>(ValueKey::property(
                account_id,
                Collection::CalendarEvent,
                document_id,
                CalendarEventField::Content,
            ))
            .await?
        else {
            continue;
        };
        let calendar_ids = resource
            .child_names()
            .iter()
            .map(|name| name.parent_id)
            .collect::<Vec<_>>();
        if let Some(view) = reader
            .view(
                server,
                access_token,
                document_id,
                &calendar_ids,
                row.unarchive::<CalendarEvent>()
                    .caused_by(trc::location!())?,
                &content,
            )
            .await?
        {
            let content = view
                .content
                .unarchive::<CalendarEventContent>()
                .caused_by(trc::location!())?;
            if !is_member
                && !rkyv_deserialize::<_, ICalendar>(&content.data.event)
                    .caused_by(trc::location!())?
                    .privacy()
                    .is_public()
            {
                continue;
            }
            index.insert(account_id, document_id, content);
        }
    }

    *filters = index.resolve(filters)?;
    Ok(())
}

/// `mask` intersected with every `DocumentSet` leaf whose enclosing groups are all `And`
/// (the top level is one). The whole filter's result is a subset of each such set, and
/// whether an event matches depends on that event alone, so a text leaf resolved over these
/// candidates gives the same result as one resolved over `mask`. An `End` that closes the
/// top level ends the evaluation, as it ends the store's.
fn za_text_candidates(mask: &RoaringBitmap, filters: &[SearchFilter]) -> RoaringBitmap {
    let mut candidates = mask.clone();
    let mut enclosing = Vec::new();
    let mut under_and = true;
    for filter in filters {
        match filter {
            SearchFilter::And => enclosing.push(under_and),
            SearchFilter::Or | SearchFilter::Not => {
                enclosing.push(under_and);
                under_and = false;
            }
            SearchFilter::End => match enclosing.pop() {
                Some(outer) => under_and = outer,
                None => break,
            },
            SearchFilter::DocumentSet(set) if under_and => candidates &= set,
            _ => {}
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::za_text_candidates;
    use nlp::language::Language;
    use store::{
        roaring::RoaringBitmap,
        search::{CalendarSearchField, SearchFilter},
    };

    #[test]
    fn text_candidates_narrow_only_under_and() {
        let mask = RoaringBitmap::from_iter(1..=9);
        let all = mask.clone();
        let t = || SearchFilter::has_text(CalendarSearchField::Title, "word", Language::None);
        let set =
            |ids: &[u32]| SearchFilter::is_in_set(RoaringBitmap::from_iter(ids.iter().copied()));
        use SearchFilter::{And, End, Not, Or};

        for (filters, expected, what) in [
            (vec![t()], all.clone(), "[T]"),
            (
                vec![set(&[1, 2, 30]), t()],
                RoaringBitmap::from_iter([1, 2]),
                "the top level is an AND",
            ),
            (
                vec![And, set(&[1, 2]), t(), End, set(&[2, 3])],
                RoaringBitmap::from_iter([2]),
                "a nested AND",
            ),
            (vec![Or, set(&[1]), t(), End], all.clone(), "an OR"),
            (vec![Not, set(&[1]), End, t()], all.clone(), "a NOT"),
            (
                vec![Or, And, set(&[1]), t(), End, End],
                all.clone(),
                "an AND under an OR does not narrow",
            ),
            (
                vec![And, t(), set(&[4, 5]), End],
                RoaringBitmap::from_iter([4, 5]),
                "a grantee's wrapper narrows to the public events",
            ),
        ] {
            assert_eq!(za_text_candidates(&mask, &filters), expected, "{what}");
        }
    }
}
