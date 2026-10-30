/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{
        ObjectId, Sequence,
        select::{self, QResync},
    },
    receiver::Receiver,
};
use std::str::FromStr;
use types::id::Id;

#[test]
fn parse_select_objectid() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A1 SELECT \"foo\" (OBJECTID)\r\n",
            select::Arguments {
                mailbox_name: "foo".into(),
                tag: "A1".into(),
                condstore: false,
                qresync: None,
                objectid: Some(ObjectId::default()),
            },
        ),
        (
            "A2 SELECT \"foo\" (OBJECTID (MAILBOXID bcd ACCOUNTID xyz))\r\n",
            select::Arguments {
                mailbox_name: "foo".into(),
                tag: "A2".into(),
                condstore: false,
                qresync: None,
                objectid: Some(ObjectId {
                    mailbox_id: Some(Id::from_str("bcd").unwrap()),
                    account_id: Some(Id::from_str("xyz").unwrap()),
                    ..Default::default()
                }),
            },
        ),
        (
            "A3 EXAMINE \"foo\" (OBJECTID (MAILBOXID bcd))\r\n",
            select::Arguments {
                mailbox_name: "foo".into(),
                tag: "A3".into(),
                condstore: false,
                qresync: None,
                objectid: Some(ObjectId {
                    mailbox_id: Some(Id::from_str("bcd").unwrap()),
                    ..Default::default()
                }),
            },
        ),
        (
            "A4 SELECT \"foo\" (CONDSTORE OBJECTID)\r\n",
            select::Arguments {
                mailbox_name: "foo".into(),
                tag: "A4".into(),
                condstore: true,
                qresync: None,
                objectid: Some(ObjectId::default()),
            },
        ),
        (
            "A5 SELECT \"foo\" (OBJECTID (FOOBAR baz MAILBOXID bcd))\r\n",
            select::Arguments {
                mailbox_name: "foo".into(),
                tag: "A5".into(),
                condstore: false,
                qresync: None,
                objectid: Some(ObjectId {
                    mailbox_id: Some(Id::from_str("bcd").unwrap()),
                    ..Default::default()
                }),
            },
        ),
        (
            "A6 SELECT \"foo\" (OBJECTID (MAILBOXID 456))\r\n",
            select::Arguments {
                mailbox_name: "foo".into(),
                tag: "A6".into(),
                condstore: false,
                qresync: None,
                objectid: Some(ObjectId::default()),
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_select(true)
                .unwrap(),
            arguments,
            "Failed to parse {command}"
        );
    }
}

#[test]
fn parse_select() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A142 SELECT INBOX\r\n",
            select::Arguments {
                mailbox_name: "INBOX".into(),
                tag: "A142".into(),
                condstore: false,
                qresync: None,
                objectid: None,
            },
        ),
        (
            "A142 SELECT \"my funky mailbox\"\r\n",
            select::Arguments {
                mailbox_name: "my funky mailbox".into(),
                tag: "A142".into(),
                condstore: false,
                qresync: None,
                objectid: None,
            },
        ),
        (
            "A142 SELECT INBOX (CONDSTORE)\r\n",
            select::Arguments {
                mailbox_name: "INBOX".into(),
                tag: "A142".into(),
                condstore: true,
                qresync: None,
                objectid: None,
            },
        ),
        (
            "A142 SELECT INBOX (QRESYNC (3857529045 20010715194032001 1:198))\r\n",
            select::Arguments {
                mailbox_name: "INBOX".into(),
                tag: "A142".into(),
                condstore: false,
                qresync: QResync {
                    uid_validity: 3857529045,
                    modseq: 20010715194032001,
                    known_uids: Some(Sequence::Range {
                        start: Some(1),
                        end: Some(198),
                    }),
                    seq_match: None,
                }
                .into(),
                objectid: None,
            },
        ),
        (
            concat!(
                "A03 SELECT INBOX (QRESYNC (67890007 90060115194045000 ",
                "41:211,214:541) CONDSTORE)\r\n"
            ),
            select::Arguments {
                mailbox_name: "INBOX".into(),
                tag: "A03".into(),
                condstore: true,
                qresync: QResync {
                    uid_validity: 67890007,
                    modseq: 90060115194045000,
                    known_uids: Some(Sequence::List {
                        items: vec![
                            Sequence::Range {
                                start: Some(41),
                                end: Some(211),
                            },
                            Sequence::Range {
                                start: Some(214),
                                end: Some(541),
                            },
                        ],
                    }),
                    seq_match: None,
                }
                .into(),
                objectid: None,
            },
        ),
        (
            concat!(
                "B04 SELECT INBOX (QRESYNC (67890007 ",
                "90060115194045000 1:29997 (5000,7500,9000,9990:9999 15000,",
                "22500,27000,29970,29973,29976,29979,29982,29985,29988,29991,",
                "29994,29997)))\r\n"
            ),
            select::Arguments {
                mailbox_name: "INBOX".into(),
                tag: "B04".into(),
                condstore: false,
                qresync: QResync {
                    uid_validity: 67890007,
                    modseq: 90060115194045000,
                    known_uids: Some(Sequence::Range {
                        start: Some(1),
                        end: Some(29997),
                    }),
                    seq_match: Some((
                        Sequence::List {
                            items: vec![
                                Sequence::Number { value: 5000 },
                                Sequence::Number { value: 7500 },
                                Sequence::Number { value: 9000 },
                                Sequence::Range {
                                    start: Some(9990),
                                    end: Some(9999),
                                },
                            ],
                        },
                        Sequence::List {
                            items: vec![
                                Sequence::Number { value: 15000 },
                                Sequence::Number { value: 22500 },
                                Sequence::Number { value: 27000 },
                                Sequence::Number { value: 29970 },
                                Sequence::Number { value: 29973 },
                                Sequence::Number { value: 29976 },
                                Sequence::Number { value: 29979 },
                                Sequence::Number { value: 29982 },
                                Sequence::Number { value: 29985 },
                                Sequence::Number { value: 29988 },
                                Sequence::Number { value: 29991 },
                                Sequence::Number { value: 29994 },
                                Sequence::Number { value: 29997 },
                            ],
                        },
                    )),
                }
                .into(),
                objectid: None,
            },
        ),
        (
            "A12 SELECT \"INBOX\" (QRESYNC (1693237464 16582))\r\n",
            select::Arguments {
                mailbox_name: "INBOX".into(),
                tag: "A12".into(),
                condstore: false,
                qresync: QResync {
                    uid_validity: 1693237464,
                    modseq: 16582,
                    known_uids: None,
                    seq_match: None,
                }
                .into(),
                objectid: None,
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap_or_else(|err| panic!("Failed to parse command '{}': {:?}", command, err))
                .parse_select(true)
                .unwrap_or_else(|err| panic!("Failed to parse command '{}': {:?}", command, err)),
            arguments,
            "Failed to parse {}",
            command
        );
    }
}
