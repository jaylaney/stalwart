/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access search format version 1 (search spec 4.2).

// Encoded layouts, including each length prefix:
// thread record = u16 len | u8 flags | u32 document_id | u32 thread_id | u32 revision
//               | u64 created | u64 deleted (0 = none) | u128 subject_hash
//               | u8 n | n × u128 hash = 48 + 16·n bytes, n ≤ K
// alias         = u16 len | u8 kind | u32 source | u32 target | u32 revision
// tombstone     = u32 document_id | u64 deleted
// revision row  = u32 revision | u64 change_id
pub const SEARCH_FORMAT_V1: u8 = 1;
pub const K: usize = 16;
pub const RECORD_MAX: usize = 48 + 16 * K;
pub const ALIAS_LEN: usize = 15;
pub const TOMBSTONE_LEN: usize = 12;
pub const REVISION_ROW_LEN: usize = 12;

/// One admitted item's charge against the hidden budget (spec 4.2, R21).
pub const fn reservation() -> usize {
    RECORD_MAX + (K - 1) * ALIAS_LEN
}

/// Round the framed length up to a power of two, with a minimum class (spec 4.2).
pub fn padded_len(framed_len: usize, min_class: usize) -> usize {
    framed_len.max(1).next_power_of_two().max(min_class)
}

/// The framed length a compacted hidden section pads to (spec 4.2, 5.5).
pub fn compaction_allowance(n_records: usize) -> usize {
    n_records * (RECORD_MAX + ALIAS_LEN) + 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_record_bound_and_the_reservation_are_the_specs() {
        assert_eq!(K, 16);
        assert_eq!(RECORD_MAX, 304);
        assert_eq!(ALIAS_LEN, 15);
        assert_eq!(TOMBSTONE_LEN, 12);
        assert_eq!(REVISION_ROW_LEN, 12);
        assert_eq!(reservation(), 529);
    }

    #[test]
    fn padding_rounds_to_a_class_above_the_minimum() {
        const M: usize = 65_536;
        assert_eq!(padded_len(1, M), M);
        assert_eq!(padded_len(33_301, M), M); // H + R + (K − 1)·a + 4 at the defaults
        assert_eq!(padded_len(M - 1, M), M); // a payload of M − 5 bytes
        assert_eq!(padded_len(M, M), M); // a payload of M − 4 bytes
        assert_eq!(padded_len(M + 1, M), 2 * M);
        assert_eq!(padded_len(2 * M, M), 2 * M);
        assert_eq!(padded_len(2 * M + 1, M), 4 * M);
        assert_eq!(padded_len(100, 4096), 4096);
    }

    #[test]
    fn the_compaction_allowance_is_a_function_of_the_record_count() {
        assert_eq!(compaction_allowance(0), 4);
        assert_eq!(compaction_allowance(1), RECORD_MAX + ALIAS_LEN + 4);
        assert_eq!(compaction_allowance(30_000), 30_000 * 319 + 4);
        // 30,000 messages: a 16 MiB class (spec 5.5, decision 2 (c)).
        assert_eq!(
            padded_len(compaction_allowance(30_000), 65_536),
            16 * 1024 * 1024
        );
    }
}
