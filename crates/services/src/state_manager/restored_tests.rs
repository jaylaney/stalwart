/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::PushBatch;
use common::ipc::{EmailPush, PushNotification};
use jmap_proto::types::state::State;
use types::{
    id::Id,
    type_state::{DataType, StateChange},
};
use utils::map::bitmap::Bitmap;

fn state_change<const N: usize>(change_id: u64, types: [DataType; N]) -> PushNotification {
    PushNotification::StateChange(StateChange {
        account_id: 1,
        change_id,
        types: Bitmap::from_iter(types),
    })
}

fn email_push(email_id: u32) -> PushNotification {
    PushNotification::EmailPush(EmailPush {
        account_id: 1,
        email_id,
        change_id: email_id.into(),
    })
}

#[test]
fn batch_keeps_newest_state_per_type() {
    let mut batch = PushBatch::default();
    batch.push(state_change(5, [DataType::Email]));
    batch.push(state_change(7, [DataType::Email, DataType::Mailbox]));
    batch.push(state_change(6, [DataType::Mailbox]));

    let mut failed = PushBatch::default();
    failed.push(state_change(
        4,
        [DataType::Email, DataType::Mailbox, DataType::Thread],
    ));
    batch.merge_failed(failed);

    let states = batch.state_changes.get(&Id::from(1u32)).unwrap();
    assert_eq!(states.len(), 3);
    assert_eq!(states.get(&DataType::Email), Some(&State::Exact(7)));
    assert_eq!(states.get(&DataType::Mailbox), Some(&State::Exact(7)));
    assert_eq!(states.get(&DataType::Thread), Some(&State::Exact(4)));

    assert!(!batch.is_empty());
    batch.clear();
    assert!(batch.is_empty());
}

#[test]
fn failed_notifications_are_retried_first() {
    let mut batch = PushBatch::default();
    batch.push(email_push(3));

    let mut failed = PushBatch::default();
    failed.push(email_push(1));
    failed.push(email_push(2));
    batch.merge_failed(failed);

    let email_ids = batch
        .notifications
        .iter()
        .filter_map(|notification| match notification {
            PushNotification::EmailPush(email_push) => Some(email_push.email_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(email_ids, [1, 2, 3]);
    assert!(batch.state_changes.is_empty());
}
