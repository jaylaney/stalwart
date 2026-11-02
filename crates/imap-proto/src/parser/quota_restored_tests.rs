/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{protocol::quota, receiver::Receiver};

#[test]
fn parse_quota() {
    let mut receiver = Receiver::new();

    let (command, arguments) = (
        "A142 GETQUOTAROOT INBOX\r\n",
        quota::Arguments {
            name: "INBOX".into(),
            tag: "A142".into(),
        },
    );
    assert_eq!(
        receiver
            .parse(&mut command.as_bytes().iter())
            .unwrap()
            .parse_get_quota_root(true)
            .unwrap(),
        arguments
    );

    let (command, arguments) = (
        "A142 GETQUOTA \"my funky mailbox\"\r\n",
        quota::Arguments {
            name: "my funky mailbox".into(),
            tag: "A142".into(),
        },
    );
    assert_eq!(
        receiver
            .parse(&mut command.as_bytes().iter())
            .unwrap()
            .parse_get_quota()
            .unwrap(),
        arguments
    );
}
