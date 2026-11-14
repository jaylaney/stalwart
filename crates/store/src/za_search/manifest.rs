/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Search index descriptors, authenticated at commit, and keyless destroy inventory.

use super::{
    Checkpoint, FormatError, SEARCH_FORMAT_V1, SegmentId, ZaSearchKey, read_array,
    thread::RevisionRow,
};
use crate::write::{AssignedIds, SerializeWithIds, assert::AssertValue};
use types::{blob_hash::BlobHash, collection::SyncCollection};
use xxhash_rust::xxh3::xxh3_64;

const INDEX: u8 = 1;
const DESTROYING: u8 = 2;
const HEADER_LEN: usize = 2;
const THREAD_ENTRY_LEN: usize = 16 + 32 + 4;
const CONTENT_HEAD_LEN: usize = 32 + 16 + 4 + 8;
const CHECKPOINT_LEN: usize = 8 + 4 + 1;
const TAG_LEN: usize = blake3::OUT_LEN;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ThreadBlobEntry {
    pub segment: SegmentId,
    pub hash: BlobHash,
    pub size: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ContentHead {
    pub hash: BlobHash,
    pub segment: SegmentId,
    pub length: u32,
    pub size: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ContentTo {
    pub checkpoint: Checkpoint,
    pub suspended: bool,
    pub cursor: Option<ContentCursor>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ContentCursor {
    /// Exclusive upper bound for the remaining documents in a newest-first walk.
    pub next_below: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RebuildCursor {
    /// Lowest remaining document id and inclusive upper bound of the snapshot.
    pub next: u32,
    pub end: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReconciliationMarker {
    pub bound: u64,
    pub gap_next: u32,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FailingDocument {
    pub document_id: u32,
    pub attempts: u8,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub revision: u32,
    pub newest_row: RevisionRow,
    pub indexed_to: Checkpoint,
    pub epoch: u64,
    pub thread_blobs: Vec<ThreadBlobEntry>,
    pub content: Option<ContentHead>,
    pub content_to: ContentTo,
    pub cursor: Option<RebuildCursor>,
    pub reconciliation: Option<ReconciliationMarker>,
    pub failing: Option<FailingDocument>,
}

impl Manifest {
    pub fn identity_version(&self) -> (u64, u32) {
        (self.epoch, self.revision)
    }

    pub fn inventory(&self) -> Inventory {
        Inventory {
            thread_blobs: self.thread_blobs.clone(),
            content: self.content.clone(),
        }
    }

    fn encode_fields(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.revision.to_le_bytes());
        bytes.extend_from_slice(&self.newest_row.revision.to_le_bytes());
        bytes.extend_from_slice(&self.newest_row.change_id.to_le_bytes());
        encode_checkpoint(bytes, &self.indexed_to);
        bytes.extend_from_slice(&self.epoch.to_le_bytes());
        encode_inventory(bytes, &self.thread_blobs, self.content.as_ref());

        encode_checkpoint(bytes, &self.content_to.checkpoint);
        bytes.push(u8::from(self.content_to.suspended));
        encode_optional(bytes, self.content_to.cursor.as_ref(), |bytes, cursor| {
            bytes.extend_from_slice(&cursor.next_below.to_le_bytes());
        });
        encode_optional(bytes, self.cursor.as_ref(), |bytes, cursor| {
            bytes.extend_from_slice(&cursor.next.to_le_bytes());
            bytes.extend_from_slice(&cursor.end.to_le_bytes());
        });
        encode_optional(bytes, self.reconciliation.as_ref(), |bytes, marker| {
            bytes.extend_from_slice(&marker.bound.to_le_bytes());
            bytes.extend_from_slice(&marker.gap_next.to_le_bytes());
        });
        encode_optional(bytes, self.failing.as_ref(), |bytes, failing| {
            bytes.extend_from_slice(&failing.document_id.to_le_bytes());
            bytes.push(failing.attempts);
        });
    }

    fn decode_fields(bytes: &mut &[u8]) -> Result<Self, FormatError> {
        let revision = u32::from_le_bytes(read_array(bytes)?);
        let newest_row = RevisionRow {
            revision: u32::from_le_bytes(read_array(bytes)?),
            change_id: u64::from_le_bytes(read_array(bytes)?),
        };
        let indexed_to = decode_checkpoint(bytes)?;
        let epoch = u64::from_le_bytes(read_array(bytes)?);
        let Inventory {
            thread_blobs,
            content,
        } = decode_inventory_fields(bytes)?;
        let content_to = ContentTo {
            checkpoint: decode_checkpoint(bytes)?,
            suspended: read_bool(bytes)?,
            cursor: decode_optional(bytes, |bytes| {
                Ok(ContentCursor {
                    next_below: u32::from_le_bytes(read_array(bytes)?),
                })
            })?,
        };
        let cursor = decode_optional(bytes, |bytes| {
            Ok(RebuildCursor {
                next: u32::from_le_bytes(read_array(bytes)?),
                end: u32::from_le_bytes(read_array(bytes)?),
            })
        })?;
        let reconciliation = decode_optional(bytes, |bytes| {
            Ok(ReconciliationMarker {
                bound: u64::from_le_bytes(read_array(bytes)?),
                gap_next: u32::from_le_bytes(read_array(bytes)?),
            })
        })?;
        let failing = decode_optional(bytes, |bytes| {
            let document_id = u32::from_le_bytes(read_array(bytes)?);
            let [attempts] = read_array(bytes)?;
            Ok(FailingDocument {
                document_id,
                attempts,
            })
        })?;
        Ok(Self {
            revision,
            newest_row,
            indexed_to,
            epoch,
            thread_blobs,
            content,
            content_to,
            cursor,
            reconciliation,
            failing,
        })
    }

    fn encoded_len(&self) -> usize {
        HEADER_LEN
            + 4
            + 4
            + 8
            + CHECKPOINT_LEN
            + 8
            + inventory_len(self.thread_blobs.len(), self.content.is_some())
            + CHECKPOINT_LEN
            + 1
            + optional_len(self.content_to.cursor.is_some(), 4)
            + optional_len(self.cursor.is_some(), 8)
            + optional_len(self.reconciliation.is_some(), 12)
            + optional_len(self.failing.is_some(), 5)
            + TAG_LEN
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub thread_blobs: Vec<ThreadBlobEntry>,
    pub content: Option<ContentHead>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestValue {
    Index(Manifest),
    Destroying(Inventory),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestError {
    Tag,
    Parse,
    NewerFormat(u8),
    OlderFormat(u8),
}

impl ManifestValue {
    pub fn encode(&self, key: &ZaSearchKey) -> Vec<u8> {
        match self {
            Self::Index(manifest) => {
                let mut bytes = Vec::with_capacity(manifest.encoded_len());
                bytes.extend_from_slice(&[SEARCH_FORMAT_V1, INDEX]);
                manifest.encode_fields(&mut bytes);
                let tag = blake3::keyed_hash(key.as_bytes(), &bytes);
                bytes.extend_from_slice(tag.as_bytes());
                bytes
            }
            Self::Destroying(inventory) => Self::encode_destroying(inventory),
        }
    }

    pub fn encode_destroying(inventory: &Inventory) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            HEADER_LEN + inventory_len(inventory.thread_blobs.len(), inventory.content.is_some()),
        );
        bytes.extend_from_slice(&[SEARCH_FORMAT_V1, DESTROYING]);
        encode_inventory(
            &mut bytes,
            &inventory.thread_blobs,
            inventory.content.as_ref(),
        );
        bytes
    }

    pub fn decode(bytes: &[u8], key: &ZaSearchKey) -> Result<Self, ManifestError> {
        Self::decode_inner(bytes, Some(key))
    }

    pub fn decode_unverified(bytes: &[u8]) -> Result<Self, ManifestError> {
        Self::decode_inner(bytes, None)
    }

    #[cfg(test)]
    pub fn decode_inventory(bytes: &[u8]) -> Result<Inventory, ManifestError> {
        Ok(match Self::decode_unverified(bytes)? {
            Self::Index(manifest) => Inventory {
                thread_blobs: manifest.thread_blobs,
                content: manifest.content,
            },
            Self::Destroying(inventory) => inventory,
        })
    }

    fn decode_inner(bytes: &[u8], key: Option<&ZaSearchKey>) -> Result<Self, ManifestError> {
        let (&version, rest) = bytes.split_first().ok_or(ManifestError::Parse)?;
        if version > SEARCH_FORMAT_V1 {
            return Err(ManifestError::NewerFormat(version));
        }
        if version < SEARCH_FORMAT_V1 {
            return Err(ManifestError::OlderFormat(version));
        }
        let (&kind, mut fields) = rest.split_first().ok_or(ManifestError::Parse)?;
        let value = match kind {
            INDEX => {
                let (body, tag) = fields
                    .split_last_chunk::<TAG_LEN>()
                    .ok_or(ManifestError::Parse)?;
                fields = body;
                if let Some(key) = key {
                    let expected =
                        blake3::keyed_hash(key.as_bytes(), &bytes[..bytes.len() - TAG_LEN]);
                    if blake3::Hash::from_bytes(*tag) != expected {
                        return Err(ManifestError::Tag);
                    }
                }
                Self::Index(Manifest::decode_fields(&mut fields).map_err(|_| ManifestError::Parse)?)
            }
            DESTROYING => Self::Destroying(
                decode_inventory_fields(&mut fields).map_err(|_| ManifestError::Parse)?,
            ),
            _ => return Err(ManifestError::Parse),
        };
        if !fields.is_empty() {
            return Err(ManifestError::Parse);
        }
        Ok(value)
    }

    fn encoded_len(&self) -> usize {
        match self {
            Self::Index(manifest) => manifest.encoded_len(),
            Self::Destroying(inventory) => {
                HEADER_LEN
                    + inventory_len(inventory.thread_blobs.len(), inventory.content.is_some())
            }
        }
    }
}

fn encode_inventory(
    bytes: &mut Vec<u8>,
    thread_blobs: &[ThreadBlobEntry],
    content: Option<&ContentHead>,
) {
    debug_assert!(thread_blobs.len() <= usize::from(u8::MAX));
    bytes.push(u8::try_from(thread_blobs.len()).expect("manifest thread blob count fits u8"));
    for entry in thread_blobs {
        bytes.extend_from_slice(&entry.segment.0);
        bytes.extend_from_slice(entry.hash.as_slice());
        bytes.extend_from_slice(&entry.size.to_le_bytes());
    }
    encode_optional(bytes, content, |bytes, head| {
        bytes.extend_from_slice(head.hash.as_slice());
        bytes.extend_from_slice(&head.segment.0);
        bytes.extend_from_slice(&head.length.to_le_bytes());
        bytes.extend_from_slice(&head.size.to_le_bytes());
    });
}

fn decode_inventory_fields(bytes: &mut &[u8]) -> Result<Inventory, FormatError> {
    let [count] = read_array(bytes)?;
    let mut thread_blobs = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        thread_blobs.push(ThreadBlobEntry {
            segment: SegmentId(read_array(bytes)?),
            hash: BlobHash(read_array(bytes)?),
            size: u32::from_le_bytes(read_array(bytes)?),
        });
    }
    let content = decode_optional(bytes, |bytes| {
        Ok(ContentHead {
            hash: BlobHash(read_array(bytes)?),
            segment: SegmentId(read_array(bytes)?),
            length: u32::from_le_bytes(read_array(bytes)?),
            size: u64::from_le_bytes(read_array(bytes)?),
        })
    })?;
    Ok(Inventory {
        thread_blobs,
        content,
    })
}

fn inventory_len(thread_count: usize, has_content: bool) -> usize {
    let thread_count = u8::try_from(thread_count).expect("manifest thread blob count fits u8");
    1 + usize::from(thread_count) * THREAD_ENTRY_LEN + optional_len(has_content, CONTENT_HEAD_LEN)
}

fn encode_checkpoint(bytes: &mut Vec<u8>, checkpoint: &Checkpoint) {
    bytes.extend_from_slice(&checkpoint.change_id.to_le_bytes());
    bytes.extend_from_slice(&checkpoint.position.to_le_bytes());
    bytes.push(u8::from(checkpoint.complete));
}

fn decode_checkpoint(bytes: &mut &[u8]) -> Result<Checkpoint, FormatError> {
    Ok(Checkpoint {
        change_id: u64::from_le_bytes(read_array(bytes)?),
        position: u32::from_le_bytes(read_array(bytes)?),
        complete: read_bool(bytes)?,
    })
}

fn encode_optional<T>(
    bytes: &mut Vec<u8>,
    value: Option<&T>,
    encode: impl FnOnce(&mut Vec<u8>, &T),
) {
    bytes.push(u8::from(value.is_some()));
    if let Some(value) = value {
        encode(bytes, value);
    }
}

fn decode_optional<T>(
    bytes: &mut &[u8],
    decode: impl FnOnce(&mut &[u8]) -> Result<T, FormatError>,
) -> Result<Option<T>, FormatError> {
    if read_bool(bytes)? {
        Ok(Some(decode(bytes)?))
    } else {
        Ok(None)
    }
}

fn read_bool(bytes: &mut &[u8]) -> Result<bool, FormatError> {
    match read_array(bytes)? {
        [0] => Ok(false),
        [1] => Ok(true),
        [value] => Err(FormatError::Kind(value)),
    }
}

fn optional_len(present: bool, body_len: usize) -> usize {
    1 + usize::from(present) * body_len
}

pub fn manifest_hash(bytes: &[u8]) -> [u8; 32] {
    blake3::hash(bytes).into()
}

pub fn manifest_assertion(previous: Option<&[u8]>) -> AssertValue {
    previous.map_or(AssertValue::None, |bytes| AssertValue::Hash(xxh3_64(bytes)))
}

#[derive(Clone, Copy, Default)]
pub struct ManifestFill {
    pub newest_row: bool,
    pub epoch: bool,
}

pub struct ManifestWrite {
    value: ManifestValue,
    key: ZaSearchKey,
    fill: ManifestFill,
    account_id: Option<u32>,
}

impl ManifestWrite {
    /// Use the batch cursor for an implicit change ID. Principal writers use `for_account`.
    pub fn new(value: ManifestValue, key: ZaSearchKey, fill: ManifestFill) -> Self {
        Self {
            value,
            key,
            fill,
            account_id: None,
        }
    }

    /// Fill from the account's Email counter even though the field is stored
    /// under Principal, whose batch cursor has no Email change id.
    pub fn for_account(
        value: ManifestValue,
        key: ZaSearchKey,
        fill: ManifestFill,
        account_id: u32,
    ) -> Self {
        Self {
            account_id: Some(account_id),
            ..Self::new(value, key, fill)
        }
    }
}

impl SerializeWithIds for ManifestWrite {
    fn serialize_with_ids(&mut self, ids: &AssignedIds) -> trc::Result<(Vec<u8>, Option<u32>)> {
        if let ManifestValue::Index(manifest) = &mut self.value
            && (self.fill.newest_row || self.fill.epoch)
        {
            let change_id = match self.account_id {
                Some(account_id) => ids.change_id(account_id, SyncCollection::Email.change_group()),
                None => ids.try_current_change_id(),
            }
            .filter(|&id| id != 0)
            .ok_or_else(|| {
                trc::StoreEvent::UnexpectedError
                    .into_err()
                    .details("search publication requires a change id")
            })?;
            if self.fill.newest_row {
                manifest.newest_row.change_id = change_id;
            }
            if self.fill.epoch {
                manifest.epoch = change_id;
            }
        }
        Ok((self.value.encode(&self.key), None))
    }

    fn size_hint(&self) -> usize {
        self.value.encoded_len()
    }
}
