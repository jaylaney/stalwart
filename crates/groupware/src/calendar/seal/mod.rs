/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access sealing of calendar data (spec 6, 7).
//!
//! The re-exports of `collection`, `event` and `tree` are added by Tasks 2-4,
//! when those modules gain content.

pub mod collection;
pub mod event;
pub mod policy;
pub mod tree;
