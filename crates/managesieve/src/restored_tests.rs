/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use imap_proto::receiver::{Error, Receiver, Request, State, Token};

use crate::core::Command;

#[test]
fn receiver_parse_managesieve() {
    let mut receiver = Receiver::new().with_start_state(State::Command { is_uid: false });

    for (frames, expected_requests) in [
        (
            vec!["Authenticate \"DIGEST-MD5\"\r\n"],
            vec![Request {
                tag: "".into(),
                command: Command::Authenticate,
                tokens: vec![Token::Argument(b"DIGEST-MD5".as_slice().into())],
            }],
        ),
        (
            vec![
                "  AUTHENTICATE  \"GSSAPI\"  {56+}\r\n",
                "cnNwYXV0aD1lYTQwZjYwMzM1YzQyN2I1NTI3Yjg0ZGJhYmNkZmZmZA==\r\n",
            ],
            vec![Request {
                tag: "".into(),
                command: Command::Authenticate,
                tokens: vec![
                    Token::Argument(b"GSSAPI".as_slice().into()),
                    Token::Argument(
                        b"cnNwYXV0aD1lYTQwZjYwMzM1YzQyN2I1NTI3Yjg0ZGJhYmNkZmZmZA=="
                            .as_slice()
                            .into(),
                    ),
                ],
            }],
        ),
        (
            vec!["Authenticate \"PLAIN\" \"QJIrweAPyo6Q1T9xu\"\r\n"],
            vec![Request {
                tag: "".into(),
                command: Command::Authenticate,
                tokens: vec![
                    Token::Argument(b"PLAIN".as_slice().into()),
                    Token::Argument(b"QJIrweAPyo6Q1T9xu".as_slice().into()),
                ],
            }],
        ),
        (
            vec!["StartTls\r\n"],
            vec![Request {
                tag: "".into(),
                command: Command::StartTls,
                tokens: vec![],
            }],
        ),
        (
            vec!["HAVESPACE \"myscript\" 999999\r\n"],
            vec![Request {
                tag: "".into(),
                command: Command::HaveSpace,
                tokens: vec![
                    Token::Argument(b"myscript".as_slice().into()),
                    Token::Argument(b"999999".as_slice().into()),
                ],
            }],
        ),
        (
            vec![
                "Putscript \"foo\" {31+}\r\n",
                "#comment\r\n",
                "InvalidSieveCommand\r\n\r\n",
            ],
            vec![Request {
                tag: "".into(),
                command: Command::PutScript,
                tokens: vec![
                    Token::Argument(b"foo".as_slice().into()),
                    Token::Argument(b"#comment\r\nInvalidSieveCommand\r\n".as_slice().into()),
                ],
            }],
        ),
        (
            vec!["Listscripts\r\n"],
            vec![Request {
                tag: "".into(),
                command: Command::ListScripts,
                tokens: vec![],
            }],
        ),
        (
            vec!["Setactive \"baz\"\r\n"],
            vec![Request {
                tag: "".into(),
                command: Command::SetActive,
                tokens: vec![Token::Argument(b"baz".as_slice().into())],
            }],
        ),
        (
            vec!["Renamescript \"foo\" \"bar\"\r\n"],
            vec![Request {
                tag: "".into(),
                command: Command::RenameScript,
                tokens: vec![
                    Token::Argument(b"foo".as_slice().into()),
                    Token::Argument(b"bar".as_slice().into()),
                ],
            }],
        ),
        (
            vec!["NOOP \"STARTTLS-SYNC-42\"\r\n"],
            vec![Request {
                tag: "".into(),
                command: Command::Noop,
                tokens: vec![Token::Argument(b"STARTTLS-SYNC-42".as_slice().into())],
            }],
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
