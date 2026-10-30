/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::authenticate::{self, Mechanism},
    receiver::Receiver,
};

#[test]
fn parse_authenticate() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "a002 AUTHENTICATE \"EXTERNAL\" {16+}\r\nfred@example.com\r\n",
            authenticate::Arguments {
                tag: "a002".into(),
                mechanism: Mechanism::External,
                params: vec!["fred@example.com".into()],
            },
        ),
        (
            "A01 AUTHENTICATE PLAIN\r\n",
            authenticate::Arguments {
                tag: "A01".into(),
                mechanism: Mechanism::Plain,
                params: vec![],
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_authenticate()
                .unwrap(),
            arguments
        );
    }
}
