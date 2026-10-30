/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn iso_639_codes() {
    for (code, expected) in [
        ("nl", Language::Dutch),
        ("fi", Language::Finnish),
        ("hu", Language::Hungarian),
        ("tr", Language::Turkish),
        ("ro", Language::Romanian),
        ("nb", Language::Bokmal),
        ("no", Language::Bokmal),
        ("yi", Language::Yiddish),
        ("eo", Language::Esperanto),
        ("uk", Language::Ukrainian),
        ("cs", Language::Czech),
        ("sk", Language::Slovak),
        ("bg", Language::Bulgarian),
        ("be", Language::Belarusian),
        ("th", Language::Thai),
        ("mr", Language::Marathi),
        ("kn", Language::Kannada),
        ("jv", Language::Javanese),
        ("en", Language::English),
        ("pl", Language::Polish),
    ] {
        assert_eq!(Language::from_iso_639(code), Some(expected), "{code}");
    }
}

#[test]
fn iso_639_locale_suffixes() {
    for code in ["nl_NL", "nl_BE", "nl_NL@euro", "nl-NL"] {
        assert_eq!(
            Language::from_iso_639(code),
            Some(Language::Dutch),
            "{code}"
        );
    }
    assert_eq!(Language::from_iso_639("xx_XX"), None);
}
