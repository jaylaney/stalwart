/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access search format version 1 (search spec 4.2).

pub mod blob;
pub mod content;
pub mod manifest;
pub mod thread;

#[cfg(test)]
mod blob_tests;
#[cfg(test)]
mod manifest_tests;

use types::blob_hash::BlobHash;
use zeroize::Zeroizing;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SegmentId(pub [u8; 16]);

impl SegmentId {
    pub fn random() -> Self {
        Self(rand::random())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BlobKind {
    Thread = 1,
    Content = 2,
}

impl TryFrom<u8> for BlobKind {
    type Error = FormatError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Thread),
            2 => Ok(Self::Content),
            value => Err(FormatError::Kind(value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SectionKind {
    Hidden = 1,
    Public = 2,
    Content = 3,
}

impl TryFrom<u8> for SectionKind {
    type Error = FormatError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Hidden),
            2 => Ok(Self::Public),
            3 => Ok(Self::Content),
            value => Err(FormatError::Kind(value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobHeader {
    pub kind: BlobKind,
    pub tier: u8,
    pub segment: SegmentId,
    pub predecessor: Option<(BlobHash, SegmentId)>,
}

impl BlobHeader {
    pub const LEN: usize = 1 + 1 + 1 + 16 + 32 + 16;

    pub fn encode(&self) -> [u8; Self::LEN] {
        let mut bytes = [0; Self::LEN];
        bytes[..3].copy_from_slice(&[SEARCH_FORMAT_V1, self.kind as u8, self.tier]);
        bytes[3..19].copy_from_slice(&self.segment.0);
        if let Some((hash, segment)) = &self.predecessor {
            bytes[19..51].copy_from_slice(hash.as_slice());
            bytes[51..].copy_from_slice(&segment.0);
        }
        bytes
    }

    pub fn decode(mut bytes: &[u8]) -> Result<(Self, &[u8]), FormatError> {
        let [version] = read_array(&mut bytes)?;
        if version != SEARCH_FORMAT_V1 {
            return Err(FormatError::Version(version));
        }
        let [kind] = read_array(&mut bytes)?;
        let kind = BlobKind::try_from(kind)?;
        let [tier] = read_array(&mut bytes)?;
        let segment = SegmentId(read_array(&mut bytes)?);
        let predecessor_hash = BlobHash(read_array(&mut bytes)?);
        let predecessor_segment = SegmentId(read_array(&mut bytes)?);
        let predecessor = (!predecessor_hash.is_empty() || predecessor_segment.0 != [0; 16])
            .then_some((predecessor_hash, predecessor_segment));
        Ok((
            Self {
                kind,
                tier,
                segment,
                predecessor,
            },
            bytes,
        ))
    }
}

#[derive(Clone)]
pub struct ZaSearchKey(Zeroizing<[u8; 32]>);

impl ZaSearchKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for ZaSearchKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ZaSearchKey(..)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadTarget {
    Publishing,
    Compaction { n_records: usize },
}

fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8], FormatError> {
    let (prefix, rest) = bytes.split_at_checked(len).ok_or(FormatError::Truncated)?;
    *bytes = rest;
    Ok(prefix)
}

fn read_array<const N: usize>(bytes: &mut &[u8]) -> Result<[u8; N], FormatError> {
    let (array, rest) = bytes.split_first_chunk().ok_or(FormatError::Truncated)?;
    *bytes = rest;
    Ok(*array)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    pub change_id: u64,
    pub position: u32,
    pub complete: bool,
}

impl Default for Checkpoint {
    fn default() -> Self {
        Self {
            change_id: 0,
            position: 0,
            complete: true,
        }
    }
}

pub struct ThreadFieldsK {
    pub hashes: Vec<u128>,
    pub subject_hash: u128,
    pub sent_at: Option<i64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FormatError {
    Version(u8),
    Kind(u8),
    Truncated,
    Count,
    AllowanceExceeded,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Version(version) => write!(f, "unsupported search format version {version}"),
            Self::Kind(kind) => write!(f, "unknown search format kind {kind}"),
            Self::Truncated => f.write_str("truncated search format"),
            Self::Count => f.write_str("invalid search format count"),
            Self::AllowanceExceeded => f.write_str("search format allowance exceeded"),
        }
    }
}

// Encoded layouts, including each length prefix:
// thread record = u16 len | u8 flags | u32 document_id | u32 thread_id | u32 revision
//               | u64 created | u64 deleted (0 = none) | u128 subject_hash
//               | u8 n | n × u128 hash = 48 + 16·n bytes, n ≤ K
// alias         = u16 len | u8 kind | u32 source | u32 target | u32 revision
// tombstone     = u32 document_id | u64 deleted
// revision row  = u32 revision | u64 change_id
pub const SEARCH_FORMAT_V1: u8 = 1;
pub const K: usize = 16;
pub const RECORD_MAX: usize = 48 + 16 * K;
pub const ALIAS_LEN: usize = 15;
pub const TOMBSTONE_LEN: usize = 12;
pub const REVISION_ROW_LEN: usize = 12;

/// One admitted item's charge against the hidden budget (spec 4.2, R21).
pub const fn reservation() -> usize {
    RECORD_MAX + (K - 1) * ALIAS_LEN
}

/// Round the framed length up to a power of two, with a minimum class (spec 4.2).
pub fn padded_len(framed_len: usize, min_class: usize) -> usize {
    checked_padded_len(framed_len, min_class).expect("search padding class fits usize")
}

fn checked_padded_len(framed_len: usize, min_class: usize) -> Option<usize> {
    Some(
        framed_len
            .max(1)
            .checked_next_power_of_two()?
            .max(min_class),
    )
}

/// The framed length a compacted hidden section pads to (spec 4.2, 5.5).
pub fn compaction_allowance(n_records: usize) -> usize {
    checked_compaction_allowance(n_records).expect("search compaction allowance fits usize")
}

fn checked_compaction_allowance(n_records: usize) -> Option<usize> {
    n_records
        .checked_mul(RECORD_MAX + ALIAS_LEN)?
        .checked_add(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_bound_and_the_reservation_are_the_specs() {
        assert_eq!(K, 16);
        assert_eq!(RECORD_MAX, 304);
        assert_eq!(ALIAS_LEN, 15);
        assert_eq!(TOMBSTONE_LEN, 12);
        assert_eq!(REVISION_ROW_LEN, 12);
        assert_eq!(reservation(), 529);
    }

    #[test]
    fn padding_rounds_to_a_class_above_the_minimum() {
        const M: usize = 65_536;
        assert_eq!(padded_len(1, M), M);
        assert_eq!(padded_len(33_301, M), M); // H + R + (K − 1)·a + 4 at the defaults
        assert_eq!(padded_len(M - 1, M), M); // a payload of M − 5 bytes
        assert_eq!(padded_len(M, M), M); // a payload of M − 4 bytes
        assert_eq!(padded_len(M + 1, M), 2 * M);
        assert_eq!(padded_len(2 * M, M), 2 * M);
        assert_eq!(padded_len(2 * M + 1, M), 4 * M);
        assert_eq!(padded_len(100, 4096), 4096);
    }

    #[test]
    fn the_compaction_allowance_is_a_function_of_the_record_count() {
        assert_eq!(compaction_allowance(0), 4);
        assert_eq!(compaction_allowance(1), RECORD_MAX + ALIAS_LEN + 4);
        assert_eq!(compaction_allowance(30_000), 30_000 * 319 + 4);
        // 30,000 messages: a 16 MiB class (spec 5.5, decision 2 (c)).
        assert_eq!(
            padded_len(compaction_allowance(30_000), 65_536),
            16 * 1024 * 1024
        );
    }
}
