/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use jmap_proto::method::query::{Filter, FilterWrapper};
use serde::Deserialize;

fn parse_filter(json: &str) -> Vec<Filter<EmailFilter>> {
    let value: Value<PushSubscriptionProperty, PushSubscriptionValue> =
        serde_json::from_str(json).expect("valid filter json");
    FilterWrapper::<EmailFilter>::deserialize(&value)
        .expect("parseable filter")
        .0
}

fn store_and_serialize(
    filter: &[Filter<EmailFilter>],
) -> Value<'static, PushSubscriptionProperty, PushSubscriptionValue> {
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&filter.to_vec()).expect("serialize");
    let archived = rkyv::access::<
        <Vec<Filter<EmailFilter>> as rkyv::Archive>::Archived,
        rkyv::rancor::Error,
    >(&bytes)
    .expect("access archived");
    archived_filter_node(&mut archived.iter().peekable()).unwrap_or(Value::Null)
}

#[test]
fn email_push_filter_round_trips() {
    let mailbox_a = Id::from_parts(0, 10).to_string();
    let mailbox_b = Id::from_parts(0, 20).to_string();

    for json in [
        format!(r#"{{"inMailbox":"{mailbox_a}"}}"#),
        format!(r#"{{"inMailbox":"{mailbox_a}","hasKeyword":"$seen"}}"#),
        format!(
            r#"{{"operator":"OR","conditions":[{{"inMailbox":"{mailbox_a}"}},{{"hasKeyword":"$notify"}}]}}"#
        ),
        format!(
            r#"{{"operator":"AND","conditions":[{{"inMailbox":"{mailbox_a}"}},{{"operator":"NOT","conditions":[{{"hasKeyword":"$junk"}}]}}]}}"#
        ),
        format!(
            r#"{{"operator":"OR","conditions":[{{"subject":"hello"}},{{"operator":"AND","conditions":[{{"from":"alice@example.com"}},{{"inMailboxOtherThan":["{mailbox_a}","{mailbox_b}"]}},{{"minSize":1024}},{{"hasAttachment":true}}]}}]}}"#
        ),
    ] {
        let parsed = parse_filter(&json);
        let serialized = store_and_serialize(&parsed);
        let round_tripped = FilterWrapper::<EmailFilter>::deserialize(&serialized)
            .expect("reparseable filter")
            .0;

        assert_eq!(parsed, round_tripped, "filter did not round-trip: {json}");
    }
}

#[test]
fn empty_filter_serializes_to_null() {
    assert!(matches!(store_and_serialize(&[]), Value::Null));
}
