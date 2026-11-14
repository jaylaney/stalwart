/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use email::mailbox::INBOX_ID;
use futures::StreamExt;
use jmap_client::{
    DataType,
    event_source::{Changes, PushNotification},
    mailbox::Role,
};
use jmap_proto::types::state::State;
use std::time::Duration;
use store::{
    ahash::AHashSet,
    query::log::{Change, PartialChange},
};
use tokio::sync::mpsc;
use types::{collection::SyncCollection, id::Id};

use crate::utils::{server::TestServer, smtp::SmtpConnection, za::key_accounts_mode};

pub async fn test(test: &TestServer) {
    println!("Running EventSource tests...");

    // Create test account
    let account = test.account("jdoe@example.com");
    let client = account.jmap_client().await;

    let mut changes = client
        .event_source(None::<Vec<_>>, false, 1.into(), None)
        .await
        .unwrap();

    let (event_tx, mut event_rx) = mpsc::channel::<Changes>(100);

    tokio::spawn(async move {
        while let Some(change) = changes.next().await {
            if let Err(_err) = event_tx
                .send(match change.unwrap() {
                    PushNotification::StateChange(changes) => changes,
                    PushNotification::CalendarAlert(_) => unreachable!(),
                })
                .await
            {
                //println!("Error sending event: {}", _err);
                break;
            }
        }
    });

    assert_ping(&mut event_rx).await;

    // Create mailbox and expect state change
    let mailbox_id = client
        .mailbox_create("EventSource Test", None::<String>, Role::None)
        .await
        .unwrap()
        .take_id();
    assert_state(&mut event_rx, account.id_string(), &[DataType::Mailbox]).await;

    // Multiple changes should be grouped and delivered in intervals
    for num in 0..5 {
        client
            .mailbox_update_sort_order(&mailbox_id, num)
            .await
            .unwrap();
    }
    assert_state(&mut event_rx, account.id_string(), &[DataType::Mailbox]).await;
    assert_ping(&mut event_rx).await; // Pings are only received in cfg(test)

    // Ingest email and expect state change
    let mut lmtp = SmtpConnection::connect().await;
    lmtp.ingest(
        "bill@example.com",
        &["jdoe@example.com"],
        concat!(
            "From: bill@example.com\r\n",
            "To: jdoe@example.com\r\n",
            "Subject: TPS Report\r\n",
            "\r\n",
            "I'm going to need those TPS reports ASAP. ",
            "So, if you could do that, that'd be great."
        ),
    )
    .await;
    lmtp.quit().await;

    assert_state(
        &mut event_rx,
        account.id_string(),
        &[
            DataType::EmailDelivery,
            DataType::Email,
            DataType::Thread,
            DataType::Mailbox,
        ],
    )
    .await;
    assert_ping(&mut event_rx).await;

    // Import email and expect EmailDelivery state change
    let account_id = account.id().document_id();
    let before_import = if key_accounts_mode() {
        Some(email_watermark(test, account_id).await)
    } else {
        None
    };
    let imported = client
        .email_import(
            concat!(
                "From: bill@example.com\r\n",
                "To: jdoe@example.com\r\n",
                "Subject: TPS Report (imported)\r\n",
                "\r\n",
                "Did you get the memo?"
            )
            .as_bytes()
            .to_vec(),
            [Id::from(INBOX_ID).to_string()],
            None::<Vec<&str>>,
            None,
        )
        .await
        .unwrap();
    if let Some(before_import) = before_import {
        let after_import = email_watermark(test, account_id).await;
        let imported_document = imported
            .id()
            .expect("the import returned a created Email ID")
            .parse::<Id>()
            .unwrap()
            .document_id();
        let (delivery, triples) = import_states(
            test,
            account_id,
            imported_document,
            before_import,
            after_import,
        )
        .await;
        assert_import_state(
            &mut event_rx,
            account.id_string(),
            &delivery,
            &triples,
            Duration::from_secs(2),
        )
        .await;
    } else {
        assert_state(
            &mut event_rx,
            account.id_string(),
            &[
                DataType::EmailDelivery,
                DataType::Email,
                DataType::Thread,
                DataType::Mailbox,
            ],
        )
        .await;
        assert_ping(&mut event_rx).await;
    }

    // Destroy mailbox
    client.mailbox_destroy(&mailbox_id, true).await.unwrap();
    assert_state(&mut event_rx, account.id_string(), &[DataType::Mailbox]).await;

    // Destroy Inbox
    client
        .mailbox_destroy(&Id::from(INBOX_ID).to_string(), true)
        .await
        .unwrap();
    assert_state(
        &mut event_rx,
        account.id_string(),
        &[DataType::Email, DataType::Thread, DataType::Mailbox],
    )
    .await;
    assert_ping(&mut event_rx).await;
    assert_ping(&mut event_rx).await;

    test.destroy_all_mailboxes(account).await;
    test.assert_is_empty().await;
}

async fn email_watermark(test: &TestServer, account_id: u32) -> u64 {
    test.server
        .store()
        .get_last_change_id(account_id, SyncCollection::Email.into())
        .await
        .unwrap()
        .expect("the EventSource fixture has an Email change log")
}

async fn import_states(
    test: &TestServer,
    account_id: u32,
    imported_document: u32,
    before: u64,
    after: u64,
) -> (String, Vec<String>) {
    let store = test.server.store();
    // Include the preceding LMTP insert to identify the only possible
    // pre-pass witness. The import uses the Inbox, which already exists.
    assert!(before > 0 && after > before);
    let email = store
        .za_changes_records_range(account_id, SyncCollection::Email, before - 1, after)
        .await
        .unwrap();
    let thread = store
        .za_changes_records_range(account_id, SyncCollection::Thread, before, after)
        .await
        .unwrap();
    assert!(!email.truncated && !thread.truncated);
    let ((lmtp_state, lmtp_changes), records) = email.records.split_first().unwrap();
    assert_eq!(*lmtp_state, before);
    let Change::InsertItem(lmtp_id) = only_item(lmtp_changes) else {
        panic!("expected the preceding LMTP insert: {lmtp_changes:?}");
    };
    let lmtp_document = Id::new(lmtp_id).document_id();

    let mut insert = None;
    let mut triples = Vec::new();
    for (change_id, changes) in records {
        assert!(
            changes.contains(&Change::UpdateContainerPartial(
                u64::from(INBOX_ID),
                PartialChange::PROPERTIES,
            )),
            "expected an Inbox change at {change_id}: {changes:?}",
        );
        let thread_change = match only_item(changes) {
            Change::InsertItem(id) => {
                assert_eq!(Id::new(id).document_id(), imported_document);
                assert!(
                    insert.replace(*change_id).is_none(),
                    "duplicate import insert"
                );
                Change::InsertContainer(u64::from(imported_document))
            }
            Change::UpdateItem(id) => {
                let document = Id::new(id).document_id();
                // A post-pass can index the LMTP row too if the pre-pass
                // did not index it. The witness is the lowest indexed ID.
                assert!(
                    document == lmtp_document
                        || (insert.is_some() && document == imported_document),
                    "unexpected publication witness at {change_id}: {changes:?}",
                );
                Change::UpdateContainerPartial(u64::from(document), PartialChange::PROPERTIES)
            }
            change => panic!("unexpected Email change at {change_id}: {change:?}"),
        };
        assert!(
            thread
                .records
                .iter()
                .any(|(id, changes)| { id == change_id && changes.as_slice() == [thread_change] }),
            "no matching Thread change at {change_id}: {:?}",
            thread.records,
        );
        triples.push(State::Exact(*change_id).to_string());
    }
    let insert = insert.expect("missing the import's durable InsertItem");
    (State::Exact(insert).to_string(), triples)
}

fn only_item(changes: &[Change]) -> Change {
    let mut items = changes.iter().copied().filter(Change::is_item_change);
    let item = items.next().expect("missing an Email item change");
    assert!(
        items.next().is_none(),
        "unexpected Email item changes: {changes:?}"
    );
    item
}

async fn assert_import_state(
    event_rx: &mut mpsc::Receiver<Changes>,
    account_id: &str,
    delivery: &str,
    triples: &[String],
    wait: Duration,
) {
    let latest = triples.last().unwrap();
    let deadline = tokio::time::Instant::now() + wait;
    let mut saw_delivery = false;
    let mut saw_latest = false;
    loop {
        let mut frame = tokio::time::timeout_at(deadline, event_rx.recv())
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "import timed out waiting for operation coverage and ping. \
                     Delivery {delivery} seen {saw_delivery}, latest state {latest} seen {saw_latest}"
                )
            })
            .expect("EventSource closed during import");
        assert_eq!(frame.changed_accounts().count(), 1, "{frame:?}");
        if frame
            .changes("ping")
            .is_some_and(|mut types| types.next().is_none())
        {
            if saw_delivery && saw_latest {
                break;
            }
            continue;
        }

        let mut changes = frame
            .account_changes(account_id)
            .unwrap_or_else(|| panic!("missing import account {account_id}: {frame:?}"));
        assert!(!changes.is_empty(), "empty import state frame");
        if let Some(state) = changes.remove(&DataType::EmailDelivery) {
            assert_eq!(state, delivery, "unexpected import delivery state");
            assert!(!saw_delivery, "duplicate import delivery {delivery}");
            saw_delivery = true;
        }
        if !changes.is_empty() {
            let email = changes.get(&DataType::Email);
            assert!(
                changes.len() == 3
                    && email.is_some()
                    && changes.get(&DataType::Thread) == email
                    && changes.get(&DataType::Mailbox) == email,
                "expected Email, Thread and Mailbox at the same state: {changes:?}",
            );
            let state = email.unwrap();
            assert!(
                triples.contains(state),
                "unexpected import triple state {state}, expected one of {triples:?}"
            );
            saw_latest |= state == latest;
        }
    }
    println!("Keyed EventSource import received delivery {delivery} and latest state {latest}");
}

async fn assert_state(
    event_rx: &mut mpsc::Receiver<Changes>,
    account_id: &str,
    state: &[DataType],
) {
    match tokio::time::timeout(Duration::from_millis(700), event_rx.recv()).await {
        Ok(Some(changes)) => {
            assert_eq!(
                changes
                    .changes(account_id)
                    .unwrap()
                    .map(|x| x.0)
                    .collect::<AHashSet<&DataType>>(),
                state.iter().collect::<AHashSet<&DataType>>()
            );
        }
        result => {
            panic!("Timeout waiting for event {:?}: {:?}", state, result);
        }
    }
}

async fn assert_ping(event_rx: &mut mpsc::Receiver<Changes>) {
    match tokio::time::timeout(Duration::from_millis(1100), event_rx.recv()).await {
        Ok(Some(changes)) => {
            assert!(changes.changes("ping").is_some(),);
        }
        _ => {
            panic!("Did not receive ping.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: &str = "account";
    const DELIVERY: &str = "sInsert";
    const EARLIER: &str = "sBefore";
    const LATEST: &str = "sAfter";

    fn frame(account: &str, states: &[(DataType, &str)]) -> Changes {
        let states = states
            .iter()
            .map(|(kind, state)| (kind.clone(), (*state).to_owned()))
            .collect();
        Changes::new(None, [(account.to_owned(), states)].into_iter().collect())
    }

    fn triple(state: &str, delivery: Option<&str>) -> Changes {
        let mut states = vec![
            (DataType::Email, state),
            (DataType::Thread, state),
            (DataType::Mailbox, state),
        ];
        if let Some(delivery) = delivery {
            states.push((DataType::EmailDelivery, delivery));
        }
        frame(ACCOUNT, &states)
    }

    fn ping() -> Changes {
        frame("ping", &[])
    }

    async fn check(frames: impl IntoIterator<Item = Changes>) -> mpsc::Receiver<Changes> {
        let (tx, mut rx) = mpsc::channel(16);
        for frame in frames {
            tx.try_send(frame).unwrap();
        }
        let triples = [EARLIER, DELIVERY, LATEST].map(str::to_owned);
        assert_import_state(
            &mut rx,
            ACCOUNT,
            DELIVERY,
            &triples,
            Duration::from_millis(100),
        )
        .await;
        rx
    }

    #[tokio::test]
    async fn split_and_combined_import_wait_for_the_ping_after_coverage() {
        for notifications in [
            vec![
                frame(ACCOUNT, &[(DataType::EmailDelivery, DELIVERY)]),
                triple(LATEST, None),
            ],
            vec![triple(LATEST, Some(DELIVERY))],
        ] {
            let sentinel = frame(ACCOUNT, &[(DataType::Mailbox, "sNext")]);
            let mut rx = check(
                [ping(), triple(EARLIER, None), ping()]
                    .into_iter()
                    .chain(notifications)
                    .chain([ping(), sentinel.clone()]),
            )
            .await;
            assert_eq!(rx.try_recv().unwrap(), sentinel);
        }
    }

    #[tokio::test]
    #[should_panic(expected = "Delivery sInsert seen false")]
    async fn import_requires_delivery_even_after_ping() {
        check([triple(LATEST, None), ping()]).await;
    }

    #[tokio::test]
    #[should_panic(expected = "latest state sAfter seen false")]
    async fn pre_import_publication_cannot_satisfy_latest_state() {
        check([triple(EARLIER, Some(DELIVERY)), ping()]).await;
    }

    #[tokio::test]
    #[should_panic(expected = "unexpected import delivery state")]
    async fn import_rejects_stale_delivery() {
        check([triple(LATEST, Some(EARLIER))]).await;
    }

    #[tokio::test]
    #[should_panic(expected = "duplicate import delivery")]
    async fn import_rejects_duplicate_delivery_before_the_ping() {
        check([
            triple(LATEST, Some(DELIVERY)),
            frame(ACCOUNT, &[(DataType::EmailDelivery, DELIVERY)]),
            ping(),
        ])
        .await;
    }

    #[tokio::test]
    #[should_panic(expected = "missing import account")]
    async fn import_rejects_other_accounts_including_nonempty_ping() {
        check([frame("ping", &[(DataType::EmailDelivery, DELIVERY)])]).await;
    }

    #[tokio::test]
    #[should_panic(expected = "expected Email, Thread and Mailbox at the same state")]
    async fn import_rejects_an_unrelated_type() {
        check([frame(ACCOUNT, &[(DataType::EmailSubmission, LATEST)])]).await;
    }

    #[tokio::test]
    #[should_panic(expected = "unexpected import triple state")]
    async fn import_rejects_unrecognized_commits() {
        check([triple("sOther", Some(DELIVERY))]).await;
    }
}
