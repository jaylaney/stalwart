/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

// 0.16's `groupware::tests` tested `encode_path_segment`, which kept a
// segment that was already a URI segment and percent-encoded any other name
// with the RFC 3986 set. 1.0 replaced it here: `canonical_path_segment`
// always encodes a stored name with `FILE_SEGMENT`, and `canonical_uri_path`
// decodes a request path's segments and re-encodes them, so one name has one
// URI. These are the three 0.16 tests written against that pair.

use super::*;

#[test]
fn path_segments_from_uris_are_preserved() {
    for name in [
        "readme.txt",
        "My%20Folder",
        "%C3%9Cnterlagen.txt",
        "file(1).txt",
        "a+b.txt",
        "Q&A.txt",
        "it's.txt",
        "mail@host.txt",
        "a:b.txt",
        "notes;v=2,rev=3!$*=.txt",
        "~backup_1-2.txt",
    ] {
        assert_eq!(canonical_uri_path(name).as_deref(), Ok(name), "{name:?}");
    }
}

#[test]
fn path_segments_from_names_are_encoded() {
    for (name, expected) in [
        ("My Folder", "My%20Folder"),
        ("Ünterlagen.txt", "%C3%9Cnterlagen.txt"),
        ("Ünterlagen 2026.txt", "%C3%9Cnterlagen%202026.txt"),
        ("100%", "100%25"),
        ("100%2", "100%252"),
        ("100%zz", "100%25zz"),
        ("a/b.txt", "a%2Fb.txt"),
        ("a<b>c.txt", "a%3Cb%3Ec.txt"),
        ("a\"b#c?d.txt", "a%22b%23c%3Fd.txt"),
        ("a\tb.txt", "a%09b.txt"),
    ] {
        assert_eq!(canonical_path_segment(name), expected, "{name:?}");
    }
}

#[test]
fn encoded_path_segments_are_stable() {
    // 0.16 also kept "a/b.txt" stable. 1.0 refuses a segment that decodes to
    // a '/', so that name has no URI path (`decode_segment`).
    for name in ["My Folder", "Ünterlagen 2026.txt", "100%", "file(1).txt"] {
        let encoded = canonical_path_segment(name).into_owned();
        assert_eq!(
            canonical_uri_path(&encoded).as_deref(),
            Ok(encoded.as_str()),
            "{name:?}"
        );
        assert_eq!(dav_file_name(&encoded).as_deref(), Ok(name), "{name:?}");
    }
    assert_eq!(
        canonical_uri_path(&canonical_path_segment("a/b.txt")),
        Err(DavFileNameError::InvalidCharacter)
    );
}
