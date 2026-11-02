/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{protocol::uidbatches, receiver::Receiver};

#[test]
fn parse_uidbatches() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A143 UIDBATCHES 2000\r\n",
            uidbatches::Arguments {
                tag: "A143".into(),
                batch_size: 2000,
                batch_range: None,
            },
        ),
        (
            "A302 UIDBATCHES 2000 10:20\r\n",
            uidbatches::Arguments {
                tag: "A302".into(),
                batch_size: 2000,
                batch_range: Some((10, 20)),
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_uidbatches()
                .unwrap(),
            arguments,
            "Failed to parse {command}"
        );
    }

    for command in [
        "A1 UIDBATCHES\r\n",
        "A2 UIDBATCHES abc\r\n",
        "A3 UIDBATCHES 2000 10\r\n",
        "A4 UIDBATCHES 0\r\n",
        "A5 UIDBATCHES 2000 0:20\r\n",
        "A6 UIDBATCHES 2000 1:2 junk\r\n",
    ] {
        assert!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_uidbatches()
                .is_err(),
            "Expected an error for {command}"
        );
    }
}
