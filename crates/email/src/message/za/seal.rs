/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! A key account's message at rest (mail spec 5.1-5.3): the sealed blob
//! `0x01 || envelope || seal(DEK, aad_blob, raw)` and the stub, a
//! `MessageMetadata` whose `raw_headers` carry
//! `0x01 || envelope || seal_padded(DEK, aad_meta, rkyv(full metadata))`.

use super::{AAD_MAIL_BLOB, AAD_MAIL_KEY, AAD_MAIL_META, MAIL_FORMAT_V1};
use crate::message::metadata::{
    ArchivedMessageMetadata, MESSAGE_HAS_ATTACHMENT, MESSAGE_RECEIVED_MASK, MessageMetadata,
    MessageMetadataContents, MessageMetadataPart, MetadataPartType,
};
use groupware::calendar::seal::{
    SealError, WRAP_MK, WRAP_PK, decode_archive, open_key_envelope, open_padded, open_pk_envelope,
    policy::POLICY_VERSION, seal_key_envelope, seal_padded, seal_pk_envelope,
};
use types::blob_hash::BlobHash;
use vault::{
    keys::{PUBLIC_KEY_LEN, Secret, WRAP_LEN, aad, open, seal},
    session::SessionKeys,
};
use zeroize::{Zeroize, Zeroizing};

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

/// The stub of mail spec 5.2 around `bundle`: one empty root part, the
/// arrival time with the attachment bit clear, the blob hash, nothing else.
pub fn stub_metadata(full: &MessageMetadata, bundle: Vec<u8>) -> MessageMetadata {
    MessageMetadata {
        contents: Box::new([MessageMetadataContents {
            html_body: Box::default(),
            text_body: Box::default(),
            attachments: Box::default(),
            parts: Box::new([MessageMetadataPart {
                headers: Box::default(),
                body: MetadataPartType::Binary,
                flags: 0,
                offset_header: 0,
                offset_body: 0,
                offset_end: 0,
            }]),
        }]),
        rcvd_attach: full.rcvd_attach & MESSAGE_RECEIVED_MASK,
        blob_hash: full.blob_hash.clone(),
        blob_body_offset: 0,
        preview: Box::default(),
        raw_headers: bundle.into_boxed_slice(),
    }
}

/// Seals the full metadata archive into a stub. `full.blob_hash` is the
/// sealed blob's hash, which the bundle's associated data binds.
pub fn seal_stub(
    full: MessageMetadata,
    dek: &Secret,
    envelope: &[u8],
    account_id: u32,
) -> Result<MessageMetadata, SealError> {
    let mut serialized =
        rkyv::to_bytes::<rkyv::rancor::Error>(&full).map_err(|_| SealError::Format)?;
    let plain = Zeroizing::new(serialized.to_vec());
    serialized.as_mut_slice().zeroize();
    let sealed = seal_padded(dek, &mail_meta_aad(account_id, &full.blob_hash), &plain);
    let mut bundle = Vec::with_capacity(1 + envelope.len() + sealed.len());
    bundle.push(MAIL_FORMAT_V1);
    bundle.extend_from_slice(envelope);
    bundle.extend_from_slice(&sealed);
    Ok(stub_metadata(&full, bundle))
}

/// A stub is recognised by the format byte at the head of `raw_headers`
/// (mail spec 5.2).
pub fn is_stub(metadata: &ArchivedMessageMetadata) -> bool {
    metadata.raw_headers.first() == Some(&MAIL_FORMAT_V1)
}

/// Opens a stub into the full metadata. The stub's time bits are
/// authoritative; the attachment bit comes from the bundle.
pub fn open_stub(
    stub: &ArchivedMessageMetadata,
    keys: &SessionKeys,
    private_key: Option<&Secret>,
    account_id: u32,
) -> Result<MessageMetadata, SealError> {
    if !is_stub(stub) {
        return Err(SealError::NotSealed);
    }
    let (envelope, bundle) = split_sealed(&stub.raw_headers)?;
    let dek = open_mail_envelope(envelope, keys, private_key, account_id)?;
    let blob_hash = BlobHash::from(&stub.blob_hash);
    let plain = open_padded(&dek, &mail_meta_aad(account_id, &blob_hash), bundle)?;
    let mut inner = decode_archive::<MessageMetadata>(&plain)?;
    inner.rcvd_attach = (inner.rcvd_attach & MESSAGE_HAS_ATTACHMENT)
        | (stub.rcvd_attach.to_native() & MESSAGE_RECEIVED_MASK);
    Ok(inner)
}

/// Whether a stub's envelope is a `pk` wrap. `SealError::NotSealed` for a
/// plaintext archive, as `open_stub` reports it.
pub fn stub_needs_private_key(stub: &ArchivedMessageMetadata) -> Result<bool, SealError> {
    if !is_stub(stub) {
        return Err(SealError::NotSealed);
    }
    split_sealed(&stub.raw_headers).and_then(|(envelope, _)| envelope_needs_private_key(envelope))
}

/// Whether a sealed blob's envelope is a `pk` wrap.
pub fn blob_needs_private_key(sealed: &[u8]) -> Result<bool, SealError> {
    split_sealed(sealed).and_then(|(envelope, _)| envelope_needs_private_key(envelope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::metadata::build_metadata_contents;
    use mail_parser::MessageParser;
    use store::{
        Deserialize, Serialize,
        write::{AlignedBytes, Archive, Archiver},
    };
    use vault::keys::{NONCE_LEN, TAG_LEN, generate_keypair};

    const SAMPLE: &[u8] = b"From: alice@example.com\r\nSubject: x\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nbody canary\r\n--b\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"a.bin\"\r\n\r\nattachment canary\r\n--b--\r\n";

    /// A root multipart holding a text part and a binary attachment part.
    fn sample_metadata(blob_hash: BlobHash) -> MessageMetadata {
        let message = MessageParser::new().parse(SAMPLE).unwrap();
        let contents = build_metadata_contents(message);
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].parts.len(), 3);
        assert!(matches!(
            contents[0].parts[0].body,
            MetadataPartType::Multipart(_)
        ));
        assert!(matches!(contents[0].parts[1].body, MetadataPartType::Text));
        assert!(matches!(
            contents[0].parts[2].body,
            MetadataPartType::Binary
        ));
        assert_eq!(
            contents[0].parts[0].headers.len(),
            3,
            "From, Subject, Content-Type"
        );
        MessageMetadata {
            contents,
            rcvd_attach: MESSAGE_HAS_ATTACHMENT | 1_700_000_000,
            blob_hash,
            blob_body_offset: 14,
            preview: "preview canary".into(),
            raw_headers: b"Subject: x\r\n\r\n".to_vec().into_boxed_slice(),
        }
    }

    /// Ruling R1: `MessageMetadata` derives no `Clone`; a copy round-trips
    /// through rkyv.
    fn copy(metadata: &MessageMetadata) -> MessageMetadata {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(metadata).unwrap();
        rkyv::from_bytes::<MessageMetadata, rkyv::rancor::Error>(&bytes).unwrap()
    }

    /// A metadata archive as a reader gets it from the store.
    struct Stored(Archive<AlignedBytes>);

    impl std::ops::Deref for Stored {
        type Target = ArchivedMessageMetadata;

        fn deref(&self) -> &ArchivedMessageMetadata {
            self.0.unarchive::<MessageMetadata>().unwrap()
        }
    }

    /// `Archiver::new(..).serialize()`, then the store's read side.
    fn archive(metadata: &MessageMetadata) -> Stored {
        let bytes = Archiver::new(copy(metadata)).serialize().unwrap();
        Stored(<Archive<AlignedBytes> as Deserialize>::deserialize(&bytes).unwrap())
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
        let full = sample_metadata(hash.clone());
        let dek = new_dek();
        let envelope = seal_mail_envelope(&dek, MailWrap::Session(&keys), 7).unwrap();
        let stub = seal_stub(copy(&full), &dek, &envelope, 7).unwrap();
        assert_eq!(stub.contents.len(), 1);
        assert_eq!(stub.contents[0].parts.len(), 1);
        let part = &stub.contents[0].parts[0];
        assert!(part.headers.is_empty() && matches!(part.body, MetadataPartType::Binary));
        assert_eq!(
            (
                part.flags,
                part.offset_header,
                part.offset_body,
                part.offset_end
            ),
            (0, 0, 0, 0)
        );
        assert!(
            stub.contents[0].html_body.is_empty()
                && stub.contents[0].text_body.is_empty()
                && stub.contents[0].attachments.is_empty()
        );
        assert_eq!(stub.rcvd_attach, 1_700_000_000);
        assert_eq!(stub.blob_hash, hash);
        assert_eq!(stub.blob_body_offset, 0);
        assert!(stub.preview.is_empty());
        assert_eq!(stub.raw_headers[0], MAIL_FORMAT_V1);
        let bundle_len = stub.raw_headers.len() - 1 - envelope.len() - NONCE_LEN - TAG_LEN;
        assert_eq!(bundle_len % 256, 0);
        let archived = archive(&stub);
        assert!(is_stub(&archived));
        let opened = open_stub(&archived, &keys, None, 7).unwrap();
        assert_eq!(opened.raw_headers, full.raw_headers);
        assert_eq!(opened.preview, full.preview);
        assert_eq!(opened.blob_body_offset, full.blob_body_offset);
        assert_eq!(opened.rcvd_attach, full.rcvd_attach);
        assert_eq!(opened.contents.len(), full.contents.len());
        assert_eq!(opened.contents[0].parts.len(), full.contents[0].parts.len());
        // Spec 7: the stub's time bits are authoritative, the attachment bit
        // comes from the bundle.
        let mut retimed = copy(&stub);
        retimed.rcvd_attach = 1_800_000_000;
        assert_eq!(
            open_stub(&archive(&retimed), &keys, None, 7)
                .unwrap()
                .rcvd_attach,
            MESSAGE_HAS_ATTACHMENT | 1_800_000_000
        );
    }

    #[test]
    fn a_stub_sealed_to_the_public_key_needs_the_private_key() {
        let keys = SessionKeys::new(7, 1, Secret::random());
        let (public, private) = generate_keypair();
        let full = sample_metadata(BlobHash::generate(b"sealed bytes"));
        let dek = new_dek();
        let envelope = seal_mail_envelope(&dek, MailWrap::Public(&public), 7).unwrap();
        let stub = archive(&seal_stub(copy(&full), &dek, &envelope, 7).unwrap());
        assert!(stub_needs_private_key(&stub).unwrap());
        assert!(matches!(
            open_stub(&stub, &keys, None, 7),
            Err(SealError::Structure("private key"))
        ));
        let opened = open_stub(&stub, &keys, Some(&private), 7).unwrap();
        assert_eq!(opened.raw_headers, full.raw_headers);
        assert_eq!(opened.preview, full.preview);
        assert_eq!(opened.blob_body_offset, full.blob_body_offset);
        assert_eq!(opened.rcvd_attach, full.rcvd_attach);
        assert_eq!(opened.contents[0].parts.len(), full.contents[0].parts.len());
        let dek = new_dek();
        let envelope = seal_mail_envelope(&dek, MailWrap::Session(&keys), 7).unwrap();
        let stub = archive(&seal_stub(copy(&full), &dek, &envelope, 7).unwrap());
        assert!(!stub_needs_private_key(&stub).unwrap());
    }

    #[test]
    fn a_stub_refuses_a_moved_bundle_a_bad_policy_and_an_unknown_wrap() {
        let keys = SessionKeys::new(7, 1, Secret::random());
        let dek = new_dek();
        let envelope = seal_mail_envelope(&dek, MailWrap::Session(&keys), 7).unwrap();
        let stub = seal_stub(
            sample_metadata(BlobHash::generate(b"a")),
            &dek,
            &envelope,
            7,
        )
        .unwrap();
        let mut moved = copy(&stub);
        moved.blob_hash = BlobHash::generate(b"b");
        assert!(
            matches!(
                open_stub(&archive(&moved), &keys, None, 7),
                Err(SealError::Aead)
            ),
            "a bundle moved to another blob"
        );
        assert!(
            matches!(
                open_stub(&archive(&stub), &keys, None, 8),
                Err(SealError::Aead)
            ),
            "an envelope moved to another account"
        );
        let mut tampered = copy(&stub);
        let last = tampered.raw_headers.len() - 1;
        tampered.raw_headers[last] ^= 1;
        assert!(matches!(
            open_stub(&archive(&tampered), &keys, None, 7),
            Err(SealError::Aead)
        ));
        let mut policy = copy(&stub);
        policy.raw_headers[1] = 2;
        assert!(matches!(
            open_stub(&archive(&policy), &keys, None, 7),
            Err(SealError::Policy(2))
        ));
        let mut wrap = copy(&stub);
        wrap.raw_headers[2] = 9;
        assert!(matches!(
            open_stub(&archive(&wrap), &keys, None, 7),
            Err(SealError::Format)
        ));
    }

    #[test]
    fn a_plaintext_archive_is_not_a_stub() {
        let full = sample_metadata(BlobHash::generate(b"a"));
        let archived = archive(&full);
        assert!(!is_stub(&archived));
        assert!(matches!(
            open_stub(
                &archived,
                &SessionKeys::new(7, 1, Secret::random()),
                None,
                7
            ),
            Err(SealError::NotSealed)
        ));
        let mut crafted = copy(&full);
        crafted.raw_headers = vec![MAIL_FORMAT_V1, 1, 1].into_boxed_slice();
        assert!(
            is_stub(&archive(&crafted)),
            "a header block starting with the format byte reads as a stub"
        );
        assert!(matches!(
            open_stub(
                &archive(&crafted),
                &SessionKeys::new(7, 1, Secret::random()),
                None,
                7
            ),
            Err(SealError::Format)
        ));
        // A `pk` envelope one byte short of its length.
        let mut truncated = vec![MAIL_FORMAT_V1, POLICY_VERSION, WRAP_PK];
        truncated.resize(truncated.len() + PUBLIC_KEY_LEN + WRAP_LEN - 1, 0);
        crafted.raw_headers = truncated.into_boxed_slice();
        assert!(matches!(
            open_stub(
                &archive(&crafted),
                &SessionKeys::new(7, 1, Secret::random()),
                None,
                7
            ),
            Err(SealError::Format)
        ));
    }
}
