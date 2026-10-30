/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::Response;
use crate::protocol::Mechanism;
use utils::chained_bytes::SliceRange;

#[test]
fn serialize_response() {
    for (cmd, expected) in [
        (
            Response::Ok("message 1 deleted".into()),
            "+OK message 1 deleted\r\n",
        ),
        (
            Response::Err("permission denied".into()),
            "-ERR permission denied\r\n",
        ),
        (
            Response::List(vec![100, 200, 300]),
            "+OK 3 messages\r\n1 100\r\n2 200\r\n3 300\r\n.\r\n",
        ),
        (
            Response::Capability {
                mechanisms: vec![Mechanism::Plain, Mechanism::CramMd5],
                stls: true,
            },
            concat!(
                "+OK Capability list follows\r\n",
                "USER\r\n",
                "SASL PLAIN CRAM-MD5\r\n",
                "STLS\r\n",
                "TOP\r\n",
                "RESP-CODES\r\n",
                "PIPELINING\r\n",
                "EXPIRE NEVER\r\n",
                "UIDL\r\n",
                "UTF8\r\n",
                "IMPLEMENTATION Stalwart Server\r\n.\r\n"
            ),
        ),
        (
            Response::Message {
                bytes: SliceRange::Split(b"Subject: test\r\n\r\n.\r\n", b"test.\r\n.test\r\na"),
                lines: None,
            },
            "+OK 37 octets\r\nSubject: test\r\n\r\n..\r\ntest.\r\n..test\r\na\r\n.\r\n",
        ),
        (
            Response::Message {
                bytes: SliceRange::Split(b"Subject: test\r\n\r\n.\r\n", b"test.\r\n.test\r\na"),
                lines: Some(0),
            },
            "+OK 17 octets\r\nSubject: test\r\n\r\n.\r\n",
        ),
        (
            Response::Message {
                bytes: SliceRange::Split(b"Subject: test\r\n\r\n.\r\n", b"test.\r\n.test\r\na"),
                lines: Some(2),
            },
            "+OK 27 octets\r\nSubject: test\r\n\r\n..\r\ntest.\r\n.\r\n",
        ),
        (
            Response::Message {
                bytes: SliceRange::Split(b"Subject: test\r\n\r\n.\r\n", b"test.\r\n.test\r\na"),
                lines: Some(100),
            },
            "+OK 37 octets\r\nSubject: test\r\n\r\n..\r\ntest.\r\n..test\r\na\r\n.\r\n",
        ),
        (
            Response::Message {
                bytes: SliceRange::Single(b"Subject: test\n\nbody\n"),
                lines: None,
            },
            "+OK 23 octets\r\nSubject: test\r\n\r\nbody\r\n.\r\n",
        ),
        (
            Response::Message {
                bytes: SliceRange::Single(b"Subject: test\n\n.leading dot\n"),
                lines: Some(1),
            },
            "+OK 31 octets\r\nSubject: test\r\n\r\n..leading dot\r\n.\r\n",
        ),
        (
            Response::Message {
                bytes: SliceRange::Single(b".dot\r\nSubject: test\r\n"),
                lines: Some(3),
            },
            "+OK 23 octets\r\n..dot\r\nSubject: test\r\n\r\n.\r\n",
        ),
    ] {
        assert_eq!(expected, String::from_utf8(cmd.serialize()).unwrap());
    }
}
