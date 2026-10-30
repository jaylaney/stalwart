/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{Sequence, copy_move},
    receiver::Receiver,
};

#[test]
fn parse_copy() {
    let mut receiver = Receiver::new();

    assert_eq!(
        receiver
            .parse(&mut "A003 COPY 2:4 MEETING\r\n".as_bytes().iter())
            .unwrap()
            .parse_copy_move(false)
            .unwrap(),
        copy_move::Arguments {
            sequence_set: Sequence::Range {
                start: 2.into(),
                end: 4.into(),
            },
            mailbox_name: "MEETING".into(),
            tag: "A003".into(),
        }
    );
    assert_eq!(
        receiver
            .parse(&mut "A003 COPY 2:4 \"You &- Me\"\r\n".as_bytes().iter())
            .unwrap()
            .parse_copy_move(false)
            .unwrap(),
        copy_move::Arguments {
            sequence_set: Sequence::Range {
                start: 2.into(),
                end: 4.into(),
            },
            mailbox_name: "You & Me".into(),
            tag: "A003".into(),
        }
    );
}
