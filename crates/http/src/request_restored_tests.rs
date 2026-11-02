/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use flate2::read::GzDecoder;
use std::io::Read;

const PAGES: [(&str, &[u8], &str); 2] = [
    (
        "calendar-rsvp.html",
        super::RSVP_PAGE,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/html-templates/calendar-rsvp.html.min"
        )),
    ),
    (
        "login.html",
        super::LOGIN_PAGE,
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/html-templates/login.html.min"
        )),
    ),
];

#[test]
fn gzipped_static_pages_are_in_sync() {
    for (name, gzipped, minified) in PAGES {
        let mut decoded = String::new();
        GzDecoder::new(gzipped)
            .read_to_string(&mut decoded)
            .unwrap_or_else(|err| panic!("{name}.min.gz failed to decompress: {err}"));

        assert_eq!(
            decoded, minified,
            "{name}.min.gz is stale, re-run resources/scripts/minify_html.sh --gzip"
        );
    }
}
