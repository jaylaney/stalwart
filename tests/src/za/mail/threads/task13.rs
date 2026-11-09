/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::utils::{account::Account, jmap::JmapResponse, server::TestServer};
use crate::za::mail::{key_mail_account, message_with};
use crate::za::{STRONG, user_permissions};
use email::mailbox::INBOX_ID;
use jmap_proto::types::state::State;
use serde_json::{Value, json};
use store::{
    query::log::{Change, ZaChangeRecords},
    za_search::Checkpoint,
};
use types::{collection::SyncCollection, id::Id};

async fn changes(
    account: &Account,
    method: &str,
    since: &State,
    max: Option<usize>,
) -> JmapResponse {
    let mut args = json!({
        "accountId": account.id_string(),
        "sinceState": since.to_string(),
    });
    if let Some(max) = max {
        args["maxChanges"] = json!(max);
    }
    account.jmap_method_call(method, args).await
}

fn response<'a>(reply: &'a JmapResponse, method: &str) -> &'a Value {
    assert_eq!(reply.name_at(0), method, "{:?}", reply.0);
    reply.method_response()
}

fn state(reply: &Value, field: &str) -> State {
    State::parse(reply[field].as_str().expect("missing state")).expect("invalid state")
}

fn exact(state: &State) -> u64 {
    let State::Exact(id) = *state else {
        panic!("expected noninitial exact state, got {state:?}");
    };
    assert_ne!(id, 0);
    id
}

fn id(value: &Value) -> Id {
    value.as_str().unwrap().parse().unwrap()
}

fn ids(reply: &Value, field: &str) -> Vec<Id> {
    reply[field].as_array().unwrap().iter().map(id).collect()
}

fn assert_initial_page(reply: &Value, since: &State) {
    assert_eq!(state(reply, "oldState"), *since);
    assert!(ids(reply, "updated").is_empty(), "{reply}");
    assert!(ids(reply, "destroyed").is_empty(), "{reply}");
    assert!(reply["updatedProperties"].is_null(), "{reply}");
}

async fn query(account: &Account, collapse: bool) -> (State, Vec<Id>) {
    let reply = account
        .jmap_method_call("Email/query", query_args(account, collapse))
        .await;
    let reply = response(&reply, "Email/query");
    (state(reply, "queryState"), ids(reply, "ids"))
}

fn query_args(account: &Account, collapse: bool) -> Value {
    json!({
        "accountId": account.id_string(),
        "sort": [{"property": "receivedAt", "isAscending": false}],
        "collapseThreads": collapse,
    })
}

async fn import(account: &Account, message: String, received_at: &str) -> Id {
    let blob = account
        .jmap_client()
        .await
        .upload(None, message.into_bytes(), None)
        .await
        .unwrap()
        .take_blob_id();
    let mailbox = Id::from(INBOX_ID).to_string();
    let reply = account
        .jmap_method_call(
            "Email/import",
            json!({
                "accountId": account.id_string(),
                "emails": { "i": {
                    "blobId": blob,
                    "mailboxIds": {(mailbox): true},
                    "receivedAt": received_at,
                } },
            }),
        )
        .await;
    id(&response(&reply, "Email/import")["created"]["i"]["id"])
}

async fn flag_seen(account: &Account, email: Id) {
    let reply = account
        .jmap_update(
            "Email",
            [(email, json!({"keywords/$seen": true}))],
            Vec::<(&str, Value)>::new(),
        )
        .await;
    let reply = response(&reply, "Email/set");
    assert!(
        reply["updated"]
            .as_object()
            .unwrap()
            .contains_key(&email.to_string())
    );
}

async fn destroy(account: &Account, email: Id) {
    let reply = account
        .jmap_destroy("Email", [email], Vec::<(&str, Value)>::new())
        .await;
    assert_eq!(ids(response(&reply, "Email/set"), "destroyed"), [email]);
}

async fn range(
    test: &TestServer,
    account: &Account,
    collection: SyncCollection,
    from: u64,
    to: u64,
) -> ZaChangeRecords {
    test.server
        .store()
        .za_changes_records_range(account.id().document_id(), collection, from, to)
        .await
        .unwrap()
}

async fn retained_bound(test: &TestServer, account: &Account) -> u64 {
    let account = account.id().document_id();
    let store = test.server.store();
    store
        .za_log_first_retained(account, SyncCollection::Email)
        .await
        .unwrap()
        .max(
            store
                .za_log_first_retained(account, SyncCollection::Thread)
                .await
                .unwrap(),
        )
        .unwrap_or(0)
}

fn contains(records: &ZaChangeRecords, target: Change) -> bool {
    records
        .records
        .iter()
        .any(|(_, entries)| entries.contains(&target))
}

mod composition;
mod history;
mod minimum;
mod retention;

const RECEIVED: &str = "2026-01-01T00:00:00Z";

async fn three_singletons(account: &Account) -> Vec<Id> {
    for subject in ["paging alpha", "paging bravo", "paging charlie"] {
        import(account, message_with(subject, subject), RECEIVED).await;
    }
    let (_, mut found) = query(account, false).await;
    found.sort_unstable_by_key(Id::document_id);
    assert_eq!(found.len(), 3);
    assert!(found.iter().all(|id| id.prefix_id() == id.document_id()));
    found
}

// The retained history has only a flag update, but all live messages are created.
async fn case_initial_after_truncation(test: &TestServer, key: &Account) {
    let live = three_singletons(key).await;
    flag_seen(key, live[1]).await;
    let (latest, control) = query(key, false).await;
    assert_eq!(control.len(), live.len());
    let manifest = composition::manifest(test, key).await;
    let mut tokens = Vec::new();
    for (method, expected) in [
        ("Email/changes", live.clone()),
        (
            "Thread/changes",
            live.iter().map(|id| Id::from(id.document_id())).collect(),
        ),
    ] {
        let reply = changes(key, method, &State::Initial, Some(1)).await;
        let (token, end, first) = first_page(response(&reply, method));
        assert_eq!(first, expected[0]);
        tokens.push((method, token, end, expected));
    }
    let marker = test
        .server
        .truncate_change_log(key.id().document_id(), SyncCollection::Email.into(), 1)
        .await
        .unwrap()
        .expect("truncation must replace a real row");
    assert!(manifest.indexed_to.change_id < marker);
    let before = range(test, key, SyncCollection::Email, 0, exact(&latest)).await;
    assert!(before.truncated && before.first_retained == Some(marker));
    assert_eq!(before.records.len(), 1);
    assert!(contains(&before, Change::UpdateItem(live[1].id())));
    for email in &live {
        assert!(!contains(&before, Change::InsertItem(email.id())));
    }

    let account_id = key.id().document_id();
    let cache = &test.server.inner.cache.za_search;
    let original = cache.get(account_id).unwrap();
    cache.remove_account(
        account_id,
        common::za::search::cache::EvictReason::Invalidation,
    );
    assert!(cache.get(account_id).is_none());
    for spelling in [State::Initial, State::new_exact(0)] {
        // Both spellings parse as Initial in the wire request.
        let reply = changes(key, "Email/changes", &spelling, None).await;
        let reply = response(&reply, "Email/changes");
        assert_eq!(
            ids(reply, "created"),
            live,
            "initial page lost live messages: {reply}"
        );
        assert_initial_page(reply, &State::Initial);
        assert_eq!(reply["hasMoreChanges"], false);
        assert_eq!(state(reply, "newState"), latest);
    }
    for (method, token, end, expected) in tokens {
        assert!(marker < end);
        assert_eq!(rest_at(key, method, token, end).await, expected[1..]);
    }
    let reloaded = cache.get(account_id).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&original, &reloaded));
    let after = composition::manifest(test, key).await;
    // The cold pass reconciles across the marker and consumes the sole
    // retained record without publishing a new identity revision.
    let retained_end = before.records[0].0;
    assert_eq!(
        after.indexed_to,
        Checkpoint {
            change_id: retained_end,
            position: 0,
            complete: true,
        }
    );
    assert!(after.reconciliation.is_none());
    assert_eq!(manifest.epoch, after.epoch);
    assert_eq!(manifest.newest_row, after.newest_row);
    assert_eq!(manifest.thread_blobs, after.thread_blobs);
    assert_eq!(manifest.content, after.content);
    for collection in [SyncCollection::Email, SyncCollection::Thread] {
        assert!(
            range(test, key, collection, retained_end, u64::MAX)
                .await
                .records
                .is_empty()
        );
    }
}

fn first_page(reply: &Value) -> (State, u64, Id) {
    let State::Intermediate(page) = state(reply, "newState") else {
        panic!("expected initial continuation: {reply}");
    };
    assert_eq!(page.from_id, 0, "not a from-zero page: {reply}");
    assert_eq!(page.items_sent, 1);
    assert_eq!(reply["hasMoreChanges"], true);
    let created = ids(reply, "created");
    assert_eq!(created.len(), 1);
    (State::Intermediate(page.clone()), page.to_id, created[0])
}

async fn rest_at(account: &Account, method: &str, mut since: State, end: u64) -> Vec<Id> {
    let mut seen = Vec::new();
    for sent in 2..=3 {
        let reply = changes(account, method, &since, Some(1)).await;
        let reply = response(&reply, method);
        assert_initial_page(reply, &since);
        let created = ids(reply, "created");
        assert_eq!(created.len(), 1, "{reply}");
        seen.extend(created);
        since = state(reply, "newState");
        if sent == 3 {
            assert_eq!(reply["hasMoreChanges"], false);
            assert_eq!(since, State::new_exact(end));
        } else {
            assert_eq!(reply["hasMoreChanges"], true);
            assert_eq!(since, State::new_intermediate(0, end, sent));
        }
    }
    seen
}

// Use an account separate from case_initial_after_truncation. The three
// initial threads remain distinct for the Thread/changes membership control.
// Add the later-prefix-changing retained-history case separately, using a
// real bridge and independent historical Thread/get/Email/get observations.
async fn case_initial_pinned_end_lifecycle(test: &TestServer, key: &Account) {
    let live = three_singletons(key).await;
    let mut pages = Vec::new();
    for (method, expected) in [
        ("Email/changes", live.clone()),
        (
            "Thread/changes",
            live.iter().map(|id| Id::from(id.document_id())).collect(),
        ),
    ] {
        let reply = changes(key, method, &State::Initial, Some(1)).await;
        let reply = response(&reply, method);
        assert_initial_page(reply, &State::Initial);
        let (token, end, first) = first_page(reply);
        assert_eq!(first, expected[0]);
        assert_eq!(
            rest_at(key, method, token.clone(), end).await,
            expected[1..]
        );
        pages.push((method, token, end, expected));
    }

    destroy(key, live[0]).await;
    let later = import(key, message_with("paging later live", "live"), RECEIVED).await;
    let transient = import(
        key,
        message_with("paging later transient", "gone"),
        RECEIVED,
    )
    .await;
    destroy(key, transient).await;
    let (current, current_ids) = query(key, false).await;
    let current = exact(&current);
    assert!(!current_ids.contains(&live[0]) && !current_ids.contains(&transient));
    assert!(current_ids.contains(&later));

    for (method, token, end, expected) in &pages {
        let suffix = range(test, key, SyncCollection::Email, *end, current).await;
        assert!(!suffix.truncated);
        assert!(contains(&suffix, Change::DeleteItem(live[0].id())));
        assert!(contains(&suffix, Change::InsertItem(later.id())));
        assert!(contains(&suffix, Change::InsertItem(transient.id())));
        assert!(contains(&suffix, Change::DeleteItem(transient.id())));
        assert_eq!(
            rest_at(key, method, token.clone(), *end).await,
            expected[1..]
        );
    }

    // Email and Thread inserts, deletes, and publications advance the common
    // change group. Assert the actual resulting marker rather than assume a
    // count translates directly to a chosen change id.
    test.server
        .delete_changes(key.id().document_id(), Some(0), None)
        .await
        .unwrap();
    let bound = retained_bound(test, key).await;
    for (method, token, end, _) in pages {
        assert!(bound > end);
        let surviving = range(test, key, SyncCollection::Email, end, current).await;
        assert!(!contains(&surviving, Change::DeleteItem(live[0].id())));
        let reply = changes(key, method, &token, Some(1)).await;
        let refusal = response(&reply, "error");
        assert_eq!(refusal["type"], "cannotCalculateChanges", "{refusal}");
        assert_eq!(
            refusal["description"], "Change log is truncated",
            "{refusal}"
        );

        let reply = changes(key, method, &State::Initial, None).await;
        let fresh = response(&reply, method);
        assert_initial_page(fresh, &State::Initial);
        let mut expected = current_ids.clone();
        expected.sort_unstable_by_key(Id::document_id);
        if method == "Thread/changes" {
            expected = expected
                .into_iter()
                .map(|id| Id::from(id.document_id()))
                .collect();
        }
        assert_eq!(ids(fresh, "created"), expected);
    }
}

fn message_id(message: &str) -> &str {
    message
        .lines()
        .find_map(|line| line.strip_prefix("Message-ID: "))
        .unwrap()
}

fn with_references(message: String, refs: &[&str]) -> String {
    message.replacen(
        "Message-ID: ",
        &format!("References: {}\r\nMessage-ID: ", refs.join(" ")),
        1,
    )
}

fn by_document(messages: &[Id], target: Id) -> Id {
    *messages
        .iter()
        .find(|id| id.document_id() == target.document_id())
        .unwrap()
}

fn unordered(mut ids: Vec<Id>) -> Vec<Id> {
    ids.sort_unstable();
    ids
}

async fn thread_state(account: &Account) -> State {
    let reply = account
        .jmap_method_call("Thread/get", json!({"accountId": account.id_string()}))
        .await;
    state(response(&reply, "Thread/get"), "state")
}

async fn query_diff(account: &Account, since: &State, collapse: bool) -> JmapResponse {
    query_diff_with(account, since, collapse, json!({})).await
}

async fn query_diff_with(
    account: &Account,
    since: &State,
    collapse: bool,
    extra: Value,
) -> JmapResponse {
    let mut args = query_args(account, collapse);
    args.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    args["sinceQueryState"] = json!(since.to_string());
    account.jmap_method_call("Email/queryChanges", args).await
}

fn added(reply: &Value) -> Vec<(Id, usize)> {
    reply["added"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| (id(&item["id"]), item["index"].as_u64().unwrap() as usize))
        .collect()
}

fn assert_diff_applies(mut old: Vec<Id>, reply: &Value, expected: &[Id]) {
    let removed = ids(reply, "removed");
    old.retain(|item| !removed.contains(item));
    for (item, position) in added(reply) {
        assert!(!old.contains(&item), "duplicate added id: {reply}");
        old.insert(position, item);
    }
    assert_eq!(old, expected);
}

async fn case_merge_rendering_parity(test: &TestServer, key: &Account, plain: &Account) {
    // Same subject bucket, distinct unreferenced roots. D is a member of A,
    // newer than every other message, and has a mapped non-singleton id. It
    // must be re-emitted by the collapsed diff even though it is untouched by
    // the bridge publication. B is the losing member, C the real bridge.
    let a = message_with("parity conversation", "root A");
    let b = message_with("parity conversation", "root B");
    let d = with_references(
        message_with("Re: parity conversation", "stable member D"),
        &[message_id(&a)],
    );
    let c = with_references(
        message_with("Re: parity conversation", "bridge C"),
        &[message_id(&a), message_id(&b)],
    );
    // Run the identical semantic assertions against both real servers paths.
    // Ids need not agree between accounts. Map fixture labels by document id,
    // and assert exact old/new prefixes independently within each account.
    for account in [plain, key] {
        let root_a = import(account, a.clone(), "2026-01-02T00:00:00Z").await;
        let member_d = import(account, d.clone(), "2026-01-04T00:00:00Z").await;
        let root_b = import(account, b.clone(), "2026-01-03T00:00:00Z").await;
        test.wait_for_tasks().await;
        let (since, old) = query(account, false).await;
        let (since_collapsed, old_collapsed) = query(account, true).await;
        let since_thread = thread_state(account).await;
        let old_a = by_document(&old, root_a);
        let old_b = by_document(&old, root_b);
        let old_d = by_document(&old, member_d);
        let surviving_thread = old_a.prefix_id();
        let losing_thread = old_b.prefix_id();
        assert_ne!(surviving_thread, losing_thread);
        assert_eq!(old_d.prefix_id(), surviving_thread);
        assert_ne!(old_d.prefix_id(), old_d.document_id());
        assert_eq!(old_collapsed, [old_d, old_b]);

        let bridge = import(account, c.clone(), "2026-01-01T00:00:00Z").await;
        test.wait_for_tasks().await;
        let (current_state, current) = query(account, false).await;
        let (_, current_collapsed) = query(account, true).await;
        let new_b = by_document(&current, old_b);
        let new_c = by_document(&current, bridge);
        assert_ne!(new_b, old_b);
        assert_eq!(new_b.prefix_id(), surviving_thread);
        assert_eq!(new_c.prefix_id(), surviving_thread);
        assert_eq!(by_document(&current, old_d), old_d);
        assert_eq!(current_collapsed, [old_d]);

        if account.id() == key.id() {
            let raw = range(
                test,
                key,
                SyncCollection::Email,
                exact(&since),
                exact(&current_state),
            )
            .await;
            assert!(!raw.truncated);
            assert!(contains(
                &raw,
                Change::InsertItem(Id::from_parts(bridge.document_id(), bridge.document_id()).id())
            ));
            assert!(contains(
                &raw,
                Change::UpdateItem(Id::from_parts(bridge.document_id(), bridge.document_id()).id())
            ));
            for untouched in [old_b, old_d] {
                assert!(
                    raw.records
                        .iter()
                        .all(|(_, entries)| entries.iter().all(|entry| {
                            entry
                                .item_id()
                                .is_none_or(|raw| raw as u32 != untouched.document_id())
                        })),
                    "unexpected raw candidate: {untouched}"
                );
            }
        }

        let reply = changes(account, "Email/changes", &since, None).await;
        let delta = response(&reply, "Email/changes");
        assert_eq!(
            unordered(ids(delta, "created")),
            unordered(vec![new_b, new_c])
        );
        assert_eq!(ids(delta, "destroyed"), [old_b]);
        assert!(ids(delta, "updated").is_empty());
        assert_eq!(state(delta, "newState"), current_state);

        let reply = changes(account, "Thread/changes", &since_thread, None).await;
        let delta = response(&reply, "Thread/changes");
        assert!(ids(delta, "created").is_empty());
        assert_eq!(ids(delta, "updated"), [Id::from(surviving_thread)]);
        assert_eq!(ids(delta, "destroyed"), [Id::from(losing_thread)]);

        let reply = query_diff(account, &since, false).await;
        let diff = response(&reply, "Email/queryChanges");
        assert_eq!(ids(diff, "removed"), [old_b]);
        assert_eq!(
            unordered(added(diff).into_iter().map(|(id, _)| id).collect()),
            unordered(vec![new_b, new_c]),
        );
        assert_eq!(state(diff, "oldQueryState"), since);
        assert_eq!(state(diff, "newQueryState"), current_state);
        assert_diff_applies(old, diff, &current);

        let reply = query_diff(account, &since_collapsed, true).await;
        let diff = response(&reply, "Email/queryChanges");
        assert_eq!(
            unordered(ids(diff, "removed")),
            unordered(vec![old_d, old_a, old_b])
        );
        assert_eq!(added(diff), [(old_d, 0)]);
        assert_eq!(state(diff, "oldQueryState"), since_collapsed);
        assert_eq!(state(diff, "newQueryState"), current_state);
        assert_diff_applies(old_collapsed, diff, &current_collapsed);

        // FS8: a real flag-only update on an unrelated singleton advances the
        // group and is visible to Email/changes. It changes no thread members.
        let u = import(
            account,
            message_with("unrelated parity flag", "U"),
            RECEIVED,
        )
        .await;
        test.wait_for_tasks().await;
        let (email_before, _) = query(account, false).await;
        let thread_before = thread_state(account).await;
        flag_seen(account, u).await;
        let reply = changes(account, "Email/changes", &email_before, None).await;
        let delta = response(&reply, "Email/changes");
        assert_eq!(ids(delta, "updated"), [u]);
        assert!(ids(delta, "created").is_empty() && ids(delta, "destroyed").is_empty());
        let reply = changes(account, "Thread/changes", &thread_before, None).await;
        let delta = response(&reply, "Thread/changes");
        assert!(
            ["created", "updated", "destroyed"]
                .iter()
                .all(|kind| ids(delta, kind).is_empty())
        );
    }
}

pub(super) async fn test(test: &mut TestServer) {
    println!("Running zero-access mapped changes and queryChanges tests...");
    let key = key_mail_account(test, "keymail-task13-initial@example.com", STRONG).await;
    case_initial_after_truncation(test, &key).await;
    test.destroy_all_mailboxes(&key).await;

    let key = key_mail_account(test, "keymail-task13-pinned@example.com", STRONG).await;
    case_initial_pinned_end_lifecycle(test, &key).await;
    test.destroy_all_mailboxes(&key).await;

    let key = key_mail_account(test, "keymail-task13-parity@example.com", STRONG).await;
    let plain = test
        .account("admin@example.com")
        .create_user_account(
            "plainmail-task13-parity@example.com",
            STRONG,
            "Plain mail",
            &[],
            user_permissions(),
        )
        .await;
    case_merge_rendering_parity(test, &key, &plain).await;
    test.destroy_all_mailboxes(&plain).await;
    test.destroy_all_mailboxes(&key).await;

    composition::test(test).await;
    retention::test(test).await;
    minimum::test(test).await;
    history::test(test).await;
}
