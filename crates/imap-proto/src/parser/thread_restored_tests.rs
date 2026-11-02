/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{
        search::Filter,
        thread::{self, Algorithm},
    },
    receiver::Receiver,
};

#[test]
fn parse_thread() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            b"A283 THREAD ORDEREDSUBJECT UTF-8 SINCE 5-MAR-2000\r\n".to_vec(),
            thread::Arguments {
                algorithm: Algorithm::OrderedSubject,
                filter: vec![Filter::Since(952214400)],
                tag: "A283".into(),
            },
        ),
        (
            b"A284 THREAD REFERENCES US-ASCII TEXT \"gewp\"\r\n".to_vec(),
            thread::Arguments {
                algorithm: Algorithm::References,
                filter: vec![Filter::Text("gewp".into())],
                tag: "A284".into(),
            },
        ),
    ] {
        let command_str = String::from_utf8_lossy(&command).into_owned();

        assert_eq!(
            receiver
                .parse(&mut command.iter())
                .unwrap()
                .parse_thread()
                .expect(&command_str),
            arguments,
            "{}",
            command_str
        );
    }
}
