/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn test_topological_sort() {
    let mut sort = TopologicalSort::with_capacity(6);
    sort.insert(1, 2);
    sort.insert(1, 3);
    sort.insert(2, 4);
    sort.insert(3, 4);
    sort.insert(4, 5);
    sort.insert(5, 6);

    let mut iter = sort.into_iterator();
    assert_eq!(iter.next(), Some(1));
    assert_eq!(iter.next(), Some(2));
    assert_eq!(iter.next(), Some(3));
    assert_eq!(iter.next(), Some(4));
    assert_eq!(iter.next(), Some(5));
    assert_eq!(iter.next(), Some(6));
    assert_eq!(iter.next(), None);
    assert!(iter.is_valid(), "{:?}", iter);
}

#[test]
fn test_topological_sort_cycle() {
    let mut sort = TopologicalSort::with_capacity(6);
    sort.insert(1, 2);
    sort.insert(2, 3);
    sort.insert(3, 1);

    let mut iter = sort.into_iterator();
    assert_eq!(iter.next(), None);
    assert!(!iter.is_valid());
}
