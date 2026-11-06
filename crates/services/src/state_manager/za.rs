/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::ipc::PushNotification;

/// Preserve delivery for a configured EmailPush subscriber whose types
/// exclude the state change that replaces the key account's EmailPush.
pub(super) fn za_email_push_fallback(
    za_key: bool,
    email_push_configured: bool,
    candidate: &PushNotification,
) -> Option<PushNotification> {
    (za_key && email_push_configured).then(|| candidate.clone())
}
