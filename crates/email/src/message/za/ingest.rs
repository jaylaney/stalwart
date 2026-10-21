/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The key-account branch of `email_ingest` (mail spec 5, 6.1, 6.2, 6.7):
//! what a message's DEK is wrapped to, chosen once, before any write; the
//! sealed blob staged in place of the queue's or the upload's; the stub
//! written in place of the metadata archive.

use super::{MailWrap, new_dek, seal_mail_blob, seal_mail_envelope, seal_stub};
use crate::message::{
    index::{IndexMessage, build_message_metadata},
    ingest::IngestSource,
    metadata::MessageData,
};
use common::{
    Server,
    auth::AccessToken,
    za::{ZA_MAIL_KEYLESS, ZA_TRACE_MAIL_KEYLESS},
};
use groupware::calendar::seal::seal_error;
use registry::schema::{
    enums::IndexDocumentType,
    structs::{Task, TaskIndexDocument, TaskStatus},
};
use std::sync::Arc;
use store::write::{BatchBuilder, BlobOp};
use trc::AddContext;
use types::{blob_hash::BlobHash, collection::Collection};
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
        Err(trc::JmapEvent::Forbidden
            .into_err()
            .account_id(account_id)
            .details(ZA_MAIL_KEYLESS)
            .code("NOPERM"))
    }
}

/// The key material of a message sealed for a key account, kept from the
/// staging of its blob to the writing of its stub.
pub struct ZaSealed {
    dek: Secret,
    envelope: Vec<u8>,
    account_id: u32,
    blob_hash: BlobHash,
}

/// Mail spec 5.1, 6.1 step 3: a fresh DEK wrapped as `wrap` and the raw
/// message sealed into a new blob, staged as upstream stages a blob (a
/// 60-second temporary link); never the queue's or the upload's blob.
/// Returns the staged hash and hold as the ordinary staging does.
pub async fn za_stage_sealed(
    server: &Server,
    wrap: &IngestWrap,
    account_id: u32,
    raw_message: &[u8],
) -> trc::Result<(ZaSealed, (BlobHash, Option<BlobOp>))> {
    let dek = new_dek();
    let envelope = seal_mail_envelope(&dek, wrap.mail_wrap(), account_id).map_err(|err| {
        // `seal_error`'s shape: the message has no document id yet.
        trc::StoreEvent::DataCorruption
            .into_err()
            .details(format!("zero-access sealing error: {err}"))
            .account_id(account_id)
            .caused_by(trc::location!())
    })?;
    let sealed = seal_mail_blob(&dek, &envelope, account_id, raw_message);
    let (blob_hash, hold) = server
        .put_temporary_blob(account_id, &sealed, 60)
        .await
        .caused_by(trc::location!())?;
    Ok((
        ZaSealed {
            dek,
            envelope,
            account_id,
            blob_hash: blob_hash.clone(),
        },
        (blob_hash, Some(hold)),
    ))
}

/// Mail spec 5.2, 5.4: the message's full metadata, whose `blob_hash` is the
/// sealed blob's, sealed into the stub and written with `data` as
/// `index_message` writes the metadata archive, without a threading key;
/// the message's index task is scheduled as for any message.
#[allow(clippy::too_many_arguments)]
pub fn za_index_stub<'x>(
    batch: &mut BatchBuilder,
    sealed: ZaSealed,
    tenant_id: Option<u32>,
    document_id: u32,
    message: mail_parser::Message<'x>,
    extra_headers: Vec<u8>,
    extra_headers_parsed: Vec<mail_parser::Header<'x>>,
    data: MessageData,
    received_at: u64,
) -> trc::Result<()> {
    let ZaSealed {
        dek,
        envelope,
        account_id,
        blob_hash,
    } = sealed;
    let stub = seal_stub(
        build_message_metadata(
            message,
            extra_headers,
            extra_headers_parsed,
            blob_hash,
            received_at,
        ),
        &dek,
        &envelope,
        account_id,
    )
    .map_err(|err| seal_error(err, account_id, document_id))?;
    batch
        .with_collection(Collection::Email)
        .with_document(document_id)
        .index_metadata(tenant_id, stub, data)
        .caused_by(trc::location!())?
        .schedule_task(Task::IndexDocument(TaskIndexDocument {
            account_id: account_id.into(),
            document_id: document_id.into(),
            document_type: IndexDocumentType::Email,
            status: TaskStatus::now(),
        }));
    Ok(())
}
