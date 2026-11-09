/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Plaintext search blob sections and hidden-section padding (search spec 4.2).

use super::{
    ALIAS_LEN, FormatError, K, PadTarget, RECORD_MAX, REVISION_ROW_LEN, TOMBSTONE_LEN,
    checked_compaction_allowance, checked_padded_len,
    content::{ContentFrame, ContentRecord, OwnedContentFrame},
    read_array, take,
    thread::{Alias, RevisionRow, ThreadRecord, Tombstone},
};
use zeroize::Zeroizing;

pub const SEAL_OVERHEAD: usize = 24 + 16;

const RECORD: u8 = 1;
const ALIAS: u8 = 2;
const MIN_RECORD_LEN: usize = RECORD_MAX - 16 * K;
const LENGTH_PREFIX: usize = size_of::<u32>();

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HiddenSection {
    pub records: Vec<ThreadRecord>,
    pub aliases: Vec<Alias>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PublicSection {
    pub tombstones: Vec<Tombstone>,
    pub rows: Vec<RevisionRow>,
}

pub fn encode_hidden(section: &HiddenSection) -> Vec<u8> {
    let aliases_len = section
        .aliases
        .len()
        .checked_mul(ALIAS_LEN)
        .expect("search alias lengths fit usize");
    let len = section
        .records
        .iter()
        .try_fold(aliases_len, |len, record| {
            len.checked_add(record_len(record))
        })
        .expect("search hidden section length fits usize");
    let mut bytes = Vec::with_capacity(len);
    for record in &section.records {
        bytes.extend_from_slice(&(record_len(record) as u16).to_le_bytes());
        bytes.push(RECORD);
        bytes.extend_from_slice(&record.document_id.to_le_bytes());
        bytes.extend_from_slice(&record.thread_id.to_le_bytes());
        bytes.extend_from_slice(&record.revision.to_le_bytes());
        bytes.extend_from_slice(&record.created.to_le_bytes());
        bytes.extend_from_slice(&record.deleted.unwrap_or_default().to_le_bytes());
        bytes.extend_from_slice(&record.subject_hash.to_le_bytes());
        bytes.push(record.hashes.len() as u8);
        for hash in &record.hashes {
            bytes.extend_from_slice(&hash.to_le_bytes());
        }
    }
    for alias in &section.aliases {
        bytes.extend_from_slice(&(ALIAS_LEN as u16).to_le_bytes());
        bytes.push(ALIAS);
        bytes.extend_from_slice(&alias.source.to_le_bytes());
        bytes.extend_from_slice(&alias.target.to_le_bytes());
        bytes.extend_from_slice(&alias.revision.to_le_bytes());
    }
    bytes
}

fn record_len(record: &ThreadRecord) -> usize {
    assert!(
        record.hashes.len() <= K,
        "search record exceeds its reference hash bound"
    );
    MIN_RECORD_LEN + 16 * record.hashes.len()
}

pub fn decode_hidden(mut bytes: &[u8]) -> Result<HiddenSection, FormatError> {
    let mut section = HiddenSection::default();
    while !bytes.is_empty() {
        let len = usize::from(u16::from_le_bytes(read_array(&mut bytes)?));
        let body_len = len
            .checked_sub(size_of::<u16>())
            .ok_or(FormatError::Count)?;
        let mut body = take(&mut bytes, body_len)?;
        match read_array(&mut body)? {
            [RECORD] => {
                let document_id = u32::from_le_bytes(read_array(&mut body)?);
                let thread_id = u32::from_le_bytes(read_array(&mut body)?);
                let revision = u32::from_le_bytes(read_array(&mut body)?);
                let created = u64::from_le_bytes(read_array(&mut body)?);
                let deleted = u64::from_le_bytes(read_array(&mut body)?);
                let subject_hash = u128::from_le_bytes(read_array(&mut body)?);
                let [count] = read_array(&mut body)?;
                let count = usize::from(count);
                if count > K {
                    return Err(FormatError::Count);
                }
                let mut hashes = Vec::with_capacity(count);
                for _ in 0..count {
                    hashes.push(u128::from_le_bytes(read_array(&mut body)?));
                }
                section.records.push(ThreadRecord {
                    document_id,
                    thread_id,
                    revision,
                    created,
                    deleted: (deleted != 0).then_some(deleted),
                    subject_hash,
                    hashes,
                });
            }
            [ALIAS] => section.aliases.push(Alias {
                source: u32::from_le_bytes(read_array(&mut body)?),
                target: u32::from_le_bytes(read_array(&mut body)?),
                revision: u32::from_le_bytes(read_array(&mut body)?),
            }),
            [kind] => return Err(FormatError::Kind(kind)),
        }
        if !body.is_empty() {
            return Err(FormatError::Count);
        }
    }
    Ok(section)
}

pub fn frame_and_pad(
    hidden: &[u8],
    target: PadTarget,
    min_class: usize,
) -> Result<Vec<u8>, FormatError> {
    let len = u32::try_from(hidden.len()).map_err(|_| FormatError::AllowanceExceeded)?;
    let framed_len = LENGTH_PREFIX
        .checked_add(hidden.len())
        .ok_or(FormatError::AllowanceExceeded)?;
    let padded_len = match target {
        PadTarget::Publishing if framed_len <= min_class => min_class,
        PadTarget::Publishing => return Err(FormatError::AllowanceExceeded),
        PadTarget::Compaction { n_records } => {
            let allowance = checked_compaction_allowance(n_records)
                .filter(|&allowance| framed_len <= allowance)
                .ok_or(FormatError::AllowanceExceeded)?;
            checked_padded_len(allowance, min_class).ok_or(FormatError::AllowanceExceeded)?
        }
    };

    // The thread blob carries the sealed hidden length as a u32.
    padded_len
        .checked_add(SEAL_OVERHEAD)
        .and_then(|sealed_len| u32::try_from(sealed_len).ok())
        .ok_or(FormatError::AllowanceExceeded)?;
    let mut padded = Zeroizing::new(Vec::new());
    padded
        .try_reserve_exact(padded_len)
        .map_err(|_| FormatError::AllowanceExceeded)?;
    padded.extend_from_slice(&len.to_le_bytes());
    padded.extend_from_slice(hidden);
    padded.resize(padded_len, 0);
    Ok(padded.to_vec())
}

pub fn unframe(mut padded: &[u8]) -> Result<&[u8], FormatError> {
    let len = read_len(&mut padded)?;
    take(&mut padded, len)
}

pub fn public_len(tombstones: usize, rows: usize) -> usize {
    tombstones
        .checked_mul(TOMBSTONE_LEN)
        .zip(rows.checked_mul(REVISION_ROW_LEN))
        .and_then(|(tombstones, rows)| tombstones.checked_add(rows))
        .and_then(|len| len.checked_add(2 * LENGTH_PREFIX))
        .expect("search public section length fits usize")
}

pub fn encode_public(section: &PublicSection) -> Vec<u8> {
    let tombstones = encoded_len(section.tombstones.len());
    let rows = encoded_len(section.rows.len());
    let mut bytes = Vec::with_capacity(public_len(section.tombstones.len(), section.rows.len()));
    bytes.extend_from_slice(&tombstones);
    for tombstone in &section.tombstones {
        bytes.extend_from_slice(&tombstone.document_id.to_le_bytes());
        bytes.extend_from_slice(&tombstone.deleted.to_le_bytes());
    }
    bytes.extend_from_slice(&rows);
    for row in &section.rows {
        bytes.extend_from_slice(&row.revision.to_le_bytes());
        bytes.extend_from_slice(&row.change_id.to_le_bytes());
    }
    bytes
}

pub fn decode_public(mut bytes: &[u8]) -> Result<PublicSection, FormatError> {
    let mut tombstone_bytes = read_fixed_entries(&mut bytes, TOMBSTONE_LEN)?;
    let mut row_bytes = read_fixed_entries(&mut bytes, REVISION_ROW_LEN)?;
    if !bytes.is_empty() {
        return Err(FormatError::Count);
    }
    let mut tombstones = Vec::with_capacity(tombstone_bytes.len() / TOMBSTONE_LEN);
    let mut rows = Vec::with_capacity(row_bytes.len() / REVISION_ROW_LEN);
    while !tombstone_bytes.is_empty() {
        tombstones.push(Tombstone {
            document_id: u32::from_le_bytes(read_array(&mut tombstone_bytes)?),
            deleted: u64::from_le_bytes(read_array(&mut tombstone_bytes)?),
        });
    }
    while !row_bytes.is_empty() {
        rows.push(RevisionRow {
            revision: u32::from_le_bytes(read_array(&mut row_bytes)?),
            change_id: u64::from_le_bytes(read_array(&mut row_bytes)?),
        });
    }
    Ok(PublicSection { tombstones, rows })
}

pub fn encode_content(records: &[&[u8]]) -> Vec<u8> {
    let count = encoded_len(records.len());
    let len = records
        .iter()
        .try_fold(LENGTH_PREFIX, |len, record| {
            len.checked_add(LENGTH_PREFIX)?.checked_add(record.len())
        })
        .expect("search content section length fits usize");
    let mut bytes = Vec::with_capacity(len);
    bytes.extend_from_slice(&count);
    for record in records {
        bytes.extend_from_slice(&encoded_len(record.len()));
        bytes.extend_from_slice(record);
    }
    bytes
}

pub fn decode_content(bytes: &[u8]) -> Result<Vec<ContentRecord>, FormatError> {
    let mut records = Vec::new();
    visit_content_frames(bytes, |frame| {
        records.push(ContentRecord::decode(frame)?);
        Ok(())
    })?;
    Ok(records)
}

#[derive(Debug)]
pub enum ContentFramesError {
    Format(FormatError),
    Budget,
}

impl From<FormatError> for ContentFramesError {
    fn from(error: FormatError) -> Self {
        Self::Format(error)
    }
}

/// Reserve additional memory beyond the caller's authenticated plaintext.
/// Scratch is disposed before owned output frames are allocated.
pub fn decode_content_frames(
    bytes: &[u8],
    mut reserve: impl FnMut(usize) -> bool,
) -> Result<Vec<OwnedContentFrame>, ContentFramesError> {
    let mut scratch_size = 0;
    let mut payload = 0usize;
    let count = visit_content_frames(bytes, |bytes| {
        let frame = ContentFrame::parse(bytes)?;
        scratch_size = scratch_size.max(frame.scratch_size()?);
        payload = payload.checked_add(bytes.len()).ok_or(FormatError::Count)?;
        Ok(())
    })?;
    if !reserve(scratch_size) {
        return Err(ContentFramesError::Budget);
    }
    let mut scratch = Zeroizing::new(Vec::with_capacity(scratch_size));
    visit_content_frames(bytes, |bytes| {
        ContentFrame::parse(bytes)?.validate_terms(&mut scratch)
    })?;
    drop(scratch);

    let size = count
        .checked_mul(size_of::<OwnedContentFrame>())
        .and_then(|metadata| payload.checked_add(metadata))
        .ok_or(FormatError::Count)?;
    if !reserve(size) {
        return Err(ContentFramesError::Budget);
    }
    let mut frames = Vec::with_capacity(count);
    visit_content_frames(bytes, |bytes| {
        frames.push(OwnedContentFrame(Zeroizing::new(bytes.into())));
        Ok(())
    })?;
    Ok(frames)
}

fn visit_content_frames(
    mut bytes: &[u8],
    mut visit: impl FnMut(&[u8]) -> Result<(), FormatError>,
) -> Result<usize, FormatError> {
    let count = read_len(&mut bytes)?;
    if count > bytes.len() / LENGTH_PREFIX {
        return Err(FormatError::Truncated);
    }
    for _ in 0..count {
        let len = read_len(&mut bytes)?;
        visit(take(&mut bytes, len)?)?;
    }
    if !bytes.is_empty() {
        return Err(FormatError::Count);
    }
    Ok(count)
}

fn encoded_len(len: usize) -> [u8; LENGTH_PREFIX] {
    u32::try_from(len)
        .expect("search format count or record length fits u32")
        .to_le_bytes()
}

fn read_len(bytes: &mut &[u8]) -> Result<usize, FormatError> {
    usize::try_from(u32::from_le_bytes(read_array(bytes)?)).map_err(|_| FormatError::Count)
}

fn read_fixed_entries<'a>(bytes: &mut &'a [u8], entry_len: usize) -> Result<&'a [u8], FormatError> {
    let count = read_len(bytes)?;
    let len = count.checked_mul(entry_len).ok_or(FormatError::Count)?;
    take(bytes, len)
}
