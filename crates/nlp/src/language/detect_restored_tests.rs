/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

#[test]
fn detect_languages() {
    let inputs = [
        (
            "The quick brown fox jumps over the lazy dog",
            Language::English,
        ),
        (
            "Jovencillo emponzoñado de whisky: ¡qué figurota exhibe!",
            Language::Spanish,
        ),
        (
            "Ma la volpe col suo balzo ha raggiunto il quieto Fido",
            Language::Italian,
        ),
        (
            "Jaz em prisão bota que vexa dez cegonhas felizes",
            Language::Portuguese,
        ),
        (
            "Zwölf Boxkämpfer jagten Victor quer über den großen Sylter Deich",
            Language::German,
        ),
        ("עטלף אבק נס דרך מזגן שהתפוצץ כי חם", Language::Hebrew),
        (
            "Съешь ещё этих мягких французских булок, да выпей же чаю",
            Language::Russian,
        ),
        (
            "Чуєш їх, доцю, га? Кумедна ж ти, прощайся без ґольфів!",
            Language::Ukrainian,
        ),
        (
            "Љубазни фењерџија чађавог лица хоће да ми покаже штос",
            Language::Serbian,
        ),
        (
            "Pijamalı hasta yağız şoföre çabucak güvendi",
            Language::Turkish,
        ),
        ("己所不欲,勿施于人。", Language::Mandarin),
        ("井の中の蛙大海を知らず", Language::Japanese),
        ("시작이 반이다", Language::Korean),
    ];

    let mut detector = LanguageDetector::new();

    for input in inputs.iter() {
        assert_eq!(detector.detect(input.0, 0.0), input.1);
    }
}

#[test]
fn weighted_language() {
    let mut detector = LanguageDetector::new();
    for lang in [
        (Language::Spanish, 0.5, 70),
        (Language::Japanese, 0.2, 100),
        (Language::Japanese, 0.3, 100),
        (Language::Japanese, 0.4, 200),
        (Language::English, 0.7, 50),
    ]
    .iter()
    {
        let w = detector
            .lang_detected
            .entry(lang.0)
            .or_insert_with(|| WeightedAverage {
                weight: 0,
                confidence: 0.0,
                occurrences: 0,
            });
        w.occurrences += 1;
        w.weight += lang.2;
        w.confidence += lang.1 * lang.2 as f64;
    }
    assert_eq!(detector.most_frequent_language(), Some(Language::Japanese));
}
