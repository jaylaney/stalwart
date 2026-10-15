/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access helpers shared by the request paths and the background
//! services: the UID creation lease (invitations spec 6.3) and, in test
//! builds, the pause and failure hooks of spec 11.

pub mod lease;

#[cfg(feature = "test_mode")]
pub mod test_hooks;
