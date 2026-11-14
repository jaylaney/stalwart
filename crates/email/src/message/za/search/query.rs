/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::ensure::Unavailable;
use common::za::search::trace::{ZA_SEARCH_UNAVAILABLE, ZA_SEARCH_UNRESOLVED, za_search_trace};
use store::{
    search::{SearchField, SearchFilter},
    za_search::content::{ContentPart, LeafKind, ResolveError},
};

pub fn za_resolve_residual(
    account_id: u32,
    mut filters: Vec<SearchFilter>,
    content: &ContentPart,
) -> Result<Vec<SearchFilter>, Unavailable> {
    filters.retain(|filter| {
        !matches!(
            filter,
            SearchFilter::Integer {
                field: SearchField::AccountId,
                ..
            }
        )
    });
    content.resolve(&filters).map_err(|error| {
        let reason = match error {
            // Corrupt is returned when reading positions for a text phrase.
            ResolveError::Unresolvable(LeafKind::Text) | ResolveError::Corrupt(_) => "text",
            ResolveError::Unresolvable(LeafKind::KeyValue) => "key-value",
            ResolveError::Unresolvable(LeafKind::Integer) => "integer",
            ResolveError::TooLarge(_) => {
                za_search_trace!(
                    ZA_SEARCH_UNAVAILABLE,
                    AccountId = account_id,
                    Reason = Unavailable::Unresolved.as_str()
                );
                return Unavailable::Unresolved;
            }
        };
        za_search_trace!(
            ZA_SEARCH_UNRESOLVED,
            AccountId = account_id,
            Reason = reason
        );
        Unavailable::Unresolved
    })
}
