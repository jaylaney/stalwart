/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access sealing of calendar data (spec 6, 7).

pub mod collection;
pub mod event;
pub mod policy;
pub mod tree;

pub use collection::{
    COLLECTION_MARKER, calendar_is_sealed, seal_calendar, unseal_calendar, unseal_calendar_archive,
};
pub use event::{seal_event, unseal_event, unseal_event_archive};
pub use tree::{SealError, seal_error, tree_has_carriers};
