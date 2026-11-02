/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::tokenizers::{
    stream::WordStemTokenizer,
    types::{TokenType, TypesTokenizer},
};

#[test]
fn stream_tokenizer() {
    let inputs = [
        (
            "The quick brown fox jumps over the lazy dog",
            vec!["quick", "brown", "fox", "jump", "lazi", "dog"],
        ),
        (
            "Jovencillo emponzoñado de whisky: ¡qué figurota exhibe!",
            vec!["jovencill", "emponzoñ", "whisky", "figurot", "exhib"],
        ),
        (
            "Ma la volpe col suo balzo ha raggiunto il quieto Fido",
            vec!["volp", "balz", "raggiunt", "quiet", "fid"],
        ),
        (
            "Jaz em prisão bota que vexa dez cegonhas felizes",
            vec!["jaz", "prisã", "bot", "vex", "dez", "cegonh", "feliz"],
        ),
        (
            "Zwölf Boxkämpfer jagten Victor quer über den großen Sylter Deich",
            vec![
                "zwolf", "boxkampf", "jagt", "victor", "quer", "gross", "sylt", "deich",
            ],
        ),
        (
            "עטלף אבק נס דרך מזגן שהתפוצץ כי חם",
            vec!["עטלף", "אבק", "נס", "דרך", "מזגן", "שהתפוצץ", "כי", "חם"],
        ),
        (
            "Съешь ещё этих мягких французских булок, да выпей же чаю",
            vec![
                "съеш",
                "ещё",
                "эт",
                "мягк",
                "французск",
                "булок",
                "вып",
                "ча",
            ],
        ),
        (
            "Чуєш їх, доцю, га? Кумедна ж ти, прощайся без ґольфів!",
            vec![
                "чуєш",
                "їх",
                "доцю",
                "га",
                "кумедна",
                "ж",
                "ти",
                "прощайся",
                "без",
                "ґольфів",
            ],
        ),
        (
            "Љубазни фењерџија чађавог лица хоће да ми покаже штос",
            vec![
                "љубазни",
                "фењерџија",
                "чађавог",
                "лица",
                "хоће",
                "да",
                "ми",
                "покаже",
                "штос",
            ],
        ),
        (
            "Pijamalı hasta yağız şoföre çabucak güvendi",
            vec!["pijamalı", "hasta", "yağız", "şoför", "çabucak", "güvendi"],
        ),
        ("己所不欲,勿施于人。", vec!["己所不欲", "勿施于人"]),
        (
            "井の中の蛙大海を知らず",
            vec!["井", "の", "中", "の", "蛙大", "海", "を", "知ら", "ず"],
        ),
        ("시작이 반이다", vec!["시작이", "반이다"]),
    ];

    for (input, expect) in inputs.iter() {
        let tokenizer = WordStemTokenizer::new(input);
        let mut result = Vec::new();
        for token in TypesTokenizer::new(&input.to_lowercase()) {
            if let TokenType::Alphabetic(word) = token.word {
                tokenizer.tokenize(word, |t| {
                    result.push(t.into_owned());
                });
            }
        }

        assert_eq!(&result, expect,);
    }
}
