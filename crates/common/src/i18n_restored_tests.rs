/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{ALL_LOCALES, locale, locale_or_default};

const LOCALES: [&str; 37] = [
    "en-US", "es-ES", "fr-FR", "de-DE", "it-IT", "pt-PT", "pt-BR", "nl-NL", "da-DK", "ca-ES",
    "el-GR", "sv-SE", "pl-PL", "ru-RU", "uk-UA", "bg-BG", "cs-CZ", "sk-SK", "sl-SI", "hr-HR",
    "lt-LT", "hu-HU", "ro-RO", "fi-FI", "nb-NO", "tr-TR", "zh-CN", "zh-TW", "ja-JP", "ko-KR",
    "th-TH", "vi-VN", "id-ID", "hi-IN", "ar-SA", "he-IL", "fa-IR",
];

#[test]
fn locales_are_named_after_themselves() {
    for lang in LOCALES {
        assert_eq!(locale(lang).expect("locale must exist").name, lang);
    }
    assert_eq!(ALL_LOCALES.len(), LOCALES.len());
}

#[test]
fn bare_and_hyphenated_language_tags_resolve() {
    for (input, expected) in [
        ("es-ES", "es-ES"),
        ("es", "es-ES"),
        ("es-MX", "es-ES"),
        ("pt-BR", "pt-BR"),
        ("pt-PT", "pt-PT"),
        ("pt", "pt-BR"),
        ("zh-Hans", "zh-CN"),
        ("zh-Hant", "zh-TW"),
        ("zh-HK", "zh-TW"),
        ("zh-Hant-HK", "zh-TW"),
        ("zh", "zh-CN"),
        ("zz", "en-US"),
        ("", "en-US"),
        // BCP 47 tags are case-insensitive
        ("ES", "es-ES"),
        ("es-es", "es-ES"),
        ("PT-br", "pt-BR"),
        ("EL-GR", "el-GR"),
        ("ZH-HANT", "zh-TW"),
    ] {
        assert_eq!(
            locale_or_default(input).name,
            expected,
            "failed for {input}"
        );
    }
}

#[test]
fn right_to_left_locales_are_flagged() {
    for lang in ["ar-SA", "he-IL", "fa-IR"] {
        assert_eq!(
            locale_or_default(lang).direction,
            "rtl",
            "failed for {lang}"
        );
    }
    for lang in ["en-US", "de-DE", "ja-JP", "ru-RU"] {
        assert_eq!(
            locale_or_default(lang).direction,
            "ltr",
            "failed for {lang}"
        );
    }
}
