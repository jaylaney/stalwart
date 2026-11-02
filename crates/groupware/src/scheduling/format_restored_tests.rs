/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{PluralCategoryExt, PluralForms, TextFormatter, i18n};
use calcard::icalendar::{
    ICalendarDay, ICalendarFrequency, ICalendarRecurrenceRule, ICalendarWeekday,
};
use icu_plurals::PluralCategory;

fn rule(freq: ICalendarFrequency, interval: Option<u16>) -> ICalendarRecurrenceRule {
    ICalendarRecurrenceRule {
        freq,
        interval,
        ..Default::default()
    }
}

fn format(language: &str, rule: &ICalendarRecurrenceRule) -> String {
    let mut out = String::new();
    TextFormatter::new(language)
        .expect("formatter")
        .recurrence(&mut out, rule);
    out
}

#[test]
fn every_shipped_locale_loads_icu_data() {
    for locale in i18n::ALL_LOCALES {
        let formatter = TextFormatter::new(locale.name)
            .unwrap_or_else(|err| panic!("{}: {err:?}", locale.name));
        assert_eq!(formatter.locale.name, locale.name);

        let mut out = String::new();
        formatter.recurrence(
            &mut out,
            &ICalendarRecurrenceRule {
                freq: ICalendarFrequency::Monthly,
                interval: Some(2),
                count: Some(5),
                bymonthday: vec![3, -1],
                byday: vec![ICalendarDay {
                    ordwk: Some(2),
                    weekday: ICalendarWeekday::Monday,
                }],
                ..Default::default()
            },
        );
        assert!(!out.is_empty(), "{} produced no text", locale.name);
    }
}

#[test]
fn plural_form_selects_category_and_falls_back_to_other() {
    let forms = PluralForms {
        zero: "many",
        one: "single",
        two: "many",
        few: "a few",
        many: "many",
        other: "many",
    };
    assert_eq!(PluralCategory::One.plural_form(&forms), "single");
    assert_eq!(PluralCategory::Few.plural_form(&forms), "a few");
    assert_eq!(PluralCategory::Many.plural_form(&forms), "many");
    assert_eq!(PluralCategory::Other.plural_form(&forms), "many");

    // Categories a locale omits are filled from "other" at build time
    let polish = i18n::locale("pl-PL").expect("locale must exist");
    assert_eq!(polish.calendar_rrule_secondly.many, "Co $n sekund");
    assert_eq!(
        polish.calendar_rrule_secondly.zero,
        polish.calendar_rrule_secondly.other
    );
}

#[test]
fn frequency_uses_cardinal_plural_rules() {
    assert_eq!(
        format("en", &rule(ICalendarFrequency::Weekly, None)),
        "Every week"
    );
    assert_eq!(
        format("en", &rule(ICalendarFrequency::Weekly, Some(2))),
        "Every 2 weeks"
    );

    // Polish distinguishes one / few / many, unlike English
    assert_eq!(
        format("pl", &rule(ICalendarFrequency::Weekly, Some(1))),
        "Co tydzień"
    );
    assert_eq!(
        format("pl", &rule(ICalendarFrequency::Weekly, Some(2))),
        "Co 2 tygodnie"
    );
    assert_eq!(
        format("pl", &rule(ICalendarFrequency::Weekly, Some(5))),
        "Co 5 tygodni"
    );
}

#[test]
fn weekday_names_are_localized_without_translation_keys() {
    let mut byday = rule(ICalendarFrequency::Weekly, None);
    byday.byday = vec![ICalendarDay {
        weekday: ICalendarWeekday::Monday,
        ordwk: None,
    }];

    assert_eq!(format("en", &byday), "Every week on Monday");
    assert_eq!(format("es", &byday), "Cada semana los lunes");
}

#[test]
fn ordinal_weekday_uses_ordinal_plural_rules() {
    let mut byday = rule(ICalendarFrequency::Monthly, None);
    byday.byday = vec![ICalendarDay {
        weekday: ICalendarWeekday::Tuesday,
        ordwk: Some(2),
    }];
    assert_eq!(format("en", &byday), "Every month on the 2nd Tuesday");

    byday.byday = vec![ICalendarDay {
        weekday: ICalendarWeekday::Tuesday,
        ordwk: Some(3),
    }];
    assert_eq!(format("en", &byday), "Every month on the 3rd Tuesday");

    byday.byday = vec![ICalendarDay {
        weekday: ICalendarWeekday::Tuesday,
        ordwk: Some(-1),
    }];
    assert_eq!(
        format("en", &byday),
        "Every month on the 1st Tuesday from the end"
    );
}

#[test]
fn count_is_pluralized_and_appended() {
    let mut counted = rule(ICalendarFrequency::Daily, None);
    counted.count = Some(1);
    assert_eq!(format("en", &counted), "Every day, 1 time");

    counted.count = Some(5);
    assert_eq!(format("en", &counted), "Every day, 5 times");
}

#[test]
fn multiple_days_are_joined_with_the_localized_conjunction() {
    let mut byday = rule(ICalendarFrequency::Weekly, None);
    byday.byday = vec![
        ICalendarDay {
            weekday: ICalendarWeekday::Monday,
            ordwk: None,
        },
        ICalendarDay {
            weekday: ICalendarWeekday::Wednesday,
            ordwk: None,
        },
        ICalendarDay {
            weekday: ICalendarWeekday::Friday,
            ordwk: None,
        },
    ];

    assert_eq!(
        format("en", &byday),
        "Every week on Monday, Wednesday and Friday"
    );
}

#[test]
fn unknown_language_falls_back_to_english_rules() {
    assert_eq!(
        format("zz", &rule(ICalendarFrequency::Weekly, Some(3))),
        "Every 3 weeks"
    );
}
