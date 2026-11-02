/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    protocol::{
        Flag,
        append::{self, Message},
    },
    receiver::{Error, Receiver},
};

#[test]
fn parse_append() {
    let mut receiver = Receiver::new();

    for (command, arguments) in [
        (
            "A003 APPEND saved-messages (\\Seen) {1+}\r\na\r\n",
            append::Arguments {
                tag: "A003".into(),
                mailbox_name: "saved-messages".into(),
                messages: vec![Message {
                    message: vec![b'a'],
                    flags: vec![Flag::Seen],
                    received_at: None,
                }],
            },
        ),
        (
            "A003 APPEND \"hello world\" (\\Seen \\Draft $MDNSent) {1+}\r\na\r\n",
            append::Arguments {
                tag: "A003".into(),
                mailbox_name: "hello world".into(),
                messages: vec![Message {
                    message: vec![b'a'],
                    flags: vec![Flag::Seen, Flag::Draft, Flag::MDNSent],
                    received_at: None,
                }],
            },
        ),
        (
            "A003 APPEND \"hi\" ($Junk) \"7-Feb-1994 22:43:04 -0800\" {1+}\r\na\r\n",
            append::Arguments {
                tag: "A003".into(),
                mailbox_name: "hi".into(),
                messages: vec![Message {
                    message: vec![b'a'],
                    flags: vec![Flag::Junk],
                    received_at: Some(760689784),
                }],
            },
        ),
        (
            "A003 APPEND \"hi\" \"20-Nov-2022 23:59:59 +0300\" {1+}\r\na\r\n",
            append::Arguments {
                tag: "A003".into(),
                mailbox_name: "hi".into(),
                messages: vec![Message {
                    message: vec![b'a'],
                    flags: vec![],
                    received_at: Some(1668977999),
                }],
            },
        ),
        (
            "A003 APPEND \"hi\" \"20-Nov-2022 23:59:59 +0300\" ~{1+}\r\na\r\n",
            append::Arguments {
                tag: "A003".into(),
                mailbox_name: "hi".into(),
                messages: vec![Message {
                    message: vec![b'a'],
                    flags: vec![],
                    received_at: Some(1668977999),
                }],
            },
        ),
        (
            "42 APPEND \"Drafts\" (\\Draft) UTF8 (~{5+}\r\nhello)\r\n",
            append::Arguments {
                tag: "42".into(),
                mailbox_name: "Drafts".into(),
                messages: vec![Message {
                    message: vec![b'h', b'e', b'l', b'l', b'o'],
                    flags: vec![Flag::Draft],
                    received_at: None,
                }],
            },
        ),
        (
            "42 APPEND \"Drafts\" (\\Draft) \"20-Nov-2022 23:59:59 +0300\" UTF8 (~{5+}\r\nhello)\r\n",
            append::Arguments {
                tag: "42".into(),
                mailbox_name: "Drafts".into(),
                messages: vec![Message {
                    message: vec![b'h', b'e', b'l', b'l', b'o'],
                    flags: vec![Flag::Draft],
                    received_at: Some(1668977999),
                }],
            },
        ),
        (
            "A003 APPEND \"&A8g- \\\"&A9QD1APUA9gD3APcA-+\\\"\" (\\Seen) \"7-Feb-1994 22:43:04 -0800\" {1+}\r\na\r\n",
            append::Arguments {
                tag: "A003".into(),
                mailbox_name: "ψ \"ϔϔϔϘϜϜ+\"".into(),
                messages: vec![Message {
                    message: vec![b'a'],
                    flags: vec![Flag::Seen],
                    received_at: Some(760689784),
                }],
            },
        ),
    ] {
        assert_eq!(
            receiver
                .parse(&mut command.as_bytes().iter())
                .expect(command)
                .parse_append(false)
                .expect(command),
            arguments,
            "{:?}",
            command
        );
    }

    // Multiappend
    for line in [
        "A003 APPEND saved-messages (\\Seen) UTF8 ({329}\r\n",
        "Date: Mon, 7 Feb 1994 21:52:25 -0800 (PST)\r\n",
        "From: Fred Foobar <foobar@Blurdybloop.example.COM>\r\n",
        "Subject: afternoon meeting\r\n",
        "To: mooch@owatagu.example.net\r\n",
        "Message-Id: <B27397-0100000@Blurdybloop.example.COM>\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII\r\n",
        "\r\n",
        "Hello Joe, do you think we can meet at 3:30 tomorrow?\r\n)",
        " (\\Seen) \"7-Feb-1994 22:43:04 -0800\" {295}\r\n",
        "Date: Mon, 7 Feb 1994 22:43:04 -0800 (PST)\r\n",
        "From: Joe Mooch <mooch@OWaTaGu.example.net>\r\n",
        "Subject: Re: afternoon meeting\r\n",
        "To: foobar@blurdybloop.example.com\r\n",
        "Message-Id: <a0434793874930@OWaTaGu.example.net>\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII\r\n\r\n",
        "3:30 is fine with me.\r\n\r\n",
    ] {
        match receiver.parse(&mut line.as_bytes().iter()) {
            Ok(request) => {
                assert_eq!(
                    request.parse_append(false).unwrap(),
                    append::Arguments {
                        tag: "A003".into(),
                        mailbox_name: "saved-messages".into(),
                        messages: vec![
                            Message {
                                message: concat!(
                                    "Date: Mon, 7 Feb 1994 21:52:25 -0800 (PST)\r\n",
                                    "From: Fred Foobar <foobar@Blurdybloop.example.COM>\r\n",
                                    "Subject: afternoon meeting\r\n",
                                    "To: mooch@owatagu.example.net\r\n",
                                    "Message-Id: <B27397-0100000@Blurdybloop.example.COM>\r\n",
                                    "MIME-Version: 1.0\r\n",
                                    "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII\r\n",
                                    "\r\n",
                                    "Hello Joe, do you think we can meet at 3:30 tomorrow?\r\n",
                                )
                                .as_bytes()
                                .to_vec(),
                                flags: vec![Flag::Seen],
                                received_at: None,
                            },
                            Message {
                                message: concat!(
                                    "Date: Mon, 7 Feb 1994 22:43:04 -0800 (PST)\r\n",
                                    "From: Joe Mooch <mooch@OWaTaGu.example.net>\r\n",
                                    "Subject: Re: afternoon meeting\r\n",
                                    "To: foobar@blurdybloop.example.com\r\n",
                                    "Message-Id: <a0434793874930@OWaTaGu.example.net>\r\n",
                                    "MIME-Version: 1.0\r\n",
                                    "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII\r\n\r\n",
                                    "3:30 is fine with me.\r\n",
                                )
                                .as_bytes()
                                .to_vec(),
                                flags: vec![Flag::Seen],
                                received_at: Some(760689784),
                            }
                        ],
                    },
                );
            }
            Err(err) => match err {
                Error::NeedsMoreData | Error::NeedsLiteral { .. } => (),
                Error::Error { response } => panic!("{:?}", response),
            },
        }
    }
}
