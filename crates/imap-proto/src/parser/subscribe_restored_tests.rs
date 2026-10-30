/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{protocol::subscribe, receiver::Receiver};

#[test]
fn parse_subscribe() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A142 SUBSCRIBE #news.comp.mail.mime\r\n",
            subscribe::Arguments {
                mailbox_name: "#news.comp.mail.mime".into(),
                tag: "A142".into(),
            },
        ),
        (
            "A142 SUBSCRIBE \"#news.comp.mail.mime\"\r\n",
            subscribe::Arguments {
                mailbox_name: "#news.comp.mail.mime".into(),
                tag: "A142".into(),
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_subscribe(true)
                .unwrap(),
            arguments
        );
    }
}
