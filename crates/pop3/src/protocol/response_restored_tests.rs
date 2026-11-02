/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::Response;
use crate::protocol::Mechanism;
use utils::chained_bytes::ChainedBytes;

/// 1.0's `Response::message` takes the header section's length (the
/// metadata's `headers_len`) instead of finding the blank line itself.
const HEADERS_LEN: usize = b"Subject: test\r\n\r\n".len();

/// The header section's length of a test message: up to and including its
/// first blank line, or the whole message when it has none.
fn headers_len(bytes: &[u8]) -> usize {
    [&b"\r\n\r\n"[..], &b"\n\n"[..]]
        .iter()
        .filter_map(|blank| {
            bytes
                .windows(blank.len())
                .position(|window| window == *blank)
                .map(|pos| pos + blank.len())
        })
        .min()
        .unwrap_or(bytes.len())
}

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
            Response::message(
                ChainedBytes::chain(b"Subject: test\r\n\r\n.\r\n", b"test.\r\n.test\r\na"),
                HEADERS_LEN,
                None,
            ),
            "+OK 37 octets\r\nSubject: test\r\n\r\n..\r\ntest.\r\n..test\r\na\r\n.\r\n",
        ),
        (
            Response::message(
                ChainedBytes::chain(b"Subject: test\r\n\r\n.\r\n", b"test.\r\n.test\r\na"),
                HEADERS_LEN,
                Some(0),
            ),
            "+OK 17 octets\r\nSubject: test\r\n\r\n.\r\n",
        ),
        (
            Response::message(
                ChainedBytes::chain(b"Subject: test\r\n\r\n.\r\n", b"test.\r\n.test\r\na"),
                HEADERS_LEN,
                Some(2),
            ),
            "+OK 27 octets\r\nSubject: test\r\n\r\n..\r\ntest.\r\n.\r\n",
        ),
        (
            Response::message(
                ChainedBytes::chain(b"Subject: test\r\n\r\n.\r\n", b"test.\r\n.test\r\na"),
                HEADERS_LEN,
                Some(100),
            ),
            "+OK 37 octets\r\nSubject: test\r\n\r\n..\r\ntest.\r\n..test\r\na\r\n.\r\n",
        ),
        (
            Response::message(
                ChainedBytes::new(b"Subject: test\n\nbody\n"),
                headers_len(b"Subject: test\n\nbody\n"),
                None,
            ),
            "+OK 23 octets\r\nSubject: test\r\n\r\nbody\r\n.\r\n",
        ),
        (
            Response::message(
                ChainedBytes::new(b"Subject: test\n\n.leading dot\n"),
                headers_len(b"Subject: test\n\n.leading dot\n"),
                Some(1),
            ),
            "+OK 31 octets\r\nSubject: test\r\n\r\n..leading dot\r\n.\r\n",
        ),
        (
            Response::message(
                ChainedBytes::new(b".dot\r\nSubject: test\r\n"),
                headers_len(b".dot\r\nSubject: test\r\n"),
                Some(3),
            ),
            "+OK 23 octets\r\n..dot\r\nSubject: test\r\n\r\n.\r\n",
        ),
    ] {
        assert_eq!(expected, String::from_utf8(cmd.serialize()).unwrap());
    }
}
