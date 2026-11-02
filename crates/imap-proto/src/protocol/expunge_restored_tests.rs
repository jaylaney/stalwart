/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::protocol::ImapResponse;

#[test]
fn serialize_expunge() {
    assert_eq!(
        String::from_utf8(
            super::Response {
                use_vanished: false,
                ids: vec![3, 4, 5]
            }
            .serialize()
        )
        .unwrap(),
        concat!("* 3 EXPUNGE\r\n", "* 3 EXPUNGE\r\n", "* 3 EXPUNGE\r\n",)
    );

    assert_eq!(
        String::from_utf8(
            super::Response {
                use_vanished: false,
                ids: vec![3, 4, 7, 9, 11]
            }
            .serialize()
        )
        .unwrap(),
        concat!(
            "* 3 EXPUNGE\r\n",
            "* 3 EXPUNGE\r\n",
            "* 5 EXPUNGE\r\n",
            "* 6 EXPUNGE\r\n",
            "* 7 EXPUNGE\r\n",
        )
    );

    assert_eq!(
        String::from_utf8(
            super::Response {
                use_vanished: true,
                ids: vec![3, 4, 5]
            }
            .serialize()
        )
        .unwrap(),
        "* VANISHED 3:5\r\n"
    );
}
