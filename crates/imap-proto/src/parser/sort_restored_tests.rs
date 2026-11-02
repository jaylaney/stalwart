/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{
        Flag,
        search::{Arguments, Comparator, Filter, ResultOption, Sort},
    },
    receiver::Receiver,
};

#[test]
fn parse_sort() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            b"A282 SORT (SUBJECT) UTF-8 SINCE 1-Feb-1994\r\n".to_vec(),
            Arguments {
                sort: vec![Comparator {
                    sort: Sort::Subject,
                    ascending: true,
                }]
                .into(),
                filter: vec![Filter::Since(760060800)],
                result_options: Vec::new(),
                is_esearch: false,
                tag: "A282".into(),
            },
        ),
        (
            b"A283 SORT (SUBJECT REVERSE DATE) UTF-8 ALL\r\n".to_vec(),
            Arguments {
                sort: vec![
                    Comparator {
                        sort: Sort::Subject,
                        ascending: true,
                    },
                    Comparator {
                        sort: Sort::Date,
                        ascending: false,
                    },
                ]
                .into(),
                filter: vec![Filter::All],
                result_options: Vec::new(),
                is_esearch: false,
                tag: "A283".into(),
            },
        ),
        (
            b"A284 SORT (SUBJECT) US-ASCII TEXT \"not in mailbox\"\r\n".to_vec(),
            Arguments {
                sort: vec![Comparator {
                    sort: Sort::Subject,
                    ascending: true,
                }]
                .into(),
                filter: vec![Filter::Text("not in mailbox".into())],
                result_options: Vec::new(),
                is_esearch: false,
                tag: "A284".into(),
            },
        ),
        (
            [
                b"A284 SORT (REVERSE ARRIVAL FROM) iso-8859-6 SUBJECT ".to_vec(),
                b"\"\xe5\xd1\xcd\xc8\xc7 \xc8\xc7\xe4\xd9\xc7\xe4\xe5\"\r\n".to_vec(),
            ]
            .concat(),
            Arguments {
                sort: vec![
                    Comparator {
                        sort: Sort::Arrival,
                        ascending: false,
                    },
                    Comparator {
                        sort: Sort::From,
                        ascending: true,
                    },
                ]
                .into(),
                filter: vec![Filter::Subject("مرحبا بالعالم".into())],
                result_options: Vec::new(),
                is_esearch: false,
                tag: "A284".into(),
            },
        ),
        (
            [
                b"E01 UID SORT RETURN (COUNT) (REVERSE DATE) ".to_vec(),
                b"UTF-8 UNDELETED UNKEYWORD $Junk\r\n".to_vec(),
            ]
            .concat(),
            Arguments {
                sort: vec![Comparator {
                    sort: Sort::Date,
                    ascending: false,
                }]
                .into(),
                filter: vec![Filter::Undeleted, Filter::Unkeyword(Flag::Junk)],
                result_options: vec![ResultOption::Count],
                is_esearch: true,
                tag: "E01".into(),
            },
        ),
    ] {
        let command_str = String::from_utf8_lossy(&command).into_owned();

        assert_eq!(
            receiver
                .parse(&mut command.iter())
                .unwrap()
                .parse_sort()
                .expect(&command_str),
            arguments,
            "{}",
            command_str
        );
    }
}
