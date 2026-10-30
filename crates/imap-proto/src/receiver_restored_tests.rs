/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::Command;

use super::{Error, Receiver, Request, Token};

#[test]
fn receiver_parse_ok() {
    let mut receiver = Receiver::new();

    for (frames, expected_requests) in [
        (
            vec!["abcd CAPABILITY\r\n"],
            vec![Request {
                tag: "abcd".into(),
                command: Command::Capability,
                tokens: vec![],
            }],
        ),
        (
            vec!["A023 LO", "GOUT\r\n"],
            vec![Request {
                tag: "A023".into(),
                command: Command::Logout,
                tokens: vec![],
            }],
        ),
        (
            vec!["  A001 AUTHENTICATE GSSAPI  \r\n"],
            vec![Request {
                tag: "A001".into(),
                command: Command::Authenticate,
                tokens: vec![Token::Argument(b"GSSAPI".to_vec().into())],
            }],
        ),
        (
            vec!["A03   AUTHENTICATE ", "PLAIN dGVzdAB0ZXN", "0AHRlc3Q=\r\n"],
            vec![Request {
                tag: "A03".into(),
                command: Command::Authenticate,
                tokens: vec![
                    Token::Argument(b"PLAIN".to_vec().into()),
                    Token::Argument(b"dGVzdAB0ZXN0AHRlc3Q=".to_vec().into()),
                ],
            }],
        ),
        (
            vec!["A003 CREATE owatagusiam/\r\n"],
            vec![Request {
                tag: "A003".into(),
                command: Command::Create,
                tokens: vec![Token::Argument(b"owatagusiam/".to_vec().into())],
            }],
        ),
        (
            vec!["A682 LIST \"\" *\r\n"],
            vec![Request {
                tag: "A682".into(),
                command: Command::List,
                tokens: vec![Token::Nil, Token::Argument(b"*".to_vec().into())],
            }],
        ),
        (
            vec!["A03 LIST () \"\" \"%\" RETURN (CHILDREN)\r\n"],
            vec![Request {
                tag: "A03".into(),
                command: Command::List,
                tokens: vec![
                    Token::ParenthesisOpen,
                    Token::ParenthesisClose,
                    Token::Nil,
                    Token::Argument(b"%".to_vec().into()),
                    Token::Argument(b"RETURN".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"CHILDREN".to_vec().into()),
                    Token::ParenthesisClose,
                ],
            }],
        ),
        (
            vec!["A05 LIST (REMOTE SUBSCRIBED) \"\" \"*\"\r\n"],
            vec![Request {
                tag: "A05".into(),
                command: Command::List,
                tokens: vec![
                    Token::ParenthesisOpen,
                    Token::Argument(b"REMOTE".to_vec().into()),
                    Token::Argument(b"SUBSCRIBED".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::Nil,
                    Token::Argument(b"*".to_vec().into()),
                ],
            }],
        ),
        (
            vec!["a1 list \"\" (\"foo\")\r\n"],
            vec![Request {
                tag: "a1".into(),
                command: Command::List,
                tokens: vec![
                    Token::Nil,
                    Token::ParenthesisOpen,
                    Token::Argument(b"foo".to_vec().into()),
                    Token::ParenthesisClose,
                ],
            }],
        ),
        (
            vec!["a3.1 LIST \"\" (% music/rock)\r\n"],
            vec![Request {
                tag: "a3.1".into(),
                command: Command::List,
                tokens: vec![
                    Token::Nil,
                    Token::ParenthesisOpen,
                    Token::Argument(b"%".to_vec().into()),
                    Token::Argument(b"music/rock".to_vec().into()),
                    Token::ParenthesisClose,
                ],
            }],
        ),
        (
            vec!["A01 LIST \"\" % RETURN (STATUS (MESSAGES UNSEEN))\r\n"],
            vec![Request {
                tag: "A01".into(),
                command: Command::List,
                tokens: vec![
                    Token::Nil,
                    Token::Argument(b"%".to_vec().into()),
                    Token::Argument(b"RETURN".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"STATUS".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"MESSAGES".to_vec().into()),
                    Token::Argument(b"UNSEEN".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::ParenthesisClose,
                ],
            }],
        ),
        (
            vec![" A01 LiSt \"\"  % RETURN ( STATUS ( MESSAGES UNSEEN ) ) \r\n"],
            vec![Request {
                tag: "A01".into(),
                command: Command::List,
                tokens: vec![
                    Token::Nil,
                    Token::Argument(b"%".to_vec().into()),
                    Token::Argument(b"RETURN".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"STATUS".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"MESSAGES".to_vec().into()),
                    Token::Argument(b"UNSEEN".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::ParenthesisClose,
                ],
            }],
        ),
        (
            vec!["A02 LIST (SUBSCRIBED RECURSIVEMATCH) \"\" % RETURN (STATUS (MESSAGES))\r\n"],
            vec![Request {
                tag: "A02".into(),
                command: Command::List,
                tokens: vec![
                    Token::ParenthesisOpen,
                    Token::Argument(b"SUBSCRIBED".to_vec().into()),
                    Token::Argument(b"RECURSIVEMATCH".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::Nil,
                    Token::Argument(b"%".to_vec().into()),
                    Token::Argument(b"RETURN".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"STATUS".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"MESSAGES".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::ParenthesisClose,
                ],
            }],
        ),
        (
            vec!["A002 CREATE \"INBOX.Sent Mail\"\r\n"],
            vec![Request {
                tag: "A002".into(),
                command: Command::Create,
                tokens: vec![Token::Argument(b"INBOX.Sent Mail".to_vec().into())],
            }],
        ),
        (
            vec!["A002 CREATE \"Maibox \\\"quo\\\\ted\\\" \"\r\n"],
            vec![Request {
                tag: "A002".into(),
                command: Command::Create,
                tokens: vec![Token::Argument(b"Maibox \"quo\\ted\" ".to_vec().into())],
            }],
        ),
        (
            vec!["A004 COPY 2:4 meeting\r\n"],
            vec![Request {
                tag: "A004".into(),
                command: Command::Copy(false),
                tokens: vec![
                    Token::Argument(b"2:4".to_vec().into()),
                    Token::Argument(b"meeting".to_vec().into()),
                ],
            }],
        ),
        (
            vec![
                "A282 SEARCH RETURN (MIN COU",
                "NT) FLAGGED SINCE 1-Feb-1994 ",
                "NOT FROM \"Smith\"\r\n",
            ],
            vec![Request {
                tag: "A282".into(),
                command: Command::Search(false),
                tokens: vec![
                    Token::Argument(b"RETURN".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"MIN".to_vec().into()),
                    Token::Argument(b"COUNT".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::Argument(b"FLAGGED".to_vec().into()),
                    Token::Argument(b"SINCE".to_vec().into()),
                    Token::Argument(b"1-Feb-1994".to_vec().into()),
                    Token::Argument(b"NOT".to_vec().into()),
                    Token::Argument(b"FROM".to_vec().into()),
                    Token::Argument(b"Smith".to_vec().into()),
                ],
            }],
        ),
        (
            vec!["F284 UID STORE $ +FLAGS.Silent (\\Deleted)\r\n"],
            vec![Request {
                tag: "F284".into(),
                command: Command::Store(true),
                tokens: vec![
                    Token::Argument(b"$".to_vec().into()),
                    Token::Argument(b"+FLAGS.Silent".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"\\Deleted".to_vec().into()),
                    Token::ParenthesisClose,
                ],
            }],
        ),
        (
            vec!["A654 FETCH 2:4 (FLAGS BODY[HEADER.FIELDS (DATE FROM)])\r\n"],
            vec![Request {
                tag: "A654".into(),
                command: Command::Fetch(false),
                tokens: vec![
                    Token::Argument(b"2:4".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"FLAGS".to_vec().into()),
                    Token::Argument(b"BODY".to_vec().into()),
                    Token::BracketOpen,
                    Token::Argument(b"HEADER".to_vec().into()),
                    Token::Dot,
                    Token::Argument(b"FIELDS".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"DATE".to_vec().into()),
                    Token::Argument(b"FROM".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::BracketClose,
                    Token::ParenthesisClose,
                ],
            }],
        ),
        (
            vec![
                "B283 UID SEARCH RETURN (SAVE) CHARSET ",
                "KOI8-R (OR $ 1,3000:3021) TEXT \"hello world\"\r\n",
            ],
            vec![Request {
                tag: "B283".into(),
                command: Command::Search(true),
                tokens: vec![
                    Token::Argument(b"RETURN".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"SAVE".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::Argument(b"CHARSET".to_vec().into()),
                    Token::Argument(b"KOI8-R".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"OR".to_vec().into()),
                    Token::Argument(b"$".to_vec().into()),
                    Token::Argument(b"1,3000:3021".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::Argument(b"TEXT".to_vec().into()),
                    Token::Argument(b"hello world".to_vec().into()),
                ],
            }],
        ),
        (
            vec![
                "P283 SEARCH CHARSET UTF-8 (OR $ 1,3000:3021) ",
                "TEXT {8+}\r\nмать\r\n",
            ],
            vec![Request {
                tag: "P283".into(),
                command: Command::Search(false),
                tokens: vec![
                    Token::Argument(b"CHARSET".to_vec().into()),
                    Token::Argument(b"UTF-8".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"OR".to_vec().into()),
                    Token::Argument(b"$".to_vec().into()),
                    Token::Argument(b"1,3000:3021".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::Argument(b"TEXT".to_vec().into()),
                    Token::Argument("мать".to_string().into_bytes().into()),
                ],
            }],
        ),
        (
            vec!["A001 LOGIN {11}\r\n", "FRED FOOBAR {7}\r\n", "fat man\r\n"],
            vec![Request {
                tag: "A001".into(),
                command: Command::Login,
                tokens: vec![
                    Token::Argument(b"FRED FOOBAR".to_vec().into()),
                    Token::Argument(b"fat man".to_vec().into()),
                ],
            }],
        ),
        (
            vec!["TAG3 CREATE \"Test-ąęć-Test\"\r\n"],
            vec![Request {
                tag: "TAG3".into(),
                command: Command::Create,
                tokens: vec![Token::Argument("Test-ąęć-Test".as_bytes().to_vec().into())],
            }],
        ),
        (
            vec!["abc LOGIN {0}\r\n", "\r\n"],
            vec![Request {
                tag: "abc".into(),
                command: Command::Login,
                tokens: vec![Token::Nil],
            }],
        ),
        (
            vec!["abc LOGIN {0+}\r\n\r\n"],
            vec![Request {
                tag: "abc".into(),
                command: Command::Login,
                tokens: vec![Token::Nil],
            }],
        ),
        (
            vec![
                "A003 APPEND saved-messages (\\Seen) {297+}\r\n",
                "Date: Mon, 7 Feb 1994 21:52:25 -0800 (PST)\r\n",
                "From: Fred Foobar <foobar@example.com>\r\n",
                "Subject: afternoon meeting\r\n",
                "To: mooch@example.com\r\n",
                "Message-Id: <B27397-0100000@example.com>\r\n",
                "MIME-Version: 1.0\r\n",
                "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII\r\n",
                "\r\n",
                "Hello Joe, do you think we can meet at 3:30 tomorrow?\r\n\r\n",
            ],
            vec![Request {
                tag: "A003".into(),
                command: Command::Append,
                tokens: vec![
                    Token::Argument(b"saved-messages".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"\\Seen".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::Argument(
                        concat!(
                            "Date: Mon, 7 Feb 1994 21:52:25 -0800 (PST)\r\n",
                            "From: Fred Foobar <foobar@example.com>\r\n",
                            "Subject: afternoon meeting\r\n",
                            "To: mooch@example.com\r\n",
                            "Message-Id: <B27397-0100000@example.com>\r\n",
                            "MIME-Version: 1.0\r\n",
                            "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII\r\n",
                            "\r\n",
                            "Hello Joe, do you think we can meet at 3:30 tomorrow?\r\n"
                        )
                        .as_bytes()
                        .to_vec()
                        .into(),
                    ),
                ],
            }],
        ),
        (
            vec![
                "A003 APPEND saved-messages (\\Seen) {326}\r\n",
                "Date: Mon, 7 Feb 1994 21:52:25 -0800 (PST)\r\n",
                "From: Fred Foobar <foobar@Blurdybloop.example>\r\n",
                "Subject: afternoon meeting\r\n",
                "To: mooch@owatagu.siam.edu.example\r\n",
                "Message-Id: <B27397-0100000@Blurdybloop.example>\r\n",
                "MIME-Version: 1.0\r\n",
                "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII\r\n",
                "\r\n",
                "Hello Joe, do you think we can meet at 3:30 tomorrow?\r\n\r\n",
            ],
            vec![Request {
                tag: "A003".into(),
                command: Command::Append,
                tokens: vec![
                    Token::Argument(b"saved-messages".to_vec().into()),
                    Token::ParenthesisOpen,
                    Token::Argument(b"\\Seen".to_vec().into()),
                    Token::ParenthesisClose,
                    Token::Argument(
                        concat!(
                            "Date: Mon, 7 Feb 1994 21:52:25 -0800 (PST)\r\n",
                            "From: Fred Foobar <foobar@Blurdybloop.example>\r\n",
                            "Subject: afternoon meeting\r\n",
                            "To: mooch@owatagu.siam.edu.example\r\n",
                            "Message-Id: <B27397-0100000@Blurdybloop.example>\r\n",
                            "MIME-Version: 1.0\r\n",
                            "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII\r\n",
                            "\r\n",
                            "Hello Joe, do you think we can meet at 3:30 tomorrow?\r\n",
                        )
                        .as_bytes()
                        .to_vec()
                        .into(),
                    ),
                ],
            }],
        ),
        (
            vec!["001 NOOP\r\n002 CAPABILITY\r\nabc LOGIN hello world\r\n"],
            vec![
                Request {
                    tag: "001".into(),
                    command: Command::Noop,
                    tokens: vec![],
                },
                Request {
                    tag: "002".into(),
                    command: Command::Capability,
                    tokens: vec![],
                },
                Request {
                    tag: "abc".into(),
                    command: Command::Login,
                    tokens: vec![
                        Token::Argument(b"hello".to_vec().into()),
                        Token::Argument(b"world".to_vec().into()),
                    ],
                },
            ],
        ),
    ] {
        let mut requests = Vec::new();
        for frame in &frames {
            let mut bytes = frame.as_bytes().iter();
            loop {
                match receiver.parse(&mut bytes) {
                    Ok(request) => requests.push(request),
                    Err(Error::NeedsMoreData | Error::NeedsLiteral { .. }) => break,
                    Err(err) => panic!("{:?} for frames {:#?}", err, frames),
                }
            }
        }
        assert_eq!(requests, expected_requests, "{:#?}", frames);
    }
}

#[test]
fn receiver_parse_invalid() {
    let mut receiver = Receiver::<Command>::new();
    for invalid in [
        //"\r\n",
        //"  \r \n",
        "a001\r\n",
        "a001 unknown\r\n",
        "a001 login {abc}\r\n",
        "a001 login {+30}\r\n",
        "a001 login {30} junk\r\n",
    ] {
        match receiver.parse(&mut invalid.as_bytes().iter()) {
            Err(Error::Error { .. }) => {}
            result => panic!("Expecter error, got: {:?}", result),
        }
    }
}

#[test]
fn receiver_discard_oversized_non_sync_literal() {
    for frames in [
        vec![
            "a1 APPEND inbox (\\Draft) {40+}\r\n",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\n",
            "b2 NOOP\r\n",
        ],
        vec![
            "a1 APPEND inbox (\\Draft) {40+}\r\nAAAAAAAAAAAAAAAAAAAA",
            "AAAAAAAAAAAAAAAAAAAA\r\nb2 NOOP\r\n",
        ],
        vec![
            "a1 APPEND inbox (\\Draft) {40+}\r\nAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\r\nb2 NOOP\r\n",
        ],
    ] {
        let mut receiver = Receiver::<Command>::with_max_request_size(30);
        let mut requests = Vec::new();
        let mut errors = 0;
        for frame in &frames {
            let mut bytes = frame.as_bytes().iter();
            loop {
                match receiver.parse(&mut bytes) {
                    Ok(request) => requests.push(request),
                    Err(Error::NeedsMoreData | Error::NeedsLiteral { .. }) => break,
                    Err(Error::Error { .. }) => errors += 1,
                }
            }
        }

        assert_eq!(errors, 1, "expected a single error for {:#?}", frames);
        assert_eq!(
            requests,
            vec![Request {
                tag: "b2".into(),
                command: Command::Noop,
                tokens: vec![],
            }],
            "connection did not resync for {:#?}",
            frames
        );
    }
}
