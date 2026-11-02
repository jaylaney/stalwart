/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::protocol::ImapResponse;

#[test]
fn serialize_thread() {
    assert_eq!(
        String::from_utf8(
            super::Response {
                is_uid: true,
                threads: vec![vec![2, 10, 11], vec![49], vec![1, 3]],
            }
            .serialize()
        )
        .unwrap(),
        "* THREAD (2 10 11)(49)(1 3)\r\n"
    );
}
