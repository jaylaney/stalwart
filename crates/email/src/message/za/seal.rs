/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! A key account's message at rest (mail spec 5.1-5.3): the sealed blob
//! `0x01 || envelope || seal(DEK, aad_blob, raw)` and the stub, a metadata
//! row (`MetadataRow`) whose header section carries
//! `0x01 || envelope || seal_padded(DEK, aad_meta, full metadata row)` and
//! whose structure names nothing but the sealed blob (`stub_structure`).

use super::{AAD_MAIL_BLOB, AAD_MAIL_KEY, AAD_MAIL_META, MAIL_FORMAT_V1};
use crate::message::metadata::{
    MessageMetadata, MetadataRow, MetadataStructure, NewMetadata, PartEntry, PartFlags, PartInfo,
    PartKind, TransferEncoding,
};
use groupware::calendar::seal::{
    SealError, WRAP_MK, WRAP_PK, open_key_envelope, open_padded, open_pk_envelope,
    policy::POLICY_VERSION, seal_key_envelope, seal_padded, seal_pk_envelope,
};
use store::Deserialize;
use types::blob_hash::BlobHash;
use vault::{
    keys::{PUBLIC_KEY_LEN, Secret, WRAP_LEN, aad, open, seal},
    session::SessionKeys,
};
use zeroize::Zeroizing;

/// `za/v1|mail-key|<account id>`: the envelope.
pub fn mail_key_aad(account_id: u32) -> Vec<u8> {
    aad(AAD_MAIL_KEY, account_id)
}

/// `za/v1|mail-blob|<account id>`: the blob's body.
pub fn mail_blob_aad(account_id: u32) -> Vec<u8> {
    aad(AAD_MAIL_BLOB, account_id)
}

/// `za/v1|mail-meta|<account id>|<blob hash>`: the stub's bundle, bound to
/// the blob so a stub cannot be moved onto another message's blob.
pub fn mail_meta_aad(account_id: u32, blob_hash: &BlobHash) -> Vec<u8> {
    let mut out = aad(AAD_MAIL_META, account_id);
    out.push(b'|');
    out.extend_from_slice(blob_hash.as_slice());
    out
}

/// What a message's DEK is wrapped to (mail spec 3.2).
#[derive(Clone, Copy)]
pub enum MailWrap<'a> {
    /// `mk`: under the session's mail wrapping key (writes in a session).
    Session(&'a SessionKeys),
    /// `pk`: to the account's public key (delivery).
    Public(&'a [u8; PUBLIC_KEY_LEN]),
}

/// A fresh per-message DEK.
pub fn new_dek() -> Secret {
    Secret::random()
}

/// The envelope of mail spec 3.2: `mk` under the MWK, `pk` to the public key.
pub fn seal_mail_envelope(
    dek: &Secret,
    wrap: MailWrap<'_>,
    account_id: u32,
) -> Result<Vec<u8>, SealError> {
    let aad = mail_key_aad(account_id);
    match wrap {
        MailWrap::Session(keys) => Ok(seal_key_envelope(dek, keys.mwk(), &aad)),
        MailWrap::Public(public) => seal_pk_envelope(dek, public, &aad),
    }
}

/// The length of the envelope at the head of `bytes` (which starts at the
/// policy byte). A reader refuses any policy byte other than 1 and any wrap
/// byte other than 1 or 2 (mail spec 3.2).
pub fn envelope_len(bytes: &[u8]) -> Result<usize, SealError> {
    let len = match bytes {
        [POLICY_VERSION, WRAP_MK, ..] => 2 + WRAP_LEN,
        [POLICY_VERSION, WRAP_PK, ..] => 2 + PUBLIC_KEY_LEN + WRAP_LEN,
        [version, ..] if *version != POLICY_VERSION => return Err(SealError::Policy(*version)),
        _ => return Err(SealError::Format),
    };
    if bytes.len() < len {
        Err(SealError::Format)
    } else {
        Ok(len)
    }
}

/// Whether the envelope at the head of `bytes` is a `pk` wrap, opened with
/// the account's private key.
pub fn envelope_needs_private_key(bytes: &[u8]) -> Result<bool, SealError> {
    envelope_len(bytes).map(|_| matches!(bytes, [_, WRAP_PK, ..]))
}

/// Opens an envelope: `mk` with the session's MWK, `pk` with the account's
/// private key, which the caller unwraps only when this envelope needs it.
pub fn open_mail_envelope(
    envelope: &[u8],
    keys: &SessionKeys,
    private_key: Option<&Secret>,
    account_id: u32,
) -> Result<Secret, SealError> {
    let aad = mail_key_aad(account_id);
    if envelope_needs_private_key(envelope)? {
        let private_key = private_key.ok_or(SealError::Structure("private key"))?;
        open_pk_envelope(envelope, private_key, &aad)
    } else {
        open_key_envelope(envelope, keys.mwk(), &aad)
    }
}

/// The sealed blob (mail spec 5.1): `0x01 || envelope || seal(DEK,
/// aad_blob, raw)`. Nothing is padded.
pub fn seal_mail_blob(dek: &Secret, envelope: &[u8], account_id: u32, raw: &[u8]) -> Vec<u8> {
    let sealed = seal(dek, &mail_blob_aad(account_id), raw);
    let mut out = Vec::with_capacity(1 + envelope.len() + sealed.len());
    out.push(MAIL_FORMAT_V1);
    out.extend_from_slice(envelope);
    out.extend_from_slice(&sealed);
    out
}

/// Splits a sealed blob or a stub's bundle into its envelope and its sealed
/// part.
pub fn split_sealed(bytes: &[u8]) -> Result<(&[u8], &[u8]), SealError> {
    match bytes.split_first() {
        None => Err(SealError::NotSealed),
        Some((&MAIL_FORMAT_V1, rest)) => envelope_len(rest).map(|len| rest.split_at(len)),
        Some(_) => Err(SealError::Format),
    }
}

/// Opens a sealed blob into the raw message.
pub fn open_mail_blob(
    sealed: &[u8],
    keys: &SessionKeys,
    private_key: Option<&Secret>,
    account_id: u32,
) -> Result<Zeroizing<Vec<u8>>, SealError> {
    let (envelope, sealed) = split_sealed(sealed)?;
    let dek = open_mail_envelope(envelope, keys, private_key, account_id)?;
    open(&dek, &mail_blob_aad(account_id), sealed).map_err(|_| SealError::Aead)
}

/// The stub's structure (mail spec 5.2): the sealed blob's hash and one
/// opaque binary root part whose header block is the bundle (`bundle_len`
/// bytes, the row's header section) and whose body is the sealed blob
/// (`sealed_len` bytes). So upstream's size readers see the stored sizes
/// (`headers_len` is the bundle's, `blob_size` the sealed blob's), and the
/// row keeps upstream's invariant that the root part's body starts where
/// the header section ends. Nothing else: no envelope, header, address,
/// text, parameter, preview or string; the arrival time, the size and the
/// keywords live in `MessageData`, and the attachment keywords are not set
/// for a key account's message.
pub fn stub_structure(
    blob_hash: BlobHash,
    bundle_len: usize,
    sealed_len: usize,
) -> MessageMetadata {
    let offset_body = u32::try_from(bundle_len).unwrap_or(u32::MAX);
    let offset_end = u32::try_from(bundle_len.saturating_add(sealed_len)).unwrap_or(u32::MAX);
    MessageMetadata {
        blob_hash,
        blob_body_offset: 0,
        parts: vec![PartEntry {
            offset_header: 0,
            offset_body,
            offset_end,
            info: PartInfo::new(PartKind::Binary, TransferEncoding::None, PartFlags(0)),
            ..PartEntry::EMPTY
        }],
        ..Default::default()
    }
}

/// Seals the full metadata row of a message into a stub row (mail spec 5.2).
/// `full.metadata.blob_hash` is the sealed blob's hash, which the bundle's
/// associated data binds; `sealed_len` is the sealed blob's length. The
/// plaintext row is held in a `Zeroizing` buffer while it is sealed.
pub fn seal_stub(
    full: NewMetadata,
    dek: &Secret,
    envelope: &[u8],
    account_id: u32,
    sealed_len: usize,
) -> Result<Vec<u8>, SealError> {
    let blob_hash = full.metadata.blob_hash.clone();
    let plain = Zeroizing::new(full.encode().map_err(|_| SealError::Format)?);
    let sealed = seal_padded(dek, &mail_meta_aad(account_id, &blob_hash), &plain);
    let mut bundle = Vec::with_capacity(1 + envelope.len() + sealed.len());
    bundle.push(MAIL_FORMAT_V1);
    bundle.extend_from_slice(envelope);
    bundle.extend_from_slice(&sealed);
    MetadataRow::encode(stub_structure(blob_hash, bundle.len(), sealed_len), &bundle)
        .map_err(|_| SealError::Format)
}

/// Whether a row's header section is a stub's bundle: the format byte at its
/// head (mail spec 5.2). A reader applies it to a key account's rows only
/// (ruling R29): an ordinary message's header block can start with any byte.
pub fn is_stub_headers(raw_headers: &[u8]) -> bool {
    raw_headers.first() == Some(&MAIL_FORMAT_V1)
}

/// A stored row is a stub when its header section is a bundle
/// (`is_stub_headers`). The error is the row's own, which quotes nothing.
pub fn is_stub(row: &MetadataRow) -> trc::Result<bool> {
    row.raw_headers().map(|headers| is_stub_headers(&headers))
}

/// Opens a stub into the full metadata row. The opened row is decoded with
/// a checked access before it is used (a `pk` envelope can be sealed by
/// anyone holding the public key), and its blob hash must be the stub's.
/// Errors are `SealError`s, which quote nothing.
pub fn open_stub(
    stub: &MetadataRow,
    keys: &SessionKeys,
    private_key: Option<&Secret>,
    account_id: u32,
) -> Result<MetadataRow, SealError> {
    let headers = stub.raw_headers().map_err(|_| SealError::Format)?;
    if !is_stub_headers(&headers) {
        return Err(SealError::NotSealed);
    }
    let (envelope, bundle) = split_sealed(&headers)?;
    let dek = open_mail_envelope(envelope, keys, private_key, account_id)?;
    let blob_hash = stub.unarchive().map_err(|_| SealError::Format)?.blob_hash();
    let plain = open_padded(&dek, &mail_meta_aad(account_id, &blob_hash), bundle)?;
    let row = decode_row(&plain)?;
    if row.unarchive().map_err(|_| SealError::Format)?.blob_hash() != blob_hash {
        return Err(SealError::Format);
    }
    Ok(row)
}

/// A checked decode of an opened row: the structure is validated with
/// rkyv's checked access before the row (read unchecked from then on) is
/// built, and its header section must decode. Errors carry no bytes.
fn decode_row(plain: &[u8]) -> Result<MetadataRow, SealError> {
    MetadataStructure::deserialize(plain)
        .map_err(|_| SealError::Format)?
        .archive()
        .unarchive_untrusted::<MessageMetadata>()
        .map_err(|_| SealError::Format)?;
    let row = MetadataRow::deserialize(plain).map_err(|_| SealError::Format)?;
    row.raw_headers().map_err(|_| SealError::Format)?;
    Ok(row)
}

/// Whether a stub's envelope is a `pk` wrap. `SealError::NotSealed` for a
/// plaintext row, as `open_stub` reports it.
pub fn stub_needs_private_key(stub: &MetadataRow) -> Result<bool, SealError> {
    let headers = stub.raw_headers().map_err(|_| SealError::Format)?;
    if !is_stub_headers(&headers) {
        return Err(SealError::NotSealed);
    }
    split_sealed(&headers).and_then(|(envelope, _)| envelope_needs_private_key(envelope))
}

/// Whether a sealed blob's envelope is a `pk` wrap.
pub fn blob_needs_private_key(sealed: &[u8]) -> Result<bool, SealError> {
    split_sealed(sealed).and_then(|(envelope, _)| envelope_needs_private_key(envelope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::metadata::{ExtraHeaders, HeaderId};
    use groupware::calendar::seal::PAD;
    use mail_parser::MessageParser;
    use vault::keys::{NONCE_LEN, TAG_LEN, generate_keypair};

    const SAMPLE: &[u8] = b"From: alice@example.com\r\nSubject: x\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nbody canary\r\n--b\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"a.bin\"\r\n\r\nattachment canary\r\n--b--\r\n";
    const SEALED_LEN: usize = 1000;

    /// The full metadata of a delivery of `SAMPLE`: a root multipart holding
    /// a text part and a binary attachment part, `Delivered-To` prepended.
    fn sample_metadata(blob_hash: BlobHash) -> NewMetadata {
        let message = MessageParser::new().parse(SAMPLE).unwrap();
        let mut extra = ExtraHeaders::default();
        extra.push(HeaderId::DELIVERED_TO, "x@example.com");
        let full = MessageMetadata::build(&message, &extra, blob_hash);
        assert_eq!(full.metadata.parts.len(), 3);
        assert!(full.has_attachments);
        full
    }

    /// The structure and the header section of a row, compared exactly.
    fn row_contents(row: &MetadataRow) -> (String, Vec<u8>) {
        let structure =
            rkyv::deserialize::<MessageMetadata, rkyv::rancor::Error>(row.unarchive().unwrap())
                .unwrap();
        (
            format!("{structure:?}"),
            row.raw_headers().unwrap().into_owned(),
        )
    }

    fn row(bytes: &[u8]) -> MetadataRow {
        MetadataRow::deserialize(bytes).unwrap()
    }

    /// A stub of `SAMPLE` under `wrap`, with its envelope length.
    fn sample_stub(wrap: MailWrap<'_>, hash: BlobHash) -> (Vec<u8>, usize) {
        let dek = new_dek();
        let envelope = seal_mail_envelope(&dek, wrap, 7).unwrap();
        (
            seal_stub(sample_metadata(hash), &dek, &envelope, 7, SEALED_LEN).unwrap(),
            envelope.len(),
        )
    }

    /// The stub's bundle moved under another structure (as a stored row).
    fn restructured(stub: &[u8], hash: BlobHash) -> Vec<u8> {
        let bundle = row(stub).raw_headers().unwrap().into_owned();
        MetadataRow::encode(stub_structure(hash, bundle.len(), SEALED_LEN), &bundle).unwrap()
    }

    /// A stub whose bundle is `bundle` (a crafted or tampered one).
    fn with_bundle(bundle: &[u8]) -> Vec<u8> {
        MetadataRow::encode(
            stub_structure(BlobHash::generate(b"a"), bundle.len(), SEALED_LEN),
            bundle,
        )
        .unwrap()
    }

    #[test]
    fn a_blob_round_trips_under_both_wraps_and_binds_the_account() {
        let keys = SessionKeys::new(7, 1, Secret::random());
        let (public, private) = generate_keypair();
        for wrap in [MailWrap::Session(&keys), MailWrap::Public(&public)] {
            let dek = new_dek();
            let envelope = seal_mail_envelope(&dek, wrap, 7).unwrap();
            let sealed = seal_mail_blob(&dek, &envelope, 7, b"raw message");
            assert_eq!(sealed[0], MAIL_FORMAT_V1);
            assert_eq!(envelope_len(&sealed[1..]).unwrap(), envelope.len());
            let opened = open_mail_blob(&sealed, &keys, Some(&private), 7).unwrap();
            assert_eq!(opened.as_slice(), b"raw message");
            assert!(matches!(
                open_mail_blob(&sealed, &keys, Some(&private), 8),
                Err(SealError::Aead)
            ));
            let mut tampered = sealed.clone();
            *tampered.last_mut().unwrap() ^= 1;
            assert!(matches!(
                open_mail_blob(&tampered, &keys, Some(&private), 7),
                Err(SealError::Aead)
            ));
        }
        let dek = new_dek();
        let pk_sealed = seal_mail_blob(
            &dek,
            &seal_mail_envelope(&dek, MailWrap::Public(&public), 7).unwrap(),
            7,
            b"x",
        );
        assert!(blob_needs_private_key(&pk_sealed).unwrap());
        assert!(matches!(
            open_mail_blob(&pk_sealed, &keys, None, 7),
            Err(SealError::Structure("private key"))
        ));
        let other = SessionKeys::new(7, 1, Secret::random());
        let mk_sealed = seal_mail_blob(
            &dek,
            &seal_mail_envelope(&dek, MailWrap::Session(&keys), 7).unwrap(),
            7,
            b"x",
        );
        assert!(!blob_needs_private_key(&mk_sealed).unwrap());
        assert!(matches!(
            open_mail_blob(&mk_sealed, &other, None, 7),
            Err(SealError::Aead)
        ));
    }

    #[test]
    fn a_stub_carries_only_the_visible_fields_and_round_trips() {
        let keys = SessionKeys::new(7, 1, Secret::random());
        let hash = BlobHash::generate(b"sealed bytes");
        let (stub, envelope_len) = sample_stub(MailWrap::Session(&keys), hash.clone());
        let stored = row(&stub);
        assert!(is_stub(&stored).unwrap());

        // The structure names the sealed blob and nothing else.
        let structure = stored.unarchive().unwrap();
        let bundle = stored.raw_headers().unwrap().into_owned();
        assert_eq!(bundle[0], MAIL_FORMAT_V1);
        assert_eq!(structure.blob_hash(), hash);
        assert_eq!(structure.blob_body_offset(), 0);
        assert_eq!(structure.headers_len(), bundle.len());
        assert_eq!(structure.blob_size(), SEALED_LEN);
        assert_eq!(structure.size(), bundle.len() + SEALED_LEN);
        assert!(structure.stored_preview().is_none());
        assert!(structure.messages.is_empty());
        assert_eq!(structure.parts.len(), 1);
        assert_eq!(structure.parts().next().unwrap().kind(), PartKind::Binary);
        assert!(
            structure.headers.is_empty()
                && structure.ids.is_empty()
                && structure.addresses.is_empty()
                && structure.texts.is_empty()
                && structure.params.is_empty()
                && structure.strings.is_empty()
        );
        assert!(structure.root().envelope().subject().is_none());
        // The bundle is padded.
        assert_eq!(
            (bundle.len() - 1 - envelope_len - NONCE_LEN - TAG_LEN) % PAD,
            0
        );

        // Opened, it is the full row: the same structure and header section.
        let full = sample_metadata(hash);
        let full = row(&full.encode().unwrap());
        assert!(!is_stub(&full).unwrap());
        let opened = open_stub(&stored, &keys, None, 7).unwrap();
        assert_eq!(row_contents(&opened), row_contents(&full));
        assert!(!is_stub(&opened).unwrap());
    }

    #[test]
    fn a_stub_sealed_to_the_public_key_needs_the_private_key() {
        let keys = SessionKeys::new(7, 1, Secret::random());
        let (public, private) = generate_keypair();
        let hash = BlobHash::generate(b"sealed bytes");
        let (stub, _) = sample_stub(MailWrap::Public(&public), hash.clone());
        let stub = row(&stub);
        assert!(stub_needs_private_key(&stub).unwrap());
        assert!(matches!(
            open_stub(&stub, &keys, None, 7),
            Err(SealError::Structure("private key"))
        ));
        let opened = open_stub(&stub, &keys, Some(&private), 7).unwrap();
        assert_eq!(
            row_contents(&opened),
            row_contents(&row(&sample_metadata(hash.clone()).encode().unwrap()))
        );
        let (stub, _) = sample_stub(MailWrap::Session(&keys), hash);
        assert!(!stub_needs_private_key(&row(&stub)).unwrap());
    }

    #[test]
    fn a_stub_refuses_a_moved_bundle_a_bad_policy_and_an_unknown_wrap() {
        let keys = SessionKeys::new(7, 1, Secret::random());
        let (stub, _) = sample_stub(MailWrap::Session(&keys), BlobHash::generate(b"a"));
        assert!(
            matches!(
                open_stub(
                    &row(&restructured(&stub, BlobHash::generate(b"b"))),
                    &keys,
                    None,
                    7
                ),
                Err(SealError::Aead)
            ),
            "a bundle moved to another blob"
        );
        assert!(
            matches!(open_stub(&row(&stub), &keys, None, 8), Err(SealError::Aead)),
            "an envelope moved to another account"
        );
        let bundle = row(&stub).raw_headers().unwrap().into_owned();
        let mut tampered = bundle.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(matches!(
            open_stub(&row(&restructured_bundle(&tampered)), &keys, None, 7),
            Err(SealError::Aead)
        ));
        let mut policy = bundle.clone();
        policy[1] = 2;
        assert!(matches!(
            open_stub(&row(&restructured_bundle(&policy)), &keys, None, 7),
            Err(SealError::Policy(2))
        ));
        let mut wrap = bundle;
        wrap[2] = 9;
        assert!(matches!(
            open_stub(&row(&restructured_bundle(&wrap)), &keys, None, 7),
            Err(SealError::Format)
        ));
    }

    /// `bundle` under the structure of a stub of blob `a` (as the stubs of
    /// these tests are sealed).
    fn restructured_bundle(bundle: &[u8]) -> Vec<u8> {
        MetadataRow::encode(
            stub_structure(BlobHash::generate(b"a"), bundle.len(), SEALED_LEN),
            bundle,
        )
        .unwrap()
    }

    #[test]
    fn a_plaintext_row_is_not_a_stub() {
        let keys = SessionKeys::new(7, 1, Secret::random());
        let full = row(&sample_metadata(BlobHash::generate(b"a")).encode().unwrap());
        assert!(!is_stub(&full).unwrap());
        assert!(matches!(
            open_stub(&full, &keys, None, 7),
            Err(SealError::NotSealed)
        ));
        assert!(matches!(
            stub_needs_private_key(&full),
            Err(SealError::NotSealed)
        ));
        let crafted = row(&with_bundle(&[MAIL_FORMAT_V1, 1, 1]));
        assert!(
            is_stub(&crafted).unwrap(),
            "a header section starting with the format byte reads as a stub"
        );
        assert!(matches!(
            open_stub(&crafted, &keys, None, 7),
            Err(SealError::Format)
        ));
        // A `pk` envelope one byte short of its length.
        let mut truncated = vec![MAIL_FORMAT_V1, POLICY_VERSION, WRAP_PK];
        truncated.resize(truncated.len() + PUBLIC_KEY_LEN + WRAP_LEN - 1, 0);
        assert!(matches!(
            open_stub(&row(&with_bundle(&truncated)), &keys, None, 7),
            Err(SealError::Format)
        ));
    }

    /// An opened bundle is decoded with a checked access: a `pk` envelope
    /// can be sealed by anyone holding the public key, so the plaintext it
    /// carries is not trusted.
    #[test]
    fn a_forged_bundle_does_not_open_as_a_row() {
        let keys = SessionKeys::new(7, 1, Secret::random());
        let (public, private) = generate_keypair();
        let hash = BlobHash::generate(b"a");
        let dek = new_dek();
        let envelope = seal_mail_envelope(&dek, MailWrap::Public(&public), 7).unwrap();
        for plain in [
            b"not a row".to_vec(),
            // A structure whose trailer claims more bytes than it holds.
            [vec![0u8; 8], 64u32.to_le_bytes().to_vec(), vec![0x80]].concat(),
        ] {
            let sealed = seal_padded(&dek, &mail_meta_aad(7, &hash), &plain);
            let bundle = [vec![MAIL_FORMAT_V1], envelope.clone(), sealed].concat();
            let stub = MetadataRow::encode(stub_structure(hash.clone(), bundle.len(), 1), &bundle)
                .unwrap();
            assert!(matches!(
                open_stub(&row(&stub), &keys, Some(&private), 7),
                Err(SealError::Format)
            ));
        }
        // A well-formed row of another blob.
        let other = sample_metadata(BlobHash::generate(b"b")).encode().unwrap();
        let sealed = seal_padded(&dek, &mail_meta_aad(7, &hash), &other);
        let bundle = [vec![MAIL_FORMAT_V1], envelope, sealed].concat();
        let stub = MetadataRow::encode(stub_structure(hash, bundle.len(), 1), &bundle).unwrap();
        assert!(matches!(
            open_stub(&row(&stub), &keys, Some(&private), 7),
            Err(SealError::Format)
        ));
    }
}
