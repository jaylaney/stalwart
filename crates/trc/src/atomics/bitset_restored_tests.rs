/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

const TEST_SIZE: usize = 1000;
type TestBitset = AtomicBitset<{ TEST_SIZE.div_ceil(USIZE_BITS) }>;
static BITSET: TestBitset = TestBitset::new();

#[test]
fn test_atomic_bitset() {
    for i in 0..TEST_SIZE {
        assert!(!BITSET.get(i), "Bit {} should be unset in new BITSET", i);
    }

    for i in 0..TEST_SIZE {
        assert!(!BITSET.get(i), "Bit {} should be initially unset", i);
        BITSET.set(i);
        assert!(BITSET.get(i), "Bit {} should be set after setting", i);
    }

    BITSET.clear_all();

    for i in 0..TEST_SIZE {
        BITSET.set(i);
        assert!(BITSET.get(i), "Bit {} should be set before clearing", i);
        BITSET.clear(i);
        assert!(!BITSET.get(i), "Bit {} should be unset after clearing", i);
    }

    BITSET.clear_all();

    // Set even bits
    for i in (0..TEST_SIZE).step_by(2) {
        BITSET.set(i);
    }

    // Check all bits
    for i in 0..TEST_SIZE {
        if i % 2 == 0 {
            assert!(BITSET.get(i), "Even bit {} should be set", i);
        } else {
            assert!(!BITSET.get(i), "Odd bit {} should be unset", i);
        }
    }

    // Clear even bits and set odd bits
    for i in 0..TEST_SIZE {
        if i % 2 == 0 {
            BITSET.clear(i);
        } else {
            BITSET.set(i);
        }
    }

    // Check all bits again
    for i in 0..TEST_SIZE {
        if i % 2 == 0 {
            assert!(!BITSET.get(i), "Even bit {} should now be unset", i);
        } else {
            assert!(BITSET.get(i), "Odd bit {} should now be set", i);
        }
    }
}
