/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{create, list::Attribute},
    receiver::Receiver,
};

#[test]
fn parse_create() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A142 CREATE 12345\r\n",
            create::Arguments {
                tag: "A142".into(),
                mailbox_name: "12345".into(),
                mailbox_role: None,
            },
        ),
        (
            "A142 CREATE \"my funky mailbox\"\r\n",
            create::Arguments {
                tag: "A142".into(),
                mailbox_name: "my funky mailbox".into(),
                mailbox_role: None,
            },
        ),
        (
            "t1 CREATE \"Important Messages\" (USE (\\Important))\r\n",
            create::Arguments {
                tag: "t1".into(),
                mailbox_name: "Important Messages".into(),
                mailbox_role: Some(Attribute::Important),
            },
        ),
        (
            "A142 CREATE \"Test-ąęć-Test\"\r\n",
            create::Arguments {
                tag: "A142".into(),
                mailbox_name: "Test-ąęć-Test".into(),
                mailbox_role: None,
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_create(true)
                .unwrap(),
            arguments
        );
    }
}
