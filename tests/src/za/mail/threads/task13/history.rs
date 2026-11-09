/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::{
    utils::za::{
        HookParked, log_records_since, pop3_delete, run_search_pass, search_config_guard, za_unlock,
    },
    za::mail::reply_to,
};
use common::za::test_hooks::set_search_view_pause;
use email::{
    cache::MessageCacheFetch,
    message::za::search::pass::{PassOutcome, ZaSearchPass},
};

pub(super) async fn test(test: &mut TestServer) {
    let key = key_mail_account(test, "keymail-task13-backlog@example.com", STRONG).await;
    case_backlog_and_paged_merges(test, &key).await;
    test.destroy_all_mailboxes(&key).await;

    let key = key_mail_account(test, "keymail-task13-deletions@example.com", STRONG).await;
    case_deletions(test, &key).await;
    test.destroy_all_mailboxes(&key).await;

    let key = key_mail_account(test, "keymail-task13-thread-pages@example.com", STRONG).await;
    case_thread_page_pins_members(test, &key).await;
    test.destroy_all_mailboxes(&key).await;

    let key = key_mail_account(test, "keymail-task13-thread-view@example.com", STRONG).await;
    case_thread_get_on_other_node(test, &key).await;
    test.destroy_all_mailboxes(&key).await;

    let key = key_mail_account(test, "keymail-task13-two-merges@example.com", STRONG).await;
    case_two_successive_merges(test, &key).await;
    test.destroy_all_mailboxes(&key).await;

    for (key_name, plain_name, capped) in [
        (
            "keymail-task13-r16@example.com",
            "plainmail-task13-r16@example.com",
            false,
        ),
        (
            "keymail-task13-r16-cap@example.com",
            "plainmail-task13-r16-cap@example.com",
            true,
        ),
    ] {
        let key = key_mail_account(test, key_name, STRONG).await;
        let plain = test
            .account("admin@example.com")
            .create_user_account(plain_name, STRONG, "Plain mail", &[], user_permissions())
            .await;
        case_grouping_after_deleting_a_reference(test, &key, &plain, capped).await;
        test.destroy_all_mailboxes(&plain).await;
        test.destroy_all_mailboxes(&key).await;
    }

    let key = key_mail_account(test, "keymail-task13-counts@example.com", STRONG).await;
    case_publication_counts_and_modseq(test, &key).await;
    test.destroy_all_mailboxes(&key).await;

    let key = key_mail_account(test, "keymail-task13-empty@example.com", STRONG).await;
    case_empty_account_after_mailbox_change(test, &key).await;
    test.destroy_all_mailboxes(&key).await;
}

#[derive(Default, Debug, PartialEq, Eq)]
struct Delta {
    created: Vec<Id>,
    updated: Vec<Id>,
    destroyed: Vec<Id>,
}

impl Delta {
    fn append(&mut self, page: &Value) -> usize {
        let created = ids(page, "created");
        let updated = ids(page, "updated");
        let destroyed = ids(page, "destroyed");
        let len = created.len() + updated.len() + destroyed.len();
        self.created.extend(created);
        self.updated.extend(updated);
        self.destroyed.extend(destroyed);
        len
    }

    fn read(page: &Value) -> Self {
        let mut delta = Self::default();
        delta.append(page);
        delta
    }

    fn apply(&self, old: Vec<Id>) -> Vec<Id> {
        let mut result = old;
        result.retain(|id| !self.destroyed.contains(id));
        result.extend(self.created.iter().copied());
        let mut unique = result.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            result.len(),
            "duplicate IDs after applying {self:?}"
        );
        unique
    }
}

async fn email_delta(key: &Account, since: &State) -> (State, Delta) {
    let reply = changes(key, "Email/changes", since, None).await;
    let page = response(&reply, "Email/changes");
    assert_eq!(state(page, "oldState"), *since);
    assert_eq!(page["hasMoreChanges"], false);
    (state(page, "newState"), Delta::read(page))
}

async fn case_backlog_and_paged_merges(test: &TestServer, key: &Account) {
    println!("backlog singleton identities and maxChanges paging across later changes");
    let subject = "paged backlog conversation";
    let root = message_with(subject, "root A");
    let other = message_with(subject, "root D");
    let a = import(key, root.clone(), RECEIVED).await;
    let d = import(key, other.clone(), RECEIVED).await;
    assert_ne!(a.prefix_id(), d.prefix_id());

    let config = search_config_guard!(test, pass_cap: 2);
    let mut delivered = Vec::new();
    for body in ["one", "two", "three", "four", "five", "six"] {
        delivered.push(super::super::deliver_doc(test, key, &reply_to(&root, subject, body)).await);
    }
    let first_map = super::super::get(key, Value::Null).await;
    let first_map = response(&first_map, "Email/get");
    let s = state(first_map, "state");
    let old = first_map["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| id(&row["id"]))
        .collect::<Vec<_>>();
    let expected_singletons: Vec<_> = delivered[2..]
        .iter()
        .map(|&doc| Id::from_parts(doc, doc))
        .collect();
    assert_eq!(
        unordered(
            old.iter()
                .copied()
                .filter(|id| id.prefix_id() != a.prefix_id() && *id != d)
                .collect()
        ),
        unordered(expected_singletons.clone())
    );
    let account_id = key.id().document_id();
    let backlog = composition::manifest(test, key).await;
    assert!(backlog.indexed_to.change_id < exact(&s));
    for _ in 0..3 {
        run_search_pass(test, account_id, key.secret()).await;
    }
    drop(config);

    let (e, at_e) = query(key, false).await;
    let indexed = composition::manifest(test, key).await;
    assert!(indexed.indexed_to.change_id >= backlog.indexed_to.change_id);
    assert!(exact(&e) > exact(&s));
    assert!(
        delivered
            .iter()
            .all(|&doc| at_e.contains(&Id::from_parts(a.prefix_id(), doc)))
    );
    let (delta_e, unpaged) = email_delta(key, &s).await;
    assert_eq!(delta_e, e);
    assert_eq!(
        unordered(unpaged.destroyed.clone()),
        unordered(expected_singletons)
    );
    assert_eq!(unpaged.apply(old.clone()), unordered(at_e.clone()));

    let first = changes(key, "Email/changes", &s, Some(1)).await;
    let first = response(&first, "Email/changes");
    let mut delta = Delta::default();
    assert_eq!(delta.append(first), 1);
    let mut page_token = state(first, "newState");
    let first_token = page_token.clone();
    assert_eq!(page_token, State::new_intermediate(exact(&s), exact(&e), 1));
    let bridge = with_references(
        message_with("Re: paged backlog conversation", "bridge B"),
        &[message_id(&root), message_id(&other)],
    );
    import(key, bridge, RECEIVED).await;
    pop3_delete(test, key, delivered[0]).await;
    let (f, at_f) = query(key, false).await;
    assert!(exact(&f) > exact(&e));

    let count = unpaged.created.len() + unpaged.updated.len() + unpaged.destroyed.len();
    for sent in 2..=count {
        let next = changes(key, "Email/changes", &page_token, Some(1)).await;
        let next = response(&next, "Email/changes");
        assert_eq!(state(next, "oldState"), page_token);
        assert_eq!(delta.append(next), 1);
        page_token = state(next, "newState");
        if sent == count {
            assert_eq!(page_token, e);
            assert_eq!(next["hasMoreChanges"], false);
        } else {
            assert_eq!(
                page_token,
                State::new_intermediate(exact(&s), exact(&e), sent)
            );
            assert_eq!(next["hasMoreChanges"], true);
        }
    }
    assert_eq!(delta, unpaged);
    assert_eq!(delta.apply(old), unordered(at_e.clone()));
    assert_ne!(state(first, "newState"), f);

    let (end, subsequent) = email_delta(key, &e).await;
    assert_eq!(end, f);
    assert_eq!(subsequent.apply(at_e), unordered(at_f));
    let State::Intermediate(first_token) = first_token else {
        unreachable!()
    };
    let exhausted = State::new_intermediate(first_token.from_id, first_token.to_id, count);
    let (end, fallback) = email_delta(key, &exhausted).await;
    assert_eq!(end, f);
    assert_eq!(fallback, subsequent);
    let limited = changes(key, "Email/changes", &exhausted, Some(1)).await;
    let limited = response(&limited, "Email/changes");
    assert_eq!(state(limited, "oldState"), exhausted);
    assert_eq!(
        state(limited, "newState"),
        State::new_intermediate(exact(&e), exact(&f), 1)
    );
}

async fn case_deletions(test: &TestServer, key: &Account) {
    println!("raw backlog deletion appears once, and deletion-only passes add no log");
    let account_id = key.id().document_id();
    let root = message_with("delete mapped member", "root");
    let a = import(key, root.clone(), RECEIVED).await;
    let d = import(
        key,
        reply_to(&root, "delete mapped member", "member D"),
        RECEIVED,
    )
    .await;
    assert_eq!(d.prefix_id(), a.prefix_id());
    let (before, _) = query(key, false).await;
    let entry = test.server.inner.cache.za_search.get(account_id).unwrap();
    let pass = entry.pass_lock.lock().await;
    flag_seen(key, a).await;

    let transient = super::super::deliver_doc(
        test,
        key,
        &reply_to(&root, "delete mapped member", "created and deleted"),
    )
    .await;
    let transient = Id::from_parts(transient, transient);
    pop3_delete(test, key, transient.document_id()).await;
    pop3_delete(test, key, d.document_id()).await;
    let loaded = entry.read_loaded().await.unwrap();
    assert_eq!(
        loaded.thread.exists_at(d.document_id(), u64::MAX),
        Some(true)
    );
    drop(loaded);
    let cache = test.server.get_cached_messages(account_id).await.unwrap();
    let deletion = cache.emails.change_id;
    let raw = range(test, key, SyncCollection::Email, exact(&before), deletion).await;
    assert!(contains(&raw, Change::InsertItem(transient.id())));
    assert!(contains(&raw, Change::DeleteItem(transient.id())));
    assert!(raw.records.iter().any(|(_, entries)| {
        entries
            .iter()
            .any(|entry| matches!(entry, Change::DeleteItem(id) if *id as u32 == d.document_id()))
    }));
    let (after, delta) = email_delta(key, &before).await;
    assert_eq!(after, State::new_exact(deletion));
    assert_eq!(
        delta,
        Delta {
            updated: vec![a],
            destroyed: vec![d],
            ..Default::default()
        }
    );
    let (_, empty) = email_delta(key, &after).await;
    assert_eq!(empty, Delta::default());
    let first = changes(key, "Email/changes", &before, Some(1)).await;
    let first = response(&first, "Email/changes");
    assert_eq!(
        Delta::read(first),
        Delta {
            updated: vec![a],
            ..Default::default()
        }
    );
    let pinned = state(first, "newState");
    assert_eq!(pinned, State::new_intermediate(exact(&before), deletion, 1));

    drop(pass);
    let revision = composition::manifest(test, key).await.newest_row;
    assert!(matches!(
        run_search_pass(test, account_id, key.secret()).await,
        PassOutcome::Advanced {
            published: false,
            ..
        }
    ));
    assert_eq!(composition::manifest(test, key).await.newest_row, revision);
    assert_eq!(email_delta(key, &before).await, (after.clone(), delta));
    let continuation = changes(key, "Email/changes", &pinned, Some(1)).await;
    let continuation = response(&continuation, "Email/changes");
    assert_eq!(
        Delta::read(continuation),
        Delta {
            destroyed: vec![d],
            ..Default::default()
        }
    );
    assert_eq!(state(continuation, "newState"), after);
    assert_eq!(continuation["hasMoreChanges"], false);

    for collection in [SyncCollection::Email, SyncCollection::Thread] {
        assert!(
            log_records_since(test, account_id, collection, deletion)
                .await
                .is_empty()
        );
    }
    let (same_state, empty) = email_delta(key, &after).await;
    assert_eq!(same_state, after);
    assert_eq!(empty, Delta::default());

    let thread_before = thread_state(key).await;
    pop3_delete(test, key, a.document_id()).await;
    let last = test
        .server
        .get_cached_messages(account_id)
        .await
        .unwrap()
        .emails
        .change_id;
    assert!(matches!(
        run_search_pass(test, account_id, key.secret()).await,
        PassOutcome::Advanced {
            published: false,
            ..
        }
    ));
    for collection in [SyncCollection::Email, SyncCollection::Thread] {
        assert!(
            log_records_since(test, account_id, collection, last)
                .await
                .is_empty()
        );
    }
    let (_, delta) = email_delta(key, &after).await;
    assert_eq!(
        delta,
        Delta {
            destroyed: vec![a],
            ..Default::default()
        }
    );
    let reply = changes(key, "Thread/changes", &thread_before, None).await;
    let page = response(&reply, "Thread/changes");
    assert_eq!(
        Delta::read(page),
        Delta {
            destroyed: vec![Id::from(a.prefix_id())],
            ..Default::default()
        }
    );
}

async fn case_thread_page_pins_members(test: &TestServer, key: &Account) {
    println!("Thread/changes keeps pinned membership through a last-member deletion and a merge");
    let subject = "thread page history";
    let root = message_with(subject, "root A");
    let other = message_with(subject, "root D");
    let a = import(key, root.clone(), RECEIVED).await;
    let d = import(key, other.clone(), RECEIVED).await;
    let s = thread_state(key).await;
    let x = import(
        key,
        message_with("thread page first new thread", "X"),
        RECEIVED,
    )
    .await;
    let z = import(
        key,
        message_with("thread page deleted after E", "Z"),
        RECEIVED,
    )
    .await;
    let c = import(key, reply_to(&root, subject, "member at E"), RECEIVED).await;
    let e = thread_state(key).await;
    let first = changes(key, "Thread/changes", &s, Some(1)).await;
    let first = response(&first, "Thread/changes");
    assert_eq!(ids(first, "created"), [Id::from(x.prefix_id())]);
    let token = state(first, "newState");
    assert_eq!(token, State::new_intermediate(exact(&s), exact(&e), 1));

    pop3_delete(test, key, z.document_id()).await;
    pop3_delete(test, key, c.document_id()).await;
    let bridge = with_references(
        message_with("Re: thread page history", "later merge witness"),
        &[message_id(&root), message_id(&other)],
    );
    import(key, bridge, RECEIVED).await;
    let f = thread_state(key).await;
    assert!(exact(&f) > exact(&e));

    let second = changes(key, "Thread/changes", &token, Some(1)).await;
    let second = response(&second, "Thread/changes");
    assert_eq!(
        Delta::read(second),
        Delta {
            created: vec![Id::from(z.prefix_id())],
            ..Default::default()
        }
    );
    let token2 = state(second, "newState");
    assert_eq!(token2, State::new_intermediate(exact(&s), exact(&e), 2));
    let last = changes(key, "Thread/changes", &token2, Some(1)).await;
    let last = response(&last, "Thread/changes");
    assert_eq!(
        Delta::read(last),
        Delta {
            updated: vec![Id::from(a.prefix_id())],
            ..Default::default()
        }
    );
    assert_eq!(state(last, "newState"), e);

    let later = changes(key, "Thread/changes", &e, None).await;
    let later = response(&later, "Thread/changes");
    assert_eq!(state(later, "newState"), f);
    assert_eq!(
        unordered(ids(later, "destroyed")),
        unordered(vec![Id::from(z.prefix_id()), Id::from(d.prefix_id())])
    );
    assert_eq!(ids(later, "updated"), [Id::from(a.prefix_id())]);
}

async fn case_thread_get_on_other_node(test: &TestServer, key: &Account) {
    println!("a Thread/get acquired before a remote publication returns its own state");
    let account_id = key.id().document_id();
    let subject = "thread read with remote merge";
    let root = message_with(subject, "root A");
    let other = message_with(subject, "root D");
    let a = import(key, root.clone(), RECEIVED).await;
    let d = import(key, other.clone(), RECEIVED).await;
    let before = thread_state(key).await;
    let remote = super::super::races::separate_publisher(test).await;
    let keys = za_unlock(test, account_id, key.secret()).await;
    let reader = key.clone();
    let parked = HookParked::start(account_id, set_search_view_pause, async move {
        reader
            .jmap_method_call("Thread/get", json!({"accountId": reader.id_string()}))
            .await
    })
    .await;
    let bridge = with_references(
        message_with("Re: thread read with remote merge", "B"),
        &[message_id(&root), message_id(&other)],
    );
    super::super::deliver_doc(test, key, &bridge).await;
    assert!(matches!(
        remote.za_search_pass(account_id, &keys).await.unwrap(),
        PassOutcome::Advanced {
            published: true,
            ..
        }
    ));
    let f = composition::manifest(test, key).await.newest_row.change_id;
    assert!(f > exact(&before));
    let reply = parked.finish().await;
    let reply = response(&reply, "Thread/get");
    assert_eq!(state(reply, "state"), before);
    let lists = reply["list"].as_array().unwrap();
    assert_eq!(lists.len(), 2);
    for id in [a, d] {
        assert!(lists.iter().any(|thread| ids(thread, "emailIds") == [id]));
    }
    let reply = changes(key, "Thread/changes", &before, None).await;
    let page = response(&reply, "Thread/changes");
    assert_eq!(state(page, "newState"), State::new_exact(f));
    assert_eq!(ids(page, "destroyed"), [Id::from(d.prefix_id())]);
    assert_eq!(ids(page, "updated"), [Id::from(a.prefix_id())]);
}

async fn case_empty_account_after_mailbox_change(test: &TestServer, key: &Account) {
    println!("empty account with a trailing mailbox container record");
    let account_id = key.id().document_id();
    assert_eq!(query(key, false).await.0, State::Initial);
    let reply = key
        .jmap_method_call(
            "Mailbox/set",
            json!({"accountId": key.id_string(), "create": {"c": {"name": "container only"}}}),
        )
        .await;
    let made = response(&reply, "Mailbox/set");
    assert!(made["created"]["c"]["id"].is_string(), "{made}");
    let cache = test.server.get_cached_messages(account_id).await.unwrap();
    assert_eq!(cache.emails.change_id, 0);
    assert!(cache.last_change_id > 0);
    let log = range(test, key, SyncCollection::Email, 0, cache.last_change_id).await;
    assert!(
        log.records
            .iter()
            .all(|(_, entries)| entries.iter().all(|change| !change.is_item_change()))
    );

    for (method, args, list) in [
        ("Email/get", json!({"accountId": key.id_string()}), "list"),
        ("Email/query", query_args(key, false), "ids"),
        (
            "Email/changes",
            json!({"accountId": key.id_string(), "sinceState": "n"}),
            "created",
        ),
        ("Thread/get", json!({"accountId": key.id_string()}), "list"),
    ] {
        let reply = key.jmap_method_call(method, args).await;
        assert_eq!(response(&reply, method)[list], json!([]));
    }
    let reply = key
        .jmap_method_call("Mailbox/get", json!({"accountId": key.id_string()}))
        .await;
    assert!(
        response(&reply, "Mailbox/get")["list"]
            .as_array()
            .unwrap()
            .len()
            > 1
    );
}

async fn case_two_successive_merges(test: &TestServer, key: &Account) {
    println!("two merge publications preserve the prefixes at each boundary");
    let subject = "two distinct merge rows";
    let a_raw = message_with(subject, "A");
    let b_raw = message_with(subject, "B");
    let c_raw = message_with(subject, "C");
    let a = import(key, a_raw.clone(), RECEIVED).await;
    let b = import(key, b_raw.clone(), RECEIVED).await;
    let c = import(key, c_raw.clone(), RECEIVED).await;
    assert_ne!(a.prefix_id(), b.prefix_id());
    assert_ne!(a.prefix_id(), c.prefix_id());
    let (s, at_s) = query(key, false).await;

    let first = with_references(
        message_with("Re: two distinct merge rows", "B into A"),
        &[message_id(&a_raw), message_id(&b_raw)],
    );
    import(key, first, RECEIVED).await;
    let (first_state, first) = query(key, false).await;
    assert_eq!(
        composition::manifest(test, key).await.newest_row.change_id,
        exact(&first_state)
    );
    let new_b = by_document(&first, b);
    assert_eq!(new_b.prefix_id(), a.prefix_id());
    assert!(first.contains(&c));
    let (_, delta) = email_delta(key, &s).await;
    assert_eq!(delta.destroyed, [b]);
    assert_eq!(delta.apply(at_s.clone()), unordered(first.clone()));

    flag_seen(key, a).await;
    let (between, _) = query(key, false).await;
    assert!(exact(&first_state) < exact(&between));
    let second = with_references(
        message_with("Re: two distinct merge rows", "C into A"),
        &[message_id(&a_raw), message_id(&c_raw)],
    );
    import(key, second, RECEIVED).await;
    let (last_state, last) = query(key, false).await;
    assert_eq!(
        composition::manifest(test, key).await.newest_row.change_id,
        exact(&last_state)
    );
    assert!(exact(&between) < exact(&last_state));
    let new_c = by_document(&last, c);
    assert_eq!(new_c.prefix_id(), a.prefix_id());

    for after in [first_state, between] {
        let (_, delta) = email_delta(key, &after).await;
        assert_eq!(delta.destroyed, [c]);
        assert_eq!(delta.apply(first.clone()), unordered(last.clone()));
    }
    let (_, delta) = email_delta(key, &s).await;
    assert_eq!(delta.destroyed, [b, c]);
    assert_eq!(delta.apply(at_s), unordered(last));
    assert_eq!(email_delta(key, &last_state).await.1, Delta::default());
}

async fn case_grouping_after_deleting_a_reference(
    test: &TestServer,
    key: &Account,
    plain: &Account,
    capped: bool,
) {
    println!("reference to a POP3-deleted member, capped={capped}");
    let subject = "thread reference disappears";
    let root = message_with(subject, "A");
    let middle = reply_to(&root, subject, "D");
    let next = reply_to(&middle, subject, "N");
    let plain_a = import(plain, root.clone(), RECEIVED).await;
    let plain_d = import(plain, middle.clone(), RECEIVED).await;
    pop3_delete(test, plain, plain_d.document_id()).await;
    let plain_n = import(plain, next.clone(), RECEIVED).await;
    test.wait_for_tasks().await;
    let (_, plain_rows) = query(plain, false).await;
    let same_thread = by_document(&plain_rows, plain_a).prefix_id()
        == by_document(&plain_rows, plain_n).prefix_id();

    let a = import(key, root, RECEIVED).await;
    let d = import(key, middle, RECEIVED).await;
    assert_eq!(d.prefix_id(), a.prefix_id());
    let (s, old) = query(key, false).await;
    let n = super::super::deliver_doc(test, key, &next).await;
    if capped {
        // The cap binds on this next insert before the planner reaches D's
        // deletion. A lone insert followed by a delete would not stop it.
        super::super::deliver_doc(test, key, &message_with("cap witness", "queued after N")).await;
    }
    pop3_delete(test, key, d.document_id()).await;
    let raw = range(test, key, SyncCollection::Email, exact(&s), u64::MAX).await;
    let insert = raw
        .records
        .iter()
        .find(|(_, entries)| entries.contains(&Change::InsertItem(Id::from_parts(n, n).id())))
        .unwrap()
        .0;
    let deletion = raw
        .records
        .iter()
        .find(|(_, entries)| {
            entries.iter().any(
                |entry| matches!(entry, Change::DeleteItem(id) if *id as u32 == d.document_id()),
            )
        })
        .unwrap()
        .0;
    assert!(insert < deletion);

    if capped {
        let _cap = search_config_guard!(test, pass_cap: 1);
        assert!(matches!(
            run_search_pass(test, key.id().document_id(), key.secret()).await,
            PassOutcome::Advanced {
                published: true,
                ..
            }
        ));
        let index = composition::manifest(test, key).await;
        assert!(insert <= index.indexed_to.change_id && index.indexed_to.change_id < deletion);
        let entry = test
            .server
            .inner
            .cache
            .za_search
            .get(key.id().document_id())
            .unwrap();
        let loaded = entry.read_loaded().await.unwrap();
        let mapped_n = Id::from_parts(loaded.thread.current_thread(n), n);
        assert_eq!(mapped_n.prefix_id() == a.prefix_id(), same_thread);
        assert_eq!(
            loaded.thread.exists_at(d.document_id(), u64::MAX),
            Some(true)
        );
        drop(loaded);
        let pass = entry.pass_lock.lock().await;
        let (_, delta) = email_delta(key, &s).await;
        assert_eq!(delta.destroyed, [d]);
        assert!(delta.created.contains(&mapped_n));
        drop(pass);
    }

    let (_, current) = query(key, false).await;
    let mapped_n = by_document(&current, Id::from(n));
    assert_eq!(mapped_n.prefix_id() == a.prefix_id(), same_thread);
    let (_, delta) = email_delta(key, &s).await;
    assert_eq!(delta.destroyed, [d]);
    assert_eq!(delta.apply(old), unordered(current));
}

async fn case_publication_counts_and_modseq(test: &TestServer, key: &Account) {
    use crate::utils::imap::{AssertResult, ImapConnection, Type};
    use imap::core::MailboxView;
    use imap_proto::ResponseType;

    println!("publication changes all mailbox counts without changing member MODSEQ");
    let account_id = key.id().document_id();
    let subject = "publication witness visibility";
    let a_raw = message_with(subject, "A");
    let d_raw = message_with(subject, "D");
    let a = import(key, a_raw.clone(), RECEIVED).await;
    let d = import(key, d_raw.clone(), RECEIVED).await;
    assert_ne!(a.prefix_id(), d.prefix_id());
    let reply = key
        .jmap_method_call("Mailbox/get", json!({"accountId": key.id_string()}))
        .await;
    let all_mailboxes = response(&reply, "Mailbox/get")["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|mailbox| id(&mailbox["id"]))
        .collect::<Vec<_>>();
    assert!(all_mailboxes.len() > 1);

    let mut imap = ImapConnection::connect(b"t13 ").await;
    imap.authenticate(key.name(), key.secret()).await;
    imap.send_ok("SELECT INBOX (CONDSTORE)").await;
    let before_publication = composition::manifest(test, key).await.newest_row;
    let entry = test.server.inner.cache.za_search.get(account_id).unwrap();
    let pass = entry.pass_lock.lock().await;
    let bridge = with_references(
        message_with("Re: publication witness visibility", "B"),
        &[message_id(&a_raw), message_id(&d_raw)],
    );
    let witness = super::super::deliver_doc(test, key, &bridge).await;
    let raw_cache = test.server.get_cached_messages(account_id).await.unwrap();
    let raw_end = raw_cache.emails.change_id;
    let mailbox = MailboxView::build(&raw_cache, INBOX_ID);
    let row = |document| {
        mailbox
            .live_rows_by_uid()
            .into_iter()
            .find(|row| row.id == document)
            .unwrap()
            .uid
    };
    let w_uid = row(witness);
    let d_uid = row(d.document_id());
    let witness_modseq = raw_end + 1;
    imap.send(&format!("UID FETCH {w_uid} (UID FLAGS MODSEQ)"))
        .await;
    let original = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    let modseq = format!("MODSEQ ({witness_modseq})");
    assert!(
        original.iter().any(|line| line.contains(&modseq)),
        "{original:?}"
    );
    // Explicit FETCH normally attempts a pass. Hold that pass until the
    // selected view has synchronized to the raw delivery and sampled MODSEQ.
    imap.send_ok("NOOP").await;
    assert_eq!(
        composition::manifest(test, key).await.newest_row,
        before_publication
    );
    drop(pass);
    let (published, emails) = query(key, false).await;
    let published = exact(&published);
    assert!(published > raw_end);
    assert_eq!(by_document(&emails, d).prefix_id(), a.prefix_id());

    let reply = changes(key, "Mailbox/changes", &State::new_exact(raw_end), None).await;
    let delta = response(&reply, "Mailbox/changes");
    assert_eq!(unordered(ids(delta, "updated")), unordered(all_mailboxes));
    assert!(ids(delta, "created").is_empty() && ids(delta, "destroyed").is_empty());
    assert_eq!(
        delta["updatedProperties"],
        json!([
            "totalEmails",
            "unreadEmails",
            "totalThreads",
            "unreadThreads"
        ])
    );
    let raw = range(test, key, SyncCollection::Email, raw_end, published).await;
    assert!(contains(
        &raw,
        Change::UpdateItem(Id::from_parts(witness, witness).id())
    ));
    assert!(
        raw.records
            .iter()
            .all(|(_, entries)| entries.iter().all(|entry| entry
                .item_id()
                .is_none_or(|id| id as u32 != d.document_id())))
    );

    imap.send("NOOP").await;
    let flags = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    let witness = flags
        .iter()
        .find(|line| {
            line.contains("FETCH")
                && line.contains("FLAGS")
                && line.contains(&format!("UID {w_uid}"))
        })
        .unwrap_or_else(|| panic!("no unsolicited witness FETCH: {flags:?}"));
    println!("Unsolicited witness response {witness}");
    assert!(
        !flags
            .iter()
            .any(|line| line.contains("FETCH") && line.contains(&format!("UID {d_uid}")))
    );
    imap.send(&format!("UID FETCH {w_uid} (UID FLAGS MODSEQ)"))
        .await;
    let after = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    assert!(after.iter().any(|line| line.contains(&modseq)), "{after:?}");
    imap.send(&format!(
        "UID FETCH {d_uid} (UID FLAGS MODSEQ) (CHANGEDSINCE {witness_modseq})"
    ))
    .await;
    let changed = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    assert!(
        !changed
            .iter()
            .any(|line| line.starts_with("* ") && line.contains("FETCH")),
        "{changed:?}"
    );
    imap.send("SELECT INBOX (CONDSTORE)").await;
    let highest = imap
        .assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .into_highest_modseq();
    assert_eq!(highest.parse::<u64>().unwrap(), published + 1);
}
