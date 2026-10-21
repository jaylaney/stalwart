/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 5.2, 7, 10: the unsealed view a reader takes of a key
//! account's message. A reader reads the stored archive as today, obtains
//! the account's keys once per operation (`za_mail_keys`), replaces the
//! archive with `za_metadata_view` and opens the blob it reads with
//! `za_open_message_blob`. A plaintext archive (an ordinary account's, or a
//! key account's pre-existing message, spec 6.6) passes through unchanged.
//! A message whose stub or blob does not open is unreadable: a
//! `DataCorruption` the reader turns into its protocol's per-message failure
//! (`za_readable`).

use super::{
    MailWrap, blob_needs_private_key, is_stub, new_dek, open_mail_blob, open_stub, seal_mail_blob,
    seal_mail_envelope, stub_needs_private_key,
};
use crate::message::metadata::MessageMetadata;
use common::{
    Server,
    auth::AccessToken,
    za::{ZA_TRACE_MAIL_COPY, ZA_TRACE_MAIL_KEYLESS, mail::za_keyless_refusal},
};
use groupware::calendar::seal::{SealError, seal_error};
use std::{borrow::Cow, future::Future, sync::Arc};
use store::{
    Deserialize, Serialize,
    write::{AlignedBytes, Archive, ArchiveVersion, Archiver},
};
use trc::AddContext;
use types::{
    blob::{BlobClass, BlobId},
    collection::Collection,
};
use vault::{keys::Secret, session::SessionKeys};
/// The opened blob's buffer, for readers outside this crate.
pub use zeroize::Zeroizing;

/// Mail spec 10: the trace reason of a message whose stub or blob does not
/// open.
pub const ZA_REASON_UNREADABLE: &str = "zero-access: unreadable message";
/// Mail spec 5.7, 9: the trace details of a refused raw read of a sealed
/// source (`za_refuse_raw_source`).
pub const ZA_TRACE_RAW_SOURCE: &str = "zero-access: raw read of a sealed source";

pub trait ZaMailView: Sync + Send {
    /// The keys a read of `account_id`'s mail uses: `Some` when the token
    /// holds that account's keys, `None` only when it is not a key account
    /// (the view and the opener rely on it). Otherwise the read is refused,
    /// traced as `Security(Unauthorized)` with `ZA_TRACE_MAIL_KEYLESS`, with
    /// the error every caller writes (ruling R22: IMAP `NO [NOPERM]`, POP3
    /// `-ERR`, JMAP `forbidden`, all with `ZA_MAIL_KEYLESS`).
    fn za_mail_keys(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> impl Future<Output = trc::Result<Option<Arc<SessionKeys>>>> + Send;

    /// Mail spec 7: the metadata archive a reader uses in place of
    /// `stored`. `keys` is `za_mail_keys`'s answer for the account, so
    /// `None` means an ordinary account: the stub check runs only on key
    /// accounts' archives (spec 5.2, ruling R29), and without keys `stored`
    /// is borrowed as it is, whatever its first byte. With keys, a
    /// plaintext archive (spec 6.6) is borrowed too, and a stub is opened
    /// (the account's private key unwrapped only for a `pk` envelope) into
    /// an owned archive of the full metadata; one that does not open is
    /// unreadable. Contract: the result is `Cow::Owned` exactly when the
    /// stored archive is a stub that was opened, so a reader passes
    /// `matches!(view, Cow::Owned(_))` to `za_open_message_blob`.
    ///
    /// As `unseal_event_archive`'s view, this one is read-only: it is never
    /// written back, never passed to a write as its current value
    /// (`AssertValue`) and never cached; writes start from the stored stub.
    /// Unlike the calendar view it does not carry the stored `version`: it
    /// is `Unversioned`, so passing it to a write as the current value fails
    /// the archive assertion instead of writing over the stub.
    fn za_metadata_view<'x>(
        &self,
        stored: &'x Archive<AlignedBytes>,
        keys: Option<&SessionKeys>,
        account_id: u32,
        document_id: u32,
    ) -> impl Future<Output = trc::Result<Cow<'x, Archive<AlignedBytes>>>> + Send;

    /// Mail spec 7: the message blob a reader uses in place of `bytes`, the
    /// blob read for a message whose stored archive is a stub
    /// (`stored_is_stub`, the view's `Cow::Owned`), opened with `keys`
    /// (`za_mail_keys`'s answer, as for the view). A plaintext message's
    /// blob, or any blob read without keys, is returned as read. Either way
    /// the bytes are held in a `Zeroizing` buffer for the request; a reader
    /// takes the inner value only where the bytes leave the server.
    fn za_open_message_blob(
        &self,
        stored_is_stub: bool,
        keys: Option<&SessionKeys>,
        account_id: u32,
        document_id: u32,
        bytes: Vec<u8>,
    ) -> impl Future<Output = trc::Result<Zeroizing<Vec<u8>>>> + Send;

    /// Mail spec 5.7: whether `blob_id` is a sealed source, by its owning
    /// account and class, not by the session asking: a key account's
    /// `Reserved` blob (an upload) or `Linked` blob of `Collection::Email`
    /// (a message, opened through its stub, or a section of one).
    fn za_is_sealed_source(
        &self,
        blob_id: &BlobId,
    ) -> impl Future<Output = trc::Result<bool>> + Send;

    /// Mail spec 5.7: an upload into a key account, sealed as the blob of
    /// 5.1 under a fresh DEK wrapped to the session's mail wrapping key
    /// (`mk`), then reserved as upstream reserves an upload. The callers
    /// check the size and the quota on `data.len()`, the plaintext, first.
    fn za_put_sealed_upload(
        &self,
        account_id: u32,
        keys: &SessionKeys,
        data: &[u8],
    ) -> impl Future<Output = trc::Result<BlobId>> + Send;

    /// Mail spec 5.7, 7: a key account's upload (a `Reserved` blob, always
    /// sealed) opened whole with `keys`, `za_mail_keys`'s answer for the
    /// account, the private key unwrapped only for a `pk` envelope. One
    /// that does not open is unreadable (`za_readable`), as a message is,
    /// without a document id.
    fn za_open_upload_blob(
        &self,
        keys: &SessionKeys,
        account_id: u32,
        bytes: Vec<u8>,
    ) -> impl Future<Output = trc::Result<Zeroizing<Vec<u8>>>> + Send;

    /// Mail spec 5.7, 9: refuses a consumer that reads `blob_id`'s stored
    /// bytes, or a section of them, without unsealing, when the id is a
    /// sealed source (`za_is_sealed_source`), whichever session asks:
    /// traced as `Security(Unauthorized)` naming the owning account, with
    /// `ZA_TRACE_RAW_SOURCE`, and answered with the keyless refusal, an
    /// error every caller writes (ruling R22).
    fn za_refuse_raw_source(
        &self,
        blob_id: &BlobId,
    ) -> impl Future<Output = trc::Result<()>> + Send;

    /// Ruling R31 (mail spec 6.3): whether `blob_id` is a sealed source
    /// (`za_is_sealed_source`) owned by an account other than
    /// `to_account_id`, which it must not enter as a message or a part of
    /// one (Email/import, an Email/set attachment), the server-side copy
    /// across accounts; traced as `Security(Unauthorized)` naming the owning
    /// account, with `ZA_TRACE_MAIL_COPY`, when it is. The caller refuses.
    fn za_foreign_sealed_source(
        &self,
        blob_id: &BlobId,
        to_account_id: u32,
    ) -> impl Future<Output = trc::Result<bool>> + Send;
}

impl ZaMailView for Server {
    async fn za_mail_keys(
        &self,
        access_token: &AccessToken,
        account_id: u32,
    ) -> trc::Result<Option<Arc<SessionKeys>>> {
        if let Some(keys) = access_token.za_keys_for(account_id) {
            Ok(Some(keys.clone()))
        } else if self
            .za_is_key_account(account_id)
            .await
            .caused_by(trc::location!())?
        {
            Err(za_keyless(account_id))
        } else {
            Ok(None)
        }
    }

    async fn za_metadata_view<'x>(
        &self,
        stored: &'x Archive<AlignedBytes>,
        keys: Option<&SessionKeys>,
        account_id: u32,
        document_id: u32,
    ) -> trc::Result<Cow<'x, Archive<AlignedBytes>>> {
        let Some(keys) = keys else {
            return Ok(Cow::Borrowed(stored));
        };
        let stub = stored
            .unarchive::<MessageMetadata>()
            .caused_by(trc::location!())?;
        if !is_stub(stub) {
            return Ok(Cow::Borrowed(stored));
        }
        let private_key = if stub_needs_private_key(stub)
            .map_err(|err| za_unreadable(err, account_id, document_id))?
        {
            Some(za_private_key(self, keys, Some(document_id)).await?)
        } else {
            None
        };
        za_open_stub_archive(stored, keys, private_key, account_id, document_id).map(Cow::Owned)
    }

    async fn za_open_message_blob(
        &self,
        stored_is_stub: bool,
        keys: Option<&SessionKeys>,
        account_id: u32,
        document_id: u32,
        bytes: Vec<u8>,
    ) -> trc::Result<Zeroizing<Vec<u8>>> {
        let (true, Some(keys)) = (stored_is_stub, keys) else {
            return Ok(Zeroizing::new(bytes));
        };
        let private_key = if blob_needs_private_key(&bytes)
            .map_err(|err| za_unreadable(err, account_id, document_id))?
        {
            Some(za_private_key(self, keys, Some(document_id)).await?)
        } else {
            None
        };
        open_mail_blob(&bytes, keys, private_key, account_id)
            .map_err(|err| za_unreadable(err, account_id, document_id))
    }

    async fn za_is_sealed_source(&self, blob_id: &BlobId) -> trc::Result<bool> {
        match blob_id.class {
            BlobClass::Reserved { account_id, .. } => self.za_is_key_account(account_id).await,
            BlobClass::Linked {
                account_id,
                collection,
                ..
            } if collection == u8::from(Collection::Email) => {
                self.za_is_key_account(account_id).await
            }
            BlobClass::Linked { .. } => Ok(false),
        }
    }

    async fn za_put_sealed_upload(
        &self,
        account_id: u32,
        keys: &SessionKeys,
        data: &[u8],
    ) -> trc::Result<BlobId> {
        let dek = new_dek();
        let envelope = seal_mail_envelope(&dek, MailWrap::Session(keys), account_id)
            .map_err(|err| za_sealing_error(err, account_id))?;
        let sealed = seal_mail_blob(&dek, &envelope, account_id, data);
        self.put_jmap_blob(account_id, &sealed)
            .await
            .caused_by(trc::location!())
    }

    async fn za_open_upload_blob(
        &self,
        keys: &SessionKeys,
        account_id: u32,
        bytes: Vec<u8>,
    ) -> trc::Result<Zeroizing<Vec<u8>>> {
        let unreadable =
            |err| za_sealing_error(err, account_id).ctx(trc::Key::Reason, ZA_REASON_UNREADABLE);
        let private_key = if blob_needs_private_key(&bytes).map_err(unreadable)? {
            Some(za_private_key(self, keys, None).await?)
        } else {
            None
        };
        open_mail_blob(&bytes, keys, private_key, account_id).map_err(unreadable)
    }

    async fn za_refuse_raw_source(&self, blob_id: &BlobId) -> trc::Result<()> {
        if self.za_is_sealed_source(blob_id).await? {
            let account_id = blob_id.class.account_id();
            trc::event!(
                Security(trc::SecurityEvent::Unauthorized),
                AccountId = account_id,
                Details = ZA_TRACE_RAW_SOURCE,
            );
            Err(za_keyless_refusal().account_id(account_id))
        } else {
            Ok(())
        }
    }

    async fn za_foreign_sealed_source(
        &self,
        blob_id: &BlobId,
        to_account_id: u32,
    ) -> trc::Result<bool> {
        let account_id = blob_id.class.account_id();
        if account_id != to_account_id && self.za_is_sealed_source(blob_id).await? {
            trc::event!(
                Security(trc::SecurityEvent::Unauthorized),
                AccountId = account_id,
                Details = ZA_TRACE_MAIL_COPY,
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// `seal_error`'s shape for a sealed object without a document id: an
/// upload, or a delivered message whose blob is staged before its id.
pub(crate) fn za_sealing_error(err: SealError, account_id: u32) -> trc::Error {
    trc::StoreEvent::DataCorruption
        .into_err()
        .details(format!("zero-access sealing error: {err}"))
        .account_id(account_id)
        .caused_by(trc::location!())
}

/// Mail spec 5.2, 7: the stub `stored` opened into the view's archive: the
/// full metadata, serialized as `index_message` serializes it and read back
/// as the store reads an archive, `Unversioned` (see `za_metadata_view`).
/// Errors name the message only (`za_unreadable`), never its content.
pub fn za_open_stub_archive(
    stored: &Archive<AlignedBytes>,
    keys: &SessionKeys,
    private_key: Option<&Secret>,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Archive<AlignedBytes>> {
    let unreadable = |err| za_unreadable(err, account_id, document_id);
    let stub = stored
        .unarchive::<MessageMetadata>()
        .caused_by(trc::location!())?;
    let metadata = open_stub(stub, keys, private_key, account_id).map_err(unreadable)?;
    // Only the intermediate serialized buffer is zeroized; the decoded
    // metadata and the view's own bytes are not.
    let bytes = Zeroizing::new(
        Archiver::new(metadata)
            .serialize()
            .map_err(|_| unreadable(SealError::Format))?,
    );
    let mut view = <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes)
        .map_err(|_| unreadable(SealError::Format))?;
    view.version = ArchiveVersion::Unversioned;
    // An unversioned archive is read with a checked access, whose error
    // would quote the bytes: check it here, under the fixed error, so the
    // reader's own access cannot fail.
    view.unarchive::<MessageMetadata>()
        .map_err(|_| unreadable(SealError::Format))?;
    Ok(view)
}

/// Mail spec 10: a stub or blob that does not open, `seal_error`'s
/// `DataCorruption` naming the account and the message, with the fixed
/// reason.
pub fn za_unreadable(err: SealError, account_id: u32, document_id: u32) -> trc::Error {
    seal_error(err, account_id, document_id).ctx(trc::Key::Reason, ZA_REASON_UNREADABLE)
}

/// Mail spec 10: `Ok(None)` for an unreadable message (`za_unreadable`),
/// traced here; every other error, the keyless refusal included, passes
/// through. The reader then fails that message alone: IMAP skips it and
/// ends the command `NO [SERVERBUG]`, JMAP lists it as not found, POP3
/// answers `-ERR`.
pub fn za_readable<T>(result: trc::Result<T>) -> trc::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(err)
            if err.matches(trc::EventType::Store(trc::StoreEvent::DataCorruption))
                && err.value_as_str(trc::Key::Reason) == Some(ZA_REASON_UNREADABLE) =>
        {
            trc::error!(err);
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

/// The account's private key for a `pk` envelope; a key that does not open
/// (a `DataCorruption`) makes the message (`document_id`) or the upload
/// (`None`) unreadable.
async fn za_private_key<'k>(
    server: &Server,
    keys: &'k SessionKeys,
    document_id: Option<u32>,
) -> trc::Result<&'k Secret> {
    server.za_private_key(keys).await.map_err(|err| {
        if err.matches(trc::EventType::Store(trc::StoreEvent::DataCorruption)) {
            match document_id {
                Some(document_id) => err.document_id(document_id),
                None => err,
            }
            .ctx(trc::Key::Reason, ZA_REASON_UNREADABLE)
        } else {
            err
        }
    })
}

/// The keyless refusal (mail spec 10, ruling R22), traced as its own event:
/// a `SecurityEvent` error is never written to the client.
fn za_keyless(account_id: u32) -> trc::Error {
    trc::event!(
        Security(trc::SecurityEvent::Unauthorized),
        AccountId = account_id,
        Details = ZA_TRACE_MAIL_KEYLESS,
    );
    za_keyless_refusal().account_id(account_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{
        index::build_message_metadata,
        metadata::ArchivedMessageMetadata,
        za::{MailWrap, new_dek, seal_mail_envelope, seal_stub},
    };
    use mail_parser::{Header, HeaderName, HeaderValue, Message, MessageParser, PartType};
    use std::collections::VecDeque;
    use store::{Deserialize, Serialize, write::Archiver};
    use types::blob_hash::BlobHash;
    use utils::chained_bytes::ChainedBytes;
    use vault::keys::Secret;

    /// Mail spec 7, review F5: the shapes whose offsets a reader applies.
    const CORPUS: &[&str] = &[
        // A simple text message.
        "From: Alice <alice@example.com>\r\nTo: bob@example.com\r\nSubject: simple\r\nMessage-ID: <s1@example.com>\r\n\r\nA simple text body.\r\n",
        // multipart/alternative with text and HTML.
        "From: alice@example.com\r\nSubject: alternative\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=\"alt\"\r\n\r\n--alt\r\nContent-Type: text/plain; charset=us-ascii\r\n\r\nPlain alternative text.\r\n--alt\r\nContent-Type: text/html; charset=us-ascii\r\n\r\n<html><body><p>HTML alternative text.</p></body></html>\r\n--alt--\r\n",
        // multipart/mixed with a base64 attachment.
        "From: alice@example.com\r\nSubject: report\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"mix\"\r\n\r\n--mix\r\nContent-Type: text/plain\r\n\r\nSee the attached report.\r\n--mix\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\nContent-Disposition: attachment; filename=\"report.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQKJcOkw7zDtsOfCjIgMCBvYmoKPDwvTGVuZ3RoIDMgMCBSPj4Kc3RyZWFtCg==\r\n--mix--\r\n",
        // A nested message/rfc822 whose inner message has its own attachment.
        "From: alice@example.com\r\nSubject: forwarded\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"outer\"\r\n\r\n--outer\r\nContent-Type: text/plain\r\n\r\nForwarded message below.\r\n--outer\r\nContent-Type: message/rfc822\r\n\r\nFrom: carol@example.com\r\nSubject: inner\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"inner\"\r\n\r\n--inner\r\nContent-Type: text/plain\r\n\r\nInner body text.\r\n--inner\r\nContent-Type: application/octet-stream; name=\"inner.bin\"\r\nContent-Disposition: attachment; filename=\"inner.bin\"\r\nContent-Transfer-Encoding: base64\r\n\r\naW5uZXIgYXR0YWNobWVudA==\r\n--inner--\r\n\r\n--outer--\r\n",
        // An 8-bit body with a long folded header.
        "From: alice@example.com\r\nSubject: a long subject that is folded across\r\n several lines of the header block so that\r\n the parser has to unfold it again\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\nCaf\u{e9}, d\u{e9}j\u{e0} vu, na\u{ef}ve fa\u{e7}ade: an 8-bit body.\r\n",
        // A header block only.
        "From: alice@example.com\r\nSubject: header block only\r\n",
    ];
    const DELIVERED_TO: &[u8] = b"Delivered-To: x@example.com\r\n";

    /// The text of every text and HTML part, per message in the order
    /// `build_metadata_contents` numbers them (the root, then nested
    /// messages as they are found).
    fn original_texts(message: &Message<'_>) -> Vec<Vec<Option<String>>> {
        let mut queue = VecDeque::from([message]);
        let mut texts = Vec::new();
        while let Some(message) = queue.pop_front() {
            texts.push(
                message
                    .parts
                    .iter()
                    .map(|part| match &part.body {
                        PartType::Text(text) | PartType::Html(text) => Some(text.to_string()),
                        PartType::Message(nested) => {
                            queue.push_back(nested);
                            None
                        }
                        _ => None,
                    })
                    .collect(),
            );
        }
        texts
    }

    /// `Archiver::new(..).serialize()`, then the store's read side.
    fn archive(metadata: MessageMetadata) -> Archive<AlignedBytes> {
        let bytes = Archiver::new(metadata).serialize().unwrap();
        <Archive<AlignedBytes> as Deserialize>::deserialize(&bytes).unwrap()
    }

    /// The full metadata of a delivery: `Delivered-To` prepended.
    fn full_metadata(raw: &[u8]) -> MessageMetadata {
        let message = MessageParser::new().parse(raw).unwrap();
        let delivered_to = Header {
            name: HeaderName::DeliveredTo,
            value: HeaderValue::Text("x@example.com".into()),
            offset_field: 0,
            offset_start: 13,
            offset_end: DELIVERED_TO.len() as u32,
        };
        build_message_metadata(
            message,
            DELIVERED_TO.to_vec(),
            vec![delivered_to],
            BlobHash::generate(b"sealed bytes"),
            1_700_000_000,
        )
    }

    /// The reconstructed message: `raw_headers || raw[blob_body_offset..]`.
    fn reconstruct<'x>(metadata: &'x ArchivedMessageMetadata, raw: &'x [u8]) -> ChainedBytes<'x> {
        ChainedBytes::new(metadata.raw_headers.as_ref()).with_last(
            raw.get(metadata.blob_body_offset.to_native() as usize..)
                .unwrap_or_default(),
        )
    }

    #[test]
    fn the_view_keeps_upstreams_offsets() {
        let keys = SessionKeys::new(7, 1, Secret::random());
        for (index, text) in CORPUS.iter().enumerate() {
            let raw = text.as_bytes();
            let full = archive(full_metadata(raw));
            let dek = new_dek();
            let envelope = seal_mail_envelope(&dek, MailWrap::Session(&keys), 7).unwrap();
            let stored = archive(seal_stub(full_metadata(raw), &dek, &envelope, 7).unwrap());
            let view = za_open_stub_archive(&stored, &keys, None, 7, 1).unwrap();
            assert_eq!(
                view.version,
                ArchiveVersion::Unversioned,
                "message {index}: the view never matches the stub's version"
            );
            // The opened metadata is the full metadata, every field (preview,
            // headers, body lists, `rcvd_attach`, contents): the same rkyv
            // bytes.
            assert_eq!(
                view.as_bytes(),
                full.as_bytes(),
                "message {index}: the opened metadata"
            );
            assert_ne!(stored.as_bytes(), full.as_bytes());
            let view = view.unarchive::<MessageMetadata>().unwrap();
            let full = full.unarchive::<MessageMetadata>().unwrap();
            assert!(!full.raw_headers.is_empty() && !full.contents[0].parts.is_empty());

            let message = reconstruct(view, raw);
            assert_eq!(
                message.to_bytes(),
                [DELIVERED_TO, raw].concat(),
                "message {index}: the delivery header and the message"
            );
            assert_eq!(message.to_bytes(), reconstruct(full, raw).to_bytes());

            let view_decoded = view.decode_contents(message.clone());
            let full_decoded = full.decode_contents(message.clone());
            let texts = original_texts(&MessageParser::new().parse(raw).unwrap());
            assert_eq!(view.contents.len(), full.contents.len(), "message {index}");
            assert_eq!(view.contents.len(), texts.len(), "message {index}");
            for (id, (view_contents, full_contents)) in
                view.contents.iter().zip(full.contents.iter()).enumerate()
            {
                assert_eq!(view_contents.parts.len(), full_contents.parts.len());
                for (part_id, (part, full_part)) in view_contents
                    .parts
                    .iter()
                    .zip(full_contents.parts.iter())
                    .enumerate()
                {
                    let what = format!("message {index}, contents {id}, part {part_id}");
                    for (range, full_range) in [
                        (part.header_to_end(), full_part.header_to_end()),
                        (part.body_to_end(), full_part.body_to_end()),
                    ] {
                        assert_eq!(range, full_range, "{what}");
                        assert_eq!(
                            view_decoded.raw_message_section(id, range),
                            full_decoded.raw_message_section(id, full_range),
                            "{what}"
                        );
                    }
                    if let Some(original) = &texts[id][part_id] {
                        assert_eq!(
                            view_decoded.text_part(id, u32::from(part.offset_header) as usize),
                            Some(original.as_str()),
                            "{what}"
                        );
                    }
                }
            }
        }
    }
}
