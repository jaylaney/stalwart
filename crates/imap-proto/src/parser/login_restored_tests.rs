/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{protocol::login, receiver::Receiver};

#[test]
fn parse_login() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "a001 LOGIN SMITH SESAME\r\n",
            login::Arguments {
                tag: "a001".into(),
                username: "SMITH".into(),
                password: "SESAME".into(),
            },
        ),
        (
            "A001 LOGIN {11+}\r\nFRED FOOBAR {7+}\r\nfat man\r\n",
            login::Arguments {
                tag: "A001".into(),
                username: "FRED FOOBAR".into(),
                password: "fat man".into(),
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_login()
                .unwrap(),
            arguments
        );
    }
}
