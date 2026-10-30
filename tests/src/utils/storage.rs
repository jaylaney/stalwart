/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::cleanup::{search_store_destroy, store_assert_is_empty_except_vaults};
use crate::utils::registry::UnwrapRegistryId;
use ahash::AHashSet;
use common::{Server, storage::search::SearchIndexStatus};
use registry::schema::structs::{Task, TaskStatus};
use registry::{
    schema::{
        enums::{BlobStoreType, DataStoreType, InMemoryStoreType, SearchStoreType},
        prelude::Object,
        structs::{
            BlobStore, DataStore, ElasticSearchStore, FileSystemStore, FoundationDbStore, HttpAuth,
            HttpAuthBasic, HttpAuthBearer, InMemoryStore, MeilisearchStore, MySqlStore,
            PostgreSqlStore, PublicStringOptional, PublicStringValue, RedisStore, RocksDbStore,
            S3Store, S3StoreCustomRegion, S3StoreRegion, SearchStore, SecretKey, SecretKeyOptional,
            SecretKeyValue, SqliteStore,
        },
    },
    types::{EnumImpl, duration::Duration},
};
use store::write::now;
use store::{
    Deserialize, IterateParams, Subspace, U32_LEN, ValueKey,
    write::{AnyKey, SearchIndex, SearchIndexClass, TaskId, TaskQueueClass, ValueClass},
};
use store::{RegistryStore, registry::write::RegistryWrite};

pub trait RegistryEnvStores {
    fn insert_stores_from_env(&self) -> impl Future<Output = ()>;
}

impl RegistryEnvStores for RegistryStore {
    async fn insert_stores_from_env(&self) {
        let path = self.path().as_os_str().to_str().unwrap();
        let mut search_store = None;
        if let Ok(store) = std::env::var("SEARCH_STORE") {
            let store = SearchStoreType::parse(&store).expect("Invalid store type");
            search_store = Some(Object::from(build_search_store(store, path).await));
        }
        let mut blob_store = None;
        if let Ok(store) = std::env::var("BLOB_STORE") {
            let store = BlobStoreType::parse(&store).expect("Invalid store type");
            blob_store = Some(Object::from(build_blob_store(store, path).await));
        }
        let mut in_memory = None;
        if let Ok(store) = std::env::var("MEMORY_STORE") {
            let store = InMemoryStoreType::parse(&store).expect("Invalid store type");
            in_memory = Some(Object::from(build_in_memory_store(store, path).await));
        }

        for store in [search_store, blob_store, in_memory].into_iter().flatten() {
            self.write(RegistryWrite::insert(&store))
                .await
                .expect("Failed to insert store into registry")
                .unwrap_id(trc::location!());
        }
    }
}

pub async fn build_data_store(typ: &str, path: &str) -> DataStore {
    if typ == "MariaDb" {
        crate::utils::containers::ensure_mariadb().await;
        return DataStore::MySql(MySqlStore {
            host: "localhost".into(),
            port: 3308,
            auth_username: "root".to_string().into(),
            auth_secret: SecretKeyOptional::Value(SecretKeyValue {
                secret: "password".into(),
            }),
            database: "stalwart".into(),
            use_tls: false,
            allow_invalid_certs: true,
            ..Default::default()
        });
    }

    match DataStoreType::parse(typ).expect("Invalid store type") {
        DataStoreType::RocksDb => DataStore::RocksDb(RocksDbStore {
            path: format!("{path}/rocks.db"),
            ..Default::default()
        }),
        DataStoreType::Sqlite => DataStore::Sqlite(SqliteStore {
            path: format!("{path}/sqlite.db"),
            ..Default::default()
        }),
        DataStoreType::FoundationDb => {
            crate::utils::containers::ensure_foundationdb().await;
            DataStore::FoundationDb(FoundationDbStore::default())
        }
        DataStoreType::PostgreSql => {
            crate::utils::containers::ensure_postgres().await;
            DataStore::PostgreSql(PostgreSqlStore {
                host: "localhost".into(),
                port: 5432,
                auth_username: "stalwart".to_string().into(),
                auth_secret: SecretKeyOptional::Value(SecretKeyValue {
                    secret: "stalwart".into(),
                }),
                database: "stalwart".into(),
                use_tls: false,
                allow_invalid_certs: true,
                ..Default::default()
            })
        }
        DataStoreType::MySql => {
            crate::utils::containers::ensure_mysql().await;
            DataStore::MySql(MySqlStore {
                host: "localhost".into(),
                port: 3307,
                auth_username: "root".to_string().into(),
                auth_secret: SecretKeyOptional::Value(SecretKeyValue {
                    secret: "password".into(),
                }),
                database: "stalwart".into(),
                use_tls: false,
                allow_invalid_certs: true,
                ..Default::default()
            })
        }
    }
}

async fn build_blob_store(typ: BlobStoreType, path: &str) -> BlobStore {
    match typ {
        BlobStoreType::S3 => {
            crate::utils::containers::ensure_minio().await;
            BlobStore::S3(S3Store {
                access_key_id: PublicStringOptional::Value(PublicStringValue {
                    value: "minioadmin".into(),
                }),
                bucket: "stalwart".into(),
                region: S3StoreRegion::Custom(S3StoreCustomRegion {
                    url: "http://localhost:9000".into(),
                    region: "eu-central-1".into(),
                }),
                secret_key: SecretKeyOptional::Value(SecretKeyValue {
                    secret: "minioadmin".into(),
                }),
                allow_invalid_certs: true,
                ..Default::default()
            })
        }
        BlobStoreType::FileSystem => BlobStore::FileSystem(FileSystemStore {
            path: path.to_string(),
            ..Default::default()
        }),
        _ => unreachable!(),
    }
}

async fn build_in_memory_store(typ: InMemoryStoreType, _path: &str) -> InMemoryStore {
    match typ {
        InMemoryStoreType::Redis => {
            crate::utils::containers::ensure_redis().await;
            InMemoryStore::Redis(RedisStore {
                url: "redis://127.0.0.1".into(),
                ..Default::default()
            })
        }
        _ => unreachable!(),
    }
}

async fn build_search_store(typ: SearchStoreType, _path: &str) -> SearchStore {
    match typ {
        SearchStoreType::ElasticSearch => {
            crate::utils::containers::ensure_opensearch().await;
            SearchStore::ElasticSearch(ElasticSearchStore {
                url: "http://localhost:9200".into(),
                allow_invalid_certs: true,
                http_auth: HttpAuth::Basic(HttpAuthBasic {
                    username: "elastic".into(),
                    secret: SecretKey::Value(SecretKeyValue {
                        secret: "changeme".into(),
                    }),
                }),
                ..Default::default()
            })
        }
        SearchStoreType::Meilisearch => {
            crate::utils::containers::ensure_meilisearch().await;
            SearchStore::Meilisearch(MeilisearchStore {
                url: "http://localhost:7700".into(),
                allow_invalid_certs: true,
                poll_interval: Duration::from_millis(100),
                http_auth: HttpAuth::Bearer(HttpAuthBearer {
                    bearer_token: SecretKey::Value(SecretKeyValue {
                        secret: "stalwart-master-key".into(),
                    }),
                }),
                ..Default::default()
            })
        }
        _ => unreachable!(),
    }
}

/// Waits until the task queue and the search index queue hold nothing more
/// to run.
///
/// - Scheduled tasks (`TaskQueueClass::Task`): a task due later is skipped
///   with `skip_not_due`, a failed one with `skip_permanent_failures`.
/// - The search index queue (subspace `SearchQueue`): its entries
///   (`SearchIndexClass::Queue`), the claim markers of the partitions being
///   indexed (`QueueIndex`) and the status keys of failed partitions
///   (`QueueStatus`, written while they wait for a retry). A partition whose
///   retry is due later is skipped with `skip_not_due`, entries included. The
///   index queue has no permanent failure: a failed partition is retried.
pub async fn wait_for_tasks(server: &Server, skip_not_due: bool, skip_permanent_failures: bool) {
    let mut count = 0;
    loop {
        let mut has_index_tasks = None;
        server
            .core
            .storage
            .data
            .iterate(
                IterateParams::new(
                    ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task {
                        id: TaskId::Assigned(0),
                    })),
                    ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task {
                        id: TaskId::Assigned(u64::MAX),
                    })),
                )
                .ascending(),
                |_, value| {
                    let task = Task::deserialize(value)?;
                    if (skip_permanent_failures && matches!(task.status(), TaskStatus::Failed(_)))
                        || (skip_not_due && task.due_timestamp() > now())
                    {
                        Ok(true)
                    } else {
                        has_index_tasks = Some(task);

                        Ok(false)
                    }
                },
            )
            .await
            .unwrap();

        let pending_index = pending_search_index_partitions(server, skip_not_due).await;

        if let Some(task) = has_index_tasks {
            count += 1;
            if count % 10 == 0 {
                println!("Waiting for pending task {:?}...", task);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        } else if !pending_index.is_empty() {
            count += 1;
            if count % 10 == 0 {
                println!("Waiting for search index partitions {:?}...", pending_index);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        } else {
            break;
        }
    }
}

/// The search index partitions, as (index, partition), that still hold
/// queue entries, a claim marker or a failure status.
async fn pending_search_index_partitions(
    server: &Server,
    skip_not_due: bool,
) -> Vec<(SearchIndex, u32)> {
    let mut pending: AHashSet<(SearchIndex, u32)> = AHashSet::new();
    let mut deferred: AHashSet<(SearchIndex, u32)> = AHashSet::new();
    let now = now();

    server
        .core
        .storage
        .data
        .iterate(
            IterateParams::new(
                AnyKey {
                    subspace: Subspace::SearchQueue,
                    key: vec![0u8],
                },
                AnyKey {
                    subspace: Subspace::SearchQueue,
                    key: vec![u8::MAX; 16],
                },
            )
            .ascending(),
            |key, value| {
                let partition = if key.first() == Some(&SearchIndexClass::QUEUE_CONTROL) {
                    // Control keys: QUEUE_CONTROL, index, partition, kind
                    let index = key.get(1).copied().and_then(SearchIndex::try_from_u8);
                    let partition = key
                        .get(2..2 + U32_LEN)
                        .and_then(|bytes| bytes.try_into().ok())
                        .map(u32::from_be_bytes);
                    let kind = key.get(2 + U32_LEN).copied();
                    match (index, partition, kind) {
                        (Some(index), Some(partition), Some(kind)) => {
                            if kind == SearchIndexClass::CONTROL_STATUS
                                && skip_not_due
                                && SearchIndexStatus::deserialize(value)?.next_retry > now
                            {
                                deferred.insert((index, partition));
                            }
                            (index, partition)
                        }
                        _ => panic!("Unexpected search index queue control key {key:?}"),
                    }
                } else {
                    // Queue entries: index, id prefix, id suffix, created at
                    let index = key.first().copied().and_then(SearchIndex::try_from_u8);
                    let id_prefix = key
                        .get(1..1 + U32_LEN)
                        .and_then(|bytes| bytes.try_into().ok())
                        .map(u32::from_be_bytes);
                    match (index, id_prefix) {
                        (Some(index), Some(id_prefix)) => {
                            (index, SearchIndexClass::queue_partition(index, id_prefix))
                        }
                        _ => panic!("Unexpected search index queue key {key:?}"),
                    }
                };
                pending.insert(partition);

                Ok(true)
            },
        )
        .await
        .unwrap();

    pending
        .into_iter()
        .filter(|partition| !deferred.contains(partition))
        .collect()
}

pub async fn assert_is_empty(server: &Server, include_registry: bool) {
    // Wait for pending index tasks
    wait_for_tasks(server, false, false).await;

    // Assert is empty
    let vault_accounts = store_assert_is_empty_except_vaults(
        server.store(),
        server.core.storage.blob.clone(),
        include_registry,
    )
    .await;
    for account_id in vault_accounts {
        assert!(
            server
                .registry()
                .object::<registry::schema::structs::Account>(account_id.into())
                .await
                .unwrap()
                .is_some(),
            "Found a zero-access vault or pin record of destroyed account {account_id}"
        );
    }
    search_store_destroy(server.search_store()).await;

    // Clean caches
    for cache in [
        &server.inner.cache.events,
        &server.inner.cache.contacts,
        &server.inner.cache.files,
        &server.inner.cache.scheduling,
    ] {
        cache.clear();
    }
    server.inner.cache.messages.clear();
}
