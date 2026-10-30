/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{
        Flag, ProtocolVersion, Sequence,
        search::{self, Filter, ModSeqEntry, ResultOption},
    },
    receiver::Receiver,
};

#[test]
fn parse_search() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            b"A282 SEARCH RETURN (MIN COUNT) FLAGGED SINCE 1-Feb-1994 NOT FROM \"Smith\"\r\n"
                .to_vec(),
            search::Arguments {
                tag: "A282".into(),
                result_options: vec![ResultOption::Min, ResultOption::Count],
                filter: vec![
                    Filter::Flagged,
                    Filter::Since(760060800),
                    Filter::Not,
                    Filter::From("Smith".into()),
                    Filter::End,
                ],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            b"A283 SEARCH RETURN () FLAGGED SINCE 1-Feb-1994 NOT FROM \"Smith\"\r\n".to_vec(),
            search::Arguments {
                tag: "A283".into(),
                result_options: vec![],
                filter: vec![
                    Filter::Flagged,
                    Filter::Since(760060800),
                    Filter::Not,
                    Filter::From("Smith".into()),
                    Filter::End,
                ],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            b"A301 SEARCH $ SMALLER 4096\r\n".to_vec(),
            search::Arguments {
                tag: "A301".into(),
                result_options: vec![],
                filter: vec![Filter::seq_saved_search(), Filter::Smaller(4096)],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            "P283 SEARCH CHARSET UTF-8 (OR $ 1,3000:3021) TEXT {8+}\r\nмать\r\n"
                .as_bytes()
                .to_vec(),
            search::Arguments {
                tag: "P283".into(),
                result_options: vec![],
                filter: vec![
                    Filter::Or,
                    Filter::seq_saved_search(),
                    Filter::Sequence(
                        Sequence::List {
                            items: vec![
                                Sequence::number(1),
                                Sequence::range(3000.into(), 3021.into()),
                            ],
                        },
                        false,
                    ),
                    Filter::End,
                    Filter::Text("мать".into()),
                ],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            b"F282 SEARCH RETURN (SAVE) KEYWORD $Junk\r\n".to_vec(),
            search::Arguments {
                tag: "F282".into(),
                result_options: vec![ResultOption::Save],
                filter: vec![Filter::Keyword(Flag::Junk)],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            [
                b"F282 SEARCH OR OR FROM hello@world.com TO ".to_vec(),
                b"test@example.com OR BCC jane@foobar.com ".to_vec(),
                b"CC john@doe.com\r\n".to_vec(),
            ]
            .concat(),
            search::Arguments {
                tag: "F282".into(),
                result_options: vec![],
                filter: vec![
                    Filter::Or,
                    Filter::Or,
                    Filter::From("hello@world.com".into()),
                    Filter::To("test@example.com".into()),
                    Filter::End,
                    Filter::Or,
                    Filter::Bcc("jane@foobar.com".into()),
                    Filter::Cc("john@doe.com".into()),
                    Filter::End,
                    Filter::End,
                ],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            [
                b"abc SEARCH OR SMALLER 10000 OR ".to_vec(),
                b"HEADER Subject \"ravioli festival\" ".to_vec(),
                b"HEADER From \"dr. ravioli\"\r\n".to_vec(),
            ]
            .concat(),
            search::Arguments {
                tag: "abc".into(),
                result_options: vec![],
                filter: vec![
                    Filter::Or,
                    Filter::Smaller(10000),
                    Filter::Or,
                    Filter::Header("Subject".into(), "ravioli festival".into()),
                    Filter::Header("From".into(), "dr. ravioli".into()),
                    Filter::End,
                    Filter::End,
                ],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            [
                b"abc SEARCH (DELETED SEEN ANSWERED) ".to_vec(),
                b"NOT (FROM john TO jane BCC bill) ".to_vec(),
                b"(1,30:* UID 1,2,3,4 $)\r\n".to_vec(),
            ]
            .concat(),
            search::Arguments {
                tag: "abc".into(),
                result_options: vec![],
                filter: vec![
                    Filter::Deleted,
                    Filter::Seen,
                    Filter::Answered,
                    // 1.0 wraps a parenthesised group under NOT in its own AND
                    // (`parse_filters` merges a group into its parent only
                    // when the parent is AND), so NOT applies to the whole
                    // conjunction.
                    Filter::Not,
                    Filter::And,
                    Filter::From("john".into()),
                    Filter::To("jane".into()),
                    Filter::Bcc("bill".into()),
                    Filter::End,
                    Filter::End,
                    Filter::Sequence(
                        Sequence::List {
                            items: vec![Sequence::number(1), Sequence::range(30.into(), None)],
                        },
                        false,
                    ),
                    Filter::Sequence(
                        Sequence::List {
                            items: vec![
                                Sequence::number(1),
                                Sequence::number(2),
                                Sequence::number(3),
                                Sequence::number(4),
                            ],
                        },
                        true,
                    ),
                    Filter::seq_saved_search(),
                ],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            [
                b"abc SEARCH *:* UID *:100,100:* ".to_vec(),
                b"(FLAGGED (DRAFT (DELETED (ANSWERED)))) ".to_vec(),
                b"OR (SENTON 20-Nov-2022) (LARGER 8196)\r\n".to_vec(),
            ]
            .concat(),
            search::Arguments {
                tag: "abc".into(),
                result_options: vec![],
                filter: vec![
                    Filter::seq_range(None, None),
                    Filter::Sequence(
                        Sequence::List {
                            items: vec![
                                Sequence::range(None, 100.into()),
                                Sequence::range(100.into(), None),
                            ],
                        },
                        true,
                    ),
                    Filter::Flagged,
                    Filter::Draft,
                    Filter::Deleted,
                    Filter::Answered,
                    Filter::Or,
                    Filter::SentOn(1668902400),
                    Filter::Larger(8196),
                    Filter::End,
                ],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            [
                b"abc SEARCH NOT (FROM john OR TO jane CC bill) ".to_vec(),
                b"OR (UNDELETED ALL) ($ NOT FLAGGED) ".to_vec(),
                b"(((KEYWORD \"tps report\")))\r\n".to_vec(),
            ]
            .concat(),
            search::Arguments {
                tag: "abc".into(),
                result_options: vec![],
                filter: vec![
                    // The NOT group gets its own AND in 1.0, as above.
                    Filter::Not,
                    Filter::And,
                    Filter::From("john".into()),
                    Filter::Or,
                    Filter::To("jane".into()),
                    Filter::Cc("bill".into()),
                    Filter::End,
                    Filter::End,
                    Filter::End,
                    Filter::Or,
                    Filter::And,
                    Filter::Undeleted,
                    Filter::All,
                    Filter::End,
                    Filter::And,
                    Filter::seq_saved_search(),
                    Filter::Not,
                    Filter::Flagged,
                    Filter::End,
                    Filter::End,
                    Filter::End,
                    Filter::Keyword(Flag::Keyword("tps report".into())),
                ],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            [
                b"B283 SEARCH RETURN (SAVE MIN MAX) CHARSET KOI8-R TEXT ".to_vec(),
                b"{11+}\r\n\xf0\xd2\xc9\xd7\xc5\xd4, \xcd\xc9\xd2\r\n".to_vec(),
            ]
            .concat(),
            search::Arguments {
                tag: "B283".into(),
                result_options: vec![ResultOption::Save, ResultOption::Min, ResultOption::Max],
                filter: vec![Filter::Text("Привет, мир".into())],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            b"B283 SEARCH CHARSET BIG5 FROM \"\xa7A\xa6n\xa1A\xa5@\xac\xc9\"\r\n".to_vec(),
            search::Arguments {
                tag: "B283".into(),
                result_options: vec![],
                filter: vec![Filter::From("你好，世界".into())],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            b"a SEARCH MODSEQ \"/flags/\\draft\" all 620162338\r\n".to_vec(),
            search::Arguments {
                tag: "a".into(),
                result_options: vec![],
                filter: vec![Filter::ModSeq((620162338, ModSeqEntry::All(Flag::Draft)))],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            b"t SEARCH OR NOT MODSEQ 720162338 LARGER 50000\r\n".to_vec(),
            search::Arguments {
                tag: "t".into(),
                result_options: vec![],
                filter: vec![
                    Filter::Or,
                    Filter::Not,
                    Filter::ModSeq((720162338, ModSeqEntry::None)),
                    Filter::End,
                    Filter::Larger(50000),
                    Filter::End,
                ],
                is_esearch: true,
                sort: None,
            },
        ),
        (
            b"5 UID SEARCH BEFORE 1-Dec-2023\r\n".to_vec(),
            search::Arguments {
                tag: "5".into(),
                result_options: vec![],
                filter: vec![Filter::Before(1701388800)],
                is_esearch: true,
                sort: None,
            },
        ),
    ] {
        let command_str = String::from_utf8_lossy(&command).into_owned();
        assert_eq!(
            receiver
                .parse(&mut command.iter())
                .unwrap()
                .parse_search(ProtocolVersion::Rev2)
                .expect(&command_str),
            arguments,
            "{}",
            command_str
        );
    }
}
