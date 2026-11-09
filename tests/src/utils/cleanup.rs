/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use ::registry::{
    schema::prelude::{OBJ_SINGLETON, ObjectType},
    types::EnumImpl,
};
use std::collections::HashSet;
use store::{
    ValueKey,
    write::{
        key::{CounterKind, DeserializeBigEndian, SystemKind},
        *,
    },
    *,
};
use trc::AddContext;
use types::{
    blob_hash::{BLOB_HASH_LEN, BlobHash},
    collection::{Collection, SyncCollection},
    field::PrincipalField,
};

// The key range that covers a whole subspace. Every key of 1.0's data store
// is shorter than this upper bound, or starts below it.
const KEY_RANGE_END: [u8; 16] = [u8::MAX; 16];

/// What the emptiness check does with the keys left in one subspace. 1.0's
/// `Subspace` enum is closed, and `subspace_rule` matches it with no
/// catch-all arm, so a subspace that upstream adds stops the build here and
/// must be classified.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rule {
    /// Every key left here is a leftover. `with_values` prints the value too.
    Leftover { with_values: bool },
    /// Registry items: singletons and the fixture types are kept, the rest
    /// are leftovers.
    RegistryItems,
    /// Registry index keys, kept for the same types as `RegistryItems`.
    RegistryIndex,
    /// Registry primary keys and references, kept for the same types.
    RegistryPrimaryKey,
    /// Counters: reset document and change allocators except those used by
    /// a retained search manifest. Keep registry id counters (as at 0.16).
    /// Any other counter the store purge did not remove is a leftover.
    Counters,
    /// Properties: zero-access vault, pin and search-manifest records are
    /// reported to the caller, which checks that their accounts still exist.
    /// The rest are leftovers.
    Properties,
    /// The schema version and the node-id claims are kept; the rest are
    /// leftovers.
    System,
    /// The internal search store's terms and documents. They are leftovers,
    /// except on PostgreSQL and MySQL, which keep the search index in tables
    /// of their own and create no such subspace (`Subspace::is_internal_fts`).
    InternalSearch,
    /// The change logs, reset after the check except for shared Email and
    /// Thread history used by a retained search manifest. 1.0 also keeps
    /// share notifications here, as log entries of
    /// `SyncCollection::ShareNotification`: an account's own state, written on
    /// every grant, change and revoke, and deleted with the logs. Their
    /// accounts are reported to the caller, which checks that each still
    /// exists (destroying an account clears its `Logs` prefix).
    Logs,
}

fn subspace_rule(subspace: Subspace) -> Rule {
    match subspace {
        Subspace::Acl => Rule::Leftover { with_values: true },
        Subspace::RegistryIndex => Rule::RegistryIndex,
        Subspace::Immutable => Rule::Leftover { with_values: true },
        Subspace::Directory => Rule::RegistryItems,
        Subspace::QueueMessage => Rule::Leftover { with_values: true },
        Subspace::TaskQueue => Rule::Leftover { with_values: true },
        Subspace::RegistryPrimaryKey => Rule::RegistryPrimaryKey,
        Subspace::ReportOut => Rule::Leftover { with_values: true },
        Subspace::Indexes => Rule::Leftover { with_values: false },
        Subspace::DeletedItems => Rule::Leftover { with_values: true },
        Subspace::BlobLink => Rule::Leftover { with_values: true },
        Subspace::Logs => Rule::Logs,
        Subspace::Counter => Rule::Counters,
        Subspace::TelemetrySpan => Rule::Leftover { with_values: true },
        Subspace::Property => Rule::Properties,
        Subspace::QueueEvent => Rule::Leftover { with_values: true },
        Subspace::ReportIn => Rule::Leftover { with_values: true },
        Subspace::Registry => Rule::RegistryItems,
        Subspace::Blobs => Rule::Leftover { with_values: true },
        Subspace::IndexProperty => Rule::Leftover { with_values: false },
        Subspace::SpamSamples => Rule::RegistryItems,
        Subspace::TelemetryMetric => Rule::Leftover { with_values: true },
        Subspace::SearchTerm => Rule::InternalSearch,
        Subspace::SearchDocument => Rule::InternalSearch,
        Subspace::SearchQueue => Rule::Leftover { with_values: true },
        Subspace::System => Rule::System,
    }
}

fn subspace_range(subspace: Subspace) -> (AnyKey<Vec<u8>>, AnyKey<Vec<u8>>) {
    (
        AnyKey {
            subspace,
            key: vec![0u8],
        },
        AnyKey {
            subspace,
            key: KEY_RANGE_END.to_vec(),
        },
    )
}

pub async fn store_destroy(store: &Store) {
    store_destroy_sql_indexes(store).await;

    for subspace in Subspace::ALL.iter().copied() {
        // PostgreSQL and MySQL create no table for the internal search store
        if subspace.is_internal_fts() && store.is_pg_or_mysql() {
            continue;
        }

        let (from_key, to_key) = subspace_range(subspace);
        store.delete_range(from_key, to_key).await.unwrap();
    }
}

pub async fn search_store_destroy(store: &SearchStore) {
    match &store {
        SearchStore::Store(store) => {
            store_destroy_sql_indexes(store).await;
            if !store.is_pg_or_mysql() {
                for subspace in [Subspace::SearchTerm, Subspace::SearchDocument] {
                    let (from_key, to_key) = subspace_range(subspace);
                    store.delete_range(from_key, to_key).await.unwrap();
                }
            }
        }
        #[cfg(feature = "postgres")]
        SearchStore::PostgreSQL(store) => {
            store_destroy_sql_indexes(&Store::PostgreSQL(store.clone())).await;
        }
        #[cfg(feature = "mysql")]
        SearchStore::MySQL(store) => {
            store_destroy_sql_indexes(&Store::MySQL(store.clone())).await;
        }
        SearchStore::ElasticSearch(_) | SearchStore::MeiliSearch(_) => {
            // 1.0 moved `drop_indexes` for these two stores into upstream's
            // private test support; no CI step uses them (upgrade test-hook
            // table, "not needed by the retained suite").
            panic!("Resetting an ElasticSearch or MeiliSearch search store is not supported.");
        }
        #[allow(unreachable_patterns)]
        _ => panic!("Resetting this search store is not supported in the tests."),
    }
}

#[allow(unused_variables)]
async fn store_destroy_sql_indexes(store: &Store) {
    #[cfg(any(feature = "postgres", feature = "mysql"))]
    {
        if store.is_pg_or_mysql() {
            for index in [
                SearchIndex::Email,
                SearchIndex::Calendar,
                SearchIndex::Contacts,
                SearchIndex::File,
                SearchIndex::Tracing,
            ] {
                #[cfg(feature = "postgres")]
                let table = index.psql_table();
                #[cfg(feature = "mysql")]
                let table = index.mysql_table();

                let _ = store
                    .sql_query::<usize>(&format!("TRUNCATE TABLE {table}"), vec![])
                    .await;
            }
        }
    }
}

pub async fn store_blob_expire_all(store: &Store) {
    // Expire every temporary link in place, as the purge would find it once
    // its hold ran out. 0.16's helper deleted the links. At 1.0 a blob held
    // for a batch that never committed (`Server::hold_blob`, for example a
    // DAV PUT refused for quota) has no commit entry, only its temporary
    // link, and the purge reads that link to delete the blob: a deleted link
    // leaves the blob in the blob store, unreachable.
    const EXPIRED: u64 = 1;
    let from_key = ValueKey {
        account_id: 0,
        collection: 0,
        document_id: 0,
        class: ValueClass::Blob(BlobOp::Commit {
            hash: BlobHash::default(),
        }),
    };
    let to_key = ValueKey {
        account_id: u32::MAX,
        collection: u8::MAX,
        document_id: u32::MAX,
        class: ValueClass::Blob(BlobOp::Link {
            hash: BlobHash::new_max(),
            to: BlobLink::Document,
        }),
    };
    let mut batch = BatchBuilder::new();
    let mut last_account_id = u32::MAX;
    store
        .iterate(
            IterateParams::new(from_key, to_key).ascending(),
            |key, value| {
                if key.len() == BLOB_HASH_LEN + U32_LEN + U64_LEN {
                    let account_id = key
                        .deserialize_be_u32(BLOB_HASH_LEN)
                        .caused_by(trc::location!())?;
                    if account_id != last_account_id {
                        last_account_id = account_id;
                        batch.with_account_id(account_id);
                    }
                    let hash =
                        BlobHash::try_from_hash_slice(key.get(..BLOB_HASH_LEN).unwrap()).unwrap();
                    let until = key
                        .deserialize_be_u64(BLOB_HASH_LEN + U32_LEN)
                        .caused_by(trc::location!())?;

                    if until > EXPIRED {
                        batch
                            .clear(ValueClass::Blob(BlobOp::Link {
                                hash: hash.clone(),
                                to: BlobLink::Temporary { until },
                            }))
                            .set(
                                ValueClass::Blob(BlobOp::Link {
                                    hash,
                                    to: BlobLink::Temporary { until: EXPIRED },
                                }),
                                value.to_vec(),
                            );
                    }
                }

                Ok(true)
            },
        )
        .await
        .unwrap();
    if !batch.is_empty() {
        store.write_batch(&mut batch).await.unwrap();
    }
}

pub async fn store_assert_is_empty(store: &Store, blob_store: BlobStore, include_registry: bool) {
    let account_state =
        store_assert_is_empty_except_account_state(store, blob_store, include_registry).await;
    assert!(
        account_state.vault_accounts.is_empty(),
        "Found zero-access vault, pin or search state of accounts {:?}",
        account_state.vault_accounts
    );
    assert!(
        account_state.share_notification_accounts.is_empty(),
        "Found share notifications of accounts {:?}",
        account_state.share_notification_accounts
    );
    account_state.reset(store).await;
}

/// The planned fixture reset and the account state whose owners must still exist.
#[derive(Debug)]
pub struct StoreCleanup {
    /// Accounts holding zero-access vault, pin or search state (kept).
    pub vault_accounts: Vec<u32>,
    /// Accounts that held share notifications (deleted with the logs).
    pub share_notification_accounts: Vec<u32>,
    delete_batch: BatchBuilder,
    clear_all_logs: bool,
}

impl StoreCleanup {
    /// Apply the fixture reset after the caller has checked account liveness.
    pub async fn reset(mut self, store: &Store) {
        if self.clear_all_logs {
            let (from_key, to_key) = subspace_range(Subspace::Logs);
            store.delete_range(from_key, to_key).await.unwrap();
        }
        if !self.delete_batch.is_empty() {
            store.write_batch(&mut self.delete_batch).await.unwrap();
        }
    }
}

/// Like `store_assert_is_empty`, but defers the counter and log reset until
/// the caller has checked the owners of the reported account state.
#[allow(unused_variables)]
#[must_use]
pub async fn store_assert_is_empty_except_account_state(
    store: &Store,
    blob_store: BlobStore,
    include_registry: bool,
) -> StoreCleanup {
    // 0.16 also expired the in-memory keys kept in the data store here
    // (`store_lookup_expire_all`). 1.0 keeps no in-memory values in the data
    // store: `InMemoryStore` has no data-store variant, so that step has no
    // subject.
    let internal_blob_key_len = match &blob_store {
        BlobStore::Store(blob) if blob.is_same(store) => Some(if blob.is_foundationdb() {
            // FoundationDB appends a u16 chunk number to each blob key.
            BLOB_HASH_LEN + U16_LEN
        } else {
            BLOB_HASH_LEN
        }),
        _ => None,
    };
    store_blob_expire_all(store).await;
    store.purge_blobs_all_shards(blob_store).await.unwrap();
    store.purge_store().await.unwrap();

    let store = store.clone();
    let mut failed = false;
    let mut delete_batch = BatchBuilder::new();
    let mut vault_accounts = Vec::new();
    let mut search_manifest_accounts = HashSet::new();
    let mut share_notification_accounts = Vec::new();
    let mut search_blob_hashes = HashSet::<[u8; BLOB_HASH_LEN]>::new();

    // A commit marker sorts before its document links. Collect eligible links
    // before classifying commits and payloads, regardless of iteration order.
    let (from_key, to_key) = subspace_range(Subspace::BlobLink);
    store
        .iterate(
            IterateParams::new(from_key, to_key).set_values(false),
            |key, _| {
                if let Some(account_id) = search_blob_account(key) {
                    search_blob_hashes.insert(key[..BLOB_HASH_LEN].try_into().unwrap());
                    vault_accounts.push(account_id);
                }
                Ok(true)
            },
        )
        .await
        .unwrap();

    // Classify manifests before staging any log or allocator reset.
    let subspaces = std::iter::once(Subspace::Property).chain(
        Subspace::ALL
            .iter()
            .copied()
            .filter(|&subspace| subspace != Subspace::Property),
    );
    for subspace in subspaces {
        let rule = subspace_rule(subspace);
        let with_values = match rule {
            Rule::InternalSearch if store.is_pg_or_mysql() => continue,
            Rule::Leftover { with_values } => with_values,
            Rule::RegistryIndex => false,
            Rule::Logs => false,
            Rule::Counters
            | Rule::RegistryItems
            | Rule::RegistryPrimaryKey
            | Rule::Properties
            | Rule::System
            | Rule::InternalSearch => subspace.shape() != Shape::Presence,
        };
        let (from_key, to_key) = subspace_range(subspace);

        store
            .iterate(
                IterateParams::new(from_key, to_key).set_values(with_values),
                |key, value| {
                    match rule {
                        Rule::Counters => {
                            let kind = key.first().copied().unwrap_or(u8::MAX);
                            if kind == CounterKind::DocumentId.byte()
                                || kind == CounterKind::ChangeId.byte()
                            {
                                // A retained search generation still uses the Email document
                                // namespace and the shared Email/Thread change allocator.
                                let email_namespace = if kind == CounterKind::DocumentId.byte() {
                                    Collection::Email as u8
                                } else {
                                    SyncCollection::Email.change_group().to_u8()
                                };
                                if key.len() == 1 + U32_LEN + 1
                                    && key[1 + U32_LEN] == email_namespace
                                    && search_manifest_accounts
                                        .contains(&key.deserialize_be_u32(1).unwrap())
                                {
                                    return Ok(true);
                                }
                                delete_batch.clear(ValueClass::Any(AnyClass {
                                    subspace,
                                    key: key.to_vec(),
                                }));
                                return Ok(true);
                            } else if kind == CounterKind::RegistryId.byte() {
                                // Registry id counters are kept
                                return Ok(true);
                            }
                            println!(
                                "Found counter of kind {kind}: {:?} = {:?}",
                                key,
                                value.deserialize_be_u64(0).ok()
                            );
                        }
                        Rule::RegistryItems => {
                            let object_id =
                                ObjectType::from_id(key.deserialize_be_u16(0).unwrap()).unwrap();

                            if include_registry && is_allowed_registry_type(object_id) {
                                return Ok(true);
                            }
                            let item_id = key.deserialize_be_u64(U16_LEN).unwrap();

                            println!(
                                "Found registry item in {:?} for object type {:?} and id {}",
                                subspace.name(),
                                object_id,
                                item_id
                            );
                        }
                        Rule::RegistryIndex => {
                            let mut id = key.deserialize_be_u16(0).unwrap();
                            if id == u16::MAX {
                                id = key.deserialize_be_u16(U16_LEN).unwrap();
                            }

                            let object_id = ObjectType::from_id(id).unwrap();

                            if include_registry && is_allowed_registry_type(object_id) {
                                return Ok(true);
                            }

                            println!(
                                "Found registry index for object type {:?}: {:?}",
                                object_id, key
                            );
                        }
                        Rule::RegistryPrimaryKey => {
                            let mut id = key.deserialize_be_u16(0).unwrap();
                            if id == RegistryClass::REFERENCE_MARKER {
                                // A reference, which 1.0 prefixes with a marker:
                                // as at 0.16, its target's type decides
                                id = key.deserialize_be_u16(U16_LEN).unwrap();
                            } else if id == u16::MAX {
                                id = value.deserialize_be_u16(0).unwrap();
                            }
                            let object_id = ObjectType::from_id(id).unwrap();
                            if include_registry && is_allowed_registry_type(object_id) {
                                return Ok(true);
                            }

                            println!(
                                "Found registry primary key or reference for object type {:?}: {:?}",
                                object_id, key
                            );
                        }
                        Rule::Logs => {
                            if key.len() == U32_LEN + 1 + U64_LEN {
                                let account_id = key.deserialize_be_u32(0).unwrap();
                                let collection = key[U32_LEN];
                                if collection == u8::from(SyncCollection::ShareNotification)
                                    && !share_notification_accounts.contains(&account_id)
                                {
                                    share_notification_accounts.push(account_id);
                                }
                                if (collection == u8::from(SyncCollection::Email)
                                    || collection == u8::from(SyncCollection::Thread))
                                    && search_manifest_accounts.contains(&account_id)
                                {
                                    return Ok(true);
                                }
                            }
                            if !search_manifest_accounts.is_empty() {
                                delete_batch.clear(ValueClass::Any(AnyClass {
                                    subspace,
                                    key: key.to_vec(),
                                }));
                            }
                            return Ok(true);
                        }
                        Rule::Properties
                            if key.len() == U32_LEN + 2 + U32_LEN
                                && key[U32_LEN] == Collection::Principal as u8
                                && (key[U32_LEN + 1] == PrincipalField::ZeroAccessVault as u8
                                    || key[U32_LEN + 1]
                                        == PrincipalField::ZeroAccessPins as u8
                                    || key[U32_LEN + 1]
                                        == PrincipalField::ZeroAccessSearch as u8) =>
                        {
                            // Vault, pin and search-manifest records persist for the life
                            // of the account. The caller checks that the account exists.
                            let account_id = key.deserialize_be_u32(0).unwrap();
                            vault_accounts.push(account_id);
                            if key[U32_LEN + 1] == PrincipalField::ZeroAccessSearch as u8
                                && key.ends_with(&0u32.to_be_bytes())
                            {
                                search_manifest_accounts.insert(account_id);
                            }
                            return Ok(true);
                        }
                        Rule::System
                            if key == [SystemKind::SchemaVersion as u8]
                                || (key.len() == 1 + U16_LEN
                                    && key[0] == SystemKind::NodeId as u8) =>
                        {
                            // The schema version and the node-id claims
                            return Ok(true);
                        }
                        Rule::Leftover { with_values: true }
                            if subspace == Subspace::BlobLink
                                && (search_blob_account(key).is_some()
                                    || (key.len() == BLOB_HASH_LEN
                                        && search_blob_hashes.contains(key))) =>
                        {
                            return Ok(true);
                        }
                        Rule::Leftover { with_values: true }
                            if subspace == Subspace::Blobs
                                && Some(key.len()) == internal_blob_key_len
                                && search_blob_hashes.contains(&key[..BLOB_HASH_LEN]) =>
                        {
                            return Ok(true);
                        }
                        Rule::Leftover { with_values: false } if subspace == Subspace::Indexes => {
                            println!(
                                concat!(
                                    "Found index key, account {}, collection {}, ",
                                    "document {}, property {}, value {:?}: {:?}"
                                ),
                                u32::from_be_bytes(key[0..4].try_into().unwrap()),
                                key[4],
                                u32::from_be_bytes(key[key.len() - 4..].try_into().unwrap()),
                                key[5],
                                String::from_utf8_lossy(&key[6..key.len() - 4]),
                                key
                            );
                        }
                        Rule::Leftover { .. }
                        | Rule::Properties
                        | Rule::System
                        | Rule::InternalSearch => {
                            println!(
                                "Found key in {:?}: {:?} ({:?}) = {:?} ({:?})",
                                subspace.name(),
                                key,
                                String::from_utf8_lossy(key),
                                value,
                                String::from_utf8_lossy(value)
                            );
                        }
                    }
                    failed = true;

                    Ok(true)
                },
            )
            .await
            .unwrap();
    }

    assert!(!failed, "Store is not empty.");

    StoreCleanup {
        vault_accounts,
        share_notification_accounts,
        delete_batch,
        clear_all_logs: search_manifest_accounts.is_empty(),
    }
}

fn search_blob_account(key: &[u8]) -> Option<u32> {
    if key.len() == BLOB_HASH_LEN + U32_LEN + 1 + U32_LEN
        && key[BLOB_HASH_LEN + U32_LEN] == Collection::Principal as u8
        && key.ends_with(&0u32.to_be_bytes())
    {
        Some(key.deserialize_be_u32(BLOB_HASH_LEN).unwrap())
    } else {
        None
    }
}

fn is_allowed_registry_type(object_type: ObjectType) -> bool {
    (object_type.flags() & OBJ_SINGLETON) != 0
        || matches!(
            object_type,
            ObjectType::Role
                | ObjectType::Account
                | ObjectType::NetworkListener
                | ObjectType::MtaDeliverySchedule
                | ObjectType::MtaRoute
                | ObjectType::MtaTlsStrategy
                | ObjectType::MtaVirtualQueue
                | ObjectType::MtaConnectionStrategy
                | ObjectType::MtaInboundThrottle
                | ObjectType::MtaQueueQuota
                | ObjectType::Tracer
                | ObjectType::Domain
        )
}

#[cfg(all(test, feature = "rocks"))]
mod tests {
    use super::*;
    use crate::utils::{registry::UnwrapRegistryId, server::TestServerBuilder};
    use ::registry::{
        schema::structs::{Account, SystemSettings, UserAccount},
        types::id::ObjectId,
    };
    use common::za::search::setup::za_setup_manifest;
    use futures::FutureExt;
    use jmap::registry::mapping::principal::schedule_account_destruction;
    use std::panic::AssertUnwindSafe;
    use store::{
        registry::write::RegistryWrite,
        write::blob_codec::{BlobKeyring, BlobOpts},
    };
    use types::{collection::VanishedCollection, id::Id};
    use vault::keys::Secret;

    #[tokio::test(flavor = "multi_thread")]
    async fn cleanup_retains_only_search_blobs_of_live_accounts() {
        // This fixture needs to see the internal payload row in the data store.
        if std::env::var("STORE").as_deref() != Ok("RocksDb")
            || std::env::var_os("BLOB_STORE").is_some()
        {
            return;
        }

        let test = TestServerBuilder::new("cleanup-search-blobs")
            .await
            .build()
            .await;
        let registry = test.server.registry();
        let domain_id = registry
            .object::<SystemSettings>(Id::singleton())
            .await
            .unwrap()
            .unwrap()
            .default_domain_id;
        let id = registry
            .write(RegistryWrite::insert(
                &Account::User(UserAccount {
                    name: "cleanup-search-blobs".into(),
                    domain_id,
                    ..Default::default()
                })
                .into(),
            ))
            .await
            .unwrap()
            .unwrap_id(trc::location!());
        let account_id = id.document_id();

        let store = test.server.store();
        let blob_store = &test.server.core.storage.blob;
        assert!(
            matches!((store, blob_store), (Store::RocksDb(_), BlobStore::Store(blob)) if blob.is_same(store))
        );
        let payload = b"sealed search blob fixture";
        let hash = BlobHash::generate(payload.as_slice());
        let link = BlobOp::Link {
            hash: hash.clone(),
            to: BlobLink::Document,
        };
        blob_store
            .put_blob(hash.as_ref(), payload, &BlobOpts::PLAIN)
            .await
            .unwrap();
        store
            .write_batch(
                BatchBuilder::new()
                    .with_account_id(account_id)
                    .with_collection(Collection::Principal)
                    .with_document(0)
                    .set(BlobOp::Commit { hash: hash.clone() }, Vec::new())
                    .set(link.clone(), Vec::new()),
            )
            .await
            .unwrap();

        let state =
            store_assert_is_empty_except_account_state(store, blob_store.clone(), true).await;
        assert_eq!(state.vault_accounts, [account_id]);
        test.assert_is_empty().await;
        assert!(store.blob_exists(&hash).await.unwrap());
        assert_eq!(
            blob_store
                .get_blob(hash.as_ref(), 0..usize::MAX, &BlobKeyring::default())
                .await
                .unwrap()
                .as_deref(),
            Some(payload.as_slice())
        );

        // A tolerated hash never excuses a different document link.
        for (collection, document) in [(Collection::Email, 0), (Collection::Principal, 1)] {
            let mut batch = BatchBuilder::new();
            batch
                .with_account_id(account_id)
                .with_collection(collection)
                .with_document(document)
                .set(link.clone(), Vec::new());
            store.write_batch(&mut batch).await.unwrap();
            assert_cleanup_panics(&test, "Store is not empty.").await;
            store
                .write_batch(
                    BatchBuilder::new()
                        .with_account_id(account_id)
                        .with_collection(collection)
                        .with_document(document)
                        .clear(link.clone()),
                )
                .await
                .unwrap();
        }

        // A payload with no matching search link remains a leftover.
        let unrelated = b"unrelated blob fixture";
        let unrelated_hash = BlobHash::generate(unrelated.as_slice());
        blob_store
            .put_blob(unrelated_hash.as_ref(), unrelated, &BlobOpts::PLAIN)
            .await
            .unwrap();
        assert_cleanup_panics(&test, "Store is not empty.").await;
        assert!(
            blob_store
                .delete_blob(unrelated_hash.as_ref())
                .await
                .unwrap()
        );

        assert_search_namespace_reset(&test, account_id, domain_id).await;

        // Remove just the registry object to simulate a destroyed account
        // whose principal link leaked. The caller must reject the account ID.
        registry
            .write(RegistryWrite::delete(ObjectId::new(
                ObjectType::Account,
                id,
            )))
            .await
            .unwrap()
            .unwrap_id(trc::location!());
        assert_cleanup_panics(&test, &format!("of destroyed account {account_id}")).await;

        store
            .write_batch(
                BatchBuilder::new()
                    .with_account_id(account_id)
                    .with_collection(Collection::Principal)
                    .with_document(0)
                    .clear(link),
            )
            .await
            .unwrap();
        test.assert_is_empty().await;
        assert!(!store.blob_exists(&hash).await.unwrap());
        assert!(
            blob_store
                .get_blob(hash.as_ref(), 0..usize::MAX, &BlobKeyring::default())
                .await
                .unwrap()
                .is_none()
        );
    }

    async fn assert_search_namespace_reset(
        test: &crate::utils::server::TestServer,
        plain_id: u32,
        domain_id: Id,
    ) {
        let account = Account::User(UserAccount {
            name: "cleanup-search-namespace".into(),
            domain_id,
            ..Default::default()
        });
        let registry = test.server.registry();
        let id = registry
            .write(RegistryWrite::insert(&account.clone().into()))
            .await
            .unwrap()
            .unwrap_id(trc::location!());
        let indexed_id = id.document_id();
        let store = test.server.store();
        let manifest_key = ValueKey::property(
            indexed_id,
            Collection::Principal,
            0,
            PrincipalField::ZeroAccessSearch,
        );
        let mut batch = BatchBuilder::new();
        batch
            .with_account_id(indexed_id)
            .with_collection(Collection::Principal)
            .with_document(0)
            .set(
                PrincipalField::ZeroAccessSearch,
                za_setup_manifest(&Secret::random(), 0),
            );

        let mut keys = Vec::new();
        for account_id in [plain_id, indexed_id] {
            batch.with_account_id(account_id);
            for (collection, class, keep) in [
                (Collection::Email, ValueClass::DocumentId, true),
                (Collection::Mailbox, ValueClass::DocumentId, false),
                (
                    Collection::Email,
                    ValueClass::ChangeId(SyncCollection::Email.into()),
                    true,
                ),
                (
                    Collection::Calendar,
                    ValueClass::ChangeId(SyncCollection::Calendar.into()),
                    false,
                ),
            ] {
                batch.with_collection(collection).add(class.clone(), 7);
                keys.push((
                    AnyKey {
                        subspace: Subspace::Counter,
                        key: ValueKey {
                            account_id,
                            collection: collection as u8,
                            document_id: 0,
                            class,
                        }
                        .serialize(0),
                    },
                    keep && account_id == indexed_id,
                ));
            }
            for (collection, change_id, data, keep) in [
                (SyncCollection::Email.into(), 5, vec![], true), // retention marker
                (SyncCollection::Email.into(), 6, vec![0], true),
                (SyncCollection::Thread.into(), 6, vec![0], true),
                (SyncCollection::Calendar.into(), 6, vec![0], false),
                (VanishedCollection::Email.into(), 6, vec![0], false),
                (
                    LogCollection::Private {
                        collection: SyncCollection::Email,
                        viewer: plain_id,
                    },
                    6,
                    vec![0],
                    false,
                ),
            ] {
                let key = collection.log_key(account_id, change_id).serialize(0);
                batch.set(
                    ValueClass::Any(AnyClass {
                        subspace: Subspace::Logs,
                        key: key.clone(),
                    }),
                    data,
                );
                keys.push((
                    AnyKey {
                        subspace: Subspace::Logs,
                        key,
                    },
                    keep && account_id == indexed_id,
                ));
            }
        }
        store.write_batch(&mut batch).await.unwrap();
        for (key, _) in &keys {
            assert!(store.key_exists(key.clone()).await.unwrap(), "{key:?}");
        }

        test.assert_is_empty().await;
        assert!(store.key_exists(manifest_key.clone()).await.unwrap());
        for (key, keep) in &keys {
            assert_eq!(
                store.key_exists(key.clone()).await.unwrap(),
                *keep,
                "{key:?}"
            );
        }

        // Run the production destroy task and inspect the raw state before a
        // subsequent fixture cleanup could erase a leaked counter or log.
        registry
            .write(RegistryWrite::delete(ObjectId::new(
                ObjectType::Account,
                id,
            )))
            .await
            .unwrap()
            .unwrap_id(trc::location!());
        schedule_account_destruction(&test.server, id, &account)
            .await
            .unwrap();
        test.wait_for_tasks().await;
        assert!(!store.key_exists(manifest_key).await.unwrap());
        for (key, keep) in keys {
            if keep {
                assert!(!store.key_exists(key.clone()).await.unwrap(), "{key:?}");
            }
        }
    }

    async fn assert_cleanup_panics(test: &crate::utils::server::TestServer, expected: &str) {
        let panic = AssertUnwindSafe(test.assert_is_empty())
            .catch_unwind()
            .await
            .expect_err("cleanup must reject this fixture");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .expect("cleanup panic must have a message");
        assert!(message.contains(expected), "unexpected panic: {message}");
    }
}
