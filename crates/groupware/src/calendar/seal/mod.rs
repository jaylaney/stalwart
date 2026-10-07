/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access sealing of calendar data (spec 6, 7).
//!
//! The re-export of `collection` is added by Task 4, when that module gains
//! content.

pub mod collection;
pub mod event;
pub mod policy;
pub mod tree;

pub use event::{seal_event, unseal_event, unseal_event_archive};
pub use tree::{SealError, seal_error, tree_has_carriers};
