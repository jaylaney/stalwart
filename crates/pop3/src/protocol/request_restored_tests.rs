/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::protocol::{Command, Mechanism, request::Error};

use super::Parser;

#[test]
fn parse_command() {
    let mut parser = Parser::default();
    let mut chunked = String::new();
    let mut chunked_expected = Vec::new();

    for (cmd, request) in [
        ("QuiT", Command::Quit),
        (" \r\n NOOP ", Command::Noop),
        ("STAT ", Command::Stat),
        ("LIST ", Command::List { msg: None }),
        (" list  100  ", Command::List { msg: 100.into() }),
        ("retr 55", Command::Retr { msg: 55 }),
        ("DELE 99", Command::Dele { msg: 99 }),
        (" rset ", Command::Rset),
        ("top 8000 1234", Command::Top { msg: 8000, n: 1234 }),
        ("uidl", Command::Uidl { msg: None }),
        ("uidl 000099999", Command::Uidl { msg: 99999.into() }),
        (
            "USER test",
            Command::User {
                name: "test".to_string(),
            },
        ),
        (
            "PASS secret",
            Command::Pass {
                string: "secret".to_string(),
            },
        ),
        (
            "APOP mrose c4c9334bac560ecc979e58001b3e22fb",
            Command::Apop {
                name: "mrose".to_string(),
                digest: "c4c9334bac560ecc979e58001b3e22fb".to_string(),
            },
        ),
        ("utf8", Command::Utf8),
        ("capa", Command::Capa),
        (
            "AUTH GSSAPI",
            Command::Auth {
                mechanism: Mechanism::Gssapi,
                params: vec![],
            },
        ),
        (
            "AUTH PLAIN dGVzdAB0ZXN0AHRlc3Q=",
            Command::Auth {
                mechanism: Mechanism::Plain,
                params: vec!["dGVzdAB0ZXN0AHRlc3Q=".to_string()],
            },
        ),
    ] {
        assert_eq!(
            parser.parse(&mut cmd.as_bytes().iter()),
            Err(Error::NeedsMoreData)
        );
        assert_eq!(
            parser.parse(&mut b"\r\n".iter()),
            Ok(request.clone()),
            "{:?}",
            cmd
        );
        chunked.push_str(cmd);
        chunked.push_str("\r\n");
        chunked_expected.push(request);
    }

    for chunk_size in [1, 2, 4, 8, 16, 32, 64, 128, 256, 512] {
        let mut parser = Parser::default();
        let mut requests = Vec::new();

        for chunk in chunked.as_bytes().chunks(chunk_size) {
            let mut chunk = chunk.iter();
            loop {
                match parser.parse(&mut chunk) {
                    Ok(request) => {
                        requests.push(request);
                    }
                    Err(Error::NeedsMoreData) => break,
                    Err(err) => {
                        panic!("Unexpected error on chunk size {chunk_size}: {err:?}");
                    }
                }
            }
        }

        assert_eq!(requests, chunked_expected, "Chunk size: {}", chunk_size);
    }

    for cmd in [
        "user",
        "pass",
        "user a b",
        "pass c d",
        "apop",
        "apop a",
        "apop a b c",
        "quit 1",
        "stat 1",
        "list 1 2",
        "retr",
        "retr 1 2",
        "dele",
        "dele 1 2",
        "noop 1",
        "rset 1",
        "top",
        "top 1 2 3",
        "uidl 1 2 3",
        "capa 1",
        "stls 1",
        "utf8 1",
        "auth",
        "auth unknown",
    ] {
        assert_eq!(
            parser.parse(&mut cmd.as_bytes().iter()),
            Err(Error::NeedsMoreData)
        );
        let result = parser.parse(&mut b"\r\n".iter());
        assert!(result.is_err(), "{:?}", result);
    }
}
