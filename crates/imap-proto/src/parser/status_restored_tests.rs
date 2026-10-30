/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{protocol::status, receiver::Receiver};

#[test]
fn parse_status() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A042 STATUS blurdybloop (UIDNEXT MESSAGES)\r\n",
            status::Arguments {
                tag: "A042".into(),
                mailbox_name: "blurdybloop".into(),
                items: vec![status::Status::UidNext, status::Status::Messages],
            },
        ),
        (
            "A043 STATUS foo (OBJECTID)\r\n",
            status::Arguments {
                tag: "A043".into(),
                mailbox_name: "foo".into(),
                items: vec![status::Status::ObjectId],
            },
        ),
        (
            "A044 STATUS foo (MESSAGES OBJECTID UIDVALIDITY)\r\n",
            status::Arguments {
                tag: "A044".into(),
                mailbox_name: "foo".into(),
                items: vec![
                    status::Status::Messages,
                    status::Status::ObjectId,
                    status::Status::UidValidity,
                ],
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_status(true)
                .unwrap(),
            arguments,
            "Failed to parse {command}"
        );
    }
}
