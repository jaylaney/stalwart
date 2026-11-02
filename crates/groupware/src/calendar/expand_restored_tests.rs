/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use calcard::{Entry, Parser};
// Restored from 0.16.27. Ruling 44: chrono is no longer a dependency at
// 1.0, so the naive timestamp is computed with jiff (same value), and
// `CalendarEventData::new` lost its alarm out-parameter.
fn naive(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    jiff::civil::date(year as i16, month as i8, day as i8)
        .at(hour as i8, minute as i8, second as i8, 0)
        .to_zoned(jiff::tz::TimeZone::UTC)
        .expect("valid date")
        .timestamp()
        .as_second()
}

fn key(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> RecurrenceKey {
    RecurrenceKey::from_recurrence_id(naive(year, month, day, hour, minute, 0))
        .expect("representable recurrence id")
}

fn event_data(ical: &str) -> CalendarEventData {
    let entry = Parser::new(ical).entry();
    let Entry::ICalendar(ical) = entry else {
        panic!("failed to parse iCalendar: {entry:?}");
    };
    CalendarEventData::new(ical, Tz::UTC, 1000)
}

fn expand_key(data: &CalendarEventData, key: RecurrenceKey) -> Vec<(u32, i64)> {
    let mut keys = AHashSet::from_iter([key]);
    data.expand_from_ids(&mut keys, Tz::UTC)
        .expect("expansion")
        .into_iter()
        .map(|expansion| (expansion.comp_id, expansion.start_naive))
        .collect()
}

const MASTER: &str = concat!(
    "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Test//EN\r\n",
    "BEGIN:VEVENT\r\nUID:u@example.com\r\nDTSTAMP:20270101T000000Z\r\n",
    "DTSTART:20270301T090000Z\r\nDTEND:20270301T100000Z\r\n",
    "RRULE:FREQ=WEEKLY;COUNT=5\r\nSUMMARY:Weekly\r\nEND:VEVENT\r\n",
);

const OVERRIDE: &str = concat!(
    "BEGIN:VEVENT\r\nUID:u@example.com\r\nDTSTAMP:20270101T000000Z\r\n",
    "RECURRENCE-ID:20270308T090000Z\r\nDTSTART:20270308T140000Z\r\n",
    "DTEND:20270308T150000Z\r\nSUMMARY:Moved\r\nEND:VEVENT\r\n",
);

#[test]
fn recurrence_key_encoding() {
    assert_eq!(
        RecurrenceKey::from_recurrence_id(RECURRENCE_KEY_EPOCH),
        Some(RecurrenceKey(1))
    );
    assert_eq!(
        RecurrenceKey::from_recurrence_id(RECURRENCE_KEY_EPOCH - 1),
        None
    );
    assert_eq!(RecurrenceKey::from_recurrence_id(i64::MAX), None);
    assert_eq!(RecurrenceKey::from_prefix(0), None);
    assert_eq!(
        RecurrenceKey::from_prefix(key(2027, 3, 15, 9, 0).prefix()),
        Some(key(2027, 3, 15, 9, 0))
    );
    assert_ne!(key(2027, 3, 15, 9, 0), key(2027, 3, 15, 9, 1));
    assert_eq!(
        RecurrenceKey::from_recurrence_id(naive(2027, 3, 15, 9, 0, 30)),
        Some(key(2027, 3, 15, 9, 0))
    );
}

#[test]
fn recurrence_keys_survive_an_override() {
    let before = event_data(&format!("{MASTER}END:VCALENDAR\r\n"));
    let after = event_data(&format!("{MASTER}{OVERRIDE}END:VCALENDAR\r\n"));

    for (day, comp_id) in [(1, 1), (15, 1), (22, 1), (29, 1)] {
        let key = key(2027, 3, day, 9, 0);
        let start_naive = naive(2027, 3, day, 9, 0, 0);
        assert_eq!(expand_key(&before, key), [(comp_id, start_naive)]);
        assert_eq!(expand_key(&after, key), [(comp_id, start_naive)]);
    }

    let overridden = key(2027, 3, 8, 9, 0);
    assert_eq!(
        expand_key(&before, overridden),
        [(1, naive(2027, 3, 8, 9, 0, 0))]
    );
    assert_eq!(
        expand_key(&after, overridden),
        [(2, naive(2027, 3, 8, 14, 0, 0))]
    );
}

#[test]
fn this_and_future_instances_get_distinct_keys() {
    const THIS_AND_FUTURE: &str = concat!(
        "BEGIN:VEVENT\r\nUID:u@example.com\r\nDTSTAMP:20270101T000000Z\r\n",
        "RECURRENCE-ID;RANGE=THISANDFUTURE:20270315T090000Z\r\n",
        "DTSTART:20270315T100000Z\r\nDTEND:20270315T113000Z\r\n",
        "SUMMARY:Longer\r\nEND:VEVENT\r\n",
    );
    let data = event_data(&format!("{MASTER}{THIS_AND_FUTURE}END:VCALENDAR\r\n"));

    assert_eq!(
        expand_key(&data, key(2027, 3, 15, 9, 0)),
        [(2, naive(2027, 3, 15, 10, 0, 0))]
    );
    // Ruling 44, an intended 1.0 change: an instance a RANGE=THISANDFUTURE
    // override shifts keeps the series' recurrence id (RFC 5545: the
    // original start, 09:00), not its shifted start; 0.16 keyed it at 10:00.
    assert_eq!(
        expand_key(&data, key(2027, 3, 22, 9, 0)),
        [(2, naive(2027, 3, 22, 10, 0, 0))]
    );
    assert_eq!(
        expand_key(&data, key(2027, 3, 29, 9, 0)),
        [(2, naive(2027, 3, 29, 10, 0, 0))]
    );
    assert!(expand_key(&data, key(2027, 3, 22, 10, 0)).is_empty());
    assert_eq!(
        expand_key(&data, key(2027, 3, 1, 9, 0)),
        [(1, naive(2027, 3, 1, 9, 0, 0))]
    );
}

#[test]
fn unmatched_recurrence_keys_are_reported_back() {
    let data = event_data(&format!("{MASTER}END:VCALENDAR\r\n"));
    let missing = key(2027, 4, 5, 9, 0);
    let present = key(2027, 3, 15, 9, 0);
    let mut keys = AHashSet::from_iter([missing, present]);

    let expansion = data.expand_from_ids(&mut keys, Tz::UTC).expect("expansion");

    assert_eq!(expansion.len(), 1);
    assert_eq!(keys.into_iter().collect::<Vec<_>>(), [missing]);
}
