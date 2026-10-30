/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{protocol::delete, receiver::Receiver};

#[test]
fn parse_delete() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A142 DELETE INBOX\r\n",
            delete::Arguments {
                mailbox_name: "INBOX".into(),
                tag: "A142".into(),
            },
        ),
        (
            "A142 DELETE \"my funky mailbox\"\r\n",
            delete::Arguments {
                mailbox_name: "my funky mailbox".into(),
                tag: "A142".into(),
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_delete(true)
                .unwrap(),
            arguments
        );
    }
}
