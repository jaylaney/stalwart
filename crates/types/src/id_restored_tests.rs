/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::str::FromStr;

use crate::id::Id;

#[test]
fn parse_jmap_id() {
    for number in [
        0,
        1,
        10,
        1000,
        Id::singleton().id(),
        u64::MAX / 2,
        u64::MAX - 1,
        u64::MAX,
    ] {
        let id = Id::from(number);
        assert_eq!(Id::from_str(&id.to_string()).unwrap(), id);
    }

    // 0.16 accepted this 26-symbol id by shifting the excess bits out. 1.0
    // decodes ids with encodify's `STALWART.decode_u64`, which refuses a
    // numeral longer than 13 symbols with `Error::Overflow`.
    assert!(Id::from_str("p333333333333p333333333333").is_err());
}
