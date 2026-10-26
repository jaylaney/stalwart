/*
 * SPDX-FileCopyrightText: 2026 Stalwart contributors
 *
 * SPDX-License-Identifier: AGPL-3.0-only
 */

/// Independent shipping product version. Internal crate versions remain upstream versions.
pub const FORK_VERSION: &str = env!("STALWART_FORK_VERSION");
pub const UPSTREAM_TAG: &str = env!("STALWART_UPSTREAM_TAG");
pub const UPSTREAM_COMMIT: &str = env!("STALWART_UPSTREAM_COMMIT");
pub const SOURCE_COMMIT: &str = env!("STALWART_SOURCE_COMMIT");
/// `clean` or `dirty` for Git checkouts, `provided` or `unknown` for source archives.
pub const SOURCE_STATE: &str = env!("STALWART_SOURCE_STATE");
pub const BUILD_IDENTITY: &str = env!("STALWART_BUILD_IDENTITY");
