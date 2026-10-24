/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Collection-key rings for zero-access calendar sharing.

pub mod format;

pub mod resolve;

#[cfg(feature = "test_mode")]
pub mod test;

pub mod boundary;

pub mod convert;
