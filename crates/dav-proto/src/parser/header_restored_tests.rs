/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn parse_accept_vcard_version() {
    for (header, expected) in [
        ("text/vcard", None),
        ("application/json", None),
        ("text/vcard; version=3.0", Some(VCardVersion::V3_0)),
        ("text/vcard;version=\"3.0\"", Some(VCardVersion::V3_0)),
        ("text/vcard; VERSION=3.0", Some(VCardVersion::V3_0)),
        ("text/vcard; version=3.0; q=1.0", Some(VCardVersion::V3_0)),
        (
            "text/vcard; version=3.0; charset=utf-8",
            Some(VCardVersion::V3_0),
        ),
        (
            "text/vcard; charset=utf-8; version=3.0",
            Some(VCardVersion::V3_0),
        ),
        (
            "text/vcard; version=4.0, text/vcard; version=3.0",
            Some(VCardVersion::V4_0),
        ),
        (
            "text/vcard; version=4.0; q=0.5, text/vcard; version=3.0",
            Some(VCardVersion::V3_0),
        ),
        (
            "text/vcard; q=0.5; version=4.0, text/vcard; q=1.0; version=3.0",
            Some(VCardVersion::V3_0),
        ),
        (
            "text/vcard; version=4.0; q=0, text/vcard; version=3.0; q=0.1",
            Some(VCardVersion::V3_0),
        ),
        ("*/*, text/vcard; version=3.0", Some(VCardVersion::V3_0)),
        ("text/vcard-custom; version=3.0", None),
    ] {
        let mut headers = RequestHeaders::new("/dav/card/test/default/");
        assert!(headers.parse("Accept", header));
        assert_eq!(headers.vcard_version, expected, "failed for {header:?}");
    }
}

#[test]
fn parse_range() {
    for (header, expected) in [
        (
            "bytes=0-499",
            Some(ByteRange::Offset {
                start: 0,
                end: Some(499),
            }),
        ),
        (
            "bytes=500-",
            Some(ByteRange::Offset {
                start: 500,
                end: None,
            }),
        ),
        (
            "bytes = 500 - 999 ",
            Some(ByteRange::Offset {
                start: 500,
                end: Some(999),
            }),
        ),
        (
            "BYTES=0-0",
            Some(ByteRange::Offset {
                start: 0,
                end: Some(0),
            }),
        ),
        ("bytes=-500", Some(ByteRange::Suffix(500))),
        ("bytes=-0", Some(ByteRange::Suffix(0))),
        ("bytes=0-499,600-999", None),
        ("bytes=499-100", None),
        ("bytes=abc-def", None),
        ("bytes=-", None),
        ("bytes=0", None),
        ("items=0-499", None),
        ("0-499", None),
    ] {
        let mut headers = RequestHeaders::new("/dav/file/test/file.txt");
        assert_eq!(headers.parse("Range", header), expected.is_some());
        assert_eq!(headers.range, expected, "failed for {header:?}");
    }

    for (range, size, expected) in [
        (
            ByteRange::Offset {
                start: 0,
                end: Some(499),
            },
            1000,
            Some(0..500),
        ),
        (
            ByteRange::Offset {
                start: 0,
                end: Some(499),
            },
            100,
            Some(0..100),
        ),
        (
            ByteRange::Offset {
                start: 500,
                end: None,
            },
            1000,
            Some(500..1000),
        ),
        (
            ByteRange::Offset {
                start: 999,
                end: Some(u64::MAX),
            },
            1000,
            Some(999..1000),
        ),
        (
            ByteRange::Offset {
                start: 1000,
                end: None,
            },
            1000,
            None,
        ),
        (
            ByteRange::Offset {
                start: 0,
                end: None,
            },
            0,
            None,
        ),
        (ByteRange::Suffix(500), 1000, Some(500..1000)),
        (ByteRange::Suffix(5000), 1000, Some(0..1000)),
        (ByteRange::Suffix(0), 1000, None),
        (ByteRange::Suffix(500), 0, None),
    ] {
        assert_eq!(
            range.resolve(size),
            expected,
            "failed for {range:?} of {size}"
        );
    }
}

#[test]
fn eval_if_range_header() {
    const LAST_MODIFIED: &str = "Mon, 10 Aug 2026 12:00:00 GMT";

    let mut headers = RequestHeaders::new("/dav/file/test/file.txt");
    assert!(headers.eval_if_range("\"etag\"", Some(LAST_MODIFIED)));

    for (validator, expected) in [
        ("\"etag\"", true),
        (LAST_MODIFIED, true),
        ("W/\"etag\"", false),
        ("\"other\"", false),
        ("Sun, 09 Aug 2026 12:00:00 GMT", false),
    ] {
        assert!(headers.parse("If-Range", validator));
        assert_eq!(
            headers.eval_if_range("\"etag\"", Some(LAST_MODIFIED)),
            expected,
            "failed for {validator:?}"
        );
    }

    for (validator, expected) in [("\"etag\"", true), (LAST_MODIFIED, false)] {
        assert!(headers.parse("If-Range", validator));
        assert_eq!(
            headers.eval_if_range("\"etag\"", None),
            expected,
            "failed for {validator:?}"
        );
    }
}

#[test]
fn base_uri() {
    for (uri, expected_base) in [
        (
            "http://host/dav/collection/account/test/",
            Some("http://host/dav/collection/account"),
        ),
        (
            "http://host/dav/collection/account/test",
            Some("http://host/dav/collection/account"),
        ),
        (
            "http://host/dav/collection/account/",
            Some("http://host/dav/collection/account"),
        ),
        (
            "http://host/dav/collection/account",
            Some("http://host/dav/collection/account"),
        ),
        (
            "http://host/dev/dav/collection/account/test/",
            Some("http://host/dev/dav/collection/account"),
        ),
        (
            "http://host/dev/dav/collection/account/test",
            Some("http://host/dev/dav/collection/account"),
        ),
        (
            "http://host/dev/dav/collection/account/",
            Some("http://host/dev/dav/collection/account"),
        ),
        (
            "http://host/dev/dav/collection/account",
            Some("http://host/dev/dav/collection/account"),
        ),
        (
            "/dav/collection/account/test/",
            Some("/dav/collection/account"),
        ),
        (
            "/dav/collection/account/test",
            Some("/dav/collection/account"),
        ),
        ("/dav/collection/account/", Some("/dav/collection/account")),
        ("/dav/collection/account", Some("/dav/collection/account")),
    ] {
        assert_eq!(RequestHeaders::new(uri).base_uri(), expected_base);
    }
}

#[test]
fn eval_if_header() {
    let mut headers = RequestHeaders::default();
    assert!(headers.parse(
        "If",
        r#"(<urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2>
   ["I am an ETag"])
   (["I am another ETag"])"#,
    ));

    assert!(headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2",
        etag: "\"I am an ETag\""
    }]));
    assert!(headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "",
        etag: "\"I am another ETag\""
    }]));
    assert!(!headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "",
        etag: "\"Unknown ETag\""
    }]));
    assert!(!headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2",
        etag: ""
    }]));
    assert!(!headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2",
        etag: "\"Other ETag\""
    }]));
    assert!(!headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "",
        etag: "\"I am an ETag\""
    }]));
    assert!(!headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "urn:blah",
        etag: "\"I am an ETag\""
    }]));

    assert!(headers.parse(
        "If",
        r#"(Not <urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2>
     <urn:uuid:58f202ac-22cf-11d1-b12d-002035b29092>)"#,
    ));
    assert!(headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "urn:uuid:58f202ac-22cf-11d1-b12d-002035b29092",
        etag: ""
    }]));
    assert!(!headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2",
        etag: ""
    }]));

    assert!(headers.parse(
        "If",
        r#"(<urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2>)
       (Not <DAV:no-lock>)"#
    ));
    assert!(headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2",
        etag: ""
    }]));
    assert!(headers.eval_if(&[ResourceState {
        resource: None,
        state_token: "urn:other-token",
        etag: ""
    }]));
}

#[test]
fn parse_headers() {
    let mut headers = RequestHeaders::default();
    assert!(headers.parse("Depth", "0"));
    assert_eq!(headers.depth, Depth::Zero);

    assert!(headers.parse("Destination", "/path/to/destination"));
    assert_eq!(headers.destination, Some("/path/to/destination"));

    assert!(headers.parse("Lock-Token", "<urn:uuid:1234>"));
    assert_eq!(headers.lock_token, Some("urn:uuid:1234"));

    for (input, expected) in [
        (
            "<urn:uuid:1234>(<urn:uuid:1234>)",
            vec![If {
                resource: "urn:uuid:1234".into(),
                list: vec![Condition::StateToken {
                    is_not: false,
                    token: "urn:uuid:1234",
                }],
            }],
        ),
        (
            "<>(<>)",
            vec![If {
                resource: "".into(),
                list: vec![Condition::StateToken {
                    is_not: false,
                    token: "",
                }],
            }],
        ),
        (
            r#"(<urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2>
       ["I am an ETag"])
       (["I am another ETag"])"#,
            vec![
                If {
                    resource: None,
                    list: vec![
                        Condition::StateToken {
                            is_not: false,
                            token: "urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2",
                        },
                        Condition::ETag {
                            is_not: false,
                            tag: "\"I am an ETag\"",
                        },
                    ],
                },
                If {
                    resource: None,
                    list: vec![Condition::ETag {
                        is_not: false,
                        tag: "\"I am another ETag\"",
                    }],
                },
            ],
        ),
        (
            r#"(Not <urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2>
     <urn:uuid:58f202ac-22cf-11d1-b12d-002035b29092>)"#,
            vec![If {
                resource: None,
                list: vec![
                    Condition::StateToken {
                        is_not: true,
                        token: "urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2",
                    },
                    Condition::StateToken {
                        is_not: false,
                        token: "urn:uuid:58f202ac-22cf-11d1-b12d-002035b29092",
                    },
                ],
            }],
        ),
        (
            r#"(<urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2>)
       (Not <DAV:no-lock>)"#,
            vec![
                If {
                    resource: None,
                    list: vec![Condition::StateToken {
                        is_not: false,
                        token: "urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2",
                    }],
                },
                If {
                    resource: None,
                    list: vec![Condition::StateToken {
                        is_not: true,
                        token: "DAV:no-lock",
                    }],
                },
            ],
        ),
        (
            r#"</resource1>
                 (<urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2>
                 [W/"A weak ETag"]) (["strong ETag"])"#,
            vec![
                If {
                    resource: "/resource1".into(),
                    list: vec![
                        Condition::StateToken {
                            is_not: false,
                            token: "urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2",
                        },
                        Condition::ETag {
                            is_not: false,
                            tag: "W/\"A weak ETag\"",
                        },
                    ],
                },
                If {
                    resource: None,
                    list: vec![Condition::ETag {
                        is_not: false,
                        tag: "\"strong ETag\"",
                    }],
                },
            ],
        ),
        (
            r#"<http://www.example.com/specs/>
            (<urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2>)"#,
            vec![If {
                resource: "http://www.example.com/specs/".into(),
                list: vec![Condition::StateToken {
                    is_not: false,
                    token: "urn:uuid:181d4fae-7d8c-11d0-a765-00a0c91e6bf2",
                }],
            }],
        ),
        (
            r#"</specs/rfc2518.doc> (["4217"])"#,
            vec![If {
                resource: "/specs/rfc2518.doc".into(),
                list: vec![Condition::ETag {
                    is_not: false,
                    tag: "\"4217\"",
                }],
            }],
        ),
        (
            r#"</specs/rfc2518.doc> (Not ["4217"])"#,
            vec![If {
                resource: "/specs/rfc2518.doc".into(),
                list: vec![Condition::ETag {
                    is_not: true,
                    tag: "\"4217\"",
                }],
            }],
        ),
        (
            r#"</test/file.txt> (["1234"]) </specs/rfc2518.doc> (Not ["4217"])"#,
            vec![
                If {
                    resource: "/test/file.txt".into(),
                    list: vec![Condition::ETag {
                        is_not: false,
                        tag: "\"1234\"",
                    }],
                },
                If {
                    resource: "/specs/rfc2518.doc".into(),
                    list: vec![Condition::ETag {
                        is_not: true,
                        tag: "\"4217\"",
                    }],
                },
            ],
        ),
    ] {
        assert!(headers.parse("If", input));
        assert_eq!(headers.if_, expected, "Failed for input: {}", input);
        headers.if_.clear();
    }

    assert!(headers.parse("If-Match", "*"));
    assert_eq!(
        headers.if_,
        vec![If {
            resource: None,
            list: vec![Condition::Exists { is_not: false }],
        }]
    );
    headers.if_.clear();

    assert!(headers.parse("If-None-Match", "etag1, etag2"));
    assert_eq!(
        headers.if_,
        vec![If {
            resource: None,
            list: vec![
                Condition::ETag {
                    is_not: true,
                    tag: "etag1",
                },
                Condition::ETag {
                    is_not: true,
                    tag: "etag2",
                }
            ],
        },]
    );

    assert!(headers.parse("Timeout", "Second-10"));
    assert_eq!(headers.timeout, Timeout::Second(10));

    assert!(headers.parse("Timeout", "Infinite, Second-4100000000"));
    assert_eq!(headers.timeout, Timeout::Infinite);

    assert!(headers.parse("Overwrite", "F"));
    assert!(headers.overwrite_fail);
}
