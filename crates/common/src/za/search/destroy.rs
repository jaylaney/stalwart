/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{error::LoadFailure, load::classify_blob_error};
use crate::{
    Server,
    auth::vault::{RawValue, ZA_PIN_WRITE_ATTEMPTS},
};
use registry::{schema::prelude::ObjectType, types::EnumImpl};
use std::{collections::HashSet, future::Future};
use store::{
    ValueKey,
    write::{
        BatchBuilder, BlobLink, BlobOp, RegistryClass, SetValue, ValueClass, assert::AssertValue,
    },
    za_search::{
        BlobHeader, BlobKind,
        manifest::{Inventory, ManifestValue, manifest_assertion},
    },
};
use trc::AddContext;
use types::{blob_hash::BlobHash, collection::Collection, field::PrincipalField};

struct DestroyRead {
    expected: AssertValue,
    inventory: Option<Inventory>,
    fenced: bool,
}

fn read_destroy_state(raw: Option<&[u8]>) -> DestroyRead {
    let expected = manifest_assertion(raw);
    let (fenced, inventory) = match raw
        .and_then(|bytes| ManifestValue::decode_unverified(bytes).ok())
    {
        Some(ManifestValue::Index(manifest)) => (
            false,
            Some(Inventory {
                thread_blobs: manifest.thread_blobs,
                content: manifest.content,
            }),
        ),
        // An empty marker could have replaced an unreadable or missing
        // manifest. Retry the scan in that case without changing the marker.
        Some(ManifestValue::Destroying(inventory)) => {
            let has_inventory = !inventory.thread_blobs.is_empty() || inventory.content.is_some();
            (true, has_inventory.then_some(inventory))
        }
        None => (false, None),
    };
    DestroyRead {
        expected,
        inventory,
        fenced,
    }
}

async fn unlink_set<F, Fut>(
    inventory: Option<Inventory>,
    mut read_header: F,
    scan: impl Future<Output = trc::Result<Vec<BlobHash>>>,
) -> trc::Result<Vec<BlobHash>>
where
    F: FnMut(BlobHash) -> Fut,
    Fut: Future<Output = trc::Result<Option<BlobHeader>>>,
{
    let Some(inventory) = inventory else {
        return scan.await;
    };
    let mut hashes = inventory
        .thread_blobs
        .into_iter()
        .map(|blob| blob.hash)
        .collect::<Vec<_>>();
    if let Some(head) = inventory.content {
        let mut next = Some((head.hash, head.segment));
        let mut visited = HashSet::new();
        for _ in 0..head.length {
            let Some((hash, segment)) = next else {
                return scan.await;
            };
            if !visited.insert(hash.clone()) {
                return scan.await;
            }
            let Some(header) = read_header(hash.clone()).await? else {
                return scan.await;
            };
            if header.kind != BlobKind::Content || header.segment != segment {
                return scan.await;
            }
            hashes.push(hash);
            next = header.predecessor;
        }
        if next.is_some() {
            return scan.await;
        }
    }
    Ok(hashes)
}

fn principal_batch(account_id: u32) -> BatchBuilder {
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0);
    batch
}

#[derive(Clone, Copy)]
enum DestroyFence {
    Marker(AssertValue),
    BothAbsent,
}

impl DestroyFence {
    fn assert_in(self, batch: &mut BatchBuilder) -> &mut BatchBuilder {
        match self {
            Self::Marker(marker) => batch.assert_value(PrincipalField::ZeroAccessSearch, marker),
            Self::BothAbsent => batch
                .assert_value(PrincipalField::ZeroAccessVault, AssertValue::None)
                .assert_value(PrincipalField::ZeroAccessSearch, AssertValue::None),
        }
    }
}

fn marker_batch(
    account_id: u32,
    live_account: Option<&ValueClass>,
    previous_vault: Option<AssertValue>,
    previous_search: AssertValue,
    bytes: Vec<u8>,
) -> BatchBuilder {
    let mut batch = principal_batch(account_id);
    if let Some(account) = live_account {
        // A reset admitted before deletion can arrive after the Property
        // range was removed. Do not recreate the marker.
        batch.assert_value(account.clone(), AssertValue::Some);
    }
    if let Some(previous_vault) = previous_vault {
        batch.assert_value(PrincipalField::ZeroAccessVault, previous_vault);
    }
    batch
        .assert_value(PrincipalField::ZeroAccessSearch, previous_search)
        .set(PrincipalField::ZeroAccessSearch, SetValue::fixed(bytes));
    batch
}

impl Server {
    pub async fn za_search_destroy(&self, account_id: u32, clear_field: bool) -> trc::Result<()> {
        let live_account = clear_field.then(|| {
            ValueClass::Registry(RegistryClass::Item {
                object_id: ObjectType::Account.to_id(),
                item_id: u64::from(account_id),
            })
        });
        let (fence, inventory) = self
            .za_search_destroy_fence(account_id, live_account.as_ref())
            .await?;

        #[cfg(feature = "test_mode")]
        {
            use crate::za::test_hooks::{search_destroy_pause_point, take_search_destroy_fail};
            search_destroy_pause_point(account_id).await;
            if take_search_destroy_fail(account_id) {
                return Err(trc::StoreEvent::AssertValueFailed
                    .into_err()
                    .details("zero-access test hook: search destroy after fence selection"));
            }
        }

        let hashes = unlink_set(
            inventory,
            |hash| async move {
                match self.get_blob(hash.as_slice(), 0..BlobHeader::LEN).await {
                    Ok(bytes) => Ok(bytes.and_then(|bytes| {
                        BlobHeader::decode(&bytes).ok().map(|(header, _)| header)
                    })),
                    Err(err) if matches!(classify_blob_error(&err), LoadFailure::Loss(_)) => {
                        Ok(None)
                    }
                    Err(err) => Err(err.caused_by(trc::location!())),
                }
            },
            self.store().za_principal_blob_links(account_id),
        )
        .await?;

        let mut batch = principal_batch(account_id);
        for hash in hashes {
            if batch.is_empty() {
                if let Some(account) = &live_account {
                    batch.assert_value(account.clone(), AssertValue::Some);
                }
                // A retried worker may scan after another reset has cleared
                // the marker and a keyed request has published fresh links.
                fence.assert_in(&mut batch);
            }
            batch.clear(BlobOp::Link {
                hash,
                to: BlobLink::Document,
            });
            if batch.is_large_bulk_batch() {
                self.store()
                    .write_batch(&mut batch)
                    .await
                    .caused_by(trc::location!())?;
                batch = principal_batch(account_id);
            }
        }
        if !batch.is_empty() {
            self.store()
                .write_batch(&mut batch)
                .await
                .caused_by(trc::location!())?;
        }

        if let Some(account) = live_account {
            // Permanent destruction starts after the registry Item is removed.
            // A reset admitted earlier must leave the marker for that task.
            let mut batch = principal_batch(account_id);
            fence
                .assert_in(batch.assert_value(account, AssertValue::Some))
                .clear(PrincipalField::ZeroAccessSearch);
            self.store()
                .write_batch(&mut batch)
                .await
                .caused_by(trc::location!())?;
        }

        Ok(())
    }

    async fn za_search_destroy_fence(
        &self,
        account_id: u32,
        live_account: Option<&ValueClass>,
    ) -> trc::Result<(DestroyFence, Option<Inventory>)> {
        let mut attempts_left = ZA_PIN_WRITE_ATTEMPTS;
        loop {
            let raw = self
                .za_search_read_manifest_raw(account_id)
                .await
                .caused_by(trc::location!())?;
            let state = read_destroy_state(raw.as_deref());
            if state.fenced {
                return Ok((DestroyFence::Marker(state.expected), state.inventory));
            }

            let previous_vault = if raw.is_none() && live_account.is_none() {
                let vault = self
                    .store()
                    .get_value::<RawValue>(ValueKey::property(
                        account_id,
                        Collection::Principal,
                        0,
                        PrincipalField::ZeroAccessVault,
                    ))
                    .await
                    .caused_by(trc::location!())?;
                let Some(vault) = vault else {
                    // A peer may already have completed the range delete.
                    // Scan for orphan links without recreating field 152.
                    return Ok((DestroyFence::BothAbsent, None));
                };
                Some(manifest_assertion(Some(&vault.0)))
            } else {
                None
            };
            let bytes = ManifestValue::encode_destroying(
                state.inventory.as_ref().unwrap_or(&Inventory::default()),
            );
            let fence = DestroyFence::Marker(manifest_assertion(Some(&bytes)));
            let mut batch = marker_batch(
                account_id,
                live_account,
                previous_vault,
                state.expected,
                bytes,
            );

            match self.store().write_batch(&mut batch).await {
                Ok(_) => return Ok((fence, state.inventory)),
                Err(err) if err.is_assertion_failure() && attempts_left > 1 => attempts_left -= 1,
                Err(err) => return Err(err.caused_by(trc::location!())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::ready;
    use store::za_search::{
        SegmentId,
        manifest::{ContentHead, ThreadBlobEntry},
    };
    use xxhash_rust::xxh3::xxh3_64;

    fn hash(byte: u8) -> BlobHash {
        BlobHash([byte; 32])
    }

    fn segment(byte: u8) -> SegmentId {
        SegmentId([byte; 16])
    }

    fn sample_inventory() -> Inventory {
        Inventory {
            thread_blobs: [4, 5]
                .into_iter()
                .map(|byte| ThreadBlobEntry {
                    segment: segment(byte),
                    hash: hash(byte),
                    size: 100,
                })
                .collect(),
            content: Some(ContentHead {
                hash: hash(3),
                segment: segment(3),
                length: 3,
                size: 300,
            }),
        }
    }

    fn header(byte: u8) -> Option<BlobHeader> {
        Some(BlobHeader {
            kind: BlobKind::Content,
            tier: 0,
            segment: segment(byte),
            predecessor: (byte > 1).then(|| (hash(byte - 1), segment(byte - 1))),
        })
    }

    #[test]
    fn marker_bytes_round_trip_the_inventory() {
        let inventory = sample_inventory();
        let marker = ManifestValue::encode_destroying(&inventory);
        let state = read_destroy_state(Some(&marker));
        assert!(state.fenced);
        assert_eq!(state.expected, AssertValue::Hash(xxh3_64(&marker)));
        assert_eq!(state.inventory, Some(inventory));
    }

    #[test]
    fn garbage_bytes_take_the_scan_and_assert_their_hash() {
        let garbage = [0x55; 40];
        let state = read_destroy_state(Some(&garbage));
        assert!(!state.fenced);
        assert_eq!(state.expected, AssertValue::Hash(xxh3_64(&garbage)));
        assert!(state.inventory.is_none());
        let marker = ManifestValue::encode_destroying(&Inventory::default());
        let retry = read_destroy_state(Some(&marker));
        assert!(retry.fenced);
        assert!(retry.inventory.is_none());
    }

    #[tokio::test]
    async fn unlink_set_takes_the_inventory_when_the_chain_is_complete() {
        let links = unlink_set(
            Some(sample_inventory()),
            |hash| ready(Ok(header(hash.0[0]))),
            async { panic!("a complete chain needs no scan") },
        )
        .await
        .unwrap();
        assert_eq!(links, vec![hash(4), hash(5), hash(3), hash(2), hash(1)]);
    }

    #[tokio::test]
    async fn unlink_set_falls_back_to_the_scan_on_a_broken_chain() {
        let links = unlink_set(
            Some(sample_inventory()),
            |hash| ready(Ok((hash.0[0] != 2).then(|| header(hash.0[0]).unwrap()))),
            ready(Ok(vec![hash(9)])),
        )
        .await
        .unwrap();
        assert_eq!(links, vec![hash(9)]);
    }

    #[tokio::test]
    async fn cyclic_and_short_chains_take_the_scan() {
        for predecessor in [None, Some((hash(3), segment(3)))] {
            let links = unlink_set(
                Some(sample_inventory()),
                |_| {
                    ready(Ok(Some(BlobHeader {
                        predecessor: predecessor.clone(),
                        ..header(3).unwrap()
                    })))
                },
                ready(Ok(vec![hash(9)])),
            )
            .await
            .unwrap();
            assert_eq!(links, vec![hash(9)]);
        }
    }

    #[tokio::test]
    async fn transient_header_read_failure_leaves_cleanup_for_retry() {
        let err = unlink_set(
            Some(sample_inventory()),
            |_| ready(Err(trc::StoreEvent::UnexpectedError.into())),
            async { panic!("a transient error must propagate") },
        )
        .await
        .unwrap_err();
        assert!(err.matches(trc::EventType::Store(trc::StoreEvent::UnexpectedError)));
    }

    #[tokio::test]
    async fn absent_search_marker_requires_the_raw_vault_it_read() {
        use store::backend::ephemeral::EphemeralStore;

        let store = EphemeralStore::open();
        let account_id = 42;
        let vault_key = || {
            ValueKey::property(
                account_id,
                Collection::Principal,
                0,
                PrincipalField::ZeroAccessVault,
            )
        };
        let search_key = || {
            ValueKey::property(
                account_id,
                Collection::Principal,
                0,
                PrincipalField::ZeroAccessSearch,
            )
        };
        // Even undecodable vault bytes are present and must bind the marker.
        let mut seed = principal_batch(account_id);
        seed.set(PrincipalField::ZeroAccessVault, vec![0x55; 40]);
        store.write_batch(&mut seed).await.unwrap();
        let old_vault = store
            .get_value::<RawValue>(vault_key())
            .await
            .unwrap()
            .unwrap();
        let previous_vault = Some(manifest_assertion(Some(&old_vault.0)));
        let marker = ManifestValue::encode_destroying(&Inventory::default());
        let build = || {
            marker_batch(
                account_id,
                None,
                previous_vault,
                AssertValue::None,
                marker.clone(),
            )
        };

        let mut remove = principal_batch(account_id);
        remove.clear(PrincipalField::ZeroAccessVault);
        store.write_batch(&mut remove).await.unwrap();
        assert!(
            store
                .write_batch(&mut build())
                .await
                .is_err_and(|err| err.is_assertion_failure()),
            "marker creation must refuse after its read vault disappears"
        );
        assert!(!store.key_exists(search_key()).await.unwrap());

        let mut changed = principal_batch(account_id);
        changed.set(PrincipalField::ZeroAccessVault, vec![0x77; 40]);
        store.write_batch(&mut changed).await.unwrap();
        assert!(
            store
                .write_batch(&mut build())
                .await
                .is_err_and(|err| err.is_assertion_failure()),
            "mere presence of a different raw vault is insufficient"
        );
        assert!(!store.key_exists(search_key()).await.unwrap());

        let mut unchanged = principal_batch(account_id);
        unchanged.set(PrincipalField::ZeroAccessVault, old_vault.0);
        store.write_batch(&mut unchanged).await.unwrap();
        store.write_batch(&mut build()).await.unwrap();
        let stored = store
            .get_value::<RawValue>(search_key())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.0, marker);
    }
}
