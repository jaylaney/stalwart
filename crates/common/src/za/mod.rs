/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access helpers shared by the request paths and the background
//! services: the UID creation lease (invitations spec 6.3), the mail login
//! gate, the mail protocols' fixed client texts and trace details (mail
//! spec 4, 8, 10) and, in test builds, the pause and failure hooks of spec
//! 11.

pub mod lease;
pub mod mail;

#[cfg(feature = "test_mode")]
pub mod test_hooks;

/// Mail spec 4: the client text of a mail login refused because it yields
/// no session keys for a key account (OAuth Bearer, API key, master user).
pub const ZA_MAIL_LOGIN_REFUSED: &str =
    "Zero-access accounts log in to mail with the password or an app password.";
/// Mail spec 10: the client text of a content read refused for a token that
/// holds no keys for the key account.
pub const ZA_MAIL_KEYLESS: &str =
    "Mail in this account is encrypted at rest and is not available to this session.";
/// Mail spec 8: the client text of a content search refused on a key account.
pub const ZA_MAIL_SEARCH_REFUSED: &str = "Mail in this account is encrypted at rest; the server cannot search message content. Search your local copy instead.";
/// Mail spec 10: the client text of a sealed message that does not open.
pub const ZA_MAIL_UNREADABLE: &str = "Message could not be read.";
/// Mail spec 4: the trace details of the refused keyless mail login.
pub const ZA_TRACE_MAIL_LOGIN: &str = "zero-access: mail login without session keys";
/// Mail spec 4, 10: the trace details of a content read refused without keys.
pub const ZA_TRACE_MAIL_KEYLESS: &str = "zero-access: mail access without session keys";

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

/// Key pinning spec 6.4: whether `contacts/pin`'s step 5 retirement must
/// fail now (`test_hooks::PIN_WRITE_FAIL_ONCE`, cleared when taken); a
/// forwarder for the HTTP crate's test builds, as above, always `false`
/// unless this crate is built with `test_mode`.
pub fn take_pin_write_fail(account_id: u32) -> bool {
    #[cfg(feature = "test_mode")]
    {
        test_hooks::take_pin_write_fail(account_id)
    }
    #[cfg(not(feature = "test_mode"))]
    {
        let _ = account_id;
        false
    }
}

/// Key pinning spec 6.4: the test-mode override of `contacts/pin`'s batch
/// (`test_hooks::set_redeliver_batch_override`); a forwarder for the HTTP
/// crate's test builds, as above, always `None` unless this crate is built
/// with `test_mode`.
pub fn redeliver_batch_override() -> Option<usize> {
    #[cfg(feature = "test_mode")]
    {
        test_hooks::redeliver_batch_override()
    }
    #[cfg(not(feature = "test_mode"))]
    {
        None
    }
}
