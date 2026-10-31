/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! The imap equivalent of 0.16's six ENVELOPE, BODY and BODYSTRUCTURE cases
//! in `imap-proto/src/protocol/fetch.rs` (Task 2's handoff). 0.16 serialized
//! hand-built `DataItem` values; 1.0 writes these items from the message
//! metadata (`write_envelope`, `write_structure`), so each case is a message
//! with the same fields, parsed and built into metadata as ingest does, and
//! its sizes and line counts are those of the message's own parts.
//!
//! Two differences from 0.16's strings are 1.0's (expectation ledger):
//! media types, subtypes and parameter names are written as the parser
//! normalizes them, in lower case, where 0.16's hand-built values were upper
//! case; and a multipart's extension data lists its `boundary` parameter,
//! which 0.16's hand-built multiparts did not have.

use super::ImapMetadata;
use email::message::metadata::{ExtraHeaders, MessageMetadata, MetadataRow};
use mail_parser::MessageParser;
use store::Deserialize;
use types::blob_hash::BlobHash;

/// `raw` parsed and built into its stored metadata row.
fn row(raw: &str) -> MetadataRow {
    let message = MessageParser::new().parse(raw.as_bytes()).unwrap();
    MetadataRow::deserialize(
        &MessageMetadata::build(
            &message,
            &ExtraHeaders::default(),
            BlobHash::generate(raw.as_bytes()),
        )
        .encode()
        .unwrap(),
    )
    .unwrap()
}

fn envelope(raw: &str) -> String {
    let row = row(raw);
    let mut buf = b"ENVELOPE ".to_vec();
    row.unarchive().unwrap().write_envelope(&mut buf, false);
    String::from_utf8(buf).unwrap()
}

fn structure(raw: &str, is_extended: bool) -> String {
    let row = row(raw);
    let mut buf = if is_extended {
        b"BODYSTRUCTURE ".to_vec()
    } else {
        b"BODY ".to_vec()
    };
    row.unarchive()
        .unwrap()
        .write_structure(&mut buf, is_extended, false);
    String::from_utf8(buf).unwrap()
}

/// An IMAP body's size and line count: its octets and its LF count.
fn stats(body: &str) -> (usize, usize) {
    (body.len(), body.matches('\n').count())
}

/// `lines` CRLF-terminated lines of `width` octets each (the CRLF
/// included), padded without whitespace, which quoted-printable decoding
/// would strip from a line's end.
fn text_body(lines: usize, width: usize) -> String {
    (0..lines)
        .map(|line| format!("{:x<1$}\r\n", format!("line{line}"), width - 2))
        .collect()
}

#[test]
fn envelope_single_addresses() {
    let raw = concat!(
        "Date: Wed, 17 Jul 1996 02:23:25 +0000\r\n",
        "From: Terry Gray <gray@cac.washington.edu>\r\n",
        "Subject: IMAP4rev2 WG mtg summary and minutes\r\n",
        "To: imap@cac.washington.edu\r\n",
        "Cc: minutes@CNRI.Reston.VA.US, John Klensin <KLENSIN@MIT.EDU>\r\n",
        "Message-Id: <B27397-0100000@cac.washington.ed>\r\n",
        "\r\n",
        "Body.\r\n"
    );
    // 0.16's case named the sender and the reply-to explicitly; 1.0 writes
    // the From addresses for both when the headers are absent, as 0.16's
    // serializer did for an empty list.
    assert_eq!(
        envelope(raw),
        concat!(
            "ENVELOPE (\"Wed, 17 Jul 1996 02:23:25 +0000\" ",
            "\"IMAP4rev2 WG mtg summary and minutes\" ",
            "((\"Terry Gray\" NIL \"gray\" \"cac.washington.edu\")) ",
            "((\"Terry Gray\" NIL \"gray\" \"cac.washington.edu\")) ",
            "((\"Terry Gray\" NIL \"gray\" \"cac.washington.edu\")) ",
            "((NIL NIL \"imap\" \"cac.washington.edu\")) ",
            "((NIL NIL \"minutes\" \"CNRI.Reston.VA.US\")",
            "(\"John Klensin\" NIL \"KLENSIN\" \"MIT.EDU\")) NIL NIL ",
            "\"<B27397-0100000@cac.washington.ed>\")"
        )
    );
}

#[test]
fn envelope_group() {
    let raw = concat!(
        "Date: Wed, 17 Jul 1996 02:23:25 +0000\r\n",
        "From: Bill Foobar <foobar@example.com>\r\n",
        "Subject: Group test\r\n",
        "To: Friends and Family: John Doe <jdoe@example.com>, ",
        "Jane Smith <jane.smith@example.com>;\r\n",
        "Message-Id: <B27397-0100000@cac.washington.ed>\r\n",
        "\r\n",
        "Body.\r\n"
    );
    assert_eq!(
        envelope(raw),
        concat!(
            "ENVELOPE (\"Wed, 17 Jul 1996 02:23:25 +0000\" ",
            "\"Group test\" ",
            "((\"Bill Foobar\" NIL \"foobar\" \"example.com\")) ",
            "((\"Bill Foobar\" NIL \"foobar\" \"example.com\")) ",
            "((\"Bill Foobar\" NIL \"foobar\" \"example.com\")) ",
            "((NIL NIL \"Friends and Family\" NIL)",
            "(\"John Doe\" NIL \"jdoe\" \"example.com\")",
            "(\"Jane Smith\" NIL \"jane.smith\" \"example.com\")",
            "(NIL NIL NIL NIL)) ",
            "NIL NIL NIL \"<B27397-0100000@cac.washington.ed>\")"
        )
    );
}

#[test]
fn body_text() {
    let body = text_body(48, 47);
    let raw = format!(
        concat!(
            "Subject: text\r\n",
            "Content-Type: TEXT/PLAIN; CHARSET=US-ASCII\r\n",
            "Content-Transfer-Encoding: 7BIT\r\n",
            "\r\n",
            "{}"
        ),
        body
    );
    let (size, lines) = stats(&body);
    assert_eq!((size, lines), (2256, 48));
    assert_eq!(
        structure(&raw, false),
        format!(
            "BODY (\"text\" \"plain\" (\"charset\" \"US-ASCII\") NIL NIL \"7BIT\" {size} {lines})"
        )
    );
}

#[test]
fn body_message() {
    let html = text_body(431, 10);
    let nested = format!(
        concat!(
            "Date: Wed, 17 Jul 1996 02:23:25 +0000\r\n",
            "From: Terry Gray <gray@cac.washington.edu>\r\n",
            "Subject: Hello world!\r\n",
            "To: imap@cac.washington.edu\r\n",
            "Message-Id: <4234324@domain.com>\r\n",
            "Content-Type: text/HTML\r\n",
            "Content-Transfer-Encoding: 8BIT\r\n",
            "\r\n",
            "{}"
        ),
        html
    );
    let raw = format!(
        concat!(
            "Subject: forwarded\r\n",
            "Content-Type: message/RFC822\r\n",
            "Content-ID: <abc@123>\r\n",
            "Content-Description: An attached email\r\n",
            "Content-Transfer-Encoding: quoted-printable\r\n",
            "\r\n",
            "{}"
        ),
        nested
    );
    let (size, lines) = stats(&nested);
    let (html_size, html_lines) = stats(&html);
    // 0.16's case wrote NIL for the HTML part's absent parameters; 1.0
    // writes the default charset of RFC 3501 for a text part without any.
    assert_eq!(
        structure(&raw, false),
        format!(
            concat!(
                "BODY (\"message\" \"rfc822\" NIL \"<abc@123>\" \"An attached email\" ",
                "\"quoted-printable\" {} (\"Wed, 17 Jul 1996 02:23:25 +0000\" ",
                "\"Hello world!\" ",
                "((\"Terry Gray\" NIL \"gray\" \"cac.washington.edu\")) ",
                "((\"Terry Gray\" NIL \"gray\" \"cac.washington.edu\")) ",
                "((\"Terry Gray\" NIL \"gray\" \"cac.washington.edu\")) ",
                "((NIL NIL \"imap\" \"cac.washington.edu\")) NIL NIL NIL ",
                "\"<4234324@domain.com>\") (\"text\" \"html\" (\"charset\" \"us-ascii\") NIL NIL ",
                "\"8BIT\" {} {}) {})"
            ),
            size, html_size, html_lines, lines
        )
    );
}

#[test]
fn body_multipart() {
    let first = text_body(23, 50);
    let second = text_body(73, 62);
    let raw = format!(
        concat!(
            "Subject: multipart\r\n",
            "Content-Type: multipart/MIXED; boundary=\"b1\"\r\n",
            "\r\n",
            "--b1\r\n",
            "Content-Type: text/PLAIN; CHARSET=US-ASCII\r\n",
            "Content-Transfer-Encoding: 7BIT\r\n",
            "\r\n",
            "{}",
            "\r\n--b1\r\n",
            "Content-Type: text/PLAIN; CHARSET=US-ASCII; NAME=cc.diff\r\n",
            "Content-ID: <960723163407.20117h@cac.washington.edu>\r\n",
            "Content-Description: Compiler diff\r\n",
            "Content-Transfer-Encoding: BASE64\r\n",
            "\r\n",
            "{}",
            "\r\n--b1--\r\n"
        ),
        first, second
    );
    let (first_size, first_lines) = stats(&first);
    let (second_size, second_lines) = stats(&second);
    assert_eq!(
        structure(&raw, false),
        format!(
            concat!(
                "BODY ((\"text\" \"plain\" (\"charset\" \"US-ASCII\") ",
                "NIL NIL \"7BIT\" {} {})",
                "(\"text\" \"plain\" (\"charset\" \"US-ASCII\" \"name\" \"cc.diff\") ",
                "\"<960723163407.20117h@cac.washington.edu>\" \"Compiler diff\" ",
                "\"BASE64\" {} {}) \"mixed\")",
            ),
            first_size, first_lines, second_size, second_lines
        )
    );
}

#[test]
fn bodystructure_nested_multipart() {
    let plain = text_body(23, 50);
    let html = text_body(994, 45);
    let word = text_body(10, 76);
    let raw = format!(
        concat!(
            "Subject: bodystructure\r\n",
            "Content-Type: multipart/MIXED; boundary=\"outer\"\r\n",
            "\r\n",
            "--outer\r\n",
            "Content-Type: multipart/ALTERNATIVE; boundary=\"inner\"; ",
            "x-param=\"a very special parameter\"\r\n",
            "Content-Language: en-US\r\n",
            "Content-Location: unknown\r\n",
            "\r\n",
            "--inner\r\n",
            "Content-Type: text/PLAIN; CHARSET=UTF-8\r\n",
            "Content-ID: <111@domain.com>\r\n",
            "Content-Description: Text part\r\n",
            "Content-Transfer-Encoding: 7BIT\r\n",
            "Content-MD5: 8o3456\r\n",
            "Content-Disposition: inline\r\n",
            "Content-Language: en-US\r\n",
            "Content-Location: right here\r\n",
            "\r\n",
            "{}",
            "\r\n--inner\r\n",
            "Content-Type: text/HTML; CHARSET=UTF-8\r\n",
            "Content-ID: <54535@domain.com>\r\n",
            "Content-Description: HTML part\r\n",
            "Content-Transfer-Encoding: 8BIT\r\n",
            "Content-MD5: 53454\r\n",
            "Content-Disposition: attachment; filename=myfile.txt\r\n",
            "Content-Language: en-US, de-DE\r\n",
            "Content-Location: right there\r\n",
            "\r\n",
            "{}",
            "\r\n--inner--\r\n",
            "\r\n--outer\r\n",
            "Content-Type: APPLICATION/MSWORD; NAME=chimichangas.docx\r\n",
            "Content-ID: <4444@chimi.changa>\r\n",
            "Content-Description: Chimichangas recipe\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "Content-MD5: 1234\r\n",
            "Content-Disposition: attachment; filename=chimichangas.docx\r\n",
            "Content-Language: en-MX\r\n",
            "Content-Location: secret location\r\n",
            "\r\n",
            "{}",
            "\r\n--outer--\r\n"
        ),
        plain, html, word
    );
    let (plain_size, plain_lines) = stats(&plain);
    let (html_size, html_lines) = stats(&html);
    let (word_size, _) = stats(&word);
    assert_eq!(
        structure(&raw, true),
        format!(
            concat!(
                "BODYSTRUCTURE (((\"text\" \"plain\" (\"charset\" \"UTF-8\") ",
                "\"<111@domain.com>\" \"Text part\" \"7BIT\" {} {} \"8o3456\" ",
                "(\"inline\" NIL) \"en-US\" \"right here\")",
                "(\"text\" \"html\" (\"charset\" \"UTF-8\") ",
                "\"<54535@domain.com>\" \"HTML part\" \"8BIT\" {} {} \"53454\" ",
                "(\"attachment\" (\"filename\" \"myfile.txt\")) ",
                "(\"en-US\" \"de-DE\") ",
                "\"right there\") \"alternative\" (\"boundary\" \"inner\" \"x-param\" ",
                "\"a very special parameter\") ",
                "NIL \"en-US\" \"unknown\")",
                "(\"application\" \"msword\" (\"name\" \"chimichangas.docx\") ",
                "\"<4444@chimi.changa>\" \"Chimichangas recipe\" \"base64\"",
                " {} \"1234\" ",
                "(\"attachment\" (\"filename\" \"chimichangas.docx\")) \"en-MX\" ",
                "\"secret location\") \"mixed\" (\"boundary\" \"outer\") NIL NIL NIL)",
            ),
            plain_size, plain_lines, html_size, html_lines, word_size
        )
    );
}
