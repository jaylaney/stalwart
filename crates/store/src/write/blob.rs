/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{BlobOp, Operation, ValueClass, ValueOp, key::DeserializeBigEndian, now};
use crate::{
    BlobStore, Deserialize, IterateParams, SerializeInfallible, Store, U16_LEN, U32_LEN, U64_LEN,
    ValueKey,
    write::{BatchBuilder, BlobLink, RegistryClass, SetValue, SizedSetValue},
};
use registry::{
    schema::prelude::Property,
    types::{EnumImpl, id::ObjectId},
};
use std::time::Instant;
use trc::{AddContext, StoreEvent};
// SPDX-SnippetBegin
// SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
// SPDX-License-Identifier: LicenseRef-SEL
#[cfg(feature = "enterprise")]
use registry::schema::prelude::ObjectType;
// SPDX-SnippetEnd
use types::{
    blob::BlobClass,
    blob_hash::{BLOB_HASH_LEN, BlobHash},
    collection::Collection,
};
use utils::codec::leb128::{Leb128, Leb128Reader, Leb128Writer};

const OBJECT_ID_LEN: usize = U16_LEN + U64_LEN;
const MAX_LEB128_U64_LEN: usize = 10;
const BLOB_DELETE_CHUNK: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobLinkValue {
    pub size: u64,
    pub object_id: Option<ObjectId>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BlobQuota {
    pub bytes: usize,
    pub count: usize,
}

impl BlobLinkValue {
    pub fn new(size: impl TryInto<u64>) -> Self {
        BlobLinkValue {
            size: size.try_into().unwrap_or(u64::MAX),
            object_id: None,
        }
    }

    pub fn with_object_id(mut self, object_id: ObjectId) -> Self {
        self.object_id = Some(object_id);
        self
    }
}

impl SerializeInfallible for BlobLinkValue {
    fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            self.size.leb128_len() + self.object_id.map_or(0, |_| OBJECT_ID_LEN),
        );
        bytes.push_leb128(self.size);
        if let Some(object_id) = &self.object_id {
            bytes.extend_from_slice(&object_id.serialize());
        }
        bytes
    }
}

impl Deserialize for BlobLinkValue {
    fn deserialize(bytes: &[u8]) -> trc::Result<Self> {
        let mut remaining = bytes;
        let size = remaining.read_leb128::<u64>();
        match (size, remaining.len()) {
            (Some(size), 0) => Ok(BlobLinkValue::new(size)),
            (Some(size), OBJECT_ID_LEN) => {
                Ok(BlobLinkValue::new(size).with_object_id(ObjectId::deserialize(remaining)?))
            }
            _ => Err(trc::StoreEvent::DataCorruption
                .into_err()
                .details("Invalid blob link value")
                .ctx(trc::Key::Value, bytes)
                .caused_by(trc::location!())),
        }
    }
}

impl From<BlobLinkValue> for SizedSetValue {
    fn from(value: BlobLinkValue) -> Self {
        SetValue::fixed(value.serialize())
    }
}

fn blob_link_shard_range(shard_index: u8) -> IterateParams<ValueKey<ValueClass>> {
    let mut from_hash = BlobHash::default();
    let mut to_hash = BlobHash::new_max();
    from_hash.0[0] = shard_index;
    to_hash.0[0] = shard_index;
    let from_key = ValueKey {
        account_id: 0,
        collection: 0,
        document_id: 0,
        class: ValueClass::Blob(BlobOp::Commit { hash: from_hash }),
    };
    let to_key = ValueKey {
        account_id: u32::MAX,
        collection: u8::MAX,
        document_id: u32::MAX,
        class: ValueClass::Blob(BlobOp::Link {
            hash: to_hash,
            to: BlobLink::Document,
        }),
    };
    IterateParams::new(from_key, to_key).ascending()
}

impl Store {
    pub async fn blob_exists(&self, hash: impl AsRef<BlobHash> + Sync + Send) -> trc::Result<bool> {
        self.key_exists(ValueKey {
            account_id: 0,
            collection: 0,
            document_id: 0,
            class: ValueClass::Blob(BlobOp::Commit {
                hash: hash.as_ref().clone(),
            }),
        })
        .await
        .caused_by(trc::location!())
    }

    pub async fn blob_link_size(
        &self,
        hash: impl AsRef<BlobHash> + Sync + Send,
        class: impl AsRef<BlobClass> + Sync + Send,
    ) -> trc::Result<Option<u64>> {
        let key = match class.as_ref() {
            BlobClass::Reserved {
                account_id,
                expires,
            } if *expires > now() => ValueKey {
                account_id: *account_id,
                collection: 0,
                document_id: 0,
                class: ValueClass::Blob(BlobOp::Link {
                    hash: hash.as_ref().clone(),
                    to: BlobLink::Temporary { until: *expires },
                }),
            },
            BlobClass::Linked {
                account_id,
                collection,
                document_id,
            } => ValueKey {
                account_id: *account_id,
                collection: *collection,
                document_id: *document_id,
                class: ValueClass::Blob(BlobOp::Link {
                    hash: hash.as_ref().clone(),
                    to: BlobLink::Document,
                }),
            },
            _ => return Ok(None),
        };

        self.get_value::<BlobLinkValue>(key)
            .await
            .map(|value| value.map(|value| value.size))
    }

    pub async fn purge_blobs_all_shards(&self, blob_store: BlobStore) -> trc::Result<()> {
        for shard_index in 0u8..=255 {
            self.purge_blobs(blob_store.clone(), shard_index).await?;
        }
        Ok(())
    }

    pub async fn purge_blobs(&self, blob_store: BlobStore, shard_index: u8) -> trc::Result<()> {
        let mut total_active = 0;
        let mut total_deleted = 0;
        let started = Instant::now();

        // Validate linked blobs
        let mut state = BlobPurgeState::new();
        self.iterate(blob_link_shard_range(shard_index), |key, value| {
            let hash =
                BlobHash::try_from_hash_slice(key.get(0..BLOB_HASH_LEN).ok_or_else(|| {
                    trc::Error::corrupted_key(key, value.into(), trc::location!())
                })?)
                .unwrap();

            state.update_hash(hash);
            state.process_key(key, value)?;

            Ok(true)
        })
        .await
        .caused_by(trc::location!())?;

        state.finalize(BlobHash::default());

        // Delete expired or unlinked blobs
        let mut hashes = Vec::with_capacity(BLOB_DELETE_CHUNK.min(state.delete_keys.len()));
        for (_, op) in &state.delete_keys {
            if let BlobOp::Commit { hash } = op {
                hashes.push(hash.as_ref());
                if hashes.len() == BLOB_DELETE_CHUNK {
                    blob_store
                        .delete_blobs(&hashes)
                        .await
                        .caused_by(trc::location!())?;
                    hashes.clear();
                }
            }
        }
        if !hashes.is_empty() {
            blob_store
                .delete_blobs(&hashes)
                .await
                .caused_by(trc::location!())?;
        }

        // Delete hashes
        let mut batch = BatchBuilder::new();
        for (account_id, op) in state.delete_keys {
            if batch.is_large_bulk_batch() {
                self.write_batch(&mut batch)
                    .await
                    .caused_by(trc::location!())?;
                batch = BatchBuilder::new();
            }

            if let Some(account_id) = account_id {
                batch.with_account_id(account_id);
            }

            batch.any_op(Operation::Value {
                class: ValueClass::Blob(op),
                op: ValueOp::Clear,
            });
        }
        for (account_id, object_id) in state.delete_registry {
            if batch.is_large_bulk_batch() {
                self.write_batch(&mut batch)
                    .await
                    .caused_by(trc::location!())?;
                batch = BatchBuilder::new();
            }

            let item_id = object_id.id().id();
            let object_id = object_id.object().to_id();

            batch
                .clear(ValueClass::Registry(RegistryClass::Index {
                    index_id: Property::AccountId.to_id(),
                    object_id,
                    item_id,
                    key: (account_id as u64).serialize(),
                }))
                .clear(ValueClass::Registry(RegistryClass::Item {
                    object_id,
                    item_id,
                }));
        }
        if !batch.is_empty() {
            self.write_batch(&mut batch)
                .await
                .caused_by(trc::location!())?;
        }

        total_active += state.total_active - 1; // Exclude default hash
        total_deleted += state.total_deleted;

        trc::event!(
            Store(StoreEvent::BlobStorePurged),
            Partition = shard_index as u16,
            Total = total_deleted,
            Used = total_active,
            Elapsed = started.elapsed()
        );

        Ok(())
    }
}

impl Store {
    /// Find the search index's document links, including links whose blob is
    /// missing. The subspace is keyed by hash, so fallback visits every shard.
    pub async fn za_principal_blob_links(&self, account_id: u32) -> trc::Result<Vec<BlobHash>> {
        let mut suffix = [0; U32_LEN + 1 + U32_LEN];
        suffix[..U32_LEN].copy_from_slice(&account_id.to_be_bytes());
        suffix[U32_LEN] = Collection::Principal as u8;

        let mut hashes = Vec::new();
        for shard_index in 0u8..=255 {
            self.iterate(blob_link_shard_range(shard_index).no_values(), |key, _| {
                if key.get(BLOB_HASH_LEN..) == Some(suffix.as_slice()) {
                    hashes.push(BlobHash::try_from_hash_slice(&key[..BLOB_HASH_LEN]).unwrap());
                }
                Ok(true)
            })
            .await
            .caused_by(trc::location!())?;
        }
        Ok(hashes)
    }

    pub async fn user_blob_hashes(&self, shard_index: u8) -> trc::Result<Vec<BlobHash>> {
        const TEMP_LINK: usize = BLOB_HASH_LEN + U32_LEN + U64_LEN;
        const DOC_LINK: usize = BLOB_HASH_LEN + U64_LEN + 1;
        const ID_LINK: usize = BLOB_HASH_LEN + U64_LEN;
        const COLLECTION_OFFSET: usize = BLOB_HASH_LEN + U32_LEN;

        let mut from_hash = BlobHash::default();
        let mut to_hash = BlobHash::new_max();
        from_hash.0[0] = shard_index;
        to_hash.0[0] = shard_index;
        let from_key = ValueKey {
            account_id: 0,
            collection: 0,
            document_id: 0,
            class: ValueClass::Blob(BlobOp::Commit { hash: from_hash }),
        };
        let to_key = ValueKey {
            account_id: u32::MAX,
            collection: u8::MAX,
            document_id: u32::MAX,
            class: ValueClass::Blob(BlobOp::Link {
                hash: to_hash,
                to: BlobLink::Document,
            }),
        };

        let mut hashes: Vec<BlobHash> = Vec::new();
        let mut committed: Option<BlobHash> = None;
        self.iterate(
            IterateParams::new(from_key, to_key).ascending(),
            |key, value| {
                let hash = key
                    .get(0..BLOB_HASH_LEN)
                    .and_then(|hash| BlobHash::try_from_hash_slice(hash).ok())
                    .ok_or_else(|| {
                        trc::Error::corrupted_key(key, value.into(), trc::location!())
                    })?;
                let is_user_data = match key.len() {
                    BLOB_HASH_LEN => {
                        committed = Some(hash);
                        return Ok(true);
                    }
                    DOC_LINK => key.get(COLLECTION_OFFSET).is_some_and(|collection| {
                        matches!(
                            Collection::from(*collection),
                            Collection::Email
                                | Collection::SieveScript
                                | Collection::FileNode
                                | Collection::Principal
                        )
                    }),
                    ID_LINK => true,
                    TEMP_LINK => BlobLinkValue::is_archived_item(value),
                    _ => false,
                };
                if is_user_data && committed.as_ref() == Some(&hash) && hashes.last() != Some(&hash)
                {
                    hashes.push(hash);
                }
                Ok(true)
            },
        )
        .await
        .caused_by(trc::location!())?;

        Ok(hashes)
    }
}

impl BlobLinkValue {
    // SPDX-SnippetBegin
    // SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
    // SPDX-License-Identifier: LicenseRef-SEL
    #[cfg(feature = "enterprise")]
    fn is_archived_item(value: &[u8]) -> bool {
        value.len() > MAX_LEB128_U64_LEN
            && value
                .split_last_chunk::<OBJECT_ID_LEN>()
                .and_then(|(_, object_id)| ObjectId::deserialize(object_id).ok())
                .is_some_and(|object_id| object_id.object() == ObjectType::ArchivedItem)
    }
    // SPDX-SnippetEnd

    #[cfg(not(feature = "enterprise"))]
    fn is_archived_item(_value: &[u8]) -> bool {
        false
    }
}

struct BlobPurgeState {
    last_hash: BlobHash,
    last_hash_is_linked: bool,
    delete_keys: Vec<(Option<u32>, BlobOp)>,
    delete_registry: Vec<(u32, ObjectId)>,
    now: u64,
    total_deleted: u64,
    total_active: u64,
}

impl BlobPurgeState {
    fn new() -> Self {
        Self {
            last_hash: BlobHash::default(),
            last_hash_is_linked: true, // Avoid deleting non-existing last_hash on first iteration
            delete_keys: Vec::new(),
            delete_registry: Vec::new(),
            now: now(),
            total_deleted: 0,
            total_active: 0,
        }
    }

    pub fn update_hash(&mut self, hash: BlobHash) {
        if self.last_hash != hash {
            self.finalize(hash);
            self.last_hash_is_linked = false;
        }
    }

    pub fn finalize(&mut self, new_hash: BlobHash) {
        if !self.last_hash_is_linked {
            self.total_deleted += 1;
            self.delete_keys.push((
                None,
                BlobOp::Commit {
                    hash: std::mem::replace(&mut self.last_hash, new_hash),
                },
            ));
        } else {
            self.total_active += 1;
            self.last_hash = new_hash;
        }
    }

    pub fn process_key(&mut self, key: &[u8], value: &[u8]) -> trc::Result<()> {
        const TEMP_LINK: usize = BLOB_HASH_LEN + U32_LEN + U64_LEN;
        const DOC_LINK: usize = BLOB_HASH_LEN + U64_LEN + 1;
        const ID_LINK: usize = BLOB_HASH_LEN + U64_LEN;

        match key.len() {
            BLOB_HASH_LEN => {
                // Main blob entry
                Ok(())
            }
            TEMP_LINK => {
                // Temporary link
                let until = key.deserialize_be_u64(BLOB_HASH_LEN + U32_LEN)?;
                if until <= self.now {
                    let account_id = key.deserialize_be_u32(BLOB_HASH_LEN)?;
                    self.delete_keys.push((
                        Some(account_id),
                        BlobOp::Link {
                            hash: self.last_hash.clone(),
                            to: BlobLink::Temporary { until },
                        },
                    ));
                    if value.len() > MAX_LEB128_U64_LEN
                        && let Some((_, object_id)) = value.split_last_chunk::<OBJECT_ID_LEN>()
                    {
                        self.delete_registry
                            .push((account_id, ObjectId::deserialize(object_id)?));
                    }
                } else {
                    self.last_hash_is_linked = true;
                }
                Ok(())
            }
            DOC_LINK | ID_LINK => {
                // Document/Id link
                self.last_hash_is_linked = true;
                Ok(())
            }
            _ => Err(trc::Error::corrupted_key(
                key,
                value.into(),
                trc::location!(),
            )),
        }
    }
}

#[cfg(all(test, stalwart_tests))]
#[rustfmt::skip]
#[path = "../../../../../stalwart-tests/unit/store/src/write/blob/tests.rs"]
mod tests;

#[cfg(test)]
mod za_tests {
    use super::*;
    use crate::backend::ephemeral::EphemeralStore;

    #[tokio::test]
    async fn za_search_recode_selects_committed_principal_and_email_document_links() {
        let store = EphemeralStore::open();
        let account_id = 42;
        let shard = 7;
        let hash = |tag: u8| {
            let mut bytes = [tag; BLOB_HASH_LEN];
            bytes[0] = shard;
            BlobHash(bytes)
        };
        let principal = hash(1);
        let email = hash(2);
        let temporary = hash(3);
        let mut batch = BatchBuilder::new();
        batch.with_account_id(account_id);
        for hash in [&principal, &email, &temporary] {
            batch.set(BlobOp::Commit { hash: hash.clone() }, Vec::new());
        }
        for (collection, hash) in [
            (Collection::Principal, principal.clone()),
            (Collection::Email, email.clone()),
        ] {
            batch.with_collection(collection).with_document(0).set(
                BlobOp::Link {
                    hash,
                    to: BlobLink::Document,
                },
                BlobLinkValue::new(10u64),
            );
        }
        batch.set(
            BlobOp::Link {
                hash: temporary,
                to: BlobLink::Temporary { until: u64::MAX },
            },
            BlobLinkValue::new(10u64),
        );
        store.write_batch(&mut batch).await.unwrap();

        assert_eq!(
            store.user_blob_hashes(shard).await.unwrap(),
            [principal, email]
        );
    }

    #[tokio::test]
    async fn za_search_scan_matches_only_exact_principal_document_zero_links() {
        let store = EphemeralStore::open();
        let account_id = 0x1234_5678;
        let low = BlobHash([0; BLOB_HASH_LEN]);
        let high = BlobHash::new_max();
        let mut batch = BatchBuilder::new();

        // The scanner needs the links even if their Commit records are gone.
        for (account, collection, document, hash) in [
            (account_id, Collection::Principal, 0, low.clone()),
            (account_id, Collection::Principal, 0, high.clone()),
            (
                account_id + 1,
                Collection::Principal,
                0,
                BlobHash([1; BLOB_HASH_LEN]),
            ),
            (
                account_id,
                Collection::Email,
                0,
                BlobHash([2; BLOB_HASH_LEN]),
            ),
            (
                account_id,
                Collection::Principal,
                1,
                BlobHash([3; BLOB_HASH_LEN]),
            ),
        ] {
            batch
                .with_account_id(account)
                .with_collection(collection)
                .with_document(document)
                .set(
                    BlobOp::Link {
                        hash,
                        to: BlobLink::Document,
                    },
                    BlobLinkValue::new(10u64),
                );
        }
        batch
            .with_account_id(account_id)
            .set(
                BlobOp::Commit {
                    hash: BlobHash([4; BLOB_HASH_LEN]),
                },
                Vec::new(),
            )
            .set(
                BlobOp::Link {
                    hash: BlobHash([5; BLOB_HASH_LEN]),
                    to: BlobLink::Temporary { until: 1 },
                },
                BlobLinkValue::new(10u64),
            )
            .set(
                BlobOp::Link {
                    hash: BlobHash([6; BLOB_HASH_LEN]),
                    to: BlobLink::Id {
                        id: u64::from(account_id),
                    },
                },
                BlobLinkValue::new(10u64),
            );
        store.write_batch(&mut batch).await.unwrap();

        assert_eq!(
            store.za_principal_blob_links(account_id).await.unwrap(),
            vec![low, high]
        );
    }
}
