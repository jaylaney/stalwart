/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn bitpack_roundtrip() {
    for num_positions in [
        1,
        10,
        BitPacker1x::BLOCK_LEN,
        BitPacker4x::BLOCK_LEN,
        BitPacker8x::BLOCK_LEN,
        BitPacker8x::BLOCK_LEN + BitPacker4x::BLOCK_LEN + BitPacker1x::BLOCK_LEN,
        BitPacker8x::BLOCK_LEN + BitPacker4x::BLOCK_LEN + BitPacker1x::BLOCK_LEN + 1,
        (BitPacker8x::BLOCK_LEN * 3)
            + (BitPacker4x::BLOCK_LEN * 3)
            + (BitPacker1x::BLOCK_LEN * 3)
            + 1,
        (BitPacker8x::BLOCK_LEN * 32) + 1,
    ] {
        let serialized = KeySerializer::new(num_positions * std::mem::size_of::<u32>())
            .bitpack_sorted(
                &(0..num_positions)
                    .map(|i| (i * i) as u32)
                    .collect::<Vec<_>>(),
            )
            .finalize();

        println!(
            "Testing block {num_positions} with {} size...",
            serialized.len()
        );

        let mut iter = BitpackIterator::new(&serialized).unwrap();

        assert_eq!(
            iter.items_left, num_positions as u32,
            "failed for num_positions: {}",
            num_positions
        );

        for i in 0..num_positions {
            assert_eq!(
                iter.next(),
                Some((i * i) as u32),
                "failed for position: {}",
                i
            );
        }
        assert_eq!(iter.next(), None, "expected end of iterator");
    }
}
