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

/// Key pinning spec 6.4: `contacts/pin`'s pause point after its last event
/// commit, before the step 5 retirement (`test_hooks::REDELIVER_CLEANUP_PAUSE`).
/// The HTTP crate calls it from its own test builds; it pauses only when
/// this crate is built with `test_mode` and is a no-op otherwise, so the
/// HTTP crate's test build does not need this crate's test mode (which
/// does not build without `enterprise`).
pub async fn redeliver_cleanup_pause_point(account_id: u32) {
    #[cfg(feature = "test_mode")]
    test_hooks::redeliver_cleanup_pause_point(account_id).await;
    #[cfg(not(feature = "test_mode"))]
    let _ = account_id;
}
