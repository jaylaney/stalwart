/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{protocol::rename, receiver::Receiver};

#[test]
fn parse_rename() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A142 RENAME \"my funky mailbox\" Private\r\n",
            rename::Arguments {
                mailbox_name: "my funky mailbox".into(),
                new_mailbox_name: "Private".into(),
                tag: "A142".into(),
            },
        ),
        (
            "A142 RENAME {1+}\r\na {1+}\r\nb\r\n",
            rename::Arguments {
                mailbox_name: "a".into(),
                new_mailbox_name: "b".into(),
                tag: "A142".into(),
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_rename(true)
                .unwrap(),
            arguments
        );
    }
}
