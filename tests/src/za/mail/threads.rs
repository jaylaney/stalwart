/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{Traced, deliver, key_mail_account, message_ids, message_with, new_message, reply_to};
use crate::{
    utils::{
        account::Account,
        jmap::JmapResponse,
        server::TestServer,
        za::{
            Keyless, force_loss, lower_changes_history, manifest_bytes, pop3_delete, purge_account,
            read_manifest, run_search_pass, search_config_guard, simulate_restart,
            wait_for_delivery,
        },
    },
    za::{STRONG, user_permissions},
};
use common::za::search::trace::{ZA_SEARCH_EVENT_TYPE, ZA_SEARCH_PASS};
use email::{mailbox::INBOX_ID, message::za::search::pass::PassOutcome};
use jmap_proto::types::state::State;
use serde_json::{Value, json};
use store::{query::log::Change, za_search::manifest::ManifestValue};
use types::{collection::SyncCollection, id::Id};

mod protocol;
mod races;
mod task12;
mod task13;

const SENDER: &str = "sender@example.org";
const KEY: &str = "keymail50@example.com";
const RACE_KEY: &str = "keymail51@example.com";
const PLAIN: &str = "plainmail-task11@example.com";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mapped identity and snapshot tests...");
    let key = key_mail_account(test, KEY, STRONG).await;

    // First behavioral RED uses the existing wire API and actual persisted
    // index. It needs no Task 10 view or Task 11 hook API.
    let (root, root_id, reply_id) = case_mapped_get_and_query(test, &key).await;
    case_set_response_view_after_mutations(test, &key, Some((&root, root_id))).await;
    case_same_call_reference(&key, &root, root_id).await;
    case_import_response_state_includes_publication(test, &key, &root, root_id).await;
    case_keyless_stale_prefix(test, &key, root_id, reply_id).await;
    protocol::test(test, &key, &root, root_id).await;

    let race_key = key_mail_account(test, RACE_KEY, STRONG).await;
    races::test(test, &race_key).await;

    let plain = test
        .account("admin@example.com")
        .create_user_account(PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    case_set_response_view_after_mutations(test, &plain, None).await;
    assert!(
        manifest_bytes(test, plain.id().document_id())
            .await
            .is_none()
    );

    for account in [&key, &race_key, &plain] {
        test.destroy_all_mailboxes(account).await;
    }
    task12::test(test).await;
    task13::test(test).await;
    test_compaction(test).await;
    test.blob_expire_all().await;
    wait_for_delivery(test).await;
}

fn method<'a>(reply: &'a JmapResponse, ordinal: usize, name: &str) -> &'a Value {
    let response = &reply.0["methodResponses"][ordinal];
    assert_eq!(response[0], name, "{response}");
    &response[1]
}

fn email_id(value: &Value) -> Id {
    value.as_str().expect("Email ID").parse().unwrap()
}

fn exact(value: &Value) -> u64 {
    match value.as_str().and_then(State::parse) {
        Some(State::Exact(change_id)) => change_id,
        _ => panic!("Expected an exact JMAP state, got {value}"),
    }
}

fn mapped(thread: u32, doc: u32) -> String {
    Id::from_parts(thread, doc).to_string()
}

fn singleton(doc: u32) -> String {
    mapped(doc, doc)
}

fn mailbox_ids() -> Value {
    json!({ (Id::from(INBOX_ID).to_string()): true })
}

async fn deliver_doc(test: &TestServer, account: &Account, message: &str) -> u32 {
    let account_id = account.id().document_id();
    let before = message_ids(test, account_id).await;
    deliver(test, SENDER, account.name(), message).await;
    new_message(test, account_id, &before).await
}

async fn get(account: &Account, ids: Value) -> JmapResponse {
    account
        .jmap_method_call(
            "Email/get",
            json!({
                "accountId": account.id_string(), "ids": ids,
                "properties": ["id", "threadId", "keywords"]
            }),
        )
        .await
}

async fn set(account: &Account, mut args: Value) -> JmapResponse {
    args["accountId"] = json!(account.id_string());
    account.jmap_method_call("Email/set", args).await
}

fn parent_message_id(raw: &str) -> &str {
    raw.lines()
        .find_map(|line| line.strip_prefix("Message-ID: "))
        .unwrap()
        .trim_matches(['<', '>'])
}

fn email_create(account: &Account, subject: &str, parent: Option<&str>) -> Value {
    let mut value = json!({
        "mailboxIds": mailbox_ids(),
        "subject": subject,
        "from": [{ "email": account.name() }],
        "to": [{ "email": account.name() }],
        "textBody": [{ "partId": "text", "type": "text/plain" }],
        "bodyValues": { "text": { "value": "Task 11 test message" } }
    });
    if let Some(parent) = parent {
        value["inReplyTo"] = json!([parent_message_id(parent)]);
    }
    value
}

async fn insertions_after(test: &TestServer, account_id: u32, after: u64) -> Vec<(u64, u32)> {
    let records = test
        .server
        .store()
        .za_changes_records_range(account_id, SyncCollection::Email, after, u64::MAX)
        .await
        .unwrap();
    assert!(!records.truncated, "{records:?}");
    records
        .records
        .into_iter()
        .flat_map(|(change_id, changes)| {
            changes.into_iter().filter_map(move |change| match change {
                Change::InsertItem(raw_id) => Some((change_id, Id::from(raw_id).document_id())),
                _ => None,
            })
        })
        .collect()
}

async fn published_insert_counts(traces: Traced, account_id: u32) -> Vec<u64> {
    traces
        .finish()
        .await
        .into_iter()
        .filter(|event| {
            event.account_id == Some(u64::from(account_id)) && event.details == ZA_SEARCH_PASS
        })
        .map(|event| {
            event
                .keys
                .iter()
                .find_map(|(key, value)| match (key, value) {
                    (trc::Key::Total, trc::Value::UInt(count)) => Some(*count),
                    _ => None,
                })
                .expect("the pass reported its insert count")
        })
        .collect()
}

async fn publication(test: &TestServer, account: &Account) -> u64 {
    let Some(ManifestValue::Index(manifest)) =
        read_manifest(test, account.id().document_id()).await
    else {
        panic!("Expected a search index for {}", account.name());
    };
    manifest.newest_row.change_id
}

async fn case_mapped_get_and_query(test: &TestServer, key: &Account) -> (String, u32, u32) {
    let subject = "Task 11 mapped id";
    let root = message_with(subject, "root");
    let root_id = deliver_doc(test, key, &root).await;
    let reply_id = deliver_doc(test, key, &reply_to(&root, subject, "reply")).await;
    assert_ne!(root_id, reply_id);

    assert!(matches!(
        run_search_pass(test, key.id().document_id(), key.secret()).await,
        PassOutcome::Advanced {
            published: true,
            ..
        }
    ));

    // Verify the oracle independently before calling the handler. This
    // distinguishes a failure to map protocol IDs from a bad reply fixture.
    let keys = crate::utils::za::za_unlock(test, key.id().document_id(), key.secret()).await;
    let entry = test
        .server
        .za_search_entry(key.id().document_id(), &keys)
        .await
        .unwrap();
    {
        let index = entry.read_loaded().await.unwrap();
        assert_eq!(index.thread.current_thread(reply_id), root_id);
    }

    let current = mapped(root_id, reply_id);
    let stale = singleton(reply_id);
    let reply = get(key, Value::Null).await;
    let response = method(&reply, 0, "Email/get");
    let listed = response["list"].as_array().unwrap();
    let reply = listed
        .iter()
        .find(|item| email_id(&item["id"]).document_id() == reply_id)
        .unwrap();
    assert_eq!(
        reply["id"], current,
        "Email/get emitted the wrong prefix: {reply}"
    );
    assert_eq!(reply["threadId"], Id::from(root_id).to_string());
    assert!(exact(&response["state"]) >= publication(test, key).await);

    let reply = get(key, json!([&current, &stale])).await;
    let response = method(&reply, 0, "Email/get");
    assert_eq!(response["list"].as_array().unwrap().len(), 1, "{response}");
    assert_eq!(response["list"][0]["id"], current);
    assert_eq!(response["notFound"], json!([&stale]));

    for anchor in [&current, &stale] {
        let reply = key
            .jmap_method_call(
                "Email/query",
                json!({ "accountId": key.id_string(), "anchor": anchor, "limit": 1 }),
            )
            .await;
        if anchor == &current {
            assert_eq!(method(&reply, 0, "Email/query")["ids"], json!([&current]));
        } else {
            assert_eq!(method(&reply, 0, "error")["type"], "anchorNotFound");
        }
    }

    (root, root_id, reply_id)
}

async fn case_set_response_view_after_mutations(
    test: &TestServer,
    account: &Account,
    root: Option<(&str, u32)>,
) {
    let update_id = deliver_doc(test, account, &message_with("Mixed Set U", "update")).await;
    let destroy_id = deliver_doc(test, account, &message_with("Mixed Set D", "destroy")).await;
    let before = get(account, Value::Null).await;
    let before_body = method(&before, 0, "Email/get");
    let ids = before_body["list"].as_array().unwrap();
    let current = |document_id| {
        ids.iter()
            .find(|item| email_id(&item["id"]).document_id() == document_id)
            .unwrap()["id"]
            .as_str()
            .unwrap()
    };
    let update = current(update_id);
    let destroy = current(destroy_id);
    let old_state = &before_body["state"];
    let reply = set(
        account,
        json!({
            "ifInState": old_state,
            "create": { "c": match root {
                Some((root, _)) => email_create(account, "Re: Task 11 mapped id", Some(root)),
                None => email_create(account, "Ordinary mixed Set C", None),
            }},
            "update": { update: { "keywords/$flagged": true } },
            "destroy": [destroy]
        }),
    )
    .await;
    let response = method(&reply, 0, "Email/set");
    assert_eq!(&response["oldState"], old_state);
    assert!(response["updated"].get(update).is_some(), "{response}");
    assert_eq!(response["destroyed"], json!([destroy]));
    assert!(response["notUpdated"].is_null(), "{response}");
    assert!(response["notDestroyed"].is_null(), "{response}");
    let created = &response["created"]["c"];
    let created_id = email_id(&created["id"]);
    if let Some((_, root_id)) = root {
        assert_eq!(created_id.prefix_id(), root_id);
        assert_eq!(created["threadId"], Id::from(root_id).to_string());
    }
    // A Set deletion leaves a pending deletion-only pass. Run a read to
    // observe the state directly, and require that the deletion-only pass
    // did not change the Email item state reported by Set.
    let after = get(account, json!([&created["id"], update, destroy])).await;
    let after = method(&after, 0, "Email/get");
    assert_eq!(after["state"], response["newState"], "{response} / {after}");
    assert_eq!(after["notFound"], json!([destroy]));
    assert!(
        after["list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == update && item["keywords"]["$flagged"] == true)
    );
    assert!(
        after["list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == created["id"] && item["threadId"] == created["threadId"])
    );
    let noop = set(account, json!({ "ifInState": response["newState"] })).await;
    assert_eq!(
        method(&noop, 0, "Email/set")["oldState"],
        response["newState"]
    );

    let update_only = set(
        account,
        json!({ "update": { update: { "keywords/$flagged": null } } }),
    )
    .await;
    let updated = method(&update_only, 0, "Email/set");
    assert!(updated["updated"].get(update).is_some(), "{updated}");
    let after_update = get(account, json!([update])).await;
    assert_eq!(
        method(&after_update, 0, "Email/get")["state"],
        updated["newState"]
    );

    let destroy_only = set(account, json!({ "destroy": [update] })).await;
    let destroyed = method(&destroy_only, 0, "Email/set");
    assert_eq!(destroyed["destroyed"], json!([update]));
    let after_destroy = get(account, json!([update])).await;
    assert_eq!(
        method(&after_destroy, 0, "Email/get")["state"],
        destroyed["newState"]
    );
}

async fn case_same_call_reference(key: &Account, root: &str, root_id: u32) {
    let valid = email_create(key, "Re: Task 11 mapped id", Some(root));
    let mut dependent = email_create(key, "Dependent create", None);
    // The server-set id property is read-only. Reaching invalidProperties
    // proves `#first` resolved while its response object was still internal.
    dependent["id"] = json!("#first");
    let reply = key
        .jmap_method_calls(json!([
            ["Email/set", {
                "accountId": key.id_string(),
                "create": { "dependent": dependent, "first": valid }
            }, "writer"],
            ["Email/get", {
                "accountId": key.id_string(), "ids": ["#first"], "properties": ["id", "threadId"]
            }, "created-ref"],
            ["Email/get", {
                "accountId": key.id_string(),
                "#ids": { "resultOf": "writer", "name": "Email/set", "path": "/created/first/id" },
                "properties": ["id", "threadId"]
            }, "result-ref"]
        ]))
        .await;
    let response = method(&reply, 0, "Email/set");
    let created = &response["created"]["first"];
    let id = email_id(&created["id"]);
    assert_ne!(id.document_id(), root_id);
    assert_eq!(id.prefix_id(), root_id);
    assert_eq!(created["threadId"], Id::from(root_id).to_string());
    assert_eq!(
        response["notCreated"]["dependent"]["type"],
        "invalidProperties"
    );
    for ordinal in [1, 2] {
        let found = method(&reply, ordinal, "Email/get");
        assert_eq!(
            found["list"],
            json!([{
                "id": created["id"], "threadId": created["threadId"]
            }]),
            "{found}"
        );
        assert_eq!(found["state"], response["newState"]);
    }
}

async fn upload(account: &Account, raw: &str) -> String {
    account
        .jmap_client()
        .await
        .upload(None, raw.as_bytes().to_vec(), None)
        .await
        .unwrap()
        .take_blob_id()
}

fn import_args(account: &Account, creation: &str, blob: &str, boxes: Value) -> Value {
    json!({
        "accountId": account.id_string(),
        "emails": { creation: { "blobId": blob, "mailboxIds": boxes } },
    })
}

async fn case_import_response_state_includes_publication(
    test: &TestServer,
    key: &Account,
    root: &str,
    root_id: u32,
) {
    let raw = reply_to(
        root,
        "Task 11 mapped id",
        "import after selected publication",
    );
    let blob = upload(key, &raw).await;
    let before = get(key, json!([])).await;
    let initial_state = &method(&before, 0, "Email/get")["state"];
    let account_id = key.id().document_id();
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let reply = key
        .jmap_method_calls(json!([
            ["Email/import", import_args(key, "i", &blob, mailbox_ids()), "import-call"],
            ["Email/get", {
                "accountId": key.id_string(),
                "#ids": { "resultOf": "import-call", "name": "Email/import", "path": "/created/i/id" },
                "properties": ["id", "threadId"]
            }, "follow"]
        ]))
        .await;
    let imported = method(&reply, 0, "Email/import");
    let followed = method(&reply, 1, "Email/get");
    assert_eq!(&imported["oldState"], initial_state);
    let created = &imported["created"]["i"];
    let insertions = insertions_after(test, account_id, exact(initial_state)).await;
    let [insertion] = insertions.as_slice() else {
        panic!("expected exactly one import insertion: {insertions:?}");
    };
    assert_eq!(insertion.1, email_id(&created["id"]).document_id());
    let published = publication(test, key).await;
    assert!(published > insertion.0);
    assert_eq!(exact(&imported["newState"]), published);
    assert_eq!(published_insert_counts(traces, account_id).await, [1]);
    assert_eq!(email_id(&created["id"]).prefix_id(), root_id);
    assert_eq!(created["threadId"], Id::from(root_id).to_string());
    assert_eq!(followed["list"][0]["id"], created["id"]);
    assert_eq!(followed["state"], imported["newState"]);

    // An earlier call acknowledges a successful insert. The selected final
    // candidate rejects all its entries, but still publishes that pending work.
    let earlier = upload(key, &reply_to(root, "Task 11 mapped id", "earlier")).await;
    let bad_box = json!({ (Id::from(u32::MAX - 1).to_string()): true });
    // Ensure the pre-request and selected post-write passes both publish.
    deliver_doc(
        test,
        key,
        &message_with("Task 11 pending backlog", "pre-pass"),
    )
    .await;
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let reply = key
        .jmap_method_calls(json!([
            ["Email/import", import_args(key, "earlier", &earlier, mailbox_ids()), "earlier"],
            ["Email/import", import_args(key, "invalid", &blob, bad_box), "last"],
            ["Email/get", { "accountId": key.id_string(), "ids": null, "properties": ["id"] }, "get"]
        ]))
        .await;
    let earlier = method(&reply, 0, "Email/import");
    let final_candidate = method(&reply, 1, "Email/import");
    let later = method(&reply, 2, "Email/get");
    let early_id = email_id(&earlier["created"]["earlier"]["id"]);
    assert_eq!(early_id.prefix_id(), early_id.document_id(), "{earlier}");
    assert!(
        final_candidate["notCreated"].get("invalid").is_some(),
        "{final_candidate}"
    );
    assert!(final_candidate["created"].is_null(), "{final_candidate}");
    assert_eq!(final_candidate["oldState"], earlier["newState"]);
    assert_eq!(
        exact(&final_candidate["newState"]),
        publication(test, key).await
    );
    assert!(exact(&earlier["newState"]) < exact(&final_candidate["newState"]));
    let later_id = mapped(root_id, early_id.document_id());
    assert!(
        later["list"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == later_id)
    );
    assert_eq!(later["state"], final_candidate["newState"]);
    assert_eq!(published_insert_counts(traces, account_id).await, [1, 1]);
}

async fn case_keyless_stale_prefix(test: &TestServer, key: &Account, root: u32, doc: u32) {
    let stale = singleton(doc);
    let current = mapped(root, doc);
    let keyed = set(
        key,
        json!({
            "update": { &stale: { "keywords/$flagged": true } },
            "destroy": [&stale],
        }),
    )
    .await;
    assert_eq!(
        method(&keyed, 0, "Email/set")["notUpdated"][&stale]["type"],
        "notFound"
    );

    let keyed = set(key, json!({ "destroy": [&stale] })).await;
    assert_eq!(
        method(&keyed, 0, "Email/set")["notDestroyed"][&stale]["type"],
        "notFound"
    );

    // Even a prefix different from both the map and the stored singleton
    // must work for the document-only keyless route.
    let unchecked = mapped(u32::MAX - 1, doc);
    let mut token = Keyless::bearer(&test.server, key.name()).await;
    let update = token
        .call(
            key,
            "Email/set",
            json!({
                "accountId": key.id_string(),
                "update": { (&unchecked): { "keywords/$seen": true } },
            }),
        )
        .await;
    assert_eq!(update[0], "Email/set");
    assert!(update[1]["updated"].get(&unchecked).is_some(), "{update}");
    let destroy = token
        .call(
            key,
            "Email/set",
            json!({
                "accountId": key.id_string(), "destroy": [&stale],
            }),
        )
        .await;
    assert_eq!(destroy[0], "Email/set");
    assert_eq!(destroy[1]["destroyed"], json!([&stale]));
    let readback = get(key, json!([&current])).await;
    assert_eq!(
        method(&readback, 0, "Email/get")["notFound"],
        json!([&current])
    );
}

pub(super) async fn test_recovery(test: &mut TestServer, key: &Account) {
    let admin = test.account("admin@example.com").clone();
    let plain = admin
        .create_user_account(
            "plainmail56@example.com",
            STRONG,
            "Rebuild threading control",
            &[],
            user_permissions(),
        )
        .await;
    for residual in [false, true] {
        case_r16_r20_rebuild_form(test, key, &plain, residual).await;
        for account in [key, &plain] {
            test.destroy_all_mailboxes(account).await;
        }
    }
    admin.destroy_account(plain).await;
    test.wait_for_tasks().await;
    for (name, recovery) in [
        (
            "keymail-task14-page-loss@example.com",
            PagingRecovery::Thread,
        ),
        (
            "keymail-task14-page-content@example.com",
            PagingRecovery::Content,
        ),
        (
            "keymail-task14-page-retention@example.com",
            PagingRecovery::Reconcile,
        ),
    ] {
        let fixture = key_mail_account(test, name, STRONG).await;
        case_initial_continuation_epoch_loss(test, &fixture, &admin, recovery).await;
        test.destroy_all_mailboxes(&fixture).await;
    }
}

async fn recovery_manifest(
    test: &TestServer,
    key: &Account,
) -> store::za_search::manifest::Manifest {
    let Some(ManifestValue::Index(manifest)) = read_manifest(test, key.id().document_id()).await
    else {
        panic!("recovery fixture needs a search index");
    };
    manifest
}

async fn trigger_recovery_pass(account: &Account) {
    let reply = account
        .jmap_method_call("Mailbox/get", json!({"accountId": account.id_string()}))
        .await;
    assert_eq!(reply.name_at(0), "Mailbox/get", "{:?}", reply.0);
}

async fn case_r16_r20_rebuild_form(
    test: &TestServer,
    key: &Account,
    plain: &Account,
    residual: bool,
) {
    use common::za::test_hooks::{reset_za_search_counters, za_search_max_item_aliases};
    use trc::Key;
    println!("case_r16_r20_rebuild_form: pending parent={residual}");
    let id = key.id().document_id();
    let subject = if residual {
        "Rebuild missing parent"
    } else {
        "Rebuild indexed parent"
    };
    let a = message_with(subject, "root");
    let d = reply_to(&a, subject, "parent");
    let n = reply_to(&d, subject, "first descendant");
    let n2 = reply_to(&d, subject, "new descendant");
    let mut tracks = Vec::new();
    for account in [key, plain] {
        let root = deliver_doc(test, account, &a).await;
        let parent = deliver_doc(test, account, &d).await;
        let child = deliver_doc(test, account, &n).await;
        tracks.push((root, parent, child));
    }
    trigger_recovery_pass(key).await;
    let _cap = search_config_guard!(test, pass_cap: if residual { 1 } else { 2 });
    force_loss(test, id).await;
    let trace = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    reset_za_search_counters(id);
    trigger_recovery_pass(key).await;
    let pending = recovery_manifest(test, key)
        .await
        .cursor
        .expect("rebuild left work pending");
    assert_eq!(pending.next > tracks[0].1, !residual);

    for (account, (_, parent, _)) in [key, plain].into_iter().zip(&tracks) {
        pop3_delete(test, account, *parent).await;
    }
    let mut next = Vec::new();
    for account in [key, plain] {
        next.push(deliver_doc(test, account, &n2).await);
    }
    for _ in 0..6 {
        trigger_recovery_pass(key).await;
    }
    assert!(recovery_manifest(test, key).await.cursor.is_none());
    for ((account, (root, parent, child)), n2) in [key, plain].into_iter().zip(tracks).zip(next) {
        let reply = account
            .jmap_method_call("Thread/get", json!({"accountId": account.id_string()}))
            .await;
        let list = method(&reply, 0, "Thread/get")["list"].as_array().unwrap();
        let mut groups = Vec::new();
        for thread in list {
            let mut ids = thread["emailIds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|email| email_id(email).document_id())
                .collect::<Vec<_>>();
            assert!(!ids.contains(&parent));
            ids.sort_unstable();
            groups.push(ids);
        }
        groups.sort_unstable();
        let mut joined = vec![root, child, n2];
        joined.sort_unstable();
        if residual && account.id() == key.id() {
            // Spec 7.5: the lost parent never entered the cursor. Its two
            // replies still share D's hash but have no bridge to A.
            let mut replies = vec![child, n2];
            replies.sort_unstable();
            let mut separate = vec![vec![root], replies];
            separate.sort_unstable();
            assert_ne!(groups, [joined]);
            assert_eq!(groups, separate);
        } else {
            assert_eq!(groups, [joined]);
        }
        let reply = get(account, Value::Null).await;
        let listed = method(&reply, 0, "Email/get")["list"].as_array().unwrap();
        assert!(
            !listed
                .iter()
                .any(|item| email_id(&item["id"]).document_id() == parent)
        );
    }

    assert!(za_search_max_item_aliases(id) <= store::za_search::K - 1);
    let events = trace.finish().await;
    let passes = events
        .iter()
        .filter(|event| event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_PASS)
        .collect::<Vec<_>>();
    assert!(!passes.is_empty());
    for event in passes {
        assert!(
            event.keys.iter().all(|(key, _)| matches!(
                key,
                Key::Details
                    | Key::AccountId
                    | Key::Total
                    | Key::TotalFailures
                    | Key::Result
                    | Key::Version
                    | Key::ChangeId
                    | Key::Size
                    | Key::Used
            )),
            "unexpected search pass field: {}",
            event.text
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum PagingRecovery {
    Thread,
    Content,
    Reconcile,
}

async fn recovery_page(key: &Account, method: &str, since: &State, max: u32) -> JmapResponse {
    key.jmap_method_call(
        method,
        json!({
            "accountId": key.id_string(), "sinceState": since.to_string(), "maxChanges": max
        }),
    )
    .await
}

async fn case_initial_continuation_epoch_loss(
    test: &TestServer,
    key: &Account,
    admin: &Account,
    recovery: PagingRecovery,
) {
    use email::message::za::search::changes::ZaRetainedBound;
    println!("case_initial_continuation_epoch_loss: {recovery:?}");
    let id = key.id().document_id();
    for ordinal in 0..3 {
        deliver_doc(
            test,
            key,
            &message_with(&format!("Recovery page {ordinal}"), "pagingrecovery"),
        )
        .await;
    }
    trigger_recovery_pass(key).await;
    let original = recovery_manifest(test, key).await;
    let reconciliation = matches!(recovery, PagingRecovery::Reconcile);
    if reconciliation {
        for ordinal in 3..15 {
            deliver_doc(
                test,
                key,
                &message_with(&format!("Recovery page {ordinal}"), "pagingrecovery"),
            )
            .await;
        }
    }
    let total = if reconciliation { 15 } else { 3 };
    let entry = test.server.inner.cache.za_search.entry(id);
    let held = entry.pass_lock.lock().await;
    let mut tokens = Vec::new();
    for name in ["Email/changes", "Thread/changes"] {
        let first = recovery_page(key, name, &State::Initial, 1).await;
        let response = method(&first, 0, name);
        let token = State::parse(response["newState"].as_str().unwrap()).unwrap();
        let State::Intermediate(page) = &token else {
            panic!("expected a pinned first page: {response}")
        };
        assert_eq!((page.from_id, page.items_sent), (0, 1));
        assert_eq!(response["hasMoreChanges"], true);
        assert_eq!(response["created"].as_array().unwrap().len(), 1);
        tokens.push((
            name,
            token.clone(),
            page.to_id,
            email_id(&response["created"][0]),
        ));
    }
    drop(held);
    assert_eq!(
        recovery_manifest(test, key).await.indexed_to,
        original.indexed_to
    );

    match recovery {
        PagingRecovery::Thread => {
            force_loss(test, id).await;
            trigger_recovery_pass(key).await;
        }
        PagingRecovery::Content => {
            let hash = &original.content.as_ref().unwrap().hash;
            test.server
                .blob_store()
                .delete_blobs(&[hash.as_slice()])
                .await
                .unwrap();
            simulate_restart(test, id).await;
            let reply = key
                .jmap_method_call(
                    "Email/query",
                    json!({
                        "accountId": key.id_string(), "filter": {"text": "pagingrecovery"}
                    }),
                )
                .await;
            assert_eq!(
                method(&reply, 0, "Email/query")["ids"]
                    .as_array()
                    .unwrap()
                    .len(),
                total
            );
        }
        PagingRecovery::Reconcile => {
            lower_changes_history(admin, Some(5)).await;
            purge_account(test, admin, key).await;
            let bound = test.server.za_retained_bound(id).await.unwrap();
            assert!(bound > original.indexed_to.change_id);
            for _ in 0..8 {
                trigger_recovery_pass(key).await;
            }
            lower_changes_history(admin, None).await;
        }
    }
    let current = recovery_manifest(test, key).await;
    let bound = test.server.za_retained_bound(id).await.unwrap();
    assert!(
        current.cursor.is_none()
            && current.content_to.cursor.is_none()
            && current.reconciliation.is_none()
    );
    if matches!(recovery, PagingRecovery::Thread) {
        assert_eq!(
            bound, 0,
            "the refusal must be caused by loss of the identity epoch"
        );
        assert!(current.epoch > original.epoch);
    } else {
        assert_eq!(current.epoch, original.epoch);
        if matches!(recovery, PagingRecovery::Content) {
            assert_ne!(current.content, original.content);
        }
    }
    for (name, token, end, first) in tokens {
        assert!(bound <= end);
        let continued = recovery_page(key, name, &token, 50).await;
        let resumed = if matches!(recovery, PagingRecovery::Thread) {
            assert!(end < current.epoch);
            assert_eq!(
                continued.error_type_at(0),
                Some("cannotCalculateChanges"),
                "{:?}",
                continued.0
            );
            None
        } else {
            let response = method(&continued, 0, name);
            assert_eq!(
                State::parse(response["newState"].as_str().unwrap()),
                Some(State::new_exact(end))
            );
            assert_eq!(response["hasMoreChanges"], false);
            let mut seen = response["created"]
                .as_array()
                .unwrap()
                .iter()
                .map(email_id)
                .collect::<Vec<_>>();
            assert!(!seen.contains(&first));
            seen.push(first);
            seen.sort_unstable();
            Some(seen)
        };
        let fresh = recovery_page(key, name, &State::Initial, 50).await;
        let mut fresh = method(&fresh, 0, name)["created"]
            .as_array()
            .unwrap()
            .iter()
            .map(email_id)
            .collect::<Vec<_>>();
        fresh.sort_unstable();
        assert_eq!(fresh.len(), total, "{name}");
        assert!(fresh.windows(2).all(|pair| pair[0] != pair[1]));
        if let Some(resumed) = resumed {
            assert_eq!(resumed, fresh);
        }
    }
}

async fn test_compaction(test: &mut TestServer) {
    let admin = test.account("admin@example.com").clone();
    let merged = key_mail_account(test, "keymail-task15-deleted-merged@example.com", STRONG).await;
    case_r3_deleted_merged_message_after_compaction(test, &merged).await;
    test.destroy_all_mailboxes(&merged).await;

    let pruned = key_mail_account(test, "keymail-task15-pruned@example.com", STRONG).await;
    case_r17_tombstone_gone_after_retention(test, &admin, &pruned).await;
    test.destroy_all_mailboxes(&pruned).await;

    let paged = key_mail_account(test, "keymail-task15-paged@example.com", STRONG).await;
    case_initial_continuation_folded_alias(test, &admin, &paged).await;
    test.destroy_all_mailboxes(&paged).await;

    case_r21_amplified_merge_probe_class(test).await;
}

fn with_references(raw: String, parents: &[&str]) -> String {
    let header = parents
        .iter()
        .map(|parent| format!("<{}>", parent_message_id(parent)))
        .collect::<Vec<_>>()
        .join(" ");
    raw.replacen("\r\n\r\n", &format!("\r\nReferences: {header}\r\n\r\n"), 1)
}

async fn compacted_thread(
    test: &TestServer,
    key: &Account,
) -> store::za_search::thread::ThreadPart {
    crate::utils::za::za_search_thread_part(test, key.id().document_id(), key.secret()).await
}

async fn case_r3_deleted_merged_message_after_compaction(test: &TestServer, key: &Account) {
    let _config = search_config_guard!(test, segments: 3);
    let id = key.id().document_id();
    let subject = "Retained merged deletion";
    let a = message_with(subject, "first root");
    let b = message_with(subject, "other root");
    let root = deliver_doc(test, key, &a).await;
    let member = deliver_doc(test, key, &b).await;
    deliver_doc(test, key, &reply_to(&a, subject, "child of the first root")).await;
    trigger_recovery_pass(key).await;
    let bridge = with_references(message_with(subject, "bridge"), &[&a, &b]);
    deliver_doc(test, key, &bridge).await;
    trigger_recovery_pass(key).await;
    let before = recovery_manifest(test, key).await;
    let part = compacted_thread(test, key).await;
    assert_eq!(part.current_thread(member), root);
    assert_eq!(part.record(member).unwrap().thread_id, member);
    assert!(
        part.aliases()
            .any(|alias| alias.source == member && alias.target == root)
    );
    assert_eq!(before.thread_blobs.len(), 2);
    pop3_delete(test, key, member).await;
    trigger_recovery_pass(key).await;
    let after = recovery_manifest(test, key).await;
    assert_eq!(after.thread_blobs.len(), 1);
    assert_eq!(after.newest_row, before.newest_row);
    assert_eq!(after.content, before.content);
    simulate_restart(test, id).await;
    let reloaded = compacted_thread(test, key).await;
    assert!(reloaded.record(member).unwrap().deleted.is_some());
    assert!(reloaded.aliases().any(|alias| alias.source == member));
    let reply = recovery_page(
        key,
        "Email/changes",
        &State::Exact(before.newest_row.change_id),
        20,
    )
    .await;
    assert_eq!(
        method(&reply, 0, "Email/changes")["destroyed"],
        json!([mapped(root, member)])
    );
}

async fn churn_retention(
    test: &TestServer,
    admin: &Account,
    key: &Account,
    live: u32,
    expired_through: u64,
) {
    use email::message::za::search::changes::ZaRetainedBound;

    // Keep the manifest current and generate enough publications for both
    // groups to drop the old rows. Flags advance the Email group alone.
    let _config = search_config_guard!(test, checkpoint: 1);
    for turn in 0..10 {
        let message = message_with(&format!("Retention publication {turn}"), "retained");
        deliver_doc(test, key, &message).await;
        trigger_recovery_pass(key).await;
    }
    let part = compacted_thread(test, key).await;
    let email_id = mapped(part.current_thread(live), live);
    for turn in 0..20 {
        let reply = set(
            key,
            json!({
                "update": { (&email_id): {"keywords/$flagged": turn % 2 == 0} },
            }),
        )
        .await;
        assert!(
            method(&reply, 0, "Email/set")["updated"]
                .as_object()
                .unwrap()
                .contains_key(&email_id)
        );
        trigger_recovery_pass(key).await;
    }
    lower_changes_history(admin, Some(8)).await;
    purge_account(test, admin, key).await;
    lower_changes_history(admin, None).await;
    let id = key.id().document_id();
    assert!(test.server.za_retained_bound(id).await.unwrap() > expired_through);
    for collection in [SyncCollection::Email, SyncCollection::Thread] {
        let marker = test
            .server
            .store()
            .za_log_first_retained(id, collection)
            .await
            .unwrap();
        assert!(
            marker.is_some_and(|change| change > expired_through),
            "{collection:?}: {marker:?}"
        );
    }
}

async fn case_r17_tombstone_gone_after_retention(
    test: &TestServer,
    admin: &Account,
    key: &Account,
) {
    use crate::utils::za::{HookParked, za_unlock};
    use common::za::test_hooks::set_search_sweep_pause;
    use email::message::za::search::pass::ZaSearchPass;

    let _no_compaction_yet = search_config_guard!(test, segments: 255);
    let id = key.id().document_id();
    let deleted = deliver_doc(test, key, &message_with("Old tombstone", "old content")).await;
    let live = deliver_doc(test, key, &message_with("Live survivor", "live content")).await;
    trigger_recovery_pass(key).await;
    let before = recovery_manifest(test, key).await;
    deliver_doc(
        test,
        key,
        &message_with("Park before log read", "parked pass"),
    )
    .await;
    let server = test.server.clone();
    let keys = za_unlock(test, id, key.secret()).await;
    let parked = HookParked::start(id, set_search_sweep_pause, async move {
        server.za_search_pass(id, &keys).await.unwrap()
    })
    .await;
    pop3_delete(test, key, deleted).await;
    assert!(matches!(
        parked.finish().await,
        PassOutcome::Advanced { .. }
    ));
    simulate_restart(test, id).await;
    let part = compacted_thread(test, key).await;
    let stamp = part
        .record(deleted)
        .unwrap()
        .deleted
        .expect("persisted tombstone");
    let assignment = part
        .revision_change_id(part.record(deleted).unwrap().revision)
        .unwrap();
    assert_eq!(assignment, before.newest_row.change_id);
    churn_retention(test, admin, key, live, stamp.max(assignment)).await;
    assert!(compacted_thread(test, key).await.record(deleted).is_some());

    let _compact = search_config_guard!(test, segments: 2);
    deliver_doc(
        test,
        key,
        &message_with("After retention", "compact both parts"),
    )
    .await;
    trigger_recovery_pass(key).await;
    let compacted = recovery_manifest(test, key).await;
    assert_eq!(compacted.thread_blobs.len(), 1);
    assert_eq!(compacted.content.as_ref().unwrap().length, 1);
    simulate_restart(test, id).await;
    assert!(compacted_thread(test, key).await.record(deleted).is_none());
    let keys = za_unlock(test, id, key.secret()).await;
    let head = compacted.content.unwrap();
    let bytes = super::stored_blob(test, &head.hash).await;
    let (_, records) = common::za::search::seal::parse_content_blob(
        keys.swk(),
        id,
        Some(head.segment),
        None,
        &bytes,
    )
    .unwrap();
    assert!(records.iter().any(|record| record.document_id() == live));
    assert!(records.iter().all(|record| record.document_id() != deleted));
}

async fn case_initial_continuation_folded_alias(test: &TestServer, admin: &Account, key: &Account) {
    let _no_compaction_yet = search_config_guard!(test, segments: 255);
    let id = key.id().document_id();
    let subject = "Continuation across a fold";
    let a = message_with(subject, "first root");
    let b = message_with(subject, "losing root");
    let root = deliver_doc(test, key, &a).await;
    let member = deliver_doc(test, key, &b).await;
    deliver_doc(test, key, &reply_to(&a, subject, "third item")).await;
    trigger_recovery_pass(key).await;
    let first = recovery_page(key, "Email/changes", &State::Initial, 2).await;
    let page = method(&first, 0, "Email/changes");
    assert_eq!(page["hasMoreChanges"], true);
    assert!(
        page["created"]
            .as_array()
            .unwrap()
            .contains(&json!(singleton(member)))
    );
    let token = State::parse(page["newState"].as_str().unwrap()).unwrap();
    let State::Intermediate(pinned) = &token else {
        panic!("expected an initial continuation")
    };
    deliver_doc(
        test,
        key,
        &with_references(message_with(subject, "bridge"), &[&a, &b]),
    )
    .await;
    trigger_recovery_pass(key).await;
    let merged = recovery_manifest(test, key).await;
    assert!(merged.newest_row.change_id > pinned.to_id);
    let part = compacted_thread(test, key).await;
    assert!(
        part.aliases()
            .any(|alias| alias.source == member && alias.target == root)
    );
    assert_eq!(part.record(member).unwrap().thread_id, member);
    churn_retention(test, admin, key, root, merged.newest_row.change_id).await;

    let _compact = search_config_guard!(test, segments: 2);
    deliver_doc(
        test,
        key,
        &message_with("Force alias fold", "after retention"),
    )
    .await;
    trigger_recovery_pass(key).await;
    assert_eq!(recovery_manifest(test, key).await.thread_blobs.len(), 1);
    simulate_restart(test, id).await;
    let part = compacted_thread(test, key).await;
    assert_eq!(part.record(member).unwrap().thread_id, root);
    assert!(!part.aliases().any(|alias| alias.source == member));
    let stale = recovery_page(key, "Email/changes", &token, 2).await;
    assert_eq!(stale.error_type_at(0), Some("cannotCalculateChanges"));
    let fresh = recovery_page(key, "Email/changes", &State::Initial, 30).await;
    let created = method(&fresh, 0, "Email/changes")["created"]
        .as_array()
        .unwrap();
    assert!(created.contains(&json!(mapped(root, member))));
}

async fn append_probe_messages(test: &TestServer, key: &Account, messages: &[String]) {
    use crate::utils::imap::{ImapConnection, Type};
    use imap_proto::ResponseType;

    let entry = test
        .server
        .inner
        .cache
        .za_search
        .entry(key.id().document_id());
    let pass = entry.pass_lock.lock().await;
    let mut imap = ImapConnection::connect(b"_compactprobe ").await;
    imap.authenticate(key.name(), key.secret()).await;
    imap.send_ok("SELECT INBOX").await;
    for message in messages {
        imap.send(&format!("APPEND INBOX {{{}}}", message.len()))
            .await;
        imap.assert_read(Type::Continuation, ResponseType::Ok).await;
        imap.send_untagged(message).await;
        imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    }
    drop(imap);
    drop(pass);
    test.wait_for_tasks().await;
}

async fn case_r21_amplified_merge_probe_class(test: &mut TestServer) {
    use store::za_search::{BlobHeader, blob::SEAL_OVERHEAD};

    let _config = search_config_guard!(test, segments: 255);
    let subject = "Amplified compaction probe";
    let a = message_with(subject, "first anchor");
    let a2 = reply_to(&a, subject, "second anchor");
    let b = message_with(subject, "third anchor");
    let b2 = reply_to(&b, subject, "fourth anchor");
    let bridge = || with_references(message_with(subject, "join the anchors"), &[&a, &b]);
    let mut sizes = Vec::new();
    for (name, already_joined) in [
        ("keymail-task15-probe-join@example.com", false),
        ("keymail-task15-probe-control@example.com", true),
    ] {
        let key = key_mail_account(test, name, STRONG).await;
        let mut messages = vec![a.clone(), a2.clone(), b.clone(), b2.clone()];
        if already_joined {
            messages.push(bridge());
        }
        for turn in 0..40 {
            messages.push(with_references(
                message_with(subject, &format!("left {turn}")),
                &[&a, &a2],
            ));
            messages.push(with_references(
                message_with(subject, &format!("right {turn}")),
                &[&b, &b2],
            ));
        }
        append_probe_messages(test, &key, &messages).await;
        trigger_recovery_pass(&key).await;
        trigger_recovery_pass(&key).await;
        let before = compacted_thread(test, &key).await;
        assert_eq!(before.records().count(), messages.len());
        assert_eq!(before.aliases().count(), usize::from(already_joined));
        deliver_doc(test, &key, &bridge()).await;
        trigger_recovery_pass(&key).await;
        let after = compacted_thread(test, &key).await;
        assert_eq!(
            after.aliases().count() - before.aliases().count(),
            usize::from(!already_joined)
        );
        let manifest = recovery_manifest(test, &key).await;
        let bytes = super::stored_blob(test, &manifest.thread_blobs.last().unwrap().hash).await;
        let (_, body) = BlobHeader::decode(&bytes).unwrap();
        let hidden = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
        assert_eq!(hidden, 65536 + SEAL_OVERHEAD);
        sizes.push(bytes.len());
        test.destroy_all_mailboxes(&key).await;
    }
    assert_eq!(sizes[0], sizes[1]);
}
