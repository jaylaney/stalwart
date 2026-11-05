/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The key-account branch of `email_ingest_into` (mail spec 5, 6.1, 6.2,
//! 6.7): what a message's DEK is wrapped to, chosen once, before any write;
//! the sealed blob staged in place of the queue's or the upload's; the stub
//! written in place of the metadata row.

use super::{
    MailWrap, new_dek, seal_mail_blob, seal_mail_envelope, seal_stub, view::za_sealing_error,
};
use crate::message::{
    ingest::IngestSource,
    messagedata::PendingMessageData,
    metadata::{ExtraHeaders, MessageMetadata},
    sortkeys::MessageSortKeys,
};
use common::{
    Server,
    auth::AccessToken,
    storage::{blob::BlobHold, index::ObjectIndexBuilder},
    za::{ZA_TRACE_MAIL_KEYLESS, mail::za_keyless_refusal},
};
use registry::schema::enums::CompressionAlgo;
use std::sync::Arc;
use store::write::{
    BatchBuilder, BlobLink, BlobOp, PendingId, blob::BlobLinkValue, blob_codec::BlobOpts,
};
use trc::AddContext;
use types::{blob_hash::BlobHash, field::EmailField};
use vault::{
    keys::{PUBLIC_KEY_LEN, Secret},
    session::SessionKeys,
};

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
/// stored in the clear; a session write without keys is refused with an
/// error every caller writes: IMAP answers `NO [NOPERM]` and JMAP
/// `forbidden`, with `ZA_MAIL_KEYLESS` (`SecurityEvent` errors are never
/// written, which would leave an IMAP client waiting).
///
/// `email_ingest_into` calls it before any operation of its batch, so a
/// refusal leaves a group's batch as it was (the restore refusal is a
/// rejection, which a group keeps its batch after; the other two are not,
/// and a group discards its batch after them).
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
        trc::event!(
            Security(trc::SecurityEvent::Unauthorized),
            AccountId = account_id,
            Details = ZA_TRACE_MAIL_KEYLESS,
        );
        Err(za_keyless_refusal().account_id(account_id))
    }
}

/// Mail spec 5.4, ruling 59: the thread an `IngestGroup` records a queued
/// message under, for the group's later messages to join, if any. A key
/// account has no threading, so its message is recorded under no thread
/// key, neither its own thread nor a shared one, and takes no part in the
/// group's pending threads: a plain reply in the same group would otherwise
/// find two threads under its key and join neither.
pub fn za_group_thread(za_key: bool, thread_id: PendingId) -> Option<PendingId> {
    (!za_key).then_some(thread_id)
}

/// Object metadata (an IMAP METADATA or JMAP metadata container) is not
/// supported on a key account's message (its containers are refused there,
/// and a container would hold its value in the clear): a rejection, raised
/// before the batch's first operation.
pub fn za_metadata_refusal(account_id: u32) -> trc::Error {
    trc::EventType::MessageIngest(trc::MessageIngestEvent::Error)
        .ctx(trc::Key::Code, 550)
        .ctx(
            trc::Key::Reason,
            "zero-access: object metadata not supported for key accounts",
        )
        .account_id(account_id)
}

/// The key material of a message sealed for a key account, kept from the
/// staging of its blob to the writing of its stub.
pub struct ZaSealed {
    dek: Secret,
    envelope: Vec<u8>,
    account_id: u32,
    blob_hash: BlobHash,
    sealed_len: usize,
}

/// Mail spec 5.1, 6.1 step 3: a fresh DEK wrapped as `wrap` and the raw
/// message sealed into a new blob, held as upstream holds a blob (a
/// 60-second temporary link); never the queue's or the upload's blob.
/// Returns the held hash and hold as the ordinary staging does.
pub async fn za_stage_sealed(
    server: &Server,
    wrap: &IngestWrap,
    account_id: u32,
    raw_message: &[u8],
) -> trc::Result<(ZaSealed, (BlobHash, Option<BlobHold>))> {
    let dek = new_dek();
    // The message has no document id yet.
    let envelope = seal_mail_envelope(&dek, wrap.mail_wrap(), account_id)
        .map_err(|err| za_sealing_error(err, account_id))?;
    let sealed = seal_mail_blob(&dek, &envelope, account_id, raw_message);
    let (blob_hash, hold) = server
        .hold_blob(
            account_id,
            BlobHash::generate(&sealed),
            &sealed,
            60,
            &BlobOpts::new(
                CompressionAlgo::None,
                server.core.blob.email.encryption.clone(),
            ),
        )
        .await
        .caused_by(trc::location!())?;
    Ok((
        ZaSealed {
            dek,
            envelope,
            account_id,
            blob_hash: blob_hash.clone(),
            sealed_len: sealed.len(),
        },
        (blob_hash, Some(hold)),
    ))
}

/// Mail spec 5.2, 5.4: the message's full metadata row, whose blob hash is
/// the sealed blob's, sealed into the stub and written with `data` as
/// upstream's `index_message` writes the row: the blob link (to the sealed
/// blob, at its length), `data` and the stub row, with sort keys that hold
/// nothing and without the attachment keywords. The caller has created the
/// document, as for `index_message`, and queues its index entry as for any
/// message; it writes no threading key.
///
/// Recorded limit: an `enterprise` build accounts a key account's message to
/// no tenant (the tenant setter is enterprise-only code the fork does not
/// name), the SEL disposition of rulings 26 and 45; the product has no
/// tenants.
pub fn za_index_stub(
    batch: &mut BatchBuilder,
    sealed: ZaSealed,
    message: &mail_parser::Message<'_>,
    extra_headers: &ExtraHeaders,
    data: PendingMessageData,
) -> trc::Result<()> {
    let ZaSealed {
        dek,
        envelope,
        account_id,
        blob_hash,
        sealed_len,
    } = sealed;
    let stub = seal_stub(
        MessageMetadata::build(message, extra_headers, blob_hash.clone()),
        &dek,
        &envelope,
        account_id,
        sealed_len,
    )
    .map_err(|err| za_sealing_error(err, account_id))?;
    batch
        .set(
            BlobOp::Link {
                hash: blob_hash,
                to: BlobLink::Document,
            },
            BlobLinkValue::new(sealed_len),
        )
        .custom(ObjectIndexBuilder::<(), _>::new().with_changes(data))
        .caused_by(trc::location!())?
        .set(EmailField::SortKeys, MessageSortKeys::default().serialize())
        .set(EmailField::Metadata, stub);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mail spec 6.1: object metadata offered with a key account's message
    /// is refused as a permanent ingest error (550), whose fixed reason and
    /// account id are all it carries.
    #[test]
    fn object_metadata_is_refused_for_a_key_account() {
        let error = za_metadata_refusal(77);
        assert!(error.matches(trc::EventType::MessageIngest(
            trc::MessageIngestEvent::Error
        )));
        assert_eq!(error.value_as_uint(trc::Key::Code), Some(550));
        assert_eq!(
            error.value_as_str(trc::Key::Reason),
            Some("zero-access: object metadata not supported for key accounts")
        );
        assert_eq!(error.value_as_uint(trc::Key::AccountId), Some(77));
        assert_eq!(error.keys().len(), 3, "{:?}", error.keys());
    }
}
