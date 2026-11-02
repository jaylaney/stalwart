/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn stemmer() {
    let inputs = [
        (
            "love loving lovingly loved lovely",
            Language::English,
            "love",
        ),
        ("querer queremos quer", Language::Spanish, "quer"),
    ];

    for (input, language, result) in inputs {
        for token in Stemmer::new(input, language, 40) {
            assert_eq!(token.stemmed_word.unwrap_or(token.word), result);
        }
    }
}
