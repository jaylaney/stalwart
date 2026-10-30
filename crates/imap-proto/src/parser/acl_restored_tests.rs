/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::acl::{self, ModRights, ModRightsOp, Rights},
    receiver::Receiver,
};

#[test]
fn parse_acl() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A003 Setacl INBOX/Drafts Byron lrswikda\r\n",
            acl::Arguments {
                tag: "A003".into(),
                mailbox_name: "INBOX/Drafts".into(),
                identifier: Some("Byron".into()),
                mod_rights: ModRights {
                    op: ModRightsOp::Replace,
                    rights: vec![
                        Rights::Lookup,
                        Rights::Read,
                        Rights::Seen,
                        Rights::Write,
                        Rights::Insert,
                        Rights::CreateMailbox,
                        Rights::DeleteMessages,
                        Rights::Administer,
                    ],
                }
                .into(),
            },
        ),
        (
            "A002 SETACL INBOX/Drafts Chris +cda\r\n",
            acl::Arguments {
                tag: "A002".into(),
                mailbox_name: "INBOX/Drafts".into(),
                identifier: Some("Chris".into()),
                mod_rights: ModRights {
                    op: ModRightsOp::Add,
                    rights: vec![
                        Rights::CreateMailbox,
                        Rights::DeleteMessages,
                        Rights::Administer,
                    ],
                }
                .into(),
            },
        ),
        (
            "A036 SETACL INBOX/Drafts John -lrswicda\r\n",
            acl::Arguments {
                tag: "A036".into(),
                mailbox_name: "INBOX/Drafts".into(),
                identifier: Some("John".into()),
                mod_rights: ModRights {
                    op: ModRightsOp::Remove,
                    rights: vec![
                        Rights::Lookup,
                        Rights::Read,
                        Rights::Seen,
                        Rights::Write,
                        Rights::Insert,
                        Rights::CreateMailbox,
                        Rights::DeleteMessages,
                        Rights::Administer,
                    ],
                }
                .into(),
            },
        ),
        (
            "A001 GETACL INBOX/Drafts\r\n",
            acl::Arguments {
                tag: "A001".into(),
                mailbox_name: "INBOX/Drafts".into(),
                identifier: None,
                mod_rights: None,
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_acl(false)
                .unwrap(),
            arguments,
            "{:?}",
            command
        );
    }
}
