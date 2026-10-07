/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access sealing of calendar data (spec 6, 7).
//!
//! The re-exports of `collection` and `event` are added by Tasks 3-4, when
//! those modules gain content.

pub mod collection;
pub mod event;
pub mod policy;
pub mod tree;

pub use tree::{SealError, seal_error, tree_has_carriers};
