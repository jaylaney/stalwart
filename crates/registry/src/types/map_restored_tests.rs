/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::{jmap::JsonPointerPatch, schema::prelude::Property};
use jmap_tools::JsonPointer;

fn patch_member(ptr_str: &str) -> Map<Id> {
    let mut map = Map::<Id>::default();
    let ptr = JsonPointer::<Property>::parse(ptr_str);
    let pointer = JsonPointerPatch::new(&ptr);
    map.patch(pointer, Value::Bool(true)).expect("patch failed");
    map
}

#[test]
fn patch_map_id_digit_keys() {
    for id in [0u64, 28, 29, 70, 861, 954, 957, 30554] {
        let id = Id::new(id);
        let key = id.as_string();
        let map = patch_member(&key);
        assert_eq!(map.0, vec![id], "id {} via key '{key}'", id.id());
    }
}
