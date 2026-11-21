/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{ExpectedState, JMAPIntermediateState, State, ZaCalendarOrigin};
use crate::request::{CopyRequestMethod, Request, RequestMethod, SetRequestMethod};
use encodify::base32::STALWART;
use serde_json::json;
use types::ChangeId;

#[test]
fn test_state_id() {
    for id in [
        State::new_initial(),
        State::new_exact(1),
        State::new_exact(12345678),
        State::new_exact(ChangeId::MAX),
        State::new_intermediate(0, 0, 1),
        State::new_intermediate(1024, 2048, 100),
        State::new_intermediate(12345678, 87654321, 1),
        State::new_intermediate(0, 0, 12345678),
        State::new_intermediate(0, 87654321, 12345678),
        State::new_intermediate(12345678, 87654321, 1),
        State::new_intermediate(12345678, 87654321, 12345678),
        State::new_intermediate(ChangeId::MAX, ChangeId::MAX, ChangeId::MAX as usize),
    ] {
        assert_eq!(State::parse(&id.to_string()).unwrap(), id);
    }
}

#[test]
fn test_state_zero_change_id_is_initial() {
    assert_eq!(
        State::parse(&State::new_exact(0).to_string()).unwrap(),
        State::Initial
    );
    assert_eq!(State::from(None), State::Initial);
}

// Release 6 spec 6.8 (D29): the transient key-calendar continuation `z`.

fn za_state(origin: ZaCalendarOrigin, from_id: u64, to_id: u64, items_sent: usize) -> State {
    State::ZaCalendarIntermediate {
        range: JMAPIntermediateState {
            from_id,
            to_id,
            items_sent,
        },
        origin,
    }
}

fn za_bytes(bytes: &[u8]) -> String {
    format!("z{}", STALWART.display(bytes))
}

fn leb(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            bytes.push(byte);
            return bytes;
        }
        bytes.push(byte | 0x80);
    }
}

/// A normal state slot (`sinceState`), which borrows the string.
fn normal_slot(value: &str) -> serde_json::Result<State> {
    serde_json::from_str(&json!(value).to_string())
}

fn invalid_sentinel() -> State {
    State::Intermediate(JMAPIntermediateState {
        from_id: 0,
        to_id: 0,
        items_sent: 0,
    })
}

#[test]
fn za_calendar_state_round_trips() {
    use ZaCalendarOrigin::{Exact, Initial};

    // Version 1, origin tag, the exact ID for tag 1, from_id, to_id - from_id, items_sent.
    assert_eq!(
        za_state(Exact(41), 42, 50, 3).to_string(),
        za_bytes(&[1, 1, 41, 42, 8, 3])
    );
    assert_eq!(za_state(Exact(41), 42, 50, 3).to_string(), "zaeasskqiam");
    assert_eq!(
        za_state(Initial, 42, 50, 3).to_string(),
        za_bytes(&[1, 0, 42, 8, 3])
    );
    assert_eq!(za_state(Initial, 42, 50, 3).to_string(), "zaeacucad");

    let max = ChangeId::MAX;
    for state in [
        za_state(Initial, 0, 0, 1),
        za_state(Initial, 42, 50, 3),
        za_state(Initial, 0, max, usize::MAX),
        za_state(Initial, max, max, usize::MAX),
        za_state(Exact(1), 1, 1, 1),
        za_state(Exact(41), 42, 50, 8),
        za_state(Exact(41), 45, 60, 8),
        za_state(Exact(max), 0, max, usize::MAX),
        za_state(Exact(max), max, max, usize::MAX),
    ] {
        let encoded = state.to_string();
        assert!(encoded.starts_with('z'), "{encoded}");
        assert_eq!(State::parse(&encoded), Some(state.clone()), "{encoded}");
        assert_eq!(normal_slot(&encoded).unwrap(), state);
        assert_eq!(serde_json::to_value(&state).unwrap(), json!(encoded));
        let State::ZaCalendarIntermediate { range, .. } = &state else {
            unreachable!()
        };
        assert_eq!(state.get_change_id(), range.to_id);
    }

    // The widest encodable state, 41 bytes, is within the 42-byte budget (two one-byte
    // fields and four ten-byte values; `from_id + delta` cannot hold two ten-byte values).
    let widest = za_state(Exact(max), 1 << 63, max, usize::MAX);
    let encoded = widest.to_string();
    assert_eq!(encoded.len(), 1 + (41 * 8usize).div_ceil(5));
    assert!(encoded.len() <= 1 + (42 * 8usize).div_ceil(5));
    assert_eq!(State::parse(&encoded), Some(widest));
    // The origin is kept apart from the range: same range, distinct origins.
    assert_ne!(
        za_state(Initial, 42, 50, 1).to_string(),
        za_state(Exact(41), 42, 50, 1).to_string()
    );
}

#[test]
fn za_calendar_state_rejects_malformed() {
    let overflow = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f];
    let unterminated = [0x80; 10];
    let mut cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("version 0", vec![0, 0, 42, 8, 3]),
        ("version 2", vec![2, 0, 42, 8, 3]),
        ("tag 2", vec![1, 2, 42, 8, 3]),
        ("tag 2 with id", vec![1, 2, 41, 42, 8, 3]),
        ("missing tag", vec![1]),
        ("missing from", vec![1, 0]),
        ("missing delta", vec![1, 0, 42]),
        ("missing count", vec![1, 0, 42, 8]),
        ("missing exact", vec![1, 1]),
        ("exact missing count", vec![1, 1, 41, 42, 8]),
        ("trailing initial", vec![1, 0, 42, 8, 3, 7]),
        ("trailing exact", vec![1, 1, 41, 42, 8, 3, 0]),
        ("zero exact", vec![1, 1, 0, 42, 8, 3]),
        ("zero count initial", vec![1, 0, 42, 8, 0]),
        ("zero count exact", vec![1, 1, 41, 42, 8, 0]),
    ];
    let mut push = |name, parts: &[&[u8]]| cases.push((name, parts.concat()));
    push("overflowing version", &[&overflow, &[0, 42, 8, 3]]);
    push("overflowing tag", &[&[1], &overflow, &[42, 8, 3]]);
    push("overflowing exact", &[&[1, 1], &overflow, &[42, 8, 3]]);
    push("overflowing from", &[&[1, 0], &overflow, &[8, 3]]);
    push("overflowing delta", &[&[1, 0, 42], &overflow, &[3]]);
    push("overflowing count", &[&[1, 0, 42, 8], &overflow]);
    push("unterminated from", &[&[1, 0], &unterminated]);
    push("unterminated count", &[&[1, 0, 42, 8], &unterminated]);
    let max = leb(ChangeId::MAX);
    push("overflowing range end", &[&[1, 0], &max, &[1, 3]]);
    push(
        "overflowing range end exact",
        &[&[1, 1, 41], &leb(2), &max, &[3]],
    );

    for (name, bytes) in &cases {
        let encoded = za_bytes(bytes);
        assert_eq!(State::parse(&encoded), None, "{name}: {encoded}");
        assert!(normal_slot(&encoded).is_err(), "{name}");
        assert_eq!(
            serde_json::from_value::<ExpectedState>(json!(encoded))
                .unwrap()
                .0,
            Some(invalid_sentinel()),
            "{name}"
        );
    }

    let valid = za_bytes(&[1, 0, 42, 8, 3]);
    assert!(State::parse(&valid).is_some());
    // A symbol that completes no byte, a symbol that does, and a byte outside the alphabet.
    for trailing in ["a", "aa", "!", "=", " "] {
        let encoded = format!("{valid}{trailing}");
        assert_eq!(State::parse(&encoded), None, "{encoded:?}");
    }
    for prefix in ["Z", "zz"] {
        let encoded = format!("{prefix}{}", &valid[1..]);
        assert_eq!(State::parse(&encoded), None, "{encoded:?}");
    }
}

#[test]
fn za_calendar_state_legacy_fixtures() {
    let max = ChangeId::MAX;
    for (state, encoded) in [
        (State::Initial, "n"),
        (State::new_exact(1), "sae"),
        (State::new_exact(12345678), "sz9bpcbi"),
        (State::new_exact(max), "s33333333333333yb"),
        (State::new_intermediate(0, 0, 1), "raaaac"),
        (State::new_intermediate(1024, 2048, 100), "rqaeiacde"),
        (
            State::new_intermediate(max, max, max as usize),
            "r33333333333333ybad33333333333333ae",
        ),
    ] {
        assert_eq!(state.to_string(), encoded);
        assert_eq!(State::parse(encoded), Some(state.clone()));
        assert_eq!(
            serde_json::from_value::<ExpectedState>(json!(encoded))
                .unwrap()
                .0,
            Some(state)
        );
    }
    // Zero exact IDs stay Initial; legacy `r` keeps its saturating range end and its
    // tolerance of trailing input.
    assert_eq!(State::parse("saa"), Some(State::Initial));
    assert_eq!(State::new_exact(0).to_string(), "saa");
    assert_eq!(State::new_exact(0).get_change_id(), 0);
    assert_eq!(State::Initial.get_change_id(), ChangeId::MAX);
    let saturating = format!(
        "r{}",
        STALWART.display(&[leb(max), leb(1), leb(1)].concat())
    );
    assert_eq!(
        State::parse(&saturating),
        Some(State::new_intermediate(max, max, 1))
    );
    assert_eq!(
        State::parse("raaaacaa"),
        Some(State::new_intermediate(0, 0, 1))
    );
    assert_eq!(State::parse("raaaaa"), None);
    assert_eq!(State::parse("x"), None);
}

#[test]
fn za_calendar_state_expected_state_mismatch() {
    let valid_initial = za_state(ZaCalendarOrigin::Initial, 42, 50, 3).to_string();
    let valid_exact = za_state(ZaCalendarOrigin::Exact(41), 42, 50, 3).to_string();
    let malformed = za_bytes(&[1, 1, 0, 42, 8, 3]);
    let sentinel = Some(invalid_sentinel());

    for value in [
        valid_initial.as_str(),
        valid_exact.as_str(),
        &malformed,
        "garbage",
    ] {
        assert_eq!(
            serde_json::from_value::<ExpectedState>(json!(value))
                .unwrap()
                .0,
            sentinel,
            "{value}"
        );
        let request = json!({
            "using": ["urn:ietf:params:jmap:core"],
            "methodCalls": [
                ["CalendarEvent/set", { "accountId": "a", "ifInState": value }, "set"],
                ["Email/import", { "accountId": "a", "ifInState": value, "emails": {} }, "import"],
                ["CalendarEvent/copy", {
                    "fromAccountId": "b", "accountId": "a", "create": {},
                    "ifInState": value, "ifFromInState": value, "destroyFromIfInState": value
                }, "copy"]
            ]
        })
        .to_string();
        let request = Request::parse(request.as_bytes(), 10, 10240).unwrap();
        let [set, import, copy] = request.method_calls.as_slice() else {
            panic!("three calls");
        };
        let RequestMethod::Set(SetRequestMethod::CalendarEvent(set)) = &set.method else {
            panic!("{value}: {:?}", set.method);
        };
        assert_eq!(set.if_in_state, sentinel, "{value}");
        let RequestMethod::ImportEmail(import) = &import.method else {
            panic!("{value}: {:?}", import.method);
        };
        assert_eq!(import.if_in_state, sentinel, "{value}");
        let RequestMethod::Copy(CopyRequestMethod::CalendarEvent(copy)) = &copy.method else {
            panic!("{value}: {:?}", copy.method);
        };
        assert_eq!(copy.if_in_state, sentinel, "{value}");
        assert_eq!(copy.if_from_in_state, sentinel, "{value}");
        assert_eq!(copy.destroy_from_if_in_state, sentinel, "{value}");
    }

    // Absent stays absent; a valid ordinary state is still the state.
    assert_eq!(
        serde_json::from_value::<ExpectedState>(json!(null))
            .unwrap()
            .0,
        None
    );
    assert_eq!(
        serde_json::from_value::<ExpectedState>(json!("sae"))
            .unwrap()
            .0,
        Some(State::Exact(1))
    );
}
