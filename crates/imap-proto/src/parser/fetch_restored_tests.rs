/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{
        Sequence,
        fetch::{self, Attribute, Section},
    },
    receiver::Receiver,
};

#[test]
fn parse_fetch() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A654 FETCH 2:4 (FLAGS BODY[HEADER.FIELDS (DATE FROM)])\r\n",
            fetch::Arguments {
                tag: "A654".into(),
                sequence_set: Sequence::range(2.into(), 4.into()),
                attributes: vec![
                    Attribute::Flags,
                    Attribute::BodySection {
                        peek: false,
                        sections: vec![Section::HeaderFields {
                            not: false,
                            fields: vec!["DATE".into(), "FROM".into()],
                        }],
                        partial: None,
                    },
                ],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "A001 FETCH 1 BODY[]\r\n",
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![Attribute::BodySection {
                    peek: false,
                    sections: vec![],
                    partial: None,
                }],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "A001 FETCH 1 (BODY[HEADER])\r\n",
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![Attribute::BodySection {
                    peek: false,
                    sections: vec![Section::Header],
                    partial: None,
                }],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "A001 FETCH 1 (BODY.PEEK[HEADER.FIELDS (X-MAILER)] PREVIEW(LAZY))\r\n",
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![
                    Attribute::BodySection {
                        peek: true,
                        sections: vec![Section::HeaderFields {
                            not: false,
                            fields: vec!["X-MAILER".into()],
                        }],
                        partial: None,
                    },
                    Attribute::Preview { lazy: true },
                ],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "A001 FETCH 1 (BODY[HEADER.FIELDS.NOT (FROM TO SUBJECT)])\r\n",
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![Attribute::BodySection {
                    peek: false,
                    sections: vec![Section::HeaderFields {
                        not: true,
                        fields: vec!["FROM".into(), "TO".into(), "SUBJECT".into()],
                    }],
                    partial: None,
                }],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            // 1.0 follows RFC 3501's section-spec, where MIME needs a part
            // number before it (`Section::parse_section_spec`); 0.16 took a
            // bare `BODY[MIME]`.
            "A001 FETCH 1 (BODY[1.MIME] BODY[TEXT] PREVIEW)\r\n",
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![
                    Attribute::BodySection {
                        peek: false,
                        sections: vec![Section::Part { num: 1 }, Section::Mime],
                        partial: None,
                    },
                    Attribute::BodySection {
                        peek: false,
                        sections: vec![Section::Text],
                        partial: None,
                    },
                    Attribute::Preview { lazy: false },
                ],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "A001 FETCH 1 (BODYSTRUCTURE ENVELOPE FLAGS INTERNALDATE UID)\r\n",
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![
                    Attribute::BodyStructure,
                    Attribute::Envelope,
                    Attribute::Flags,
                    Attribute::InternalDate,
                    Attribute::Uid,
                ],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "A001 FETCH 1 (RFC822 RFC822.HEADER RFC822.SIZE RFC822.TEXT)\r\n",
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![
                    Attribute::Rfc822,
                    Attribute::Rfc822Header,
                    Attribute::Rfc822Size,
                    Attribute::Rfc822Text,
                ],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            concat!(
                "A001 FETCH 1 (",
                "BODY[4.2.HEADER]<0.20> ",
                "BODY.PEEK[3.2.2.2] ",
                "BODY[4.2.TEXT]<4.100> ",
                "BINARY[1.2.3] ",
                "BINARY.PEEK[4] ",
                "BINARY[6.5.4]<100.200> ",
                "BINARY.PEEK[7]<9.88> ",
                "BINARY.SIZE[9.1]",
                ")\r\n"
            ),
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![
                    Attribute::BodySection {
                        peek: false,
                        sections: vec![
                            Section::Part { num: 4 },
                            Section::Part { num: 2 },
                            Section::Header,
                        ],
                        partial: Some((0, 20)),
                    },
                    Attribute::BodySection {
                        peek: true,
                        sections: vec![
                            Section::Part { num: 3 },
                            Section::Part { num: 2 },
                            Section::Part { num: 2 },
                            Section::Part { num: 2 },
                        ],
                        partial: None,
                    },
                    Attribute::BodySection {
                        peek: false,
                        sections: vec![
                            Section::Part { num: 4 },
                            Section::Part { num: 2 },
                            Section::Text,
                        ],
                        partial: Some((4, 100)),
                    },
                    Attribute::Binary {
                        peek: false,
                        sections: vec![1, 2, 3],
                        partial: None,
                    },
                    Attribute::Binary {
                        peek: true,
                        sections: vec![4],
                        partial: None,
                    },
                    Attribute::Binary {
                        peek: false,
                        sections: vec![6, 5, 4],
                        partial: Some((100, 200)),
                    },
                    Attribute::Binary {
                        peek: true,
                        sections: vec![7],
                        partial: Some((9, 88)),
                    },
                    Attribute::BinarySize {
                        sections: vec![9, 1],
                    },
                ],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "A001 FETCH 1 ALL\r\n",
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![
                    Attribute::Flags,
                    Attribute::InternalDate,
                    Attribute::Rfc822Size,
                    Attribute::Envelope,
                ],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "A001 FETCH 1 FULL\r\n",
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![
                    Attribute::Flags,
                    Attribute::InternalDate,
                    Attribute::Rfc822Size,
                    Attribute::Envelope,
                    Attribute::Body,
                ],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "A001 FETCH 1 FAST\r\n",
            fetch::Arguments {
                tag: "A001".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![
                    Attribute::Flags,
                    Attribute::InternalDate,
                    Attribute::Rfc822Size,
                ],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "s100 UID FETCH 1:* (FLAGS MODSEQ) (CHANGEDSINCE 12345 VANISHED)\r\n",
            fetch::Arguments {
                tag: "s100".into(),
                sequence_set: Sequence::range(1.into(), None),
                attributes: vec![Attribute::Flags, Attribute::ModSeq],
                changed_since: 12345.into(),
                include_vanished: true,
            },
        ),
        (
            "9 UID FETCH 1:* UID (VANISHED CHANGEDSINCE 1)\r\n",
            fetch::Arguments {
                tag: "9".into(),
                sequence_set: Sequence::range(1.into(), None),
                attributes: vec![Attribute::Uid],
                changed_since: 1.into(),
                include_vanished: true,
            },
        ),
        (
            "A010 FETCH 1:* (OBJECTID)\r\n",
            fetch::Arguments {
                tag: "A010".into(),
                sequence_set: Sequence::range(1.into(), None),
                attributes: vec![Attribute::ObjectId],
                changed_since: None,
                include_vanished: false,
            },
        ),
        (
            "A011 FETCH 1 (UID OBJECTID FLAGS)\r\n",
            fetch::Arguments {
                tag: "A011".into(),
                sequence_set: Sequence::number(1),
                attributes: vec![Attribute::Uid, Attribute::ObjectId, Attribute::Flags],
                changed_since: None,
                include_vanished: false,
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_fetch()
                .expect(command),
            arguments,
            "{}",
            command
        );
    }
}
