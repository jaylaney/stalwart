/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn test_cheeky_hash_all() {
    // 1.0 moved the length byte to the end of the array and stores up to
    // HASH_SIZE bytes directly (`as_key`, `len`, `from_key_bytes` replace
    // 0.16's `as_bytes`, `payload` and `deserialize`). Tests 1-6 check the same
    // properties against that layout.

    // Test 1: Empty input
    let hash_empty = CheekyHash::new([]);
    assert_eq!(hash_empty.len(), 0, "Empty input should have length 0");
    assert_eq!(
        hash_empty.as_key().len(),
        0,
        "Empty input should have an empty key"
    );

    // Test 2: Single byte input
    let hash_single = CheekyHash::new([42]);
    assert_eq!(hash_single.len(), 1, "Single byte should have length 1");
    assert_eq!(
        hash_single.as_key()[0],
        42,
        "Single byte value should be preserved"
    );
    assert_eq!(hash_single.as_key().len(), 1);

    // Test 3: Small input (less than HASH_SIZE)
    let small_data = b"hello";
    let hash_small = CheekyHash::new(small_data);
    assert_eq!(hash_small.len(), 5, "Length should be 5");
    assert_eq!(
        hash_small.as_key(),
        small_data,
        "Small data should be stored directly"
    );
    assert_eq!(hash_small.key_len(), 5);

    // Test 4: Input exactly at the HASH_SIZE boundary
    let boundary_data = vec![1u8; HASH_SIZE];
    let hash_boundary = CheekyHash::new(&boundary_data);
    assert_eq!(hash_boundary.len(), HASH_SIZE, "Length should be HASH_SIZE");
    assert_eq!(
        hash_boundary.as_key(),
        &boundary_data[..],
        "Boundary data should be stored directly"
    );

    // Test 5: Large input (greater than HASH_SIZE) - uses hashing
    let large_data = vec![7u8; HASH_SIZE + 1];
    let hash_large = CheekyHash::new(&large_data);
    assert_eq!(
        hash_large.len(),
        HASH_SIZE + 1,
        "Large data should keep its length"
    );
    assert_eq!(
        hash_large.as_key().len(),
        HASH_SIZE,
        "Large data hash should be full length"
    );
    // Verify it's actually hashed (not raw data)
    assert_ne!(
        hash_large.as_key(),
        &large_data[..HASH_SIZE],
        "Large data should be hashed, not stored directly"
    );

    // Test 6: the key round-trips through from_key_bytes
    for hash in [
        hash_empty,
        hash_single,
        hash_small,
        hash_boundary,
        hash_large,
    ] {
        assert_eq!(
            CheekyHash::from_key_bytes(hash.as_key(), hash.len() as u8),
            Some(hash),
            "from_key_bytes should rebuild the hash from its key"
        );
    }
    assert_eq!(
        CheekyHash::from_key_bytes(b"abc", 4),
        None,
        "a key whose length disagrees with the length byte is refused"
    );

    // Test 7: Copy, Clone, PartialEq traits
    let hash1 = CheekyHash::new(b"identical");
    let hash2 = hash1; // Copy
    assert_eq!(hash1, hash2, "Copied hashes should be equal");

    // Test 8: Different inputs produce different hashes
    let hash_a = CheekyHash::new(b"abc");
    let hash_b = CheekyHash::new(b"def");
    assert_ne!(
        hash_a, hash_b,
        "Different inputs should produce different hashes"
    );

    // Test 9: Same input produces same hash (deterministic)
    let hash_x1 = CheekyHash::new(b"deterministic");
    let hash_x2 = CheekyHash::new(b"deterministic");
    assert_eq!(
        hash_x1, hash_x2,
        "Same input should produce identical hashes"
    );

    // Test 10: Large inputs with different content produce different hashes
    let large1 = vec![1u8; 100];
    let large2 = vec![2u8; 100];
    let hash_large1 = CheekyHash::new(&large1);
    let hash_large2 = CheekyHash::new(&large2);
    assert_ne!(
        hash_large1, hash_large2,
        "Different large inputs should produce different hashes"
    );

    // Test 11: Hash trait (can be used in HashMap/HashSet)
    use std::collections::HashMap;
    let mut map = HashMap::new();
    let key = CheekyHash::new(b"key");
    map.insert(key, "value");
    assert_eq!(
        map.get(&key),
        Some(&"value"),
        "CheekyHash should work as HashMap key"
    );

    // Test 12: Debug trait
    let hash = CheekyHash::new(b"debug");
    let debug_str = format!("{:?}", hash);
    assert!(
        debug_str.contains("CheekyHash"),
        "Debug output should contain type name"
    );

    // Test 13: CheekyHashSet and CheekyHashMap
    let mut cheeky_set: CheekyHashSet = CheekyHashSet::default();
    cheeky_set.insert(CheekyHash::new(b"set_item"));
    assert!(cheeky_set.contains(&CheekyHash::new(b"set_item")));
    let mut cheeky_map: CheekyHashMap<&str> = CheekyHashMap::default();
    cheeky_map.insert(CheekyHash::new(b"map_key"), "map_value");
    assert_eq!(
        cheeky_map.get(&CheekyHash::new(b"map_key")),
        Some(&"map_value")
    );

    println!("All CheekyHash tests passed!");
}
