/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::protocol::{
    ObjectId,
    status::{Status, StatusItem, StatusItemType},
};
use types::id::Id;

#[test]
fn serialize_status() {
    let objectid = ObjectId {
        mailbox_id: Some(Id::from(1u32)),
        account_id: Some(Id::from(2u32)),
        ..Default::default()
    };
    let mut kvpairs = Vec::new();
    objectid.serialize_kvpairs(&mut kvpairs);
    let kvpairs = String::from_utf8(kvpairs).unwrap();

    let mut buf = Vec::new();
    StatusItem {
        mailbox_name: "blurdybloop".into(),
        items: vec![
            (Status::Messages, StatusItemType::Number(231)),
            (Status::UidNext, StatusItemType::Number(44292)),
            (Status::ObjectId, StatusItemType::ObjectId(objectid.clone())),
        ],
    }
    .serialize(&mut buf, true);

    assert_eq!(
        String::from_utf8(buf).unwrap(),
        format!("* STATUS \"blurdybloop\" (MESSAGES 231 UIDNEXT 44292 OBJECTID {kvpairs})\r\n")
    );
}
