/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{capability::Capability, enable},
    receiver::Receiver,
};

#[test]
fn parse_enable() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "t2 ENABLE IMAP4rev2 CONDSTORE\r\n",
            enable::Arguments {
                tag: "t2".into(),
                capabilities: vec![Capability::IMAP4rev2, Capability::CondStore],
            },
        ),
        (
            "t3 ENABLE OBJECTID+\r\n",
            enable::Arguments {
                tag: "t3".into(),
                capabilities: vec![Capability::ObjectIdPlus],
            },
        ),
        (
            "t4 ENABLE CONDSTORE OBJECTID+ UTF8=ACCEPT\r\n",
            enable::Arguments {
                tag: "t4".into(),
                capabilities: vec![
                    Capability::CondStore,
                    Capability::ObjectIdPlus,
                    Capability::Utf8Accept,
                ],
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_enable()
                .unwrap(),
            arguments,
            "Failed to parse {command}"
        );
    }
}
