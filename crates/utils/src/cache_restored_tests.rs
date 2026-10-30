/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn single_shard_retains_large_entry() {
    let capacity = 10_000_000u64;
    let cache = Cache::<u32, String>::new_single_shard(capacity, 1000);
    assert_eq!(cache.inner().num_shards(), 1);

    let value = "x".repeat(9_000_000);
    cache.insert(0, value.clone());
    assert_eq!(cache.get(&0), Some(value));
}

#[test]
fn sharded_cache_drops_entry_larger_than_a_shard() {
    let capacity = 10_000_000u64;
    let cache = Cache::<u32, String>::new_estimated(10_000, capacity);

    let value = "x".repeat((capacity / 2) as usize);
    cache.insert(0, value);

    if cache.inner().num_shards() > 1 {
        assert_eq!(cache.get(&0), None);
    }
}
