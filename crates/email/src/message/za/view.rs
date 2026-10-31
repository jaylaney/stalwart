/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 5.2, 7, 10: the unsealed view a reader takes of a key
//! account's message. A reader reads the stored metadata row as today
//! (`MetadataRow`: a key account's reader needs the row, since a stub's
//! structure alone names only the sealed blob), obtains the account's keys
//! once per operation (`za_mail_keys`), replaces the row with
//! `za_metadata_view` and opens the blob it reads with
//! `za_open_message_blob`. An ordinary account's row passes through
//! unchanged. A message whose stub or blob does not open is unreadable: a
//! `DataCorruption` the reader turns into its protocol's per-message failure
//! (`za_readable`).

use super::{
    MailWrap, blob_needs_private_key, is_stub, new_dek, open_mail_blob, open_stub, seal_mail_blob,
    seal_mail_envelope, stub_needs_private_key,
};
use crate::message::metadata::MetadataRow;
use common::{
    Server,
    auth::AccessToken,
    za::{ZA_TRACE_MAIL_COPY, ZA_TRACE_MAIL_KEYLESS, mail::za_keyless_refusal},
};
use groupware::calendar::seal::{SealError, seal_error};
use std::{future::Future, sync::Arc};
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

/// A reader's metadata row (`za_metadata_view`).
pub struct ZaMetadataRow {
    /// The stored row, or the stub's full row opened.
    pub row: MetadataRow,
    /// The stored row was a stub that was opened: the message's blob is
    /// sealed, and the reader passes this to `za_open_message_blob`.
    pub opened: bool,
}

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

    /// Mail spec 7: the metadata row a reader uses in place of `stored`.
    /// `keys` is `za_mail_keys`'s answer for the account, so `None` means an
    /// ordinary account: the stub check runs only on key accounts' rows
    /// (spec 5.2, ruling R29), and without keys `stored` is returned as it
    /// is, whatever its first header byte. With keys, a stub is opened (the
    /// account's private key unwrapped only for a `pk` envelope) into the
    /// full row; one that does not open is unreadable. A key account's row
    /// that is not a stub is returned as it is (`opened: false`).
    ///
    /// As `unseal_event_archive`'s view, this one is read-only: its row is
    /// never written back, never copied (`index_verbatim` takes the stored
    /// bytes) and never cached; writes start from the stored stub.
    fn za_metadata_view(
        &self,
        stored: MetadataRow,
        keys: Option<&SessionKeys>,
        account_id: u32,
        document_id: u32,
    ) -> impl Future<Output = trc::Result<ZaMetadataRow>> + Send;

    /// Mail spec 7: the message blob a reader uses in place of `bytes`, the
    /// blob read for a message whose stored row is a stub
    /// (`stored_is_stub`, the view's `opened`), opened with `keys`
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
    /// `Reserved` blob (an upload) or `Linked` or `Embedded` blob of
    /// `Collection::Email` (a message, opened through its stub, or a
    /// section of one).
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

    async fn za_metadata_view(
        &self,
        stored: MetadataRow,
        keys: Option<&SessionKeys>,
        account_id: u32,
        document_id: u32,
    ) -> trc::Result<ZaMetadataRow> {
        let Some(keys) = keys else {
            return Ok(ZaMetadataRow {
                row: stored,
                opened: false,
            });
        };
        if !is_stub(&stored).caused_by(trc::location!())? {
            return Ok(ZaMetadataRow {
                row: stored,
                opened: false,
            });
        }
        let private_key = if stub_needs_private_key(&stored)
            .map_err(|err| za_unreadable(err, account_id, document_id))?
        {
            Some(za_private_key(self, keys, Some(document_id)).await?)
        } else {
            None
        };
        za_open_stub_row(&stored, keys, private_key, account_id, document_id)
            .map(|row| ZaMetadataRow { row, opened: true })
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
            }
            | BlobClass::Embedded {
                account_id,
                collection,
                ..
            } if collection == u8::from(Collection::Email) => {
                self.za_is_key_account(account_id).await
            }
            BlobClass::Linked { .. } | BlobClass::Embedded { .. } => Ok(false),
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

/// Mail spec 5.2, 7: the stub `stored` opened into the view's row: the
/// full metadata row, decoded with a checked access (`open_stub`). Errors
/// name the message only (`za_unreadable`), never its content.
pub fn za_open_stub_row(
    stored: &MetadataRow,
    keys: &SessionKeys,
    private_key: Option<&Secret>,
    account_id: u32,
    document_id: u32,
) -> trc::Result<MetadataRow> {
    open_stub(stored, keys, private_key, account_id)
        .map_err(|err| za_unreadable(err, account_id, document_id))
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
        metadata::{ExtraHeaders, HeaderId, MessageMetadata},
        za::{MailWrap, new_dek, seal_mail_envelope, seal_stub},
    };
    use mail_parser::MessageParser;
    use store::Deserialize;
    use types::blob_hash::BlobHash;
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
        // A base64 message/rfc822 nested twice (a decoded source chain).
        "From: alice@example.com\r\nSubject: encoded\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"o\"\r\n\r\n--o\r\nContent-Type: message/rfc822\r\nContent-Transfer-Encoding: base64\r\n\r\nU3ViamVjdDogZW5jb2RlZCBpbm5lcg0KDQpFbmNvZGVkIGlubmVyIGJvZHku\r\n--o--\r\n",
        // An 8-bit body with a long folded header.
        "From: alice@example.com\r\nSubject: a long subject that is folded across\r\n several lines of the header block so that\r\n the parser has to unfold it again\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\nCaf\u{e9}, d\u{e9}j\u{e0} vu, na\u{ef}ve fa\u{e7}ade: an 8-bit body.\r\n",
        // A header block only.
        "From: alice@example.com\r\nSubject: header block only\r\n",
    ];
    const DELIVERED_TO: &[u8] = b"Delivered-To: x@example.com\r\n";

    /// The text of every text and HTML part of every message, nested ones
    /// included, as the parser reads it.
    fn original_texts(raw: &[u8]) -> Vec<String> {
        let message = MessageParser::new().parse(raw).unwrap();
        let mut texts = message
            .parts()
            .filter_map(|part| part.text().map(|text| text.into_owned()))
            .collect::<Vec<_>>();
        texts.sort();
        texts
    }

    /// The full metadata of a delivery: `Delivered-To` prepended.
    fn full_metadata(raw: &[u8], blob_hash: BlobHash) -> crate::message::metadata::NewMetadata {
        let message = MessageParser::new().parse(raw).unwrap();
        let mut extra = ExtraHeaders::default();
        extra.push(HeaderId::DELIVERED_TO, "x@example.com");
        MessageMetadata::build(&message, &extra, blob_hash)
    }

    /// Every text part's text, read through upstream's sources.
    fn texts(row: &MetadataRow, raw: &[u8]) -> Vec<String> {
        let metadata = row.unarchive().unwrap();
        let headers = row.raw_headers().unwrap();
        let source = metadata.raw_message(Some(&headers), raw);
        let mut texts = Vec::new();
        for message in metadata.messages() {
            let source = metadata.source(message, source).unwrap();
            for part in message.parts() {
                if let Some(text) = part.text(&source) {
                    texts.push(text.text.into_owned());
                }
            }
        }
        texts.sort();
        texts
    }

    #[test]
    fn the_view_keeps_upstreams_offsets() {
        let keys = SessionKeys::new(7, 1, Secret::random());
        for (index, text) in CORPUS.iter().enumerate() {
            let raw = text.as_bytes();
            let hash = BlobHash::generate(b"sealed bytes");
            let full =
                MetadataRow::deserialize(&full_metadata(raw, hash.clone()).encode().unwrap())
                    .unwrap();
            let dek = new_dek();
            let envelope = seal_mail_envelope(&dek, MailWrap::Session(&keys), 7).unwrap();
            let stored = MetadataRow::deserialize(
                &seal_stub(full_metadata(raw, hash), &dek, &envelope, 7, 4096).unwrap(),
            )
            .unwrap();
            let view = za_open_stub_row(&stored, &keys, None, 7, 1).unwrap();

            // The opened row is the full row, every field: the same structure
            // and the same header section.
            let structure = |row: &MetadataRow| {
                format!(
                    "{:?}",
                    rkyv::deserialize::<MessageMetadata, rkyv::rancor::Error>(
                        row.unarchive().unwrap()
                    )
                    .unwrap()
                )
            };
            assert_eq!(structure(&view), structure(&full), "message {index}");
            assert_ne!(structure(&stored), structure(&full), "message {index}");
            assert_eq!(
                view.raw_headers().unwrap(),
                full.raw_headers().unwrap(),
                "message {index}"
            );

            // The reconstructed message: the header section, then the blob
            // from the body offset.
            let metadata = view.unarchive().unwrap();
            let headers = view.raw_headers().unwrap();
            assert_eq!(
                metadata
                    .raw_message(Some(&headers), raw)
                    .whole()
                    .unwrap()
                    .to_vec(),
                [DELIVERED_TO, raw].concat(),
                "message {index}: the delivery header and the message"
            );

            // Upstream's part readers find every text part's text, nested and
            // decoded messages included.
            assert_eq!(texts(&view, raw), original_texts(raw), "message {index}");
            assert_eq!(texts(&view, raw), texts(&full, raw), "message {index}");
        }
    }
}
