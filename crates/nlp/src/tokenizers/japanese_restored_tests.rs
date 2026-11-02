/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::tokenizers::{Token, japanese::JapaneseTokenizer, word::WordTokenizer};

#[test]
fn japanese_tokenizer() {
    assert_eq!(
        JapaneseTokenizer::new(WordTokenizer::new(
            "お先に失礼します あなたの名前は何ですか 123 abc-872",
            40
        ))
        .collect::<Vec<_>>(),
        vec![
            Token {
                word: "お先".into(),
                from: 0,
                to: 6
            },
            Token {
                word: "に".into(),
                from: 6,
                to: 9
            },
            Token {
                word: "失礼".into(),
                from: 9,
                to: 15
            },
            Token {
                word: "し".into(),
                from: 15,
                to: 18
            },
            Token {
                word: "ます".into(),
                from: 18,
                to: 24
            },
            Token {
                word: "あなた".into(),
                from: 25,
                to: 34
            },
            Token {
                word: "の".into(),
                from: 34,
                to: 37
            },
            Token {
                word: "名前".into(),
                from: 37,
                to: 43
            },
            Token {
                word: "は".into(),
                from: 43,
                to: 46
            },
            Token {
                word: "何".into(),
                from: 46,
                to: 49
            },
            Token {
                word: "です".into(),
                from: 49,
                to: 55
            },
            Token {
                word: "か".into(),
                from: 55,
                to: 58
            },
            Token {
                word: "123".into(),
                from: 59,
                to: 62
            },
            Token {
                word: "abc".into(),
                from: 63,
                to: 66
            },
            Token {
                word: "872".into(),
                from: 67,
                to: 70
            }
        ]
    );
}
