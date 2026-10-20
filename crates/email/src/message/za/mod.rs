/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access sealing of mail (mail spec 3, 5).

pub mod seal;

pub use seal::*;

/// The format byte at the head of a sealed blob and of a stub's bundle (mail
/// spec 5.1, 5.2).
pub const MAIL_FORMAT_V1: u8 = 1;
/// Associated-data purpose of a message's key envelope (mail spec 5.3).
pub const AAD_MAIL_KEY: &str = "mail-key";
/// Associated-data purpose of a sealed blob's body (mail spec 5.3).
pub const AAD_MAIL_BLOB: &str = "mail-blob";
/// Associated-data purpose of a stub's metadata bundle (mail spec 5.3).
pub const AAD_MAIL_META: &str = "mail-meta";
