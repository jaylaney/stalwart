/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::Response;

#[test]
fn serialize_uidbatches() {
    assert_eq!(
        String::from_utf8(
            Response {
                ranges: vec![(215295, 99696), (99695, 20351), (20350, 7830), (7829, 1)],
            }
            .serialize("A143")
        )
        .unwrap(),
        concat!(
            "* UIDBATCHES (TAG \"A143\") ",
            "215295:99696,99695:20351,20350:7830,7829:1\r\n"
        )
    );

    assert_eq!(
        String::from_utf8(Response { ranges: vec![] }.serialize("A144")).unwrap(),
        "* UIDBATCHES (TAG \"A144\")\r\n"
    );
}
