/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::protocol::{
    ImapResponse,
    status::{Status, StatusItem, StatusItemType},
};

use super::{Attribute, ChildInfo, ListItem, Tag};

#[test]
fn serialize_list_item() {
    for (response, expected_v2, expected_v1) in [
        (
            super::ListItem {
                mailbox_name: "".into(),
                attributes: vec![],
                tags: vec![],
                objectid: None,
                metadata: None,
            },
            "* LIST () \"/\" \"\"\r\n",
            "* LIST () \"/\" \"\"\r\n",
        ),
        (
            super::ListItem {
                mailbox_name: "中國書店".into(),
                attributes: vec![Attribute::NoInferiors, Attribute::Drafts],
                tags: vec![],
                objectid: None,
                metadata: None,
            },
            // 1.0 sends a rev2 or UTF8=ACCEPT client the name as stored, with no
            // OLDNAME tag (RFC 9051 5.1; `ListItem::serialize`).
            "* LIST (\\NoInferiors \\Drafts) \"/\" \"中國書店\"\r\n",
            "* LIST (\\NoInferiors \\Drafts) \"/\" \"&Ti1XC2b4Xpc-\"\r\n",
        ),
        (
            super::ListItem {
                mailbox_name: "☺".into(),
                attributes: vec![Attribute::Subscribed, Attribute::Remote],
                tags: vec![Tag::ChildInfo(vec![ChildInfo::Subscribed])],
                objectid: None,
                metadata: None,
            },
            concat!(
                "* LIST (\\Subscribed \\Remote) \"/\" \"☺\" ",
                "(\"CHILDINFO\" (\"SUBSCRIBED\"))\r\n"
            ),
            concat!(
                "* LIST (\\Subscribed \\Remote) \"/\" \"&Jjo-\" ",
                "(\"CHILDINFO\" (\"SUBSCRIBED\"))\r\n"
            ),
        ),
        (
            super::ListItem {
                mailbox_name: "foo".into(),
                attributes: vec![Attribute::HasNoChildren],
                tags: vec![Tag::ChildInfo(vec![ChildInfo::Subscribed])],
                objectid: None,
                metadata: None,
            },
            "* LIST (\\HasNoChildren) \"/\" \"foo\" (\"CHILDINFO\" (\"SUBSCRIBED\"))\r\n",
            "* LIST (\\HasNoChildren) \"/\" \"foo\" (\"CHILDINFO\" (\"SUBSCRIBED\"))\r\n",
        ),
    ] {
        let mut buf_1 = Vec::with_capacity(100);
        let mut buf_2 = Vec::with_capacity(100);

        response.serialize(&mut buf_1, false, false, false);
        response.serialize(&mut buf_2, true, true, false);

        let response_v1 = String::from_utf8(buf_1).unwrap();
        let response_v2 = String::from_utf8(buf_2).unwrap();

        assert_eq!(response_v2, expected_v2);
        assert_eq!(response_v1, expected_v1);
    }
}

#[test]
fn serialize_list() {
    let mut response = super::Response {
        list_items: vec![
            ListItem {
                mailbox_name: "INBOX".into(),
                attributes: vec![Attribute::Subscribed],
                tags: vec![],
                objectid: None,
                metadata: None,
            },
            ListItem {
                mailbox_name: "foo".into(),
                attributes: vec![],
                tags: vec![Tag::ChildInfo(vec![ChildInfo::Subscribed])],
                objectid: None,
                metadata: None,
            },
        ],
        status_items: vec![
            StatusItem {
                mailbox_name: "INBOX".into(),
                items: vec![(Status::Messages, StatusItemType::Number(17))],
            },
            StatusItem {
                mailbox_name: "foo".into(),
                items: vec![
                    (Status::Messages, StatusItemType::Number(30)),
                    (Status::Unseen, StatusItemType::Number(29)),
                ],
            },
        ],
        is_lsub: false,
        is_rev2: true,
        is_utf8: true,
    };
    let expected_v2 = concat!(
        "* LIST (\\Subscribed) \"/\" \"INBOX\"\r\n",
        "* STATUS \"INBOX\" (MESSAGES 17)\r\n",
        "* LIST () \"/\" \"foo\" (\"CHILDINFO\" (\"SUBSCRIBED\"))\r\n",
        "* STATUS \"foo\" (MESSAGES 30 UNSEEN 29)\r\n",
    );
    let expected_v1 = concat!(
        "* LSUB (\\Subscribed) \"/\" \"INBOX\"\r\n",
        "* LSUB () \"/\" \"foo\" (\"CHILDINFO\" (\"SUBSCRIBED\"))\r\n",
    );

    let response_v2 = String::from_utf8(response.clone().serialize()).unwrap();
    response.is_rev2 = false;
    response.is_utf8 = false;
    response.is_lsub = true;
    response.status_items.clear();
    let response_v1 = String::from_utf8(response.serialize()).unwrap();

    assert_eq!(response_v2, expected_v2);
    assert_eq!(response_v1, expected_v1);
}
