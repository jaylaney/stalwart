/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;

fn config(spam: f32, discard: f32, reject: f32) -> SpamFilterScoreConfig {
    SpamFilterScoreConfig {
        reject_threshold: reject,
        discard_threshold: discard,
        spam_threshold: spam,
    }
}

#[test]
fn spam_percentage_defaults() {
    let config = config(5.0, 0.0, 0.0);

    for (score, expected) in [
        (-10.0, 0),
        (0.0, 0),
        (0.5, 5),
        (2.5, 25),
        (4.9, 49),
        (4.999, 49),
        (5.0, 50),
        (7.5, 75),
        (9.9, 99),
        (10.0, 100),
        (50.0, 100),
    ] {
        assert_eq!(config.spam_percentage(score), expected, "score {score}");
    }
}

#[test]
fn spam_percentage_matches_is_spam() {
    for config in [
        config(5.0, 0.0, 0.0),
        config(5.0, 20.0, 15.0),
        config(1.0, 0.0, 3.0),
        config(12.5, 25.0, 0.0),
        config(0.0, 0.0, 0.0),
    ] {
        for score in (-2000..=4000).map(|score| score as f32 / 100.0) {
            assert_eq!(
                config.spam_percentage(score) >= 50,
                config.is_spam(score),
                "score {score} with {config:?}"
            );
        }
    }
}

#[test]
fn spam_percentage_ceiling_is_lowest_enabled_threshold() {
    let reject_lowest = config(5.0, 20.0, 15.0);
    assert_eq!(reject_lowest.spam_percentage(10.0), 75);
    assert_eq!(reject_lowest.spam_percentage(15.0), 100);

    let discard_only = config(5.0, 15.0, 0.0);
    assert_eq!(discard_only.spam_percentage(10.0), 75);

    let below_spam_threshold = config(5.0, 3.0, 0.0);
    assert_eq!(below_spam_threshold.spam_percentage(7.5), 75);
}

#[test]
fn spam_status_from_percentage() {
    assert!(matches!(spam_status(None), SpamStatus::Unknown));
    assert!(matches!(spam_status(Some(0)), SpamStatus::Ham));
    assert!(matches!(spam_status(Some(100)), SpamStatus::Spam));
    assert!(matches!(
        spam_status(Some(50)),
        SpamStatus::MaybeSpam(fraction) if fraction == 0.5
    ));
}
