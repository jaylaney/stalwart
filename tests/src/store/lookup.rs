/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::server::TestServerBuilder;
use registry::schema::structs::Rate;
use registry::types::duration::Duration;
use store::dispatch::lookup::KeyValue;

#[tokio::test]
pub async fn lookup_tests() {
    let test = TestServerBuilder::new("lookup_tests").await.build().await;
    let store = test.server.in_memory_store().clone();
    let rate = Rate {
        count: 1,
        period: Duration::from_millis(1000),
    };

    println!(
        "Testing in-memory store {}...",
        std::env::var("MEMORY_STORE").unwrap_or_else(|_| "default".to_string())
    );
    // Upgrade ruling 75 (subject deleted): 1.0 has no in-memory store kept in the
    // data store (`InMemoryStore::Sql` refuses every operation), so 0.16's
    // `InMemoryStore::Store` branches, their `store_destroy` and
    // `store_assert_is_empty` checks, and the `purge_in_memory_store` calls
    // (a local store expires entries when read) have no subject. 1.0 has
    // no `counter_get`: a counter is read as an increment by zero.
    // Reset the counter
    store
        .key_set(KeyValue::new("abc", "0".as_bytes().to_vec()))
        .await
        .unwrap();

    // Test key
    let key = "xyz".as_bytes().to_vec();
    store
        .key_set(KeyValue::new(key.clone(), "world".to_string().into_bytes()))
        .await
        .unwrap();
    assert_eq!(
        store.key_get::<String>(key.clone()).await.unwrap(),
        Some("world".to_string())
    );

    // Test value expiry
    store
        .key_set(KeyValue::new(key.clone(), "hello".to_string().into_bytes()).expires(1))
        .await
        .unwrap();
    assert_eq!(
        store.key_get::<String>(key.clone()).await.unwrap(),
        Some("hello".to_string())
    );
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    assert_eq!(None, store.key_get::<String>(key.clone()).await.unwrap());

    // Test counter
    let key = "abc".as_bytes().to_vec();
    store.key_incr(KeyValue::new(key.clone(), 1)).await.unwrap();
    assert_eq!(
        1,
        store.key_incr(KeyValue::new(key.clone(), 0)).await.unwrap()
    );
    store.key_incr(KeyValue::new(key.clone(), 2)).await.unwrap();
    assert_eq!(
        3,
        store.key_incr(KeyValue::new(key.clone(), 0)).await.unwrap()
    );
    store
        .key_incr(KeyValue::new(key.clone(), -3))
        .await
        .unwrap();
    assert_eq!(
        0,
        store.key_incr(KeyValue::new(key.clone(), 0)).await.unwrap()
    );

    // Test counter expiry
    let key = "fgh".as_bytes().to_vec();
    store
        .key_incr(KeyValue::new(key.clone(), 1).expires(1))
        .await
        .unwrap();
    assert_eq!(
        1,
        store.key_incr(KeyValue::new(key.clone(), 0)).await.unwrap()
    );
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    assert_eq!(
        0,
        store.key_incr(KeyValue::new(key.clone(), 0)).await.unwrap()
    );

    // Test rate limiter
    assert!(
        store
            .is_rate_allowed(0, "rate".as_bytes(), &rate)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .is_rate_allowed(0, "rate".as_bytes(), &rate)
            .await
            .unwrap()
            .is_some()
    );
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    assert!(
        store
            .is_rate_allowed(0, "rate".as_bytes(), &rate)
            .await
            .unwrap()
            .is_none()
    );
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Test locking
    for iteration in [1, 2] {
        let mut tasks = Vec::new();
        for _ in 0..100 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store.try_lock(0, "lock".as_bytes(), 1).await.unwrap()
            }));
        }
        // Only one should return true
        let mut count = 0;
        for task in tasks {
            if task.await.unwrap() {
                count += 1;
            }
        }
        assert_eq!(1, count, "Iteration {}", iteration);

        // Wait 2 seconds for the lock to expire
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }

    // Failed lock attempts must not extend the lock expiry
    assert!(store.try_lock(0, "lock".as_bytes(), 2).await.unwrap());
    let mut acquired = false;
    for _ in 0..12 {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        if store.try_lock(0, "lock".as_bytes(), 2).await.unwrap() {
            acquired = true;
            break;
        }
    }
    assert!(acquired, "Abandoned lock was never released");
    store.remove_lock(0, "lock".as_bytes()).await.unwrap();

    // Test prefix delete
    store
        .key_set(KeyValue::with_prefix(
            1,
            [0],
            "hello".to_string().into_bytes(),
        ))
        .await
        .unwrap();
    for v in 0u32..2020u32 {
        store
            .key_set(KeyValue::with_prefix(
                0,
                pack_u32(0, v),
                "world".to_string().into_bytes(),
            ))
            .await
            .unwrap();
        store
            .key_incr(KeyValue::with_prefix(0, pack_u32(1, v), 123).expires(3600))
            .await
            .unwrap();
    }

    // Make sure the keys are there
    assert_eq!(
        Some("hello"),
        store
            .key_get::<String>(KeyValue::<()>::build_key(1, [0]))
            .await
            .unwrap()
            .as_deref()
    );
    for v in [0, 1000, 1001, 2000, 2001] {
        assert_eq!(
            Some("world"),
            store
                .key_get::<String>(KeyValue::<()>::build_key(0, pack_u32(0, v)))
                .await
                .unwrap()
                .as_deref()
        );
    }
    for v in [0, 1000, 1001, 2000, 2001] {
        assert_ne!(
            0,
            store
                .key_incr(KeyValue::new(
                    KeyValue::<()>::build_key(0, pack_u32(1, v)),
                    0
                ))
                .await
                .unwrap()
        );
    }

    // Delete [0, 0, 0, 0, 1] prefix and make sure only the keys with that prefix are gone
    store
        .key_delete_prefix(&KeyValue::<()>::build_key(0, 1u32.to_be_bytes()))
        .await
        .unwrap();

    assert_eq!(
        Some("hello"),
        store
            .key_get::<String>(KeyValue::<()>::build_key(1, [0]))
            .await
            .unwrap()
            .as_deref()
    );
    for v in [0, 1000, 1001, 2000, 2001] {
        assert_eq!(
            Some("world"),
            store
                .key_get::<String>(KeyValue::<()>::build_key(0, pack_u32(0, v)))
                .await
                .unwrap()
                .as_deref()
        );
    }

    for v in [0, 1000, 1001, 2000, 2001] {
        assert_eq!(
            0,
            store
                .key_incr(KeyValue::new(
                    KeyValue::<()>::build_key(0, pack_u32(1, v)),
                    0
                ))
                .await
                .unwrap()
        );
    }

    // Delete [0, 0, 0, 0, 0] prefix and make sure only the keys with that prefix are gone
    store
        .key_delete_prefix(&KeyValue::<()>::build_key(0, 0u32.to_be_bytes()))
        .await
        .unwrap();

    assert_eq!(
        Some("hello"),
        store
            .key_get::<String>(KeyValue::<()>::build_key(1, [0]))
            .await
            .unwrap()
            .as_deref()
    );
    for v in [0, 1000, 1001, 2000, 2001] {
        assert_eq!(
            None,
            store
                .key_get::<String>(KeyValue::<()>::build_key(0, pack_u32(0, v)))
                .await
                .unwrap()
                .as_deref()
        );
    }

    // Delete [1, ...] prefix and make sure it's all gone
    store.key_delete_prefix(&[1u8]).await.unwrap();

    assert_eq!(
        None,
        store
            .key_get::<String>(KeyValue::<()>::build_key(1, [0]))
            .await
            .unwrap()
            .as_deref()
    );
}

fn pack_u32(a: u32, b: u32) -> Vec<u8> {
    (((a as u64) << 32) | b as u64).to_be_bytes().to_vec()
}
