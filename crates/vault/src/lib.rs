/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

pub mod cache;
pub mod keys;
pub mod record;
pub mod recovery;
pub mod session;

pub use zeroize::Zeroizing;

/// Registry password secret that marks a key account (spec 4.3).
pub const ZA_MARKER: &str = "$za$";
