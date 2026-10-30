/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn request_url_keeps_case() {
    let url = UrlParts::new(" https://Bit.ly/3AbCdEf ");
    assert_eq!(url.url, "https://bit.ly/3abcdef");
    assert_eq!(url.request_url(), "https://Bit.ly/3AbCdEf");

    let url = UrlParts::no_scheme("Bit.ly/3AbCdEf");
    assert_eq!(url.url, "https://bit.ly/3abcdef");
    assert_eq!(url.request_url(), "https://Bit.ly/3AbCdEf");
}
