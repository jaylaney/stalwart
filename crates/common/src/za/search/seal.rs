/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Authenticated sealing of the search index's independent blob sections.

use super::error::LossClass;
use store::za_search::{
    BlobHeader, BlobKind, FormatError, PadTarget, SectionKind, SegmentId, ZaSearchKey,
    blob::{
        HiddenSection, PublicSection, decode_content, decode_hidden, decode_public, encode_content,
        encode_hidden, encode_public, frame_and_pad, unframe,
    },
    content::ContentRecord,
};
use types::blob_hash::BlobHash;
use vault::{
    Zeroizing,
    keys::{Secret, aad, open, seal},
};

#[cfg(test)]
#[path = "seal_tests.rs"]
mod tests;

pub const AAD_SEARCH: &str = "search";

pub fn section_aad(account_id: u32, header: &BlobHeader, kind: SectionKind) -> Vec<u8> {
    let mut bytes = aad(AAD_SEARCH, account_id);
    bytes.push(b'|');
    bytes.extend_from_slice(&header.encode());
    bytes.push(kind as u8);
    bytes
}

pub fn seal_section(
    key: &Secret,
    account_id: u32,
    header: &BlobHeader,
    kind: SectionKind,
    plaintext: &[u8],
) -> Vec<u8> {
    seal(key, &section_aad(account_id, header, kind), plaintext)
}

pub fn open_section(
    key: &Secret,
    account_id: u32,
    header: &BlobHeader,
    kind: SectionKind,
    sealed: &[u8],
) -> Result<Zeroizing<Vec<u8>>, LossClass> {
    open(key, &section_aad(account_id, header, kind), sealed).map_err(|_| LossClass::Aead)
}

pub fn build_thread_blob(
    key: &Secret,
    account_id: u32,
    header: &BlobHeader,
    hidden: Option<(&HiddenSection, PadTarget)>,
    public: &PublicSection,
    min_class: usize,
) -> Result<Vec<u8>, FormatError> {
    if header.kind != BlobKind::Thread {
        return Err(FormatError::Kind(header.kind as u8));
    }

    let hidden_sealed = if let Some((hidden, target)) = hidden {
        let padded = {
            let plaintext = Zeroizing::new(encode_hidden(hidden));
            Zeroizing::new(frame_and_pad(&plaintext, target, min_class)?)
        };
        seal_section(key, account_id, header, SectionKind::Hidden, &padded)
    } else {
        Vec::new()
    };
    let hidden_len =
        u32::try_from(hidden_sealed.len()).map_err(|_| FormatError::AllowanceExceeded)?;
    let public_sealed = {
        let plaintext = Zeroizing::new(encode_public(public));
        seal_section(key, account_id, header, SectionKind::Public, &plaintext)
    };

    let mut blob = Vec::with_capacity(
        BlobHeader::LEN + size_of::<u32>() + hidden_sealed.len() + public_sealed.len(),
    );
    blob.extend_from_slice(&header.encode());
    blob.extend_from_slice(&hidden_len.to_le_bytes());
    blob.extend_from_slice(&hidden_sealed);
    blob.extend_from_slice(&public_sealed);
    Ok(blob)
}

pub fn parse_thread_blob(
    key: &Secret,
    account_id: u32,
    expected: &BlobHeader,
    bytes: &[u8],
) -> Result<(Option<HiddenSection>, PublicSection), LossClass> {
    let (header, body) = BlobHeader::decode(bytes).map_err(|_| LossClass::Parse)?;
    if header.kind != BlobKind::Thread || &header != expected {
        return Err(LossClass::Parse);
    }
    let (hidden_len, rest) = body.split_first_chunk().ok_or(LossClass::Parse)?;
    let hidden_len =
        usize::try_from(u32::from_le_bytes(*hidden_len)).map_err(|_| LossClass::Parse)?;
    let (hidden_sealed, public_sealed) =
        rest.split_at_checked(hidden_len).ok_or(LossClass::Parse)?;

    let public = {
        let plaintext = open_section(key, account_id, &header, SectionKind::Public, public_sealed)?;
        decode_public(&plaintext).map_err(|_| LossClass::Parse)?
    };
    let hidden = if hidden_sealed.is_empty() {
        None
    } else {
        let padded = open_section(key, account_id, &header, SectionKind::Hidden, hidden_sealed)?;
        let plaintext = unframe(&padded).map_err(|_| LossClass::Parse)?;
        Some(decode_hidden(plaintext).map_err(|_| LossClass::Parse)?)
    };
    Ok((hidden, public))
}

pub fn build_content_blob(
    key: &Secret,
    account_id: u32,
    header: &BlobHeader,
    records: &[&[u8]],
) -> Vec<u8> {
    seal_content_blob(
        key,
        account_id,
        header,
        Zeroizing::new(encode_content(records)),
    )
}

pub fn seal_content_blob(
    key: &Secret,
    account_id: u32,
    header: &BlobHeader,
    plaintext: Zeroizing<Vec<u8>>,
) -> Vec<u8> {
    debug_assert_eq!(header.kind, BlobKind::Content);
    let sealed = seal_section(key, account_id, header, SectionKind::Content, &plaintext);
    drop(plaintext);
    let mut blob = Vec::with_capacity(BlobHeader::LEN + sealed.len());
    blob.extend_from_slice(&header.encode());
    blob.extend_from_slice(&sealed);
    blob
}

pub fn parse_content_blob(
    key: &Secret,
    account_id: u32,
    expected_segment: Option<SegmentId>,
    expected_hash_of_predecessor: Option<&(BlobHash, SegmentId)>,
    bytes: &[u8],
) -> Result<(BlobHeader, Vec<ContentRecord>), LossClass> {
    let (header, plaintext) = open_content_blob(
        key,
        account_id,
        expected_segment,
        expected_hash_of_predecessor,
        bytes,
    )?;
    let records = decode_content(&plaintext).map_err(|_| LossClass::Parse)?;
    Ok((header, records))
}

pub(super) fn open_content_blob(
    key: &Secret,
    account_id: u32,
    expected_segment: Option<SegmentId>,
    expected_hash_of_predecessor: Option<&(BlobHash, SegmentId)>,
    bytes: &[u8],
) -> Result<(BlobHeader, Zeroizing<Vec<u8>>), LossClass> {
    let (header, sealed) = BlobHeader::decode(bytes).map_err(|_| LossClass::Parse)?;
    if header.kind != BlobKind::Content
        || expected_segment.is_some_and(|segment| header.segment != segment)
        || expected_hash_of_predecessor.is_some_and(|(hash, segment)| {
            header.segment != *segment || BlobHash::generate(bytes) != *hash
        })
    {
        return Err(LossClass::Parse);
    }

    let plaintext = open_section(key, account_id, &header, SectionKind::Content, sealed)?;
    Ok((header, plaintext))
}

pub fn za_search_key(secret: &Secret) -> ZaSearchKey {
    ZaSearchKey::new(*secret.as_bytes())
}
