/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::list::{self, SelectionOption},
    receiver::Receiver,
};

#[test]
fn parse_lsub() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A002 LSUB \"#news.\" \"comp.mail.*\"\r\n",
            list::Arguments::Extended {
                tag: "A002".into(),
                reference_name: "#news.".into(),
                mailbox_name: vec!["comp.mail.*".into()],
                selection_options: vec![SelectionOption::Subscribed],
                return_options: vec![],
            },
        ),
        (
            "A002 LSUB \"#news.\" \"comp.%\"\r\n",
            list::Arguments::Extended {
                tag: "A002".into(),
                reference_name: "#news.".into(),
                mailbox_name: vec!["comp.%".into()],
                selection_options: vec![SelectionOption::Subscribed],
                return_options: vec![],
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .unwrap()
                .parse_lsub(false)
                .unwrap(),
            arguments
        );
    }
}
