/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use std::borrow::Cow;
use utils::chained_bytes::ChainedBytes;

use crate::protocol::Flag;

use super::{FetchItem, Section};

#[test]
fn serialize_fetch_data_item() {
    // 0.16's six ENVELOPE, BODY and BODYSTRUCTURE cases are not here. 1.0
    // removed `DataItem::{Envelope, Body, BodyStructure}` and the `Envelope`
    // and `BodyPart` types: the imap crate writes those items straight from
    // the message metadata (`imap/src/op/fetch/structure.rs`,
    // `write_envelope` and `write_structure`). Their equivalent test is
    // handed to Task 5, the owner of the IMAP readers.
    for (item, expected_response) in [
        (
            super::DataItem::Binary {
                sections: vec![1, 2, 3].into(),
                offset: 10.into(),
                contents: super::BodyContents::Owned(b"hello".to_vec()),
            },
            "BINARY[1.2.3]<10> ~{5}\r\nhello",
        ),
        (
            super::DataItem::Binary {
                sections: vec![1, 2, 3].into(),
                offset: None,
                contents: super::BodyContents::Text("hello".into()),
            },
            "BINARY[1.2.3] {5}\r\nhello",
        ),
        (
            super::DataItem::BodySection {
                sections: vec![
                    Section::Part { num: 1 },
                    Section::Part { num: 2 },
                    Section::Mime,
                ]
                .into(),
                origin_octet: 11.into(),
                contents: Cow::Borrowed(&b"howdy"[..]).into(),
            },
            "BODY[1.2.MIME]<11> {5}\r\nhowdy",
        ),
        (
            super::DataItem::BodySection {
                sections: vec![Section::HeaderFields {
                    not: true,
                    fields: vec!["Subject".into(), "x-special".into()],
                }]
                .into(),
                origin_octet: None,
                contents: Cow::Borrowed(&b"howdy"[..]).into(),
            },
            "BODY[HEADER.FIELDS.NOT (SUBJECT X-SPECIAL)] {5}\r\nhowdy",
        ),
        (
            super::DataItem::BodySection {
                sections: vec![Section::HeaderFields {
                    not: false,
                    fields: vec!["From".into(), "List-Archive".into()],
                }]
                .into(),
                origin_octet: None,
                contents: Cow::Borrowed(&b"howdy"[..]).into(),
            },
            "BODY[HEADER.FIELDS (FROM LIST-ARCHIVE)] {5}\r\nhowdy",
        ),
        (
            super::DataItem::Flags {
                flags: vec![Flag::Seen],
            },
            "FLAGS (\\Seen)",
        ),
        (
            super::DataItem::InternalDate { date: 482374938 },
            "INTERNALDATE \"15-Apr-1985 01:02:18 +0000\"",
        ),
    ] {
        let mut buf = Vec::with_capacity(100);

        item.serialize(&mut buf);

        assert_eq!(String::from_utf8(buf).unwrap(), expected_response);
    }
}

#[test]
fn serialize_fetch() {
    let mut buf = Vec::new();
    FetchItem {
        id: 123,
        is_uidonly: false,
        items: vec![
            super::DataItem::Flags {
                flags: vec![Flag::Deleted, Flag::Flagged],
            },
            super::DataItem::Uid { uid: 983 },
            super::DataItem::Rfc822Size { size: 443 },
            super::DataItem::Rfc822Text {
                contents: ChainedBytes::new(&b"hi"[..]),
            },
            super::DataItem::Rfc822Header {
                contents: ChainedBytes::new(&b"header"[..]),
            },
        ],
    }
    .serialize(&mut buf);
    assert_eq!(
        String::from_utf8(buf).unwrap(),
        concat!(
            "* 123 FETCH (FLAGS (\\Deleted \\Flagged) ",
            "UID 983 ",
            "RFC822.SIZE 443 ",
            "RFC822.TEXT {2}\r\nhi ",
            "RFC822.HEADER {6}\r\nheader)\r\n",
        )
    );
}
