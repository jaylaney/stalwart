/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::protocol::acl::{GetAclResponse, ListRightsResponse, MyRightsResponse, Rights};

#[test]
fn serialize_acl() {
    assert_eq!(
        String::from_utf8(
            GetAclResponse {
                mailbox_name: "INBOX".into(),
                permissions: vec![
                    (
                        "Fred".into(),
                        vec![
                            Rights::Lookup,
                            Rights::Read,
                            Rights::Seen,
                            Rights::Write,
                            Rights::Insert,
                            Rights::CreateMailbox,
                            Rights::DeleteMessages,
                            Rights::Administer,
                        ]
                    ),
                    (
                        "David".into(),
                        vec![
                            Rights::CreateMailbox,
                            Rights::DeleteMessages,
                            Rights::Administer,
                        ]
                    )
                ]
            }
            .into_bytes(true)
        )
        .unwrap(),
        "* ACL \"INBOX\" \"Fred\" lrswikta \"David\" kta\r\n"
    );

    assert_eq!(
        String::from_utf8(
            ListRightsResponse {
                mailbox_name: "Deleted Items".into(),
                identifier: "Fred".into(),
                permissions: vec![
                    vec![Rights::Lookup, Rights::Read],
                    vec![Rights::Administer],
                    vec![Rights::DeleteMailbox]
                ]
            }
            .into_bytes(true)
        )
        .unwrap(),
        "* LISTRIGHTS \"Deleted Items\" \"Fred\" lr a x\r\n"
    );

    assert_eq!(
        String::from_utf8(
            MyRightsResponse {
                mailbox_name: "Important".into(),
                rights: vec![Rights::Lookup, Rights::Read, Rights::DeleteMailbox]
            }
            .into_bytes(true)
        )
        .unwrap(),
        "* MYRIGHTS \"Important\" lrx\r\n"
    );
}
