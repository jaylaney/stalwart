/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::*;
use crate::utils::{
    imap::{AssertResult, ImapConnection, Type},
    za::{HookParked, za_unlock},
};
use common::{
    Caches, Data, Inner, Server,
    manager::boot::build_ipc,
    za::search::error::{NoMapReason, TransientClass},
    za::search::seal::za_search_key,
    za::test_hooks::{
        set_search_cache_hold, set_search_manifest_fail_once, set_search_snapshot_pause,
        set_search_swap_pause, set_search_view_pause,
    },
};
use email::{
    cache::{MessageCacheFetch, email::MessageCacheAccess},
    message::za::search::{
        ensure::Unavailable,
        pass::ZaSearchPass,
        view::{ViewError, ViewNeed, za_view_with_minimum},
    },
};
use imap_proto::ResponseType;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use store::{
    query::log::Change,
    registry::bootstrap::Bootstrap,
    write::{BatchBuilder, PendingId, SetValue},
    za_search::manifest::{ManifestFill, ManifestWrite, manifest_hash},
};
use types::{
    collection::{Collection, SyncCollection},
    field::PrincipalField,
};

const DEADLINE: Duration = Duration::from_secs(4);

pub(super) async fn test(test: &TestServer, key: &Account) {
    case_empty_account_validates_group_counter(test, key).await;
    case_r2_cross_node(test, key, false).await;
    case_r2_cross_node(test, key, true).await;
    case_failed_manifest_read(key).await;
    case_metadata_query_changes_borrows_one_map(test, key).await;
    case_set_cross_node_publication_before_response(test, key).await;
    case_write_response_requires_own_change_id(test, key, false).await;
    case_write_response_requires_own_change_id(test, key, true).await;
    case_import_requires_own_change_id(test, key).await;
}

async fn case_empty_account_validates_group_counter(test: &TestServer, key: &Account) {
    let id = key.id().document_id();
    let initial = get(key, Value::Null).await;
    assert_eq!(State::parse(initial.state()), Some(State::Initial));
    let keys = za_unlock(test, id, key.secret()).await;
    let manifest = read_manifest(test, id).await.unwrap();
    // Emulate an authenticated epoch boundary using a real container-only
    // record. This exercises the view before Task 14 supplies recovery.
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Email)
        .log_container_property_change(SyncCollection::Email, PendingId::Assigned(INBOX_ID))
        .with_collection(Collection::Principal)
        .with_document(0)
        .set(
            PrincipalField::ZeroAccessSearch,
            SetValue::serializable(ManifestWrite::for_account(
                manifest,
                za_search_key(keys.swk()),
                ManifestFill {
                    newest_row: false,
                    epoch: true,
                },
                id,
            )),
        );
    let assigned = test.server.commit_batch(batch).await.unwrap();
    let epoch = assigned.last_change_id(id, SyncCollection::Email);
    let Some(ManifestValue::Index(manifest)) = read_manifest(test, id).await else {
        panic!("the boundary must retain an index manifest");
    };
    assert_ne!(epoch, 0);
    assert_eq!(manifest.epoch, epoch);
    let cache = test.server.get_cached_messages(id).await.unwrap();
    assert_eq!(cache.last_change_id, epoch);
    assert_eq!(cache.emails.change_id, 0);
    let reply = get(key, Value::Null).await;
    assert_eq!(method(&reply, 0, "Email/get")["list"], json!([]));
    assert_eq!(State::parse(reply.state()), Some(State::Initial));
}

fn assert_published(outcome: PassOutcome) {
    assert!(
        matches!(
            outcome,
            PassOutcome::Advanced {
                published: true,
                ..
            }
        ),
        "{outcome:?}"
    );
}

async fn durable_hash(test: &TestServer, account_id: u32) -> [u8; 32] {
    manifest_hash(&manifest_bytes(test, account_id).await.unwrap())
}

struct Flag {
    restore: fn(Option<u32>),
}

impl Flag {
    fn arm(set: fn(Option<u32>), account_id: u32) -> Self {
        set(Some(account_id));
        Self { restore: set }
    }
}

impl Drop for Flag {
    fn drop(&mut self) {
        (self.restore)(None);
    }
}

async fn case_r2_cross_node(test: &TestServer, key: &Account, hold_cache: bool) {
    let account_id = key.id().document_id();
    let subject = format!("Task 11 paused protocol Get hold={hold_cache}");
    let root = message_with(&subject, "root");
    let a = deliver_doc(test, key, &root).await;
    let before = get(key, json!([])).await;
    let before_counter = exact(&method(&before, 0, "Email/get")["state"]);
    let before_hash = durable_hash(test, account_id).await;
    let entry = test.server.inner.cache.za_search.get(account_id).unwrap();

    let reader = key.clone();
    let read = HookParked::start(account_id, set_search_snapshot_pause, async move {
        get(&reader, Value::Null).await
    })
    .await;
    let b = deliver_doc(test, key, &reply_to(&root, &subject, "reply")).await;

    let server = test.server.clone();
    let keys = za_unlock(test, account_id, key.secret()).await;
    let writer = HookParked::start(account_id, set_search_swap_pause, async move {
        server.za_search_pass(account_id, &keys).await.unwrap()
    })
    .await;
    let committed = durable_hash(test, account_id).await;
    let newest = publication(test, key).await;
    assert_ne!(before_hash, committed);
    assert!(newest > before_counter);
    assert_eq!(
        entry.read_loaded().await.unwrap().manifest_bytes_hash,
        before_hash
    );
    assert!(entry.pass_lock.try_lock().is_err());

    // The resident message cache may include the delivery fetched by the
    // writer, but that still precedes its durable publication.
    let hold = hold_cache.then(|| Flag::arm(set_search_cache_hold, account_id));
    let reply = tokio::time::timeout(DEADLINE, read.finish())
        .await
        .expect("Email/get waited for the parked pass");
    assert!(entry.pass_lock.try_lock().is_err());
    drop(hold);
    if hold_cache {
        assert_eq!(method(&reply, 0, "error")["type"], "serverUnavailable");
    } else {
        assert_email_get_publication(&reply, a, b, newest);
        assert_eq!(
            entry.read_loaded().await.unwrap().manifest_bytes_hash,
            committed
        );
    }

    assert_published(
        tokio::time::timeout(DEADLINE, writer.finish())
            .await
            .unwrap(),
    );
    let after = get(key, json!([mapped(a, a), mapped(a, b)])).await;
    assert_email_get_publication(&after, a, b, newest);
    if !hold_cache {
        assert_eq!(
            method(&reply, 0, "Email/get")["state"],
            method(&after, 0, "Email/get")["state"]
        );
    }

    // Task 13 adds mapped changes rendering. At this checkpoint a read
    // since the latest publication needs no mapped history to be empty.
    let changes = key
        .jmap_method_call(
            "Email/changes",
            json!({
                "accountId": key.id_string(),
                "sinceState": method(&after, 0, "Email/get")["state"],
            }),
        )
        .await;
    let changes = method(&changes, 0, "Email/changes");
    assert_eq!(changes["created"], json!([]), "{changes}");
    assert_eq!(changes["updated"], json!([]), "{changes}");
    assert_eq!(changes["destroyed"], json!([]), "{changes}");
}

fn assert_email_get_publication(reply: &JmapResponse, a: u32, b: u32, newest: u64) {
    let response = method(reply, 0, "Email/get");
    for doc in [a, b] {
        let expected = mapped(a, doc);
        let row = response["list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == expected)
            .unwrap_or_else(|| panic!("missing mapped ID {expected}: {response}"));
        assert_eq!(row["threadId"], Id::from(a).to_string());
    }
    assert!(exact(&response["state"]) >= newest, "{response}");
}

async fn case_failed_manifest_read(key: &Account) {
    let account_id = key.id().document_id();
    for name in ["Email/get", "Thread/get", "Mailbox/get"] {
        let reader = key.clone();
        let read = HookParked::start(account_id, set_search_snapshot_pause, async move {
            reader
                .jmap_method_call(name, json!({ "accountId": reader.id_string(), "ids": [] }))
                .await
        })
        .await;
        let fail = Flag::arm(set_search_manifest_fail_once, account_id);
        let reply = tokio::time::timeout(DEADLINE, read.finish()).await.unwrap();
        assert_eq!(method(&reply, 0, "error")["type"], "serverUnavailable");
        drop(fail);
        let read = key
            .jmap_method_call(name, json!({ "accountId": key.id_string(), "ids": [] }))
            .await;
        method(&read, 0, name);
    }

    let mut imap = ImapConnection::connect(b"_t11fail ").await;
    imap.authenticate(key.name(), key.secret()).await;
    imap.send_ok("SELECT INBOX").await;
    let read = HookParked::start(account_id, set_search_snapshot_pause, async move {
        imap.send("FETCH 1 (OBJECTID)").await;
        let response = imap.assert_read(Type::Tagged, ResponseType::No).await;
        (imap, response)
    })
    .await;
    let fail = Flag::arm(set_search_manifest_fail_once, account_id);
    let (mut imap, response) = tokio::time::timeout(DEADLINE, read.finish()).await.unwrap();
    assert!(
        response
            .iter()
            .all(|line| !line.contains("EMAILID") && !line.contains("THREADID"))
    );
    response.assert_response_code("UNAVAILABLE");
    drop(fail);
    imap.send("FETCH 1 (OBJECTID)").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("EMAILID ");
}

async fn case_metadata_query_changes_borrows_one_map(test: &TestServer, key: &Account) {
    let id = key.id().document_id();
    let filter = json!({ "inMailbox": Id::from(INBOX_ID).to_string() });
    let before = key
        .jmap_method_call(
            "Email/query",
            json!({ "accountId": key.id_string(), "filter": filter }),
        )
        .await;
    let state = method(&before, 0, "Email/query")["queryState"].clone();
    let doc = deliver_doc(
        test,
        key,
        &message_with("Task 11 metadata delta", "new row"),
    )
    .await;
    let entry = test.server.inner.cache.za_search.get(id).unwrap();
    let reader = key.clone();
    let response = HookParked::start(id, set_search_view_pause, async move {
        reader
            .jmap_method_call(
                "Email/queryChanges",
                json!({
                    "accountId": reader.id_string(), "filter": filter, "sinceQueryState": state,
                }),
            )
            .await
    })
    .await;

    assert!(
        entry.index.try_write().is_err(),
        "the Map view must hold a read guard"
    );
    // Queue a write after the first read guard. A second acquisition
    // would block behind it until the handler released the first one.
    let mut writer = Box::pin(entry.index.write());
    assert!(futures::poll!(writer.as_mut()).is_pending());
    let response = tokio::time::timeout(DEADLINE, response.finish())
        .await
        .expect("metadata queryChanges attempted another acquisition");
    drop(writer);
    let response = method(&response, 0, "Email/queryChanges");
    let expected = singleton(doc);
    assert!(
        response["added"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == expected)
    );
}

async fn separate_publisher(test: &TestServer) -> Server {
    let core = test.server.core.clone();
    let mut bootstrap = Bootstrap::new(test.server.registry().clone()).await;
    let data = Data::parse(&mut bootstrap).await;
    let (cache, _swap_rx) = Caches::parse(&mut bootstrap, &core.storage).await;
    let (ipc, _receivers) = build_ipc(false);
    let inner = Arc::new(Inner {
        shared_core: core.as_ref().clone().into_shared(),
        data,
        cache,
        ipc,
    });
    Server { inner, core }
}

async fn item_updates_after(test: &TestServer, account_id: u32, after: u64, doc: u32) -> Vec<u64> {
    let logs = test
        .server
        .store()
        .za_changes_records_range(account_id, SyncCollection::Email, after, u64::MAX)
        .await
        .unwrap();
    assert!(!logs.truncated, "{logs:?}");
    logs.records
        .into_iter()
        .filter_map(|(change_id, changes)| {
            changes
                .into_iter()
                .any(|change| matches!(change, Change::UpdateItem(id) if id as u32 == doc))
                .then_some(change_id)
        })
        .collect()
}

async fn case_set_cross_node_publication_before_response(test: &TestServer, key: &Account) {
    let id = key.id().document_id();
    let subject = "Task 11 mixed Set bridge";
    // Both threads have the same normalized subject but different references.
    // Three candidates on the left beat the right and its just-created reply.
    let left = message_with(subject, "left root");
    let left_id = deliver_doc(test, key, &left).await;
    for body in ["left 1", "left 2"] {
        deliver_doc(test, key, &reply_to(&left, subject, body)).await;
    }
    let right = message_with(subject, "right root");
    let right_id = deliver_doc(test, key, &right).await;
    let update = deliver_doc(test, key, &message_with("Bridge update U", "update")).await;
    let original = get(key, json!([])).await;
    let old_state = original.state().to_string();
    let prior_ids = message_ids(test, id).await;
    let server = separate_publisher(test).await;
    let keys = za_unlock(test, id, key.secret()).await;
    let request_account = key.clone();
    let request_old_state = old_state.clone();
    let update_id = singleton(update);
    let request_update = update_id.clone();
    let create = email_create(key, &format!("Re: {subject}"), Some(&right));
    let request = HookParked::start(id, set_search_view_pause, async move {
        set(
            &request_account,
            json!({
                "ifInState": request_old_state,
                "create": { "c": create },
                "update": { request_update: { "keywords/$flagged": true } }
            }),
        )
        .await
    })
    .await;

    let local_entry = test.server.inner.cache.za_search.get(id).unwrap();
    assert!(
        local_entry.index.try_write().is_err(),
        "Set input guard is not held"
    );
    let remote_entry = server.za_search_entry(id, &keys).await.unwrap();
    assert!(!Arc::ptr_eq(&local_entry, &remote_entry));
    let local_cache = test.server.get_cached_messages(id).await.unwrap();
    let remote_cache = server.get_cached_messages(id).await.unwrap();
    assert!(!Arc::ptr_eq(&local_cache, &remote_cache));
    let created = new_message(test, id, &prior_ids).await;
    let input_publication = publication(test, key).await;
    let input_hash = durable_hash(test, id).await;
    {
        let index = remote_entry.read_loaded().await.unwrap();
        assert_eq!(index.manifest_bytes_hash, input_hash);
        assert_eq!(index.thread.current_thread(created), right_id);
    }

    let bridge = message_with(&format!("Re: {subject}"), "merge witness").replacen(
        "\r\n\r\n",
        &format!(
            "\r\nReferences: <{}> <{}>\r\n\r\n",
            parent_message_id(&left),
            parent_message_id(&right)
        ),
        1,
    );
    deliver_doc(test, key, &bridge).await;
    // Local commit invalidation cannot reach this second cache.
    server.inner.mark_cache_stale(id, SyncCollection::Email);
    let remote = tokio::time::timeout(DEADLINE, server.za_search_pass(id, &keys))
        .await
        .expect("publisher waited on the request's local guard")
        .unwrap();
    assert_published(remote);
    let merge = publication(test, key).await;
    let final_hash = durable_hash(test, id).await;
    assert!(merge > input_publication);
    assert_ne!(final_hash, input_hash);
    assert_eq!(
        remote_entry
            .read_loaded()
            .await
            .unwrap()
            .thread
            .current_thread(created),
        left_id
    );
    assert!(local_entry.index.try_write().is_err());
    assert!(item_updates_after(test, id, merge, update).await.is_empty());

    let reply = tokio::time::timeout(DEADLINE, request.finish())
        .await
        .expect("Set kept the input guard during final acquisition");
    let updates = item_updates_after(test, id, merge, update).await;
    let [write] = updates.as_slice() else {
        panic!("expected exactly one acknowledged update after merge: {updates:?}");
    };
    assert!(*write > merge);
    assert_eq!(
        durable_hash(test, id).await,
        final_hash,
        "unexpected post-write publication"
    );

    // No repairing request before these assertions.
    let response = method(&reply, 0, "Email/set");
    assert_eq!(response["oldState"], old_state);
    assert_eq!(exact(&response["newState"]), *write);
    assert!(response["updated"].get(&update_id).is_some(), "{response}");
    assert_eq!(response["created"]["c"]["id"], mapped(left_id, created));
    assert_eq!(
        response["created"]["c"]["threadId"],
        Id::from(left_id).to_string()
    );
}

async fn case_write_response_requires_own_change_id(
    test: &TestServer,
    key: &Account,
    fail_manifest: bool,
) {
    let id = key.id().document_id();
    let doc = deliver_doc(
        test,
        key,
        &message_with(
            &format!("Task 11 floor with failed manifest={fail_manifest}"),
            "unchanged manifest after flag update",
        ),
    )
    .await;
    let before = get(key, json!([singleton(doc)])).await;
    let floor_at_start = publication(test, key).await;
    assert_eq!(
        State::parse(before.state()),
        Some(State::Exact(floor_at_start))
    );
    let before_hash = durable_hash(test, id).await;
    let entry = test.server.inner.cache.za_search.get(id).unwrap();
    let account = key.clone();
    let request = HookParked::start(id, set_search_view_pause, async move {
        set(
            &account,
            json!({ "update": { singleton(doc): { "keywords/$flagged": true } } }),
        )
        .await
    })
    .await;

    let cache = test.server.get_cached_messages(id).await.unwrap();
    assert_eq!(cache.last_change_id, floor_at_start);
    assert!(cache.email_by_id(&doc).is_some());
    let scanned_before = entry.read_loaded().await.unwrap().scanned_to;
    let flag = if fail_manifest {
        Flag::arm(set_search_manifest_fail_once, id)
    } else {
        Flag::arm(set_search_cache_hold, id)
    };
    let reply = tokio::time::timeout(DEADLINE, request.finish())
        .await
        .unwrap();

    // Inspect persisted writes without a protocol read that could run a pass.
    let updates = item_updates_after(test, id, floor_at_start, doc).await;
    let [write] = updates.as_slice() else {
        panic!("expected one update after the input view: {updates:?}");
    };
    assert!(*write > floor_at_start);
    assert_eq!(durable_hash(test, id).await, before_hash);
    assert_eq!(
        entry.read_loaded().await.unwrap().scanned_to,
        scanned_before
    );
    if !fail_manifest {
        let held_cache = test.server.get_cached_messages(id).await.unwrap();
        assert!(Arc::ptr_eq(&cache, &held_cache));
        assert!(held_cache.last_change_id < *write);
    }
    assert_eq!(method(&reply, 0, "error")["type"], "serverUnavailable");
    if fail_manifest {
        drop(flag);
    } else {
        exercise_caller_minimum(test, key, doc, floor_at_start, *write, flag).await;
    }

    let read = get(key, json!([singleton(doc)])).await;
    let read = method(&read, 0, "Email/get");
    assert_eq!(exact(&read["state"]), *write);
    assert_eq!(read["list"][0]["keywords"]["$flagged"], true);
}

async fn exercise_caller_minimum(
    test: &TestServer,
    key: &Account,
    doc: u32,
    published: u64,
    write: u64,
    hold: Flag,
) {
    let id = key.id().document_id();
    let keys = za_unlock(test, id, key.secret()).await;
    let content_need = || ViewNeed::Content {
        deadline: Instant::now() + DEADLINE,
    };
    for minimum in [None, Some(published)] {
        let view = za_view_with_minimum(&test.server, id, &keys, ViewNeed::Map, minimum)
            .await
            .unwrap();
        assert_eq!(view.state(), published);
        drop(view);

        let view = za_view_with_minimum(&test.server, id, &keys, content_need(), minimum)
            .await
            .unwrap();
        assert_eq!(view.state(), published);
        assert!(view.content().document_ids().contains(doc));
    }
    let map = za_view_with_minimum(&test.server, id, &keys, ViewNeed::Map, Some(write)).await;
    assert!(matches!(
        map,
        Err(ViewError::NoMap(NoMapReason::Transient(
            TransientClass::Validation
        )))
    ));
    let content = za_view_with_minimum(&test.server, id, &keys, content_need(), Some(write)).await;
    let reason = match content {
        Err(ViewError::Unavailable(
            reason @ Unavailable::Transient(TransientClass::Validation),
        )) => reason,
        _ => panic!("content must report validation exhaustion for the same held cache"),
    };
    let error = jmap::email::za_search::za_unavailable(&"text", false, &reason);
    assert!(error.matches(trc::EventType::Jmap(trc::JmapEvent::UnsupportedFilter)));
    assert!(
        error
            .value_as_str(trc::Key::Details)
            .unwrap()
            .starts_with("text: ")
    );

    let server = test.server.clone();
    let task_keys = keys.clone();
    let budget = Duration::from_secs(2);
    let deadline = Instant::now() + budget;
    let first_round = HookParked::start(id, set_search_snapshot_pause, async move {
        za_view_with_minimum(
            &server,
            id,
            &task_keys,
            ViewNeed::Content { deadline },
            Some(write),
        )
        .await
    })
    .await;
    let deadline = tokio::time::Instant::from_std(deadline);
    // Allow a below-minimum comparison before expiry, with enough elapsed
    // time to distinguish the original deadline from a renewed budget.
    tokio::time::sleep_until(deadline - budget / 2).await;
    // Arm the second pause before releasing the first. The one-shot hook
    // has already been consumed, so this arrival proves a new round began.
    let second_round = HookParked::start(id, set_search_snapshot_pause, first_round.finish()).await;
    assert!(
        tokio::time::Instant::now() < deadline,
        "the held cache must trigger a retry before the original deadline"
    );
    tokio::time::sleep_until(deadline).await;
    let expired = tokio::select! {
        biased;
        _ = tokio::time::sleep_until(deadline + budget / 4) => {
            panic!("the retried view exceeded its original deadline");
        }
        result = second_round.finish() => result,
    };
    assert!(matches!(
        expired,
        Err(ViewError::Unavailable(Unavailable::NotCurrent)),
    ));

    let server = test.server.clone();
    let refreshed = HookParked::start(id, set_search_snapshot_pause, async move {
        za_view_with_minimum(&server, id, &keys, ViewNeed::Map, Some(write)).await
    })
    .await;
    drop(hold);
    let refreshed = tokio::time::timeout(DEADLINE, refreshed.finish())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(refreshed.state(), write);
    assert!(refreshed.cache.email_by_id(&doc).unwrap().change_id() >= write);
}

async fn case_import_requires_own_change_id(test: &TestServer, key: &Account) {
    let id = key.id().document_id();
    let subject = "Task 11 import floor";
    let root = message_with(subject, "root");
    let thread = deliver_doc(test, key, &root).await;
    let blob = upload(key, &reply_to(&root, subject, "pending insert")).await;
    let before = get(key, json!([])).await;
    let published = publication(test, key).await;
    assert_eq!(State::parse(before.state()), Some(State::Exact(published)));
    // Ensure the request's pre-pass and selected post-pass both see a cache
    // already covered by the resident checkpoint when the hold is armed.
    assert_eq!(
        run_search_pass(test, id, key.secret()).await,
        PassOutcome::Current
    );
    let entry = test.server.inner.cache.za_search.get(id).unwrap();
    assert!(entry.read_loaded().await.unwrap().scanned_to >= published);
    let cache = test.server.get_cached_messages(id).await.unwrap();
    assert_eq!(cache.last_change_id, published);
    let before_hash = durable_hash(test, id).await;
    // A pass reads the durable counter directly. Keep both request attempts
    // busy at their real pass slot while the independently read Map can serve.
    let busy = entry.pass_lock.lock().await;
    let hold = Flag::arm(set_search_cache_hold, id);
    let response = key
        .jmap_method_call("Email/import", import_args(key, "i", &blob, mailbox_ids()))
        .await;
    let insertions = insertions_after(test, id, published).await;
    let [(write, doc)] = insertions.as_slice() else {
        panic!("expected exactly one import insertion: {insertions:?}");
    };
    assert!(*write > published);
    assert_eq!(durable_hash(test, id).await, before_hash);
    assert!(Arc::ptr_eq(
        &test.server.get_cached_messages(id).await.unwrap(),
        &cache
    ));
    assert_eq!(method(&response, 0, "error")["type"], "serverUnavailable");
    drop(hold);
    drop(busy);

    let expected = mapped(thread, *doc);
    let refreshed = get(key, json!([&expected])).await;
    let refreshed = method(&refreshed, 0, "Email/get");
    assert_eq!(refreshed["list"][0]["id"], expected);
    assert!(exact(&refreshed["state"]) >= *write);
}
