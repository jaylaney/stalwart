/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

pub mod leb128;

// Zero-access fork (upgrade Task 2): 0.16's `base32_custom` codec moved to the
// external `encodify` crate. Its round-trip test keeps its 0.16 module path.
#[cfg(test)]
#[path = "base32_custom_restored_tests.rs"]
mod base32_custom;
