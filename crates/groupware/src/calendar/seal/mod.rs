/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access sealing of calendar data (spec 6, 7).

pub mod collection;
pub mod event;
pub mod item;
pub mod policy;
pub mod token;
pub mod tree;

pub use collection::{
    COLLECTION_MARKER, calendar_is_sealed, seal_calendar, unseal_calendar, unseal_calendar_archive,
};
pub use event::{
    archived_event_is_sealed, event_token_pairs, seal_event, unseal_event, unseal_event_archive,
    unseal_event_pairs,
};
pub use item::{
    AAD_ITIP_KEY, ITIP_SCOPE, PendingCarriers, RSVP_TOKEN_PROP, archived_item_is_pending,
    item_is_sealed, open_drained_item, open_pending_item, seal_drained_item, seal_pending_item,
    unseal_item_archive,
};
pub use token::{TOKEN_LEN, TokenPairs, tokens_equal};
pub use tree::{SealError, seal_error, tree_has_carriers};
