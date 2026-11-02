/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{
        list::{self, ReturnOption, SelectionOption},
        status::Status,
    },
    receiver::Receiver,
};

#[test]
fn parse_list() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A682 LIST \"\" *\r\n",
            list::Arguments::Basic {
                tag: "A682".into(),
                reference_name: "".into(),
                mailbox_name: "*".into(),
            },
        ),
        (
            "A02 LIST (SUBSCRIBED) \"\" \"*\"\r\n",
            list::Arguments::Extended {
                tag: "A02".into(),
                reference_name: "".into(),
                mailbox_name: vec!["*".into()],
                selection_options: vec![SelectionOption::Subscribed],
                return_options: vec![],
            },
        ),
        (
            "A03 LIST () \"\" \"%\" RETURN (CHILDREN)\r\n",
            list::Arguments::Extended {
                tag: "A03".into(),
                reference_name: "".into(),
                mailbox_name: vec!["%".into()],
                selection_options: vec![],
                return_options: vec![ReturnOption::Children],
            },
        ),
        (
            "A04 LIST (REMOTE) \"\" \"%\" RETURN (CHILDREN)\r\n",
            list::Arguments::Extended {
                tag: "A04".into(),
                reference_name: "".into(),
                mailbox_name: vec!["%".into()],
                selection_options: vec![SelectionOption::Remote],
                return_options: vec![ReturnOption::Children],
            },
        ),
        (
            "A05 LIST (REMOTE SUBSCRIBED) \"\" \"*\"\r\n",
            list::Arguments::Extended {
                tag: "A05".into(),
                reference_name: "".into(),
                mailbox_name: vec!["*".into()],
                selection_options: vec![SelectionOption::Remote, SelectionOption::Subscribed],
                return_options: vec![],
            },
        ),
        (
            "A06 LIST (REMOTE) \"\" \"*\" RETURN (SUBSCRIBED)\r\n",
            list::Arguments::Extended {
                tag: "A06".into(),
                reference_name: "".into(),
                mailbox_name: vec!["*".into()],
                selection_options: vec![SelectionOption::Remote],
                return_options: vec![ReturnOption::Subscribed],
            },
        ),
        (
            "C04 LIST (SUBSCRIBED RECURSIVEMATCH) \"\" \"%\"\r\n",
            list::Arguments::Extended {
                tag: "C04".into(),
                reference_name: "".into(),
                mailbox_name: vec!["%".into()],
                selection_options: vec![
                    SelectionOption::Subscribed,
                    SelectionOption::RecursiveMatch,
                ],
                return_options: vec![],
            },
        ),
        (
            "C04 LIST (SUBSCRIBED RECURSIVEMATCH) \"\" \"%\" RETURN (CHILDREN)\r\n",
            list::Arguments::Extended {
                tag: "C04".into(),
                reference_name: "".into(),
                mailbox_name: vec!["%".into()],
                selection_options: vec![
                    SelectionOption::Subscribed,
                    SelectionOption::RecursiveMatch,
                ],
                return_options: vec![ReturnOption::Children],
            },
        ),
        (
            "a1 LIST \"\" (\"foo\")\r\n",
            list::Arguments::Extended {
                tag: "a1".into(),
                reference_name: "".into(),
                mailbox_name: vec!["foo".into()],
                selection_options: vec![],
                return_options: vec![],
            },
        ),
        (
            "a3.1 LIST \"\" (% music/rock)\r\n",
            list::Arguments::Extended {
                tag: "a3.1".into(),
                reference_name: "".into(),
                mailbox_name: vec!["%".into(), "music/rock".into()],
                selection_options: vec![],
                return_options: vec![],
            },
        ),
        (
            "BBB LIST \"\" (\"INBOX\" \"Drafts\" \"Sent/%\")\r\n",
            list::Arguments::Extended {
                tag: "BBB".into(),
                reference_name: "".into(),
                mailbox_name: vec!["INBOX".into(), "Drafts".into(), "Sent/%".into()],
                selection_options: vec![],
                return_options: vec![],
            },
        ),
        (
            "A01 LIST \"\" % RETURN (STATUS (MESSAGES UNSEEN))\r\n",
            list::Arguments::Extended {
                tag: "A01".into(),
                reference_name: "".into(),
                mailbox_name: vec!["%".into()],
                selection_options: vec![],
                return_options: vec![ReturnOption::Status(vec![Status::Messages, Status::Unseen])],
            },
        ),
        (
            concat!(
                "A02 LIST (SUBSCRIBED RECURSIVEMATCH) \"\" ",
                "% RETURN (CHILDREN STATUS (MESSAGES))\r\n"
            ),
            list::Arguments::Extended {
                tag: "A02".into(),
                reference_name: "".into(),
                mailbox_name: vec!["%".into()],
                selection_options: vec![
                    SelectionOption::Subscribed,
                    SelectionOption::RecursiveMatch,
                ],
                return_options: vec![
                    ReturnOption::Children,
                    ReturnOption::Status(vec![Status::Messages]),
                ],
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_list(true)
                .unwrap(),
            arguments
        );
    }
}
