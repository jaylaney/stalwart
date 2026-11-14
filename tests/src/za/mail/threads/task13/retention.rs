/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::utils::za::{HookParked, za_unlock};
use common::za::{search::seal::za_search_key, test_hooks::set_search_page_pause};
use email::cache::MessageCacheFetch;
use store::{
    write::{BatchBuilder, SetValue},
    za_search::manifest::{ManifestFill, ManifestValue, ManifestWrite},
};
use types::{collection::Collection, field::PrincipalField};

pub(super) async fn test(test: &mut TestServer) {
    for (name, initial) in [
        ("keymail-task13-retention-zero@example.com", true),
        ("keymail-task13-retention-positive@example.com", false),
    ] {
        let key = key_mail_account(test, name, STRONG).await;
        case_retention_advances_between_bound_checks(test, &key, initial).await;
        test.destroy_all_mailboxes(&key).await;
    }

    let key = key_mail_account(test, "keymail-task13-equality@example.com", STRONG).await;
    case_admission_equality(test, &key).await;
    test.destroy_all_mailboxes(&key).await;

    let key = key_mail_account(test, "keymail-task13-shorter-log@example.com", STRONG).await;
    case_shorter_email_log(test, &key).await;
    test.destroy_all_mailboxes(&key).await;
}

async fn case_admission_equality(test: &TestServer, key: &Account) {
    println!("initial continuation at the retention and epoch boundaries");
    let account_id = key.id().document_id();
    let live = three_singletons(key).await;
    let mut pages = Vec::new();
    for (method, expected) in [
        ("Email/changes", live.clone()),
        (
            "Thread/changes",
            live.iter().map(|id| Id::from(id.document_id())).collect(),
        ),
    ] {
        let first = changes(key, method, &State::Initial, Some(1)).await;
        let (token, end, item) = first_page(response(&first, method));
        assert_eq!(item, expected[0]);
        pages.push((method, token, end, expected));
    }
    let end = pages[0].2;
    assert_eq!(pages[1].2, end);
    let before = composition::manifest(test, key).await;
    let bound = test
        .server
        .truncate_change_log(account_id, SyncCollection::Email.into(), 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bound, end);
    assert_eq!(retained_bound(test, key).await, end);
    for (method, token, end, expected) in &pages {
        assert_eq!(
            rest_at(key, method, token.clone(), *end).await,
            expected[1..]
        );
    }
    let after = composition::manifest(test, key).await;
    assert_eq!(before.indexed_to, after.indexed_to);
    assert_eq!(before.epoch, after.epoch);

    // Exercise admission at a validated epoch without invoking Task 14's
    // identity-loss recovery. Seal only the epoch of the existing manifest.
    let mut epoch = after;
    epoch.epoch = end;
    let keys = za_unlock(test, account_id, key.secret()).await;
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .set(
            PrincipalField::ZeroAccessSearch,
            SetValue::serializable(ManifestWrite::for_account(
                ManifestValue::Index(epoch),
                za_search_key(keys.swk()),
                ManifestFill::default(),
                account_id,
            )),
        );
    test.server.commit_batch(batch).await.unwrap();
    assert_eq!(composition::manifest(test, key).await.epoch, end);
    for (method, token, end, expected) in pages {
        assert_eq!(rest_at(key, method, token, end).await, expected[1..]);
        let exact = State::new_exact(end);
        let empty = changes(key, method, &exact, None).await;
        let empty = response(&empty, method);
        assert_eq!(state(empty, "newState"), exact);
        assert!(
            ["created", "updated", "destroyed"]
                .iter()
                .all(|kind| ids(empty, kind).is_empty())
        );
    }
}

async fn case_retention_advances_between_bound_checks(
    test: &TestServer,
    key: &Account,
    initial: bool,
) {
    println!("retention advances past the admitted cache, from zero={initial}");
    let account_id = key.id().document_id();
    let subject = "retained paging race";
    let root = message_with(subject, "root A");
    let other = message_with(subject, "root D");
    let a = import(key, root.clone(), RECEIVED).await;
    let d = import(key, other.clone(), RECEIVED).await;
    let u = import(
        key,
        message_with("retained paging unrelated", "U"),
        RECEIVED,
    )
    .await;
    assert_ne!(a.prefix_id(), d.prefix_id());
    let (s, _) = query(key, false).await;
    let since = if initial { State::Initial } else { s.clone() };
    if !initial {
        flag_seen(key, u).await;
        let bridge = with_references(
            message_with("Re: retained paging race", "B"),
            &[message_id(&root), message_id(&other)],
        );
        import(key, bridge, RECEIVED).await;
    }
    let (last, current) = query(key, false).await;
    let first = changes(key, "Email/changes", &since, Some(1)).await;
    let first = response(&first, "Email/changes");
    let token = state(first, "newState");
    let State::Intermediate(page) = &token else {
        panic!("expected continuation: {first}")
    };
    let (s, e) = (page.from_id, page.to_id);
    assert_eq!(e, exact(&last));
    assert_eq!(page.items_sent, 1);
    assert_eq!(s == 0, initial);
    if !initial {
        assert_eq!(ids(first, "updated"), [u]);
        let moved = by_document(&current, d);
        assert_ne!(moved, d);
        assert_eq!(moved.prefix_id(), a.prefix_id());
        let log = range(test, key, SyncCollection::Email, s, e).await;
        assert!(contains(&log, Change::UpdateItem(u.id())));
        assert!(
            log.records
                .iter()
                .all(|(_, changes)| changes.iter().all(|change| change
                    .item_id()
                    .is_none_or(|id| id as u32 != d.document_id())))
        );
        let reference = changes(key, "Email/changes", &token, None).await;
        assert_eq!(ids(response(&reference, "Email/changes"), "destroyed"), [d]);
    }
    let c = test
        .server
        .get_cached_messages(account_id)
        .await
        .unwrap()
        .last_change_id;
    let b_before = retained_bound(test, key).await;
    assert!(b_before <= if initial { e } else { s });
    assert!(s <= e && e <= c);
    let reader = key.clone();
    let parked = HookParked::start(account_id, set_search_page_pause, async move {
        changes(&reader, "Email/changes", &token, Some(1)).await
    })
    .await;

    // A real flag write advances Email's log without publishing a revision.
    // Truncation replaces this new record, beyond both bounded reads.
    flag_seen(key, a).await;
    let b_after = test
        .server
        .truncate_change_log(account_id, SyncCollection::Email.into(), 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retained_bound(test, key).await, b_after);
    assert!(c < b_after, "{c} < {b_after}");
    for (from, to) in [(s, e), (e, c)] {
        let log = range(test, key, SyncCollection::Email, from, to).await;
        assert!(!log.truncated && log.records.is_empty(), "{log:?}");
    }
    let reply = parked.finish().await;
    assert_truncation(&reply);
}

fn assert_truncation(reply: &JmapResponse) {
    let error = response(reply, "error");
    assert_eq!(error["type"], "cannotCalculateChanges", "{error}");
    assert_eq!(error["description"], "Change log is truncated", "{error}");
}

async fn case_shorter_email_log(test: &TestServer, key: &Account) {
    println!("Thread/changes uses the shorter of the two logs");
    let account_id = key.id().document_id();
    let live = three_singletons(key).await;
    let since = thread_state(key).await;
    for email in live {
        flag_seen(key, email).await;
    }
    let (latest, _) = query(key, false).await;
    let bound = test
        .server
        .truncate_change_log(account_id, SyncCollection::Email.into(), 1)
        .await
        .unwrap()
        .unwrap();
    assert!(bound > exact(&since));
    assert_eq!(
        test.server
            .store()
            .za_log_first_retained(account_id, SyncCollection::Thread)
            .await
            .unwrap(),
        None
    );
    let lost = changes(key, "Thread/changes", &since, None).await;
    assert_truncation(&lost);

    let empty = changes(key, "Thread/changes", &latest, None).await;
    let empty = response(&empty, "Thread/changes");
    assert_eq!(state(empty, "newState"), latest);
    assert!(
        ["created", "updated", "destroyed"]
            .iter()
            .all(|kind| ids(empty, kind).is_empty())
    );
}
