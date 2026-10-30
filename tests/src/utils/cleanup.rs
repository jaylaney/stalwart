/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use ::registry::{
    schema::prelude::{OBJ_SINGLETON, ObjectType},
    types::EnumImpl,
};
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
    /// Counters: the document-id and change-id counters are deleted, the
    /// registry id counters kept (as at 0.16), and any other counter that the
    /// store purge did not remove (it removes zero counters) is a leftover.
    Counters,
    /// Properties: a zero-access vault or pin record is reported to the
    /// caller, which checks that its account still exists; the rest are
    /// leftovers.
    Properties,
    /// The schema version and the node-id claims are kept; the rest are
    /// leftovers.
    System,
    /// The internal search store's terms and documents. They are leftovers,
    /// except on PostgreSQL and MySQL, which keep the search index in tables
    /// of their own and create no such subspace (`Subspace::is_internal_fts`).
    InternalSearch,
    /// The change logs, deleted after the check as at 0.16. 1.0 also keeps
    /// share notifications here, as log entries of
    /// `SyncCollection::ShareNotification`; a share notification left behind is
    /// a leftover.
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
    // Delete all temporary hashes
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
            |key, _| {
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

                    batch.clear(ValueClass::Blob(BlobOp::Link {
                        hash,
                        to: BlobLink::Temporary { until },
                    }));
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
    let vault_accounts =
        store_assert_is_empty_except_vaults(store, blob_store, include_registry).await;
    assert!(
        vault_accounts.is_empty(),
        "Found zero-access vault records of accounts {vault_accounts:?}"
    );
}

/// Like `store_assert_is_empty`, but returns the accounts holding a
/// zero-access vault record instead of reporting them. The caller must check
/// that each account still exists: a vault record lives exactly as long as
/// its account.
#[allow(unused_variables)]
#[must_use]
pub async fn store_assert_is_empty_except_vaults(
    store: &Store,
    blob_store: BlobStore,
    include_registry: bool,
) -> Vec<u32> {
    // 0.16 also expired the in-memory keys kept in the data store here
    // (`store_lookup_expire_all`). 1.0 keeps no in-memory values in the data
    // store: `InMemoryStore` has no data-store variant, so that step has no
    // subject.
    store_blob_expire_all(store).await;
    store.purge_blobs_all_shards(blob_store).await.unwrap();
    store.purge_store().await.unwrap();

    let store = store.clone();
    let mut failed = false;
    let mut delete_batch = BatchBuilder::new();
    let mut vault_accounts = Vec::new();

    for subspace in Subspace::ALL.iter().copied() {
        let rule = subspace_rule(subspace);
        let with_values = match rule {
            Rule::InternalSearch if store.is_pg_or_mysql() => continue,
            Rule::Leftover { with_values } => with_values,
            Rule::RegistryIndex => false,
            Rule::Logs => true,
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
                                // Document-id and change-id counters
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
                        Rule::Logs
                            if !(key.len() == U32_LEN + 1 + U64_LEN
                                && key[U32_LEN] == u8::from(SyncCollection::ShareNotification)) =>
                        {
                            // A change-log entry, deleted below
                            return Ok(true);
                        }
                        Rule::Properties
                            if key.len() == U32_LEN + 2 + U32_LEN
                                && key[U32_LEN] == Collection::Principal as u8
                                && (key[U32_LEN + 1] == PrincipalField::ZeroAccessVault as u8
                                    || key[U32_LEN + 1]
                                        == PrincipalField::ZeroAccessPins as u8) =>
                        {
                            // Zero-access vault and pin records persist for the life
                            // of the account; the caller checks that the account
                            // exists.
                            vault_accounts.push(key.deserialize_be_u32(0).unwrap());
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
                        | Rule::InternalSearch
                        | Rule::Logs => {
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

    // Delete logs and counters
    let (from_key, to_key) = subspace_range(Subspace::Logs);
    store.delete_range(from_key, to_key).await.unwrap();

    if !delete_batch.is_empty() {
        store.write_batch(&mut delete_batch).await.unwrap();
    }

    if failed {
        panic!("Store is not empty.");
    }

    vault_accounts
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
