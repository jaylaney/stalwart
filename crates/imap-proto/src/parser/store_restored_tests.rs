/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{
        Flag, Sequence,
        store::{self, Operation},
    },
    receiver::Receiver,
};

#[test]
fn parse_store() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A003 STORE 2:4 +FLAGS (\\Deleted)\r\n",
            store::Arguments {
                sequence_set: Sequence::Range {
                    start: 2.into(),
                    end: 4.into(),
                },
                is_silent: false,
                operation: Operation::Add,
                keywords: vec![Flag::Deleted],
                tag: "A003".into(),
                unchanged_since: None,
            },
        ),
        (
            "A004 STORE *:100 -FLAGS.SILENT ($Phishing $Junk)\r\n",
            store::Arguments {
                sequence_set: Sequence::Range {
                    start: None,
                    end: 100.into(),
                },
                is_silent: true,
                operation: Operation::Clear,
                keywords: vec![Flag::Phishing, Flag::Junk],
                tag: "A004".into(),
                unchanged_since: None,
            },
        ),
        (
            "d105 STORE 7,5,9 (UNCHANGEDSINCE 320162338) +FLAGS.SILENT \\Deleted\r\n",
            store::Arguments {
                sequence_set: Sequence::List {
                    items: vec![
                        Sequence::Number { value: 7 },
                        Sequence::Number { value: 5 },
                        Sequence::Number { value: 9 },
                    ],
                },
                is_silent: true,
                operation: Operation::Add,
                keywords: vec![Flag::Deleted],
                tag: "d105".into(),
                unchanged_since: Some(320162338),
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_store()
                .unwrap(),
            arguments
        );
    }
}
