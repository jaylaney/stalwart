/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{Command, protocol::Sequence, receiver::CommandParser};

#[test]
fn parse_command() {
    assert_eq!(
        Command::parse(b"GETJMAPACCESS", false),
        Some(Command::GetJmapAccess)
    );
    assert_eq!(Command::parse(b"NOTACOMMAND", false), None);
}

#[test]
fn parse_sequence_set() {
    for (sequence, expected_result) in [
        ("$", Sequence::SavedSearch),
        (
            "*",
            Sequence::Range {
                start: None,
                end: None,
            },
        ),
        (
            "1,3000:3021",
            Sequence::List {
                items: vec![
                    Sequence::Number { value: 1 },
                    Sequence::Range {
                        start: 3000.into(),
                        end: 3021.into(),
                    },
                ],
            },
        ),
        (
            "2,4:7,9,12:*",
            Sequence::List {
                items: vec![
                    Sequence::Number { value: 2 },
                    Sequence::Range {
                        start: 4.into(),
                        end: 7.into(),
                    },
                    Sequence::Number { value: 9 },
                    Sequence::Range {
                        start: 12.into(),
                        end: None,
                    },
                ],
            },
        ),
        (
            "*:4,5:7",
            Sequence::List {
                items: vec![
                    Sequence::Range {
                        start: None,
                        end: 4.into(),
                    },
                    Sequence::Range {
                        start: 5.into(),
                        end: 7.into(),
                    },
                ],
            },
        ),
        (
            "2,4,5",
            Sequence::List {
                items: vec![
                    Sequence::Number { value: 2 },
                    Sequence::Number { value: 4 },
                    Sequence::Number { value: 5 },
                ],
            },
        ),
    ] {
        assert_eq!(
            super::parse_sequence_set(sequence.as_bytes()).unwrap(),
            expected_result
        );
    }
}
