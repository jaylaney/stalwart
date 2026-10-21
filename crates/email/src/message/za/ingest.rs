/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The key-account branch of `email_ingest` (mail spec 6.1, 6.2, 6.7): what
//! a message's DEK is wrapped to, chosen once, before any write.

use super::MailWrap;
use crate::message::ingest::IngestSource;
use common::{Server, auth::AccessToken, za::ZA_TRACE_MAIL_KEYLESS};
use std::sync::Arc;
use trc::AddContext;
use vault::{keys::PUBLIC_KEY_LEN, session::SessionKeys};

/// The wrap of a key account's ingested message.
pub enum IngestWrap {
    /// `mk`: the session's keys (IMAP APPEND, JMAP Email/set and
    /// Email/import).
    Session(Arc<SessionKeys>),
    /// `pk`: the account's served public key (delivery).
    Public([u8; PUBLIC_KEY_LEN]),
}

impl IngestWrap {
    pub fn mail_wrap(&self) -> MailWrap<'_> {
        match self {
            IngestWrap::Session(keys) => MailWrap::Session(keys),
            IngestWrap::Public(public) => MailWrap::Public(public),
        }
    }
}

/// The wrap of a message ingested into the key account `account_id`. A
/// restore re-parses archived bytes and is refused (spec 6.7, a permanent
/// failure). A token holding the account's keys wraps under the mail
/// wrapping key. Otherwise a delivery wraps to the served public key, read
/// fresh from the vault record, and fails temporarily without one (state
/// `PendingSetup`, a missing record), so the queue retries and nothing is
/// stored in the clear; a session write without keys is refused, which the
/// mail login gate and the JMAP dispatcher make unreachable.
pub async fn za_ingest_wrap(
    server: &Server,
    access_token: &AccessToken,
    account_id: u32,
    source: IngestSource<'_>,
) -> trc::Result<IngestWrap> {
    if source == IngestSource::Restore {
        trc::event!(
            Security(trc::SecurityEvent::Unauthorized),
            AccountId = account_id,
            Details = "zero-access: restore of a key account's message",
        );
        return Err(
            trc::EventType::MessageIngest(trc::MessageIngestEvent::Error)
                .ctx(trc::Key::Code, 550)
                .ctx(
                    trc::Key::Reason,
                    "zero-access: restore not supported for key accounts",
                ),
        );
    }

    if let Some(keys) = access_token.za_keys_for(account_id) {
        Ok(IngestWrap::Session(keys.clone()))
    } else if source.is_smtp() {
        server
            .za_served_public_key(account_id)
            .await
            .caused_by(trc::location!())?
            .map(IngestWrap::Public)
            .ok_or_else(|| {
                trc::StoreEvent::UnexpectedError
                    .into_err()
                    .account_id(account_id)
                    .ctx(
                        trc::Key::Reason,
                        "zero-access: delivery without a served public key",
                    )
                    .caused_by(trc::location!())
            })
    } else {
        Err(trc::SecurityEvent::Unauthorized
            .into_err()
            .account_id(account_id)
            .details(ZA_TRACE_MAIL_KEYLESS))
    }
}
