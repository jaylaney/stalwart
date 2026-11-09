/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Request-owned indexing against sealed LMTP/IMAP mail and the real store.

use super::{
    Traced, corrupt_blob, deliver, key_mail_account, message_ids, message_with, stored_blob,
    stored_hash,
};
use crate::{
    utils::{
        account::Account,
        imap::{ImapConnection, Type},
        pop3::{Pop3Connection, ResponseType as Pop3Reply},
        server::TestServer,
        za::{
            HookParked, expire_and_purge_blobs, index_holds, log_records_since, manifest_bytes,
            principal_blob_links, read_manifest, run_search_pass, search_config_guard,
            simulate_restart, temporary_blob_hashes, wait_for_delivery, write_manifest_bytes,
            za_setup_token, za_unlock,
        },
    },
    za::{
        tracing::{ws_call, ws_close, ws_connect},
        user_permissions,
    },
};
use common::auth::vault::ZaVaultExtra;
use common::za::{
    search::{
        ZaSearchConfig,
        cache::EvictReason,
        error::{LoadFailure, NoMapReason, TransientClass},
        load::entry_outcome,
        seal::za_search_key,
        setup::za_setup_manifest,
        trace::{ZA_SEARCH_EVENT_TYPE, ZA_SEARCH_LOST, ZA_SEARCH_PASS, ZA_SEARCH_SKIPPED},
    },
    test_hooks::{
        reset_za_search_counters, set_search_blob_fail_once, set_search_blob_slow,
        set_search_commit_pause, set_search_config_override, set_search_destroy_fail_once,
        set_search_destroy_pause, set_search_hold_expire, set_search_pass_pause,
        set_search_swap_pause, set_search_swap_skip, za_search_blob_reads, za_search_message_opens,
    },
};
use email::{
    cache::MessageCacheFetch,
    mailbox::{DRAFTS_ID, INBOX_ID},
    message::za::search::{
        ensure::{Unavailable, ZaSearchEnsure, is_current},
        pass::{PassOutcome, ZaSearchPass},
    },
};
use imap_proto::ResponseType;
use jmap_proto::types::state::State;
use registry::{schema::prelude::ObjectType, types::EnumImpl};
use serde_json::{Value, json};
use std::{
    str::FromStr,
    time::{Duration, Instant},
};
use store::{
    ValueKey,
    query::log::{Change, PartialChange},
    write::{
        BatchBuilder, BlobLink, BlobOp, RegistryClass, ValueClass, assert::AssertValue,
        blob::BlobLinkValue, blob_codec::BlobOpts,
    },
    za_search::{
        BlobHeader, Checkpoint,
        manifest::{FailingDocument, Inventory, Manifest, ManifestValue, manifest_hash},
    },
};
use trc::{EventType, Key, StoreEvent, Value as TraceValue};
use types::{
    blob_hash::BlobHash,
    collection::{Collection, SyncCollection},
    field::{Field, PrincipalField},
    id::Id,
};

const SENDER: &str = "sender@example.org";

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mail index pass tests...");
    let admin = test.account("admin@example.com").clone();
    let key = key_mail_account(test, "keymail46@example.com", "Keymail-46!").await;
    let split = key_mail_account(test, "keymail47@example.com", "Keymail-47!").await;
    let plain = admin
        .create_user_account(
            "plainmail50@example.com",
            "Plainmail-50!",
            "Search control",
            &[],
            user_permissions(),
        )
        .await;

    let triggers = key_mail_account(test, "keymail48@example.com", "Keymail-48!").await;
    let waits = key_mail_account(test, "keymail49@example.com", "Keymail-49!").await;
    let plain_triggers = admin
        .create_user_account(
            "plainmail51@example.com",
            "Plainmail-51!",
            "Search trigger control",
            &[],
            user_permissions(),
        )
        .await;

    case_blob_hook_does_not_create_mailboxes(test, &triggers, true).await;
    case_triggers(test, &triggers).await;
    clean_messages(test, &triggers).await;
    case_pipelined_imap_commands(test, &triggers, &plain_triggers).await;
    clean_messages(test, &triggers).await;
    case_busy_pass_does_not_wait_for_reload(test, &waits).await;
    clean_messages(test, &waits).await;
    case_reads_do_not_wait_for_a_pass(test, &waits).await;
    clean_messages(test, &waits).await;
    case_content_wait_budget(test, &waits).await;
    clean_messages(test, &waits).await;
    case_content_wait_uses_deadline_during_reload(test, &waits).await;
    clean_messages(test, &waits).await;
    case_content_loader_uses_original_deadline(test, &waits).await;
    clean_messages(test, &waits).await;
    case_time_cap_bounds_the_command(test, &waits).await;
    clean_messages(test, &waits).await;
    case_plain_control_unaffected(test, &plain_triggers).await;
    for import_first in [false, true] {
        case_r8_two_adding_calls(test, &triggers, import_first, true).await;
        clean_messages(test, &triggers).await;
        case_r8_two_adding_calls(test, &plain_triggers, import_first, false).await;
        clean_plain_messages(test, &plain_triggers).await;
    }
    for failure in [
        CandidateFailure::Reference,
        CandidateFailure::State,
        CandidateFailure::NotCreated,
    ] {
        case_r8_failed_candidate(test, &triggers, failure, true, true).await;
        clean_messages(test, &triggers).await;
        case_r8_failed_candidate(test, &plain_triggers, failure, false, true).await;
        clean_plain_messages(test, &plain_triggers).await;
        case_r8_failed_candidate(test, &triggers, failure, true, false).await;
    }
    case_r8_non_candidates(test, &triggers).await;
    clean_messages(test, &triggers).await;
    case_r8_generated_call(test, &triggers, &plain_triggers).await;
    clean_messages(test, &triggers).await;
    case_r8_foreign_plain_writer(test, &triggers, &plain_triggers).await;
    clean_messages(test, &triggers).await;
    case_r8_same_call_reference(test, &triggers).await;
    clean_messages(test, &triggers).await;

    case_setup_manifest(test, &key).await;
    case_persistence_and_thread_only_load(test, &key).await;
    clean_messages(test, &key).await;
    case_catch_up_after_offline_deliveries(test, &key, &plain).await;
    clean_messages(test, &key).await;
    case_multiappend_split_record_with_restart(test, &split).await;
    clean_messages(test, &split).await;
    case_pl4_unreadable_prefix_of_a_group(test, &split, false).await;
    clean_messages(test, &split).await;
    case_pl4_unreadable_prefix_of_a_group(test, &split, true).await;
    clean_messages(test, &split).await;

    let document = deliver_one(test, &key, "Keyword changes").await;
    advanced(test, &key, true).await;
    assert_eq!(pass(test, &key).await, PassOutcome::Current);
    case_flag_changes_write_nothing(test, &key, document).await;
    clean_messages(test, &key).await;

    let document = deliver_one(test, &key, "Checkpoint changes").await;
    advanced(test, &key, true).await;
    case_update_only_checkpoints(test, &key, document).await;
    clean_messages(test, &key).await;

    case_unreadable_insert_is_consumed_once(test, &key).await;
    clean_messages(test, &key).await;
    case_transient_blob_error_aborts_then_indexes(test, &key).await;
    clean_messages(test, &key).await;
    case_lost_assertion(test, &key).await;
    clean_messages(test, &key).await;
    case_invalidation_between_commit_and_swap(test, &key).await;
    clean_messages(test, &key).await;
    case_slow_store_hold_outlives_the_wait(test, &key, &admin).await;
    clean_messages(test, &key).await;
    case_expired_hold_does_not_publish(test, &key, &admin).await;
    clean_messages(test, &key).await;
    case_deletion_only_pass_writes_no_entry(test, &key).await;
    clean_messages(test, &key).await;
    case_repeated_delta_changes_nothing(test, &key).await;
    clean_messages(test, &key).await;

    for account in [&key, &split, &plain, &triggers, &waits, &plain_triggers] {
        test.destroy_all_mailboxes(account).await;
    }
    case_blob_hook_does_not_create_mailboxes(test, &triggers, false).await;
    test.wait_for_tasks().await;
    destroy_cases(test, &admin).await;
    println!("Zero-access mail index pass tests finished");
}

fn account_id(account: &Account) -> u32 {
    account.id().document_id()
}

async fn index(test: &TestServer, id: u32) -> Manifest {
    match read_manifest(test, id).await {
        Some(ManifestValue::Index(manifest)) => manifest,
        other => panic!("account {id} has no index manifest: {other:?}"),
    }
}

async fn pass(test: &TestServer, account: &Account) -> PassOutcome {
    run_search_pass(test, account_id(account), account.secret()).await
}

async fn advanced(test: &TestServer, account: &Account, published: bool) -> Checkpoint {
    match pass(test, account).await {
        PassOutcome::Advanced {
            published: actual,
            indexed_to,
        } if actual == published => indexed_to,
        other => panic!(
            "{}: expected Advanced(published={published}), got {other:?}",
            account.name()
        ),
    }
}

async fn change_id(test: &TestServer, id: u32) -> u64 {
    test.server
        .get_cached_messages(id)
        .await
        .unwrap()
        .last_change_id
}

async fn deliver_one(test: &TestServer, account: &Account, subject: &str) -> u32 {
    let before = message_ids(test, account_id(account)).await;
    deliver(
        test,
        SENDER,
        account.name(),
        &message_with(subject, "Index fixture body"),
    )
    .await;
    super::new_message(test, account_id(account), &before).await
}

async fn destroy(account: &Account, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let reply = account
        .jmap_method_call(
            "Email/set",
            json!({ "accountId": account.id_string(), "destroy": ids }),
        )
        .await;
    let destroyed = reply
        .pointer("/methodResponses/0/1/destroyed")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("Email/set destroy: {:?}", reply.0));
    assert_eq!(destroyed.len(), ids.len(), "{:?}", reply.0);
}

fn email_id(document_id: u32) -> String {
    Id::from_parts(document_id, document_id).to_string()
}

async fn clean_messages(test: &TestServer, account: &Account) {
    let expected = message_ids(test, account_id(account)).await.len();
    if expected == 0 {
        destroy(account, &[]).await;
    } else {
        // Search spec 6.2: use the IDs emitted by Get after the request's
        // pre-pass. The result reference avoids a second request pre-pass
        // changing their prefixes before the keyed Set checks them.
        let reply = account
            .jmap_method_calls(json!([
                ["Email/get", {
                    "accountId": account.id_string(), "ids": null, "properties": ["id"],
                }, "ids"],
                ["Email/set", {
                    "accountId": account.id_string(),
                    "#destroy": { "resultOf": "ids", "name": "Email/get", "path": "/list/*/id" },
                }, "cleanup"]
            ]))
            .await;
        let responses = reply.0["methodResponses"].as_array().unwrap();
        assert_eq!(responses[0][0], "Email/get", "{:?}", reply.0);
        assert_eq!(responses[1][0], "Email/set", "{:?}", reply.0);
        assert_eq!(
            responses[1][1]["destroyed"].as_array().map(Vec::len),
            Some(expected),
            "{:?}",
            reply.0
        );
        advanced(test, account, false).await;
    }
    test.wait_for_tasks().await;
}

async fn loaded(test: &TestServer, id: u32) -> common::za::search::cache::LoadedGuard {
    test.server
        .inner
        .cache
        .za_search
        .get(id)
        .unwrap()
        .read_loaded()
        .await
        .unwrap()
}

async fn assert_held(test: &TestServer, id: u32, documents: &[u32], count: usize) {
    for (pos, &document) in documents.iter().enumerate() {
        assert_eq!(
            index_holds(test, id, document).await,
            pos < count,
            "document {document}"
        );
    }
}

async fn assert_manifest_links(test: &TestServer, id: u32, manifest: &Manifest) {
    let mut expected = manifest
        .thread_blobs
        .iter()
        .map(|b| (b.hash.clone(), b.size as usize))
        .collect::<Vec<_>>();
    if let Some(head) = &manifest.content {
        let mut hash = head.hash.clone();
        for pos in 0..head.length {
            let bytes = stored_blob(test, &hash).await;
            expected.push((hash, bytes.len()));
            let (header, _) = BlobHeader::decode(&bytes).unwrap();
            if let Some((previous, _)) = header.predecessor {
                assert!(pos + 1 < head.length);
                hash = previous;
            } else {
                assert_eq!(pos + 1, head.length);
                break;
            }
        }
    }
    expected.sort_by(|(left, _), (right, _)| left.as_slice().cmp(right.as_slice()));
    assert_eq!(principal_blob_links(test, id).await, expected);
    for (hash, size) in expected {
        assert_eq!(stored_blob(test, &hash).await.len(), size);
    }
}

async fn mailbox_get(account: &Account) {
    let reply = account
        .jmap_method_call("Mailbox/get", json!({ "accountId": account.id_string() }))
        .await;
    assert_eq!(
        reply.0["methodResponses"][0][0], "Mailbox/get",
        "{:?}",
        reply.0
    );
}

async fn append_one(imap: &mut ImapConnection, subject: &str) {
    let message = message_with(subject, "Request trigger fixture");
    imap.send(&format!("APPEND INBOX {{{}}}", message.len()))
        .await;
    imap.assert_read(Type::Continuation, ResponseType::Ok).await;
    imap.send_untagged(&message).await;
    let reply = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    assert!(reply.last().unwrap().contains("[APPENDUID "), "{reply:?}");
}

async fn upload_message(account: &Account, message: &str) -> String {
    account
        .jmap_client()
        .await
        .upload(None, message.as_bytes().to_vec(), None)
        .await
        .unwrap()
        .take_blob_id()
}

fn create_request(account: &Account, creation_id: &str, subject: &str) -> Value {
    let inbox = Id::from(INBOX_ID).to_string();
    json!({
        "accountId": account.id_string(),
        "create": { (creation_id): {
            "mailboxIds": { inbox: true },
            "from": [{ "email": account.name() }],
            "to": [{ "email": SENDER }],
            "subject": subject,
            "textBody": [{ "partId": "t", "type": "text/plain" }],
            "bodyValues": { "t": { "value": "Request trigger fixture" } },
        }},
    })
}

fn import_request(account: &Account, creation_id: &str, blob_id: &str) -> Value {
    let inbox = Id::from(INBOX_ID).to_string();
    json!({
        "accountId": account.id_string(),
        "emails": { (creation_id): { "blobId": blob_id, "mailboxIds": { inbox: true } } },
    })
}

fn created_id<'a>(body: &'a Value, creation_id: &str) -> &'a str {
    body["created"][creation_id]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("missing created ID for {creation_id}: {body:?}"))
}

async fn assert_indexed(test: &TestServer, id: u32, document: u32, since: u64) {
    let record = log_records_since(test, id, SyncCollection::Email, since)
        .await
        .into_iter()
        .find_map(|(change_id, changes)| {
            changes.into_iter().any(|change| {
                matches!(change, Change::InsertItem(item) if Id::from(item).document_id() == document)
            }).then_some(change_id)
        })
        .expect("message has an acknowledged insert record");
    assert!(index(test, id).await.indexed_to.change_id >= record);
    assert!(index_holds(test, id, document).await, "document {document}");
}

async fn case_blob_hook_does_not_create_mailboxes(test: &TestServer, key: &Account, fresh: bool) {
    let phase = if fresh {
        "fresh"
    } else {
        "after mailbox deletion"
    };
    println!("case_blob_hook_does_not_create_mailboxes: {phase}");
    let id = account_id(key);
    let mailbox_ids = async || {
        let mut ids = Vec::new();
        test.server
            .archives(id, Collection::Mailbox, Field::ARCHIVE, &(), |id, _| {
                ids.push(id);
                Ok(true)
            })
            .await
            .unwrap();
        ids
    };
    assert!(
        mailbox_ids().await.is_empty(),
        "{phase}: setup has mailbox rows"
    );
    let reply = key.jmap_method_call("Blob/upload", json!({
        "accountId": key.id_string(),
        "create": { "vcard": { "data": [{ "data:asText": "BEGIN:VCARD\r\nEND:VCARD\r\n" }] }},
    })).await;
    assert!(
        reply.0["methodResponses"][0][1]["created"]["vcard"]["id"].is_string(),
        "{:?}",
        reply.0
    );
    assert!(
        mailbox_ids().await.is_empty(),
        "{phase}: the Blob hook created mailboxes"
    );
    if fresh {
        // A real mailbox request still uses normal lazy provisioning.
        mailbox_get(key).await;
        assert!(!mailbox_ids().await.is_empty());
    }
}

async fn case_triggers(test: &TestServer, key: &Account) {
    println!("case_triggers");
    let id = account_id(key);
    let since = change_id(test, id).await;
    let document = deliver_one(test, key, "JMAP trigger backlog").await;
    assert!(index(test, id).await.indexed_to.change_id < change_id(test, id).await);
    mailbox_get(key).await;
    assert_indexed(test, id, document, since).await;

    let mut imap = ImapConnection::connect(b"_trigger ").await;
    imap.authenticate(key.name(), key.secret()).await;
    let since = change_id(test, id).await;
    let document = deliver_one(test, key, "IMAP trigger backlog").await;
    let before = index(test, id).await.indexed_to;
    imap.send_ok("NOOP").await;
    assert_eq!(index(test, id).await.indexed_to, before);
    imap.send_ok("SELECT INBOX").await;
    assert_indexed(test, id, document, since).await;

    let waiting = deliver_one(test, key, "POP3 and keyless JMAP backlog").await;
    let before = index(test, id).await.indexed_to;
    let mut pop3 = Pop3Connection::connect().await;
    pop3.authenticate(key.name(), key.secret()).await;
    pop3.send("STAT").await;
    pop3.assert_read(Pop3Reply::Ok).await;
    pop3.send("QUIT").await;
    pop3.assert_read(Pop3Reply::Ok).await;
    assert_eq!(index(test, id).await.indexed_to, before);

    let waiting_id = email_id(waiting);
    let mut socket = ws_connect(key.name(), key.secret()).await;
    let request = json!({
        "@type": "Request",
        "id": "1",
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [["Email/set", {
            "accountId": key.id_string(),
            "update": { &waiting_id: { "keywords/$flagged": true } },
        }, "flag"]],
    });
    let reply: Value =
        serde_json::from_str(&ws_call(&mut socket, &request.to_string()).await).unwrap();
    assert!(
        reply["methodResponses"][0][1]["updated"]
            .as_object()
            .unwrap()
            .contains_key(&waiting_id),
        "{reply}"
    );
    ws_close(socket).await;
    assert_eq!(index(test, id).await.indexed_to, before);

    let since = change_id(test, id).await;
    let before = message_ids(test, id).await;
    append_one(&mut imap, "APPEND post-write trigger").await;
    let appended = super::new_message(test, id, &before).await;
    assert_indexed(test, id, appended, since).await;

    let blob = upload_message(key, &message_with("Import trigger", "Imported")).await;
    let since = change_id(test, id).await;
    let reply = key
        .jmap_method_call("Email/import", import_request(key, "imported", &blob))
        .await;
    let imported = Id::from_str(created_id(&reply.0["methodResponses"][0][1], "imported"))
        .unwrap()
        .document_id();
    assert_indexed(test, id, imported, since).await;

    let since = change_id(test, id).await;
    let reply = key
        .jmap_method_call(
            "Email/set",
            create_request(key, "made", "Set post-write trigger"),
        )
        .await;
    let created = Id::from_str(created_id(&reply.0["methodResponses"][0][1], "made"))
        .unwrap()
        .document_id();
    assert_indexed(test, id, created, since).await;
}

async fn case_pipelined_imap_commands(test: &TestServer, key: &Account, plain: &Account) {
    println!("case_pipelined_imap_commands");
    let _config = search_config_guard!(test, pass_cap: 1);
    for account in [key, plain] {
        let id = account_id(account);
        let since = change_id(test, id).await;
        let first = deliver_one(test, account, "First pending pipelined command").await;
        let second = deliver_one(test, account, "Second pending pipelined command").await;
        let mut imap = ImapConnection::connect(b"_pipe ").await;
        imap.authenticate(account.name(), account.secret()).await;

        let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
        // Write both same-kind commands together with distinct tags. The
        // CREATE handler sends each tagged response in order.
        imap.send_raw("_pipe1 CREATE PipelineOne\r\n_pipe CREATE PipelineTwo\r\n")
            .await;
        let responses = imap.assert_read(Type::Tagged, ResponseType::Ok).await;
        assert!(
            responses.iter().any(|line| line.starts_with("_pipe1 OK")),
            "{responses:?}"
        );

        if id == account_id(key) {
            assert_indexed(test, id, first, since).await;
            assert_indexed(test, id, second, since).await;
            assert_eq!(search_pass_sizes(traces, id).await, [1, 1]);
        } else {
            assert!(search_pass_sizes(traces, id).await.is_empty());
            assert_plain_no_index(test, id).await;
            clean_plain_messages(test, plain).await;
        }
        imap.send_ok("DELETE PipelineOne").await;
        imap.send_ok("DELETE PipelineTwo").await;
    }
}

async fn case_busy_pass_does_not_wait_for_reload(test: &TestServer, key: &Account) {
    println!("case_busy_pass_does_not_wait_for_reload");
    let id = account_id(key);
    let document = deliver_one(test, key, "Busy reload backlog").await;
    let keys = za_unlock(test, id, key.secret()).await;
    let entry = test.server.za_search_entry(id, &keys).await.unwrap();
    {
        // A pass can hold these together while reloading a moved manifest.
        let _pass = entry.pass_lock.lock().await;
        let _reload = entry.index.write().await;
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            test.server.za_search_pass(id, &keys),
        )
        .await
        .expect("busy pass attempted to read the index before checking its pass lock")
        .unwrap();
        assert_eq!(outcome, PassOutcome::Running);
    }
    advanced(test, key, true).await;
    assert!(index_holds(test, id, document).await);
}

async fn case_reads_do_not_wait_for_a_pass(test: &TestServer, key: &Account) {
    println!("case_reads_do_not_wait_for_a_pass");
    let id = account_id(key);
    let _config = search_config_guard!(test, pass_cap: 1);
    let document = deliver_one(test, key, "Parked request first").await;
    deliver_one(test, key, "Parked request second").await;
    let reader = key.clone();
    let parked = HookParked::start(id, set_search_pass_pause, async move {
        reader.jmap_method_call("Email/get", json!({
            "accountId": reader.id_string(), "ids": [email_id(document)], "properties": ["id"],
        })).await
    })
    .await;

    tokio::time::timeout(Duration::from_secs(2), set_flag(key, document, true))
        .await
        .expect("keyword update waited for a publishing pass");
    tokio::time::timeout(Duration::from_secs(2), mailbox_get(key))
        .await
        .expect("Mailbox/get waited for a publishing pass");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), pass(test, key))
            .await
            .expect("a separate pass waited for a publishing pass"),
        PassOutcome::Running,
    );
    let response = parked.finish().await;
    assert_eq!(
        response.0["methodResponses"][0][1]["list"][0]["id"],
        email_id(document)
    );
    advanced(test, key, true).await;
}

async fn case_content_wait_budget(test: &TestServer, key: &Account) {
    println!("case_content_wait_budget");
    let id = account_id(key);
    let _config = search_config_guard!(test, wait_secs: 1, pass_cap: 1);
    let mut documents = Vec::new();
    for n in 0..6 {
        documents.push(deliver_one(test, key, &format!("Content wait backlog {n}")).await);
    }
    let checkpoint = index(test, id).await.indexed_to;
    let parked = park(test, key, set_search_pass_pause).await;
    let keys = za_unlock(test, id, key.secret()).await;
    let start = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        test.server
            .za_search_ensure_current(id, &keys, start + Duration::from_secs(1)),
    )
    .await
    .expect("content waiter exceeded its deadline");
    assert!(matches!(result, Err(Unavailable::NotCurrent)));
    assert!(
        start.elapsed() >= Duration::from_millis(900),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(index(test, id).await.indexed_to, checkpoint);
    assert!(matches!(
        parked.finish().await,
        PassOutcome::Advanced {
            published: true,
            ..
        }
    ));
    assert!(index(test, id).await.indexed_to.change_id > checkpoint.change_id);
    assert_held(test, id, &documents, 1).await;

    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let entry = test
        .server
        .za_search_ensure_current(id, &keys, Instant::now() + Duration::from_secs(30))
        .await
        .unwrap_or_else(|error| panic!("index did not catch up: {error:?}"));
    let state = change_id(test, id).await;
    let loaded = entry
        .read_loaded()
        .await
        .expect("current index remains loaded");
    assert!(is_current(&loaded, state));
    assert!(loaded.content.is_some());
    drop(loaded);
    let committed = traces
        .finish()
        .await
        .into_iter()
        .filter(|event| event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_PASS)
        .count();
    assert_eq!(committed, documents.len() - 1);
    assert_held(test, id, &documents, documents.len()).await;
}

async fn case_content_wait_uses_deadline_during_reload(test: &TestServer, key: &Account) {
    println!("case_content_wait_uses_deadline_during_reload");
    let id = account_id(key);
    let document = deliver_one(test, key, "Content waiter while a pass reloads").await;
    let keys = za_unlock(test, id, key.secret()).await;
    let entry = test.server.za_search_entry(id, &keys).await.unwrap();
    let checkpoint = index(test, id).await.indexed_to;

    {
        // Reproduce the running pass's reload lock order. The content waiter
        // must find the pass slot without first reading or loading the index.
        let _pass = entry.pass_lock.lock().await;
        let _reload = entry.index.write().await;
        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            test.server
                .za_search_ensure_current(id, &keys, start + Duration::from_millis(200)),
        )
        .await
        .expect("content waiter blocked on the reload guard beyond its original deadline");
        assert!(matches!(result, Err(Unavailable::NotCurrent)));
        assert!(
            start.elapsed() >= Duration::from_millis(190),
            "{:?}",
            start.elapsed()
        );
        assert_eq!(index(test, id).await.indexed_to, checkpoint);
    }

    let current = test
        .server
        .za_search_ensure_current(id, &keys, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap_or_else(|error| panic!("content wait did not recover after reload: {error:?}"));
    let state = change_id(test, id).await;
    let loaded = current.read_loaded().await.unwrap();
    assert!(is_current(&loaded, state));
    assert!(loaded.content.is_some());
    drop(loaded);
    assert!(index_holds(test, id, document).await);
}

async fn case_content_loader_uses_original_deadline(test: &TestServer, key: &Account) {
    println!("case_content_loader_uses_original_deadline");
    let id = account_id(key);
    let _config = search_config_guard!(test, wait_secs: 30);
    deliver_one(
        test,
        key,
        "Current thread part with an unloaded content head",
    )
    .await;
    advanced(test, key, true).await;
    simulate_restart(test, id).await;
    assert_eq!(pass(test, key).await, PassOutcome::Current);

    let keys = za_unlock(test, id, key.secret()).await;
    let entry = test.server.za_search_entry(id, &keys).await.unwrap();
    {
        let state = change_id(test, id).await;
        let loaded = entry.read_loaded().await.unwrap();
        assert!(is_current(&loaded, state));
        assert!(loaded.manifest.content.is_some() && loaded.content.is_none());
    }

    // The thread entry is resident and current, so the pass needs no load
    // flight. Only the actual content load waits on this lock.
    let load_flight = entry.load_flight.lock().await;
    let budget = test.server.inner.cache.za_search.content_used();
    let start = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        test.server
            .za_search_ensure_current(id, &keys, start + Duration::from_millis(200)),
    )
    .await
    .expect("content load ignored the caller's shorter absolute deadline");
    assert!(matches!(result, Err(Unavailable::NotCurrent)));
    assert!(
        start.elapsed() >= Duration::from_millis(190),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(test.server.inner.cache.za_search.content_used(), budget);
    assert!(entry.read_loaded().await.unwrap().content.is_none());
    drop(load_flight);

    let current = test
        .server
        .za_search_ensure_current(id, &keys, Instant::now() + Duration::from_secs(5))
        .await
        .unwrap_or_else(|error| panic!("content loader did not recover after timeout: {error:?}"));
    assert!(current.read_loaded().await.unwrap().content.is_some());
}

async fn case_time_cap_bounds_the_command(test: &TestServer, key: &Account) {
    println!("case_time_cap_bounds_the_command");
    let id = account_id(key);
    let _config = search_config_guard!(test, pass_ms: 1);
    let before = index(test, id).await.indexed_to;
    let attachment =
        "Attachment content for the command time-cap regression test.\r\n".repeat(5120);
    for n in 0..12 {
        let fixture = format!(
            concat!(
                "From: {}\r\nTo: {}\r\nSubject: Time-cap fixture {}\r\n",
                "MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=cap\r\n\r\n",
                "--cap\r\nContent-Type: text/plain\r\n\r\nMessage text\r\n",
                "--cap\r\nContent-Type: text/plain; charset=utf-8\r\n",
                "Content-Disposition: attachment; filename=attachment.txt\r\n\r\n{}--cap--\r\n",
            ),
            SENDER,
            key.name(),
            n,
            attachment
        );
        deliver(test, SENDER, key.name(), &fixture).await;
    }
    let latest = change_id(test, id).await;
    let docs = message_ids(test, id).await;
    assert_eq!(docs.len(), 12);
    let mut imap = ImapConnection::connect(b"_cap ").await;
    imap.authenticate(key.name(), key.secret()).await;
    tokio::time::timeout(Duration::from_secs(5), imap.send_ok("SELECT INBOX"))
        .await
        .expect("one SELECT exceeded the command budget");
    let first = index(test, id).await.indexed_to;
    assert!(
        first.change_id > before.change_id && first.change_id < latest,
        "{first:?}"
    );

    for _ in 1..docs.len() {
        if index(test, id).await.indexed_to.change_id >= latest {
            break;
        }
        imap.send_ok("SELECT INBOX").await;
    }
    assert!(index(test, id).await.indexed_to.change_id >= latest);
    assert_held(test, id, &docs, docs.len()).await;
}

async fn case_plain_control_unaffected(test: &TestServer, plain: &Account) {
    println!("case_plain_control_unaffected");
    let id = account_id(plain);
    deliver_one(test, plain, "Ordinary trigger control").await;
    mailbox_get(plain).await;
    let mut imap = ImapConnection::connect(b"_plain ").await;
    imap.authenticate(plain.name(), plain.secret()).await;
    imap.send_ok("NOOP").await;
    imap.send_ok("SELECT INBOX").await;
    imap.send_ok("LOGOUT").await;
    assert!(manifest_bytes(test, id).await.is_none());
    assert!(principal_blob_links(test, id).await.is_empty());

    clean_plain_messages(test, plain).await;
}

async fn clean_plain_messages(test: &TestServer, plain: &Account) {
    let cache = test
        .server
        .get_cached_messages(account_id(plain))
        .await
        .unwrap();
    let ids = cache
        .emails
        .iter()
        .map(|email| Id::from_parts(email.thread_id(), email.document_id()).to_string())
        .collect::<Vec<_>>();
    destroy(plain, &ids).await;
    test.wait_for_tasks().await;
}

async fn search_pass_sizes(traces: Traced, id: u32) -> Vec<u64> {
    traces
        .finish()
        .await
        .into_iter()
        .filter_map(|event| {
            if event.account_id != Some(u64::from(id)) || event.details != ZA_SEARCH_PASS {
                return None;
            }
            Some(
                event
                    .keys
                    .iter()
                    .find_map(|(key, value)| match (key, value) {
                        (Key::Total, TraceValue::UInt(count)) => Some(*count),
                        _ => None,
                    })
                    .expect("a committed pass has an insert count"),
            )
        })
        .collect()
}

fn get_created(account: &Account, create_id: &str) -> Value {
    json!(["Email/get", {
        "accountId": account.id_string(),
        "ids": [format!("#{create_id}")],
        "properties": ["id"],
    }, "get-created"])
}

fn get_imported(account: &Account, call_id: &str, create_id: &str) -> Value {
    json!(["Email/get", {
        "accountId": account.id_string(),
        "#ids": { "resultOf": call_id, "name": "Email/import", "path": format!("/created/{create_id}/id") },
        "properties": ["id"],
    }, "get-imported"])
}

fn assert_get_created(body: &Value, expected_id: &str) -> u64 {
    assert_eq!(body[0], "Email/get", "{body}");
    assert_eq!(body[1]["list"][0]["id"], expected_id, "{body}");
    let Some(State::Exact(state)) = body[1]["state"].as_str().and_then(State::parse) else {
        panic!("missing exact Email/get state: {body}");
    };
    state
}

async fn assert_plain_no_index(test: &TestServer, id: u32) {
    assert!(manifest_bytes(test, id).await.is_none());
    assert!(principal_blob_links(test, id).await.is_empty());
}

async fn case_r8_two_adding_calls(
    test: &TestServer,
    account: &Account,
    import_first: bool,
    keyed: bool,
) {
    println!("case_r8_two_adding_calls: import_first={import_first}, keyed={keyed}");
    let id = account_id(account);
    let root = message_with("R8 root", "pre-request backlog");
    let parent = root
        .lines()
        .find_map(|line| line.strip_prefix("Message-ID: "))
        .unwrap();
    let parent_id = parent.trim_matches(['<', '>']);
    let imported = message_with("Re: R8 root", "import reply").replacen(
        "\r\n\r\n",
        &format!("\r\nIn-Reply-To: {parent}\r\n\r\n"),
        1,
    );
    let blob = upload_message(account, &imported).await;
    let since = change_id(test, id).await;
    deliver(test, SENDER, account.name(), &root).await;

    let mut create = create_request(account, "create", "Re: R8 root");
    create["create"]["create"]["inReplyTo"] = json!([parent_id]);
    let create = json!(["Email/set", create, "create-call"]);
    let import = json!([
        "Email/import",
        import_request(account, "import", &blob),
        "import-call"
    ]);
    let read_create = get_created(account, "create");
    let read_import = get_imported(account, "import-call", "import");
    let (calls, creations) = if import_first {
        (
            [import, read_import, create, read_create],
            ["import", "create"],
        )
    } else {
        (
            [create, read_create, import, read_import],
            ["create", "import"],
        )
    };

    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let reply = account.jmap_method_calls(json!(calls)).await;
    let responses = reply.0["methodResponses"].as_array().unwrap();
    assert_eq!(responses.len(), 4, "{:?}", reply.0);
    for n in 0..4 {
        assert_eq!(responses[n][0], calls[n][0], "{:?}", reply.0);
        assert_eq!(responses[n][2], calls[n][2], "{:?}", reply.0);
    }
    let first = created_id(&responses[0][1], creations[0]);
    let last = created_id(&responses[2][1], creations[1]);
    let earlier_view = assert_get_created(&responses[1], first);
    let later_view = assert_get_created(&responses[3], last);
    if keyed {
        let manifest = index(test, id).await;
        assert!(earlier_view < manifest.newest_row.change_id);
        assert!(later_view >= manifest.newest_row.change_id);
        assert_eq!(search_pass_sizes(traces, id).await, [1, 2]);
        for email_id in [first, last] {
            let document = Id::from_str(email_id).unwrap().document_id();
            assert_indexed(test, id, document, since).await;
        }
    } else {
        assert!(search_pass_sizes(traces, id).await.is_empty());
        assert_plain_no_index(test, id).await;
    }
}

#[derive(Clone, Copy, Debug)]
enum CandidateFailure {
    Reference,
    State,
    NotCreated,
}

async fn case_r8_failed_candidate(
    test: &TestServer,
    account: &Account,
    failure: CandidateFailure,
    keyed: bool,
    earlier_write: bool,
) {
    println!("case_r8_failed_candidate: {failure:?}, keyed={keyed}, earlier_write={earlier_write}");
    let id = account_id(account);
    let since = change_id(test, id).await;
    let before = manifest_bytes(test, id).await;
    let mut calls = Vec::new();
    if earlier_write {
        calls.push(json!([
            "Email/set",
            create_request(account, "earlier", "Before a failed candidate"),
            "create",
        ]));
    }
    let failed = match failure {
        CandidateFailure::Reference => json!([
            "Email/import",
            import_request(account, "failed", "#missing-upload"),
            "selected",
        ]),
        CandidateFailure::State => {
            let mut request = create_request(account, "failed", "State mismatch");
            // A well-formed but different state, even on an empty account.
            request["ifInState"] = json!(State::Exact(u64::MAX - 1));
            json!(["Email/set", request, "selected"])
        }
        CandidateFailure::NotCreated => {
            let blob = upload_message(account, &message_with("No mailbox", "Not created")).await;
            let mut request = import_request(account, "failed", &blob);
            request["emails"]["failed"]["mailboxIds"] = json!({});
            json!(["Email/import", request, "selected"])
        }
    };
    calls.push(failed);
    if earlier_write {
        calls.push(get_created(account, "earlier"));
    }

    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let reply = account.jmap_method_calls(json!(calls)).await;
    let failed = &reply.0["methodResponses"][usize::from(earlier_write)];
    assert_eq!(failed[2], "selected");
    match failure {
        CandidateFailure::Reference => {
            assert_eq!(failed[0], "error", "{:?}", reply.0);
            assert_eq!(failed[1]["type"], "invalidResultReference", "{:?}", reply.0);
        }
        CandidateFailure::State => {
            assert_eq!(failed[0], "error", "{:?}", reply.0);
            assert_eq!(failed[1]["type"], "stateMismatch", "{:?}", reply.0);
        }
        CandidateFailure::NotCreated => {
            assert_eq!(failed[0], "Email/import", "{:?}", reply.0);
            assert_eq!(
                failed[1]["notCreated"]["failed"]["type"], "invalidProperties",
                "{:?}",
                reply.0
            );
            assert!(failed[1].get("created").is_none(), "{:?}", reply.0);
        }
    }
    if earlier_write {
        let first = created_id(&reply.0["methodResponses"][0][1], "earlier");
        let following_view = assert_get_created(&reply.0["methodResponses"][2], first);
        if keyed {
            assert!(following_view >= index(test, id).await.newest_row.change_id);
            let document = Id::from_str(first).unwrap().document_id();
            assert_indexed(test, id, document, since).await;
        }
    }
    assert_eq!(
        search_pass_sizes(traces, id).await,
        if earlier_write && keyed {
            vec![1]
        } else {
            vec![]
        },
    );
    if !earlier_write {
        assert_eq!(manifest_bytes(test, id).await, before);
        assert!(message_ids(test, id).await.is_empty());
    }
    if !keyed {
        assert_plain_no_index(test, id).await;
    }
}

async fn case_r8_non_candidates(test: &TestServer, key: &Account) {
    println!("case_r8_non_candidates");
    let id = account_id(key);
    let prior = deliver_one(test, key, "Existing message to update").await;
    advanced(test, key, true).await;
    let prior_id = email_id(prior);
    let since = change_id(test, id).await;
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let reply = key
        .jmap_method_calls(json!([
            ["Email/set", create_request(key, "writer", "Last actual writer"), "shared-call-id"],
            get_created(key, "writer"),
            ["Email/set", {
                "accountId": key.id_string(),
                "update": { &prior_id: { "keywords/$flagged": true } },
            }, "shared-call-id"],
            ["Email/import", { "accountId": key.id_string(), "emails": {} }, "shared-call-id"],
        ]))
        .await;
    let responses = reply.0["methodResponses"].as_array().unwrap();
    assert_eq!(responses.len(), 4, "{:?}", reply.0);
    let created = created_id(&responses[0][1], "writer");
    let view = assert_get_created(&responses[1], created);
    assert_eq!(responses[2][0], "Email/set", "{:?}", reply.0);
    assert!(
        responses[2][1]["updated"].get(&prior_id).is_some(),
        "{:?}",
        reply.0
    );
    assert_eq!(responses[3][0], "Email/import", "{:?}", reply.0);
    assert!(responses[3][1].get("created").is_none(), "{:?}", reply.0);
    assert!(view >= index(test, id).await.newest_row.change_id);
    assert_eq!(search_pass_sizes(traces, id).await, [1]);
    assert_indexed(
        test,
        id,
        Id::from_str(created).unwrap().document_id(),
        since,
    )
    .await;
}

async fn case_r8_generated_call(test: &TestServer, key: &Account, plain: &Account) {
    println!("case_r8_generated_call");
    let id = account_id(key);
    let identity = key
        .jmap_create(
            "Identity",
            [json!({ "name": "Generated call", "email": key.name() })],
            Vec::<(&str, &str)>::new(),
        )
        .await
        .created_id(0)
        .to_string();
    let mut draft = create_request(key, "draft", "Local generated-call fixture");
    let email = &mut draft["create"]["draft"];
    let drafts = Id::from(DRAFTS_ID).to_string();
    email["mailboxIds"] = json!({ drafts: true });
    email["to"] = json!([{ "email": plain.name() }]);
    email["keywords"] = json!({ "$draft": true });
    let draft = key.jmap_method_call("Email/set", draft).await;
    let draft_id = created_id(&draft.0["methodResponses"][0][1], "draft");
    let blob = upload_message(key, &message_with("After generated call", "Import fixture")).await;
    let since = change_id(test, id).await;
    deliver_one(test, key, "Before generated call").await;

    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let reply = key
        .jmap_method_calls(json!([
            ["Email/set", create_request(key, "first", "Before submission"), "create"],
            ["EmailSubmission/set", {
                "accountId": key.id_string(),
                "create": { "sub": { "identityId": &identity, "emailId": draft_id } },
                "onSuccessUpdateEmail": { "#sub": { "keywords/$flagged": true } },
            }, "shared-call-id"],
            get_created(key, "first"),
            ["Email/import", import_request(key, "last", &blob), "shared-call-id"],
            get_created(key, "last"),
        ]))
        .await;
    let responses = reply.0["methodResponses"].as_array().unwrap();
    assert_eq!(responses.len(), 6, "{:?}", reply.0);
    let first = created_id(&responses[0][1], "first");
    assert_eq!(responses[1][0], "EmailSubmission/set", "{:?}", reply.0);
    let submission = created_id(&responses[1][1], "sub");
    assert_eq!(responses[2][0], "Email/set", "{:?}", reply.0);
    assert_eq!(responses[2][2], "shared-call-id");
    assert!(
        responses[2][1]["updated"].get(draft_id).is_some(),
        "{:?}",
        reply.0
    );
    let after_generated = assert_get_created(&responses[3], first);
    let last = created_id(&responses[4][1], "last");
    let after_selected = assert_get_created(&responses[5], last);
    let publication = index(test, id).await.newest_row.change_id;
    assert!(after_generated < publication && after_selected >= publication);
    assert_eq!(search_pass_sizes(traces, id).await, [1, 2]);
    for created in [first, last] {
        let document = Id::from_str(created).unwrap().document_id();
        assert_indexed(test, id, document, since).await;
    }

    wait_for_delivery(test).await;
    assert_eq!(message_ids(test, account_id(plain)).await.len(), 1);
    assert_plain_no_index(test, account_id(plain)).await;
    clean_plain_messages(test, plain).await;
    key.jmap_destroy("EmailSubmission", [submission], Vec::<(&str, &str)>::new())
        .await;
    key.jmap_destroy("Identity", [&identity], Vec::<(&str, &str)>::new())
        .await;
}

async fn case_r8_foreign_plain_writer(test: &TestServer, key: &Account, plain: &Account) {
    println!("case_r8_foreign_plain_writer");
    let inbox = Id::from(INBOX_ID).to_string();
    mailbox_get(plain).await;
    let share = plain
        .jmap_method_call(
            "Mailbox/set",
            json!({
                "accountId": plain.id_string(),
                "update": { &inbox: { "shareWith": {
                    key.id_string(): { "mayReadItems": true, "mayAddItems": true },
                } } },
            }),
        )
        .await;
    assert!(
        share.0["methodResponses"][0][1]["updated"]
            .get(&inbox)
            .is_some(),
        "{:?}",
        share.0
    );

    let id = account_id(key);
    let since = change_id(test, id).await;
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let reply = key
        .jmap_method_calls(json!([
            [
                "Email/set",
                create_request(key, "owner", "Owner's last writer"),
                "owner"
            ],
            get_created(key, "owner"),
            [
                "Email/set",
                create_request(plain, "foreign", "Foreign ordinary writer"),
                "foreign"
            ],
        ]))
        .await;
    let responses = reply.0["methodResponses"].as_array().unwrap();
    assert_eq!(responses.len(), 3, "{:?}", reply.0);
    let owner_email = created_id(&responses[0][1], "owner");
    let owner_view = assert_get_created(&responses[1], owner_email);
    let foreign = created_id(&responses[2][1], "foreign");
    let foreign_doc = Id::from_str(foreign).unwrap().document_id();
    assert!(
        message_ids(test, account_id(plain))
            .await
            .contains(&foreign_doc)
    );
    assert!(owner_view >= index(test, id).await.newest_row.change_id);
    assert_eq!(search_pass_sizes(traces, id).await, [1]);
    assert_indexed(
        test,
        id,
        Id::from_str(owner_email).unwrap().document_id(),
        since,
    )
    .await;
    assert_plain_no_index(test, account_id(plain)).await;
    clean_plain_messages(test, plain).await;

    let unshare = plain
        .jmap_method_call(
            "Mailbox/set",
            json!({
                "accountId": plain.id_string(),
                "update": { &inbox: { (format!("shareWith/{}", key.id_string())): null } },
            }),
        )
        .await;
    assert!(
        unshare.0["methodResponses"][0][1]["updated"]
            .get(&inbox)
            .is_some(),
        "{:?}",
        unshare.0
    );
}

async fn case_r8_same_call_reference(test: &TestServer, key: &Account) {
    println!("case_r8_same_call_reference");
    let id = account_id(key);
    let since = change_id(test, id).await;
    let mut request = create_request(key, "first", "Referenced grouped create");
    let mut dependent = create_request(key, "dependent", "Same-call reference fixture");
    // A creation ID resolves to an Email id. The id is server-set, so the
    // dependent create must reach property validation rather than notFound.
    dependent["create"]["dependent"]["id"] = json!("#first");
    request["create"]["dependent"] = dependent["create"]["dependent"].take();
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let reply = key
        .jmap_method_calls(json!([
            ["Email/set", request, "writer"],
            get_created(key, "first"),
        ]))
        .await;
    let responses = reply.0["methodResponses"].as_array().unwrap();
    let created = created_id(&responses[0][1], "first");
    assert_eq!(
        responses[0][1]["notCreated"]["dependent"]["type"], "invalidProperties",
        "{:?}",
        reply.0
    );
    let view = assert_get_created(&responses[1], created);
    assert!(view >= index(test, id).await.newest_row.change_id);
    assert_eq!(search_pass_sizes(traces, id).await, [1]);
    assert_indexed(
        test,
        id,
        Id::from_str(created).unwrap().document_id(),
        since,
    )
    .await;
}

async fn case_setup_manifest(test: &TestServer, key: &Account) {
    let id = account_id(key);
    let bytes = manifest_bytes(test, id).await.unwrap();
    let manifest = index(test, id).await;
    assert_eq!(manifest.revision, 0);
    assert_eq!(manifest.epoch, 0);
    assert!(manifest.thread_blobs.is_empty() && manifest.content.is_none());
    assert!(manifest.indexed_to.complete);
    assert_eq!(manifest.content_to.checkpoint, manifest.indexed_to);
    assert_eq!(pass(test, key).await, PassOutcome::Current);
    assert_eq!(manifest_bytes(test, id).await.unwrap(), bytes);
}

async fn case_persistence_and_thread_only_load(test: &TestServer, key: &Account) {
    let id = account_id(key);
    let root = message_with("Threaded fixture", "Root");
    let parent = root
        .lines()
        .find_map(|line| line.strip_prefix("Message-ID: "))
        .unwrap();
    deliver(test, SENDER, key.name(), &root).await;
    let parent_id = message_ids(test, id).await[0];
    let reply = message_with("Re: Threaded fixture", "Reply").replacen(
        "\r\n\r\n",
        &format!("\r\nIn-Reply-To: {parent}\r\n\r\n"),
        1,
    );
    deliver(test, SENDER, key.name(), &reply).await;
    let reply_id = super::new_message(test, id, &[parent_id]).await;
    deliver_one(test, key, "Independent fixture").await;
    let ids = message_ids(test, id).await;

    advanced(test, key, true).await;
    let manifest = index(test, id).await;
    assert_eq!(manifest.revision, 1);
    assert_eq!(manifest.thread_blobs.len(), 1);
    let bytes = stored_blob(test, &manifest.thread_blobs[0].hash).await;
    let (_, sections) = BlobHeader::decode(&bytes).unwrap();
    let sealed_hidden_len = u32::from_le_bytes(sections[..4].try_into().unwrap()) as usize;
    assert_eq!(
        sealed_hidden_len,
        test.server.za_search_config().pad_min + vault::keys::NONCE_LEN + vault::keys::TAG_LEN,
    );
    assert_eq!(manifest.content.as_ref().unwrap().length, 1);
    assert_manifest_links(test, id, &manifest).await;
    let committed = manifest_bytes(test, id).await.unwrap();
    assert_eq!(
        loaded(test, id).await.manifest_bytes_hash,
        manifest_hash(&committed)
    );

    simulate_restart(test, id).await;
    reset_za_search_counters(id);
    assert_eq!(pass(test, key).await, PassOutcome::Current);
    assert_eq!(za_search_blob_reads(id), 1);
    assert_eq!(za_search_message_opens(id), 0);
    assert_held(test, id, &ids, 3).await;
    let view = loaded(test, id).await;
    assert_eq!(view.thread.current_thread(reply_id), parent_id);
    assert!(view.content.is_none());
}

async fn case_catch_up_after_offline_deliveries(test: &TestServer, key: &Account, plain: &Account) {
    let id = account_id(key);
    let plain_id = account_id(plain);
    let plain_since = change_id(test, plain_id).await;
    for n in 0..5 {
        let bytes = message_with(&format!("Offline {n}"), "Offline body");
        deliver(test, SENDER, key.name(), &bytes).await;
        deliver(test, SENDER, plain.name(), &bytes).await;
    }
    let cache = test.server.get_cached_messages(id).await.unwrap();
    let latest_delivery = cache.last_change_id;
    let ids = message_ids(test, id).await;
    let witness = ids[0];
    let mut email_expected = cache
        .mailboxes
        .items
        .iter()
        .map(|m| {
            Change::UpdateContainerPartial(u64::from(m.document_id), PartialChange::PROPERTIES)
        })
        .collect::<Vec<_>>();
    email_expected.push(Change::UpdateItem(Id::from_parts(witness, witness).id()));
    let checkpoint = advanced(test, key, true).await;
    assert!(checkpoint.complete);
    assert_eq!(checkpoint.change_id, latest_delivery);
    assert_held(test, id, &ids, 5).await;

    let publication = index(test, id).await.newest_row.change_id;
    let email = log_records_since(test, id, SyncCollection::Email, latest_delivery).await;
    assert_eq!(email.len(), 1);
    assert_eq!(email[0].0, publication);
    assert_eq!(email[0].1.len(), email_expected.len());
    for change in email_expected {
        assert!(
            email[0].1.contains(&change),
            "missing {change:?}: {email:?}"
        );
    }
    assert_eq!(
        log_records_since(test, id, SyncCollection::Thread, latest_delivery).await,
        [(
            publication,
            vec![Change::UpdateContainerPartial(
                u64::from(witness),
                PartialChange::PROPERTIES
            )]
        )],
    );
    let plain_log = log_records_since(test, plain_id, SyncCollection::Email, plain_since).await;
    assert_eq!(
        plain_log
            .iter()
            .flat_map(|(_, changes)| changes)
            .filter(|c| matches!(c, Change::InsertItem(_)))
            .count(),
        5
    );
    assert!(
        !plain_log
            .iter()
            .flat_map(|(_, changes)| changes)
            .any(|c| matches!(c, Change::UpdateItem(_)))
    );
    assert!(manifest_bytes(test, plain_id).await.is_none());
    assert!(principal_blob_links(test, plain_id).await.is_empty());

    let plain_cache = test.server.get_cached_messages(plain_id).await.unwrap();
    let plain_messages = plain_cache
        .emails
        .iter()
        .map(|m| Id::from_parts(m.thread_id(), m.document_id()).to_string())
        .collect::<Vec<_>>();
    destroy(plain, &plain_messages).await;
    test.wait_for_tasks().await;
}

async fn multiappend(test: &TestServer, key: &Account, count: usize) -> (u64, Vec<u32>) {
    let id = account_id(key);
    assert!(message_ids(test, id).await.is_empty());
    let before = change_id(test, id).await;
    // Seed one grouped insert for the explicit checkpoint pass below. Task 9's
    // command and APPEND hooks see the real busy slot during this fixture.
    let entry = test.server.inner.cache.za_search.entry(id);
    let seed_guard = entry.pass_lock.lock().await;
    let mut imap = ImapConnection::connect(b"_index ").await;
    imap.authenticate(key.name(), key.secret()).await;
    imap.send_ok("SELECT INBOX").await;
    for n in 0..count {
        let message = message_with(&format!("Append {n}"), "Grouped fixture");
        if n == 0 {
            imap.send(&format!("APPEND INBOX {{{}}}", message.len()))
                .await;
        } else {
            imap.send_untagged(&format!(" {{{}}}", message.len())).await;
        }
        imap.assert_read(Type::Continuation, ResponseType::Ok).await;
        if n + 1 == count {
            imap.send_untagged(&message).await;
            imap.assert_read(Type::Tagged, ResponseType::Ok).await;
        } else {
            imap.send_raw(&message).await;
        }
    }
    drop(imap);
    drop(seed_guard);
    test.wait_for_tasks().await;
    let docs = message_ids(test, id).await;
    let groups = log_records_since(test, id, SyncCollection::Email, before)
        .await
        .into_iter()
        .filter_map(|(change_id, changes)| {
            let mut inserts = changes
                .into_iter()
                .filter_map(|change| match change {
                    Change::InsertItem(id) => Some(id as u32),
                    _ => None,
                })
                .collect::<Vec<_>>();
            inserts.sort_unstable();
            (!inserts.is_empty()).then_some((change_id, inserts))
        })
        .collect::<Vec<_>>();
    assert_eq!(docs.len(), count);
    assert_eq!(
        groups.len(),
        1,
        "MULTIAPPEND must exercise one grouped insert record"
    );
    assert_eq!(groups[0].1, docs);
    (groups[0].0, docs)
}

fn set_pass_cap(cap: usize) {
    set_search_config_override(Some(ZaSearchConfig {
        pass_cap: cap,
        ..ZaSearchConfig::from_env().unwrap()
    }));
}

async fn case_multiappend_split_record_with_restart(test: &TestServer, key: &Account) {
    let id = account_id(key);
    set_pass_cap(3);
    let (change_id, docs) = multiappend(test, key, 8).await;
    let first = Checkpoint {
        change_id,
        position: docs[2],
        complete: false,
    };
    assert_eq!(advanced(test, key, true).await, first);
    assert_eq!(index(test, id).await.indexed_to, first);
    assert_held(test, id, &docs, 3).await;
    simulate_restart(test, id).await;
    let second = Checkpoint {
        change_id,
        position: docs[5],
        complete: false,
    };
    assert_eq!(advanced(test, key, true).await, second);
    assert_eq!(index(test, id).await.indexed_to, second);
    assert_held(test, id, &docs, 6).await;
    assert!(advanced(test, key, true).await.complete);
    let manifest = index(test, id).await;
    assert!(manifest.indexed_to.complete);
    assert_eq!(manifest.revision, 3);
    assert_held(test, id, &docs, 8).await;
    let witnesses = log_records_since(test, id, SyncCollection::Email, change_id)
        .await
        .into_iter()
        .flat_map(|(_, changes)| changes)
        .filter_map(|change| match change {
            Change::UpdateItem(id) => Some(id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        witnesses,
        [docs[0], docs[3], docs[6]].map(|doc| Id::from_parts(doc, doc).id())
    );
    set_search_config_override(None);
}

async fn case_pl4_unreadable_prefix_of_a_group(test: &TestServer, key: &Account, restart: bool) {
    let id = account_id(key);
    set_pass_cap(3);
    let (change_id, docs) = multiappend(test, key, 5).await;
    for &document in &docs[..3] {
        corrupt_blob(test, &stored_hash(test, id, document).await).await;
    }
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let first = Checkpoint {
        change_id,
        position: docs[2],
        complete: false,
    };
    assert_eq!(advanced(test, key, false).await, first);
    assert_eq!(index(test, id).await.indexed_to, first);
    assert!(loaded(test, id).await.scanned_to < change_id);
    assert_eq!(
        traces
            .finish()
            .await
            .iter()
            .filter(|event| event.account_id == Some(u64::from(id))
                && event.details == ZA_SEARCH_SKIPPED)
            .count(),
        3
    );
    if restart {
        simulate_restart(test, id).await;
    }
    assert!(advanced(test, key, true).await.complete);
    for (position, document) in docs.into_iter().enumerate() {
        assert_eq!(index_holds(test, id, document).await, position >= 3);
    }
    set_search_config_override(None);
}

async fn set_flag(key: &Account, document: u32, flagged: bool) {
    let id = email_id(document);
    let reply = key
        .jmap_method_call(
            "Email/set",
            json!({
                "accountId": key.id_string(),
                "update": { &id: { "keywords/$flagged": flagged.then_some(true) } },
            }),
        )
        .await;
    assert!(
        reply
            .pointer(&format!("/methodResponses/0/1/updated/{id}"))
            .is_some(),
        "{:?}",
        reply.0
    );
}

async fn case_flag_changes_write_nothing(test: &TestServer, key: &Account, document: u32) {
    let id = account_id(key);
    let bytes = manifest_bytes(test, id).await;
    let links = principal_blob_links(test, id).await;
    for n in 0..100 {
        set_flag(key, document, n % 2 == 0).await;
        assert_eq!(pass(test, key).await, PassOutcome::Current);
    }
    assert_eq!(manifest_bytes(test, id).await, bytes);
    assert_eq!(principal_blob_links(test, id).await, links);
    let last_change = change_id(test, id).await;
    assert!(loaded(test, id).await.scanned_to >= last_change);
}

async fn case_update_only_checkpoints(test: &TestServer, key: &Account, document: u32) {
    let id = account_id(key);
    // Checkpoint the publication update by itself, then start counting flags.
    set_search_config_override(Some(ZaSearchConfig {
        checkpoint: 1,
        ..ZaSearchConfig::from_env().unwrap()
    }));
    advanced(test, key, false).await;
    set_search_config_override(Some(ZaSearchConfig {
        checkpoint: 10,
        ..ZaSearchConfig::from_env().unwrap()
    }));
    let since = change_id(test, id).await;
    let links = principal_blob_links(test, id).await;
    let mut bytes = manifest_bytes(test, id).await;
    let mut last_checkpoint = 0;
    let mut last_change = since;
    for n in 1..=25 {
        set_flag(key, document, n % 2 == 1).await;
        last_change = change_id(test, id).await;
        if n == 10 || n == 20 {
            assert_eq!(advanced(test, key, false).await.change_id, last_change);
            let new = manifest_bytes(test, id).await;
            assert_ne!(new, bytes, "checkpoint after update {n}");
            bytes = new;
            last_checkpoint = last_change;
        } else {
            assert_eq!(pass(test, key).await, PassOutcome::Current);
            assert_eq!(manifest_bytes(test, id).await, bytes, "update {n}");
        }
    }
    set_search_config_override(None);
    let manifest = index(test, id).await;
    assert_eq!(manifest.indexed_to.change_id, last_checkpoint);
    assert_eq!(manifest.content_to.checkpoint, manifest.indexed_to);
    assert_eq!(loaded(test, id).await.scanned_to, last_change);
    assert_eq!(principal_blob_links(test, id).await, links);
    let logs = log_records_since(test, id, SyncCollection::Email, since).await;
    assert_eq!(logs.len(), 25);
    let item = Change::UpdateItem(Id::from_parts(document, document).id());
    assert!(
        logs.iter().all(|(_, changes)| changes.contains(&item)),
        "{logs:?}"
    );
}

async fn case_unreadable_insert_is_consumed_once(test: &TestServer, key: &Account) {
    let id = account_id(key);
    let subject = "Unreadable Search Subject Canary";
    let document = deliver_one(test, key, subject).await;
    corrupt_blob(test, &stored_hash(test, id, document).await).await;
    let latest = change_id(test, id).await;
    let links = principal_blob_links(test, id).await;
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    assert_eq!(advanced(test, key, false).await.change_id, latest);
    assert_eq!(principal_blob_links(test, id).await, links);
    let events = traces.finish().await;
    let skipped = events
        .iter()
        .filter(|event| {
            event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_SKIPPED
        })
        .collect::<Vec<_>>();
    assert_eq!(skipped.len(), 1);
    assert!(
        skipped[0]
            .keys
            .contains(&(Key::DocumentId, TraceValue::UInt(u64::from(document))))
    );
    assert!(!skipped[0].text.contains(subject));
    reset_za_search_counters(id);
    assert_eq!(pass(test, key).await, PassOutcome::Current);
    assert_eq!(pass(test, key).await, PassOutcome::Current);
    simulate_restart(test, id).await;
    assert_eq!(pass(test, key).await, PassOutcome::Current);
    assert_eq!(za_search_message_opens(id), 0);

    let readable = deliver_one(test, key, "Readable after loss").await;
    advanced(test, key, true).await;
    assert!(index_holds(test, id, readable).await);
    assert!(!index_holds(test, id, document).await);
}

async fn case_transient_blob_error_aborts_then_indexes(test: &TestServer, key: &Account) {
    let id = account_id(key);
    let document = deliver_one(test, key, "Transient open").await;
    let links = principal_blob_links(test, id).await;
    let before = index(test, id).await.indexed_to;
    set_search_blob_fail_once(Some(id));
    assert_eq!(
        pass(test, key).await,
        PassOutcome::Aborted(TransientClass::BlobStore)
    );
    let failed = index(test, id).await;
    assert_eq!(
        failed.failing,
        Some(FailingDocument {
            document_id: document,
            attempts: 1
        })
    );
    assert_eq!(failed.indexed_to, before);
    assert_eq!(principal_blob_links(test, id).await, links);
    advanced(test, key, true).await;
    assert!(index_holds(test, id, document).await);
    assert!(index(test, id).await.failing.is_none());

    let next = deliver_one(test, key, "Repeat transient open").await;
    set_search_config_override(Some(ZaSearchConfig {
        skip_after: 2,
        ..ZaSearchConfig::from_env().unwrap()
    }));
    set_search_blob_fail_once(Some(id));
    assert_eq!(
        pass(test, key).await,
        PassOutcome::Aborted(TransientClass::BlobStore)
    );
    assert_eq!(
        index(test, id).await.failing,
        Some(FailingDocument {
            document_id: next,
            attempts: 1
        })
    );
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    set_search_blob_fail_once(Some(id));
    advanced(test, key, false).await;
    set_search_blob_fail_once(None);
    set_search_config_override(None);
    assert!(index(test, id).await.failing.is_none());
    assert!(!index_holds(test, id, next).await);
    let events = traces.finish().await;
    assert!(
        events.iter().any(|event| {
            event.account_id == Some(u64::from(id))
                && event.details == ZA_SEARCH_SKIPPED
                && event.reason == TransientClass::BlobStore.as_str()
                && event
                    .keys
                    .contains(&(Key::DocumentId, TraceValue::UInt(u64::from(next))))
        }),
        "{events:?}"
    );
}

async fn park(
    test: &TestServer,
    key: &Account,
    set: fn(Option<std::sync::Arc<common::za::test_hooks::Pause>>),
) -> HookParked<PassOutcome> {
    let id = account_id(key);
    let server = test.server.clone();
    let keys = za_unlock(test, id, key.secret()).await;
    HookParked::start(id, set, async move {
        server.za_search_pass(id, &keys).await.unwrap()
    })
    .await
}

async fn case_lost_assertion(test: &TestServer, key: &Account) {
    let id = account_id(key);
    let document = deliver_one(test, key, "Lost assertion").await;
    let links = principal_blob_links(test, id).await;
    let mut competing = index(test, id).await;
    competing.failing = Some(FailingDocument {
        document_id: u32::MAX,
        attempts: 1,
    });
    let keys = za_unlock(test, id, key.secret()).await;
    let bytes = ManifestValue::Index(competing).encode(&za_search_key(keys.swk()));
    let parked = park(test, key, set_search_commit_pause).await;
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .set(PrincipalField::ZeroAccessSearch, bytes.clone());
    test.server.store().write_batch(&mut batch).await.unwrap();
    assert_eq!(parked.finish().await, PassOutcome::Lost);
    assert_eq!(principal_blob_links(test, id).await, links);
    assert_eq!(manifest_bytes(test, id).await.unwrap(), bytes);
    assert!(!index_holds(test, id, document).await);
    advanced(test, key, true).await;
    assert!(index_holds(test, id, document).await);
    assert_eq!(index(test, id).await.failing.unwrap().document_id, u32::MAX);
}

async fn case_invalidation_between_commit_and_swap(test: &TestServer, key: &Account) {
    let id = account_id(key);
    let document = deliver_one(test, key, "Invalidation between commit and swap").await;
    let parked = park(test, key, set_search_swap_pause).await;
    let committed = manifest_bytes(test, id).await.unwrap();
    test.server
        .inner
        .cache
        .za_search
        .remove_account(id, EvictReason::Invalidation);
    test.server.inner.cache.za_keys.remove_account(id);
    assert!(matches!(
        parked.finish().await,
        PassOutcome::Advanced {
            published: true,
            ..
        }
    ));
    if let Some(entry) = test.server.inner.cache.za_search.get(id)
        && let Some(loaded) = entry.read_loaded().await
    {
        assert_eq!(loaded.manifest_bytes_hash, manifest_hash(&committed));
    }
    assert_eq!(pass(test, key).await, PassOutcome::Current);
    assert!(index_holds(test, id, document).await);
}

async fn case_slow_store_hold_outlives_the_wait(test: &TestServer, key: &Account, admin: &Account) {
    let id = account_id(key);
    let document = deliver_one(test, key, "Slow blob staging").await;
    set_search_config_override(Some(ZaSearchConfig {
        wait_secs: 1,
        ..ZaSearchConfig::from_env().unwrap()
    }));
    set_search_blob_slow(Some((id, 2000)));
    let parked = park(test, key, set_search_pass_pause).await;
    let view = tokio::time::timeout(Duration::from_secs(1), loaded(test, id))
        .await
        .unwrap();
    assert!(!view.thread.is_present(document));
    drop(view);
    assert_eq!(pass(test, key).await, PassOutcome::Running);
    assert!(matches!(
        parked.finish().await,
        PassOutcome::Advanced {
            published: true,
            ..
        }
    ));
    set_search_blob_slow(None);
    set_search_config_override(None);
    expire_and_purge_blobs(test, admin).await;
    assert_manifest_links(test, id, &index(test, id).await).await;
}

async fn case_expired_hold_does_not_publish(test: &TestServer, key: &Account, admin: &Account) {
    let id = account_id(key);
    let document = deliver_one(test, key, "Expired blob staging").await;
    let bytes = manifest_bytes(test, id).await;
    let links = principal_blob_links(test, id).await;
    set_search_hold_expire(Some(id));
    let parked = park(test, key, set_search_commit_pause).await;
    expire_and_purge_blobs(test, admin).await;
    assert_eq!(
        parked.finish().await,
        PassOutcome::Aborted(TransientClass::BlobStore)
    );
    set_search_hold_expire(None);
    assert_eq!(manifest_bytes(test, id).await, bytes);
    assert_eq!(principal_blob_links(test, id).await, links);
    assert!(!index_holds(test, id, document).await);
    advanced(test, key, true).await;
    assert!(index_holds(test, id, document).await);
}

async fn case_deletion_only_pass_writes_no_entry(test: &TestServer, key: &Account) {
    let id = account_id(key);
    let document = deliver_one(test, key, "Delete only first").await;
    let remaining = deliver_one(test, key, "Delete only second").await;
    advanced(test, key, true).await;
    let before = index(test, id).await;
    let keys = za_unlock(test, id, key.secret()).await;
    let entry = test.server.inner.cache.za_search.get(id).unwrap();
    test.server
        .za_search_load_content(&entry, &keys)
        .await
        .unwrap();
    assert!(
        loaded(test, id)
            .await
            .content
            .as_ref()
            .unwrap()
            .part
            .document_ids()
            .contains(document)
    );
    destroy(key, &[email_id(document)]).await;
    let after_destroy = change_id(test, id).await;
    advanced(test, key, false).await;
    let after = index(test, id).await;
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.content, before.content);
    assert_eq!(after.thread_blobs.len(), before.thread_blobs.len() + 1);
    let bytes = stored_blob(test, &after.thread_blobs.last().unwrap().hash).await;
    let (_, sections) = BlobHeader::decode(&bytes).unwrap();
    assert_eq!(sections[..4], [0; 4], "hidden section length");
    assert!(bytes.len() < test.server.za_search_config().pad_min);
    for collection in [SyncCollection::Email, SyncCollection::Thread] {
        assert!(
            log_records_since(test, id, collection, after_destroy)
                .await
                .is_empty()
        );
    }
    let view = loaded(test, id).await;
    assert_eq!(view.thread.exists_at(document, u64::MAX), Some(false));
    assert_eq!(view.thread.exists_at(remaining, u64::MAX), Some(true));
    let content_ids = view.content.as_ref().unwrap().part.document_ids();
    assert!(!content_ids.contains(document));
    assert!(content_ids.contains(remaining));
}

async fn case_repeated_delta_changes_nothing(test: &TestServer, key: &Account) {
    let id = account_id(key);
    let bytes = message_with("Redelivered delta", "Identical wire message");
    deliver(test, SENDER, key.name(), &bytes).await;
    advanced(test, key, true).await;
    let baseline = manifest_bytes(test, id).await;
    for _ in 0..5 {
        assert_eq!(pass(test, key).await, PassOutcome::Current);
        assert_eq!(manifest_bytes(test, id).await, baseline);
    }
    deliver(test, SENDER, key.name(), &bytes).await;
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 2);
    advanced(test, key, true).await;
    let view = loaded(test, id).await;
    let thread = view.thread.current_thread(ids[0]);
    assert_eq!(view.thread.present_members(thread), ids);
}

async fn destroy_cases(test: &TestServer, admin: &Account) {
    case_stale_permanent_destroy_creates_no_marker(test, admin).await;
    case_double_absence_removes_only_exact_orphan_links(test, admin).await;
    case_double_absence_scan_refuses_new_state(test, admin, false).await;
    case_double_absence_scan_refuses_new_state(test, admin, true).await;
    case_r7_interrupted_transitions_leave_nothing(test, admin).await;
    case_r7_middle_node_break(test, admin).await;
    case_f23_unparseable_manifest(test, admin, false).await;
    case_f23_pre_release_account_without_a_manifest(test, admin).await;
    case_r19_paused_pass_loses_to_the_marker(test, admin).await;
    case_g12_destroy_dies_after_the_marker(test, admin).await;
    case_empty_marker_retry_scans(test, admin).await;
    case_reset_clears_a_live_account(test, admin).await;
    case_reset_cannot_clear_after_registry_deletion(test, admin).await;
    case_reset_after_destroy_creates_no_marker(test, admin).await;
    case_stale_reset_scan_keeps_new_links(test, admin).await;
}

async fn destroy_fixture(admin: &Account, name: &'static str) -> Account {
    // These fixtures destroy their own accounts and therefore stay out of
    // TestServer's end-of-suite list of live accounts.
    admin
        .create_key_user_account(
            name,
            "Destroy-Search-46!",
            "Search destroy",
            &[],
            user_permissions(),
        )
        .await
}

async fn assert_destroyed_index(test: &TestServer, admin: &Account, id: u32, hashes: &[BlobHash]) {
    tokio::time::timeout(Duration::from_secs(30), test.wait_for_tasks())
        .await
        .expect("search fixture's destroy task did not finish");
    expire_and_purge_blobs(test, admin).await;
    assert!(
        principal_blob_links(test, id).await.is_empty(),
        "orphaned index links for account {id}"
    );
    assert!(read_manifest(test, id).await.is_none());
    for hash in hashes {
        assert!(
            test.server
                .get_blob(hash.as_slice(), 0..usize::MAX)
                .await
                .unwrap()
                .is_none(),
            "orphaned index blob {hash:?} for account {id}"
        );
    }
}

async fn case_r7_middle_node_break(test: &TestServer, admin: &Account) {
    let key = destroy_fixture(admin, "search-destroy-2@example.com").await;
    let id = account_id(&key);
    set_search_config_override(Some(ZaSearchConfig {
        segments: 8,
        ..ZaSearchConfig::from_env().unwrap()
    }));
    for number in 1..=6 {
        deliver_one(test, &key, &format!("Broken chain fixture {number}")).await;
        advanced(test, &key, true).await;
    }
    set_search_config_override(None);

    let manifest = index(test, id).await;
    assert_manifest_links(test, id, &manifest).await;
    let head = manifest.content.unwrap();
    assert_eq!(head.length, 6);
    let mut middle = head.hash;
    for _ in 0..2 {
        let bytes = stored_blob(test, &middle).await;
        middle = BlobHeader::decode(&bytes).unwrap().0.predecessor.unwrap().0;
    }
    let hashes = principal_hashes(test, id).await;
    assert!(hashes.len() > 6 && hashes.contains(&middle));
    assert!(
        test.server
            .blob_store()
            .delete_blob(middle.as_slice())
            .await
            .unwrap()
    );
    simulate_restart(test, id).await;
    admin.destroy_account(key).await;
    assert_destroyed_index(test, admin, id, &hashes).await;
}

async fn principal_hashes(test: &TestServer, id: u32) -> Vec<BlobHash> {
    principal_blob_links(test, id)
        .await
        .into_iter()
        .map(|(hash, _)| hash)
        .collect()
}

async fn staged_hashes(test: &TestServer, id: u32, before: &[BlobHash]) -> Vec<BlobHash> {
    let staged = temporary_blob_hashes(test, id)
        .await
        .into_iter()
        .filter(|hash| !before.contains(hash))
        .collect::<Vec<_>>();
    assert_eq!(staged.len(), 2, "expected one thread and one content hold");
    staged
}

async fn park_destroy(admin: &Account, key: &Account) -> HookParked<()> {
    let id = account_id(key);
    let admin = admin.clone();
    let key = key.clone();
    HookParked::start(id, set_search_destroy_pause, async move {
        admin.destroy_account(key).await;
    })
    .await
}

async fn park_reset(test: &TestServer, id: u32) -> HookParked<trc::Result<()>> {
    let server = test.server.clone();
    HookParked::start(id, set_search_destroy_pause, async move {
        // Reindex's admission gate will be wired in Task 16b.
        assert!(server.za_is_key_account(id).await.unwrap());
        server.za_search_destroy(id, true).await
    })
    .await
}

async fn assert_failed_first_destroy(traces: Traced, id: u32) {
    let events = traces.finish().await;
    assert!(
        events.iter().any(|event| {
            event.typ == EventType::Store(StoreEvent::AssertValueFailed)
                && event.account_id == Some(u64::from(id))
                && event.text.contains("Failed to destroy account")
        }),
        "the injected first destroy must fail: {events:?}",
    );
}

async fn case_r7_interrupted_transitions_leave_nothing(test: &TestServer, admin: &Account) {
    let key = destroy_fixture(admin, "search-destroy-1@example.com").await;
    let id = account_id(&key);
    for number in 1..=5 {
        deliver_one(
            test,
            &key,
            &format!("Interrupted publication fixture {number}"),
        )
        .await;
    }
    let holds = temporary_blob_hashes(test, id).await;

    // Expiry is sampled during staging. The pre-commit interruption must
    // leave only temporary links, with no publication to recover.
    set_search_hold_expire(Some(id));
    let parked = park(test, &key, set_search_commit_pause).await;
    let mut hashes = staged_hashes(test, id, &holds).await;
    for hash in &hashes {
        assert!(!stored_blob(test, hash).await.is_empty());
    }
    assert!(principal_hashes(test, id).await.is_empty());
    assert_eq!(
        parked.finish().await,
        PassOutcome::Aborted(TransientClass::BlobStore)
    );
    set_search_hold_expire(None);
    simulate_restart(test, id).await;

    // The other interruption happens after publication, before resident
    // state can be installed. Those blobs are already permanently linked.
    set_search_swap_skip(Some(id));
    let parked = park(test, &key, set_search_swap_pause).await;
    let committed = principal_hashes(test, id).await;
    assert_eq!(committed.len(), 2);
    assert!(committed.iter().all(|hash| !hashes.contains(hash)));
    assert_manifest_links(test, id, &index(test, id).await).await;
    hashes.extend(committed);
    assert!(matches!(
        parked.finish().await,
        PassOutcome::Advanced {
            published: true,
            ..
        }
    ));
    set_search_swap_skip(None);
    simulate_restart(test, id).await;

    admin.destroy_account(key).await;
    assert_destroyed_index(test, admin, id, &hashes).await;
}

async fn case_f23_unparseable_manifest(test: &TestServer, admin: &Account, retry: bool) {
    let name = if retry {
        "search-destroy-garbage-retry@example.com"
    } else {
        "search-destroy-3@example.com"
    };
    let key = destroy_fixture(admin, name).await;
    let id = account_id(&key);
    for number in 1..=3 {
        deliver_one(
            test,
            &key,
            &format!("Unparseable manifest fixture {number}"),
        )
        .await;
    }
    advanced(test, &key, true).await;
    let hashes = principal_hashes(test, id).await;
    assert_eq!(hashes.len(), 2);
    write_manifest_bytes(test, id, Some(vec![0x55; 40])).await;

    let traces = retry.then(|| Traced::start(&[EventType::Store(StoreEvent::AssertValueFailed)]));
    let parked = park_destroy(admin, &key).await;
    assert_eq!(
        read_manifest(test, id).await,
        Some(ManifestValue::Destroying(Inventory::default()))
    );
    assert_eq!(principal_hashes(test, id).await, hashes);
    if retry {
        set_search_destroy_fail_once(Some(id));
    }
    parked.finish().await;
    assert_destroyed_index(test, admin, id, &hashes).await;
    if let Some(traces) = traces {
        set_search_destroy_fail_once(None);
        assert_failed_first_destroy(traces, id).await;
    }
}

async fn case_empty_marker_retry_scans(test: &TestServer, admin: &Account) {
    case_f23_unparseable_manifest(test, admin, true).await;
}

async fn case_f23_pre_release_account_without_a_manifest(test: &TestServer, admin: &Account) {
    let key = destroy_fixture(admin, "search-destroy-4@example.com").await;
    let id = account_id(&key);
    write_manifest_bytes(test, id, None).await;
    assert!(read_manifest(test, id).await.is_none());

    // No keyed pass ran. Seed an orphan to distinguish the fallback scan
    // from an empty-inventory no-op when field 152 is missing.
    let hash = seed_orphan_link(test, id, b"orphaned search index fixture").await;
    assert_eq!(principal_hashes(test, id).await, vec![hash.clone()]);

    let parked = park_destroy(admin, &key).await;
    assert_eq!(
        read_manifest(test, id).await,
        Some(ManifestValue::Destroying(Inventory::default()))
    );
    parked.finish().await;
    assert_destroyed_index(test, admin, id, &[hash]).await;
}

async fn case_r19_paused_pass_loses_to_the_marker(test: &TestServer, admin: &Account) {
    let key = destroy_fixture(admin, "search-destroy-5@example.com").await;
    let id = account_id(&key);
    deliver_one(test, &key, "Previously committed index").await;
    advanced(test, &key, true).await;
    let manifest = index(test, id).await;
    let committed = principal_hashes(test, id).await;
    assert_eq!(committed.len(), 2);

    deliver_one(test, &key, "Staged before destroy").await;
    let holds = temporary_blob_hashes(test, id).await;
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let pass = park(test, &key, set_search_commit_pause).await;
    let staged = staged_hashes(test, id, &holds).await;
    let destroy = park_destroy(admin, &key).await;

    assert_eq!(
        read_manifest(test, id).await,
        Some(ManifestValue::Destroying(manifest.inventory()))
    );
    assert_eq!(principal_hashes(test, id).await, committed);
    // Keep the holds live through the CAS. Expiry would abort before
    // reaching the assertion and would not demonstrate the marker fence.
    assert_eq!(pass.finish().await, PassOutcome::Lost);
    assert_eq!(principal_hashes(test, id).await, committed);
    let holds = temporary_blob_hashes(test, id).await;
    assert!(staged.iter().all(|hash| holds.contains(hash)));
    let events = traces.finish().await;
    assert!(events.iter().any(|event| event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_LOST), "{events:?}");

    destroy.finish().await;
    let hashes = committed.into_iter().chain(staged).collect::<Vec<_>>();
    assert_destroyed_index(test, admin, id, &hashes).await;
}

async fn case_g12_destroy_dies_after_the_marker(test: &TestServer, admin: &Account) {
    let key = destroy_fixture(admin, "search-destroy-6@example.com").await;
    let id = account_id(&key);
    for number in 1..=3 {
        deliver_one(test, &key, &format!("Destroy retry fixture {number}")).await;
        advanced(test, &key, true).await;
    }
    let manifest = index(test, id).await;
    assert_manifest_links(test, id, &manifest).await;
    let hashes = principal_hashes(test, id).await;
    let traces = Traced::start(&[EventType::Store(StoreEvent::AssertValueFailed)]);
    let parked = park_destroy(admin, &key).await;
    assert_eq!(
        read_manifest(test, id).await,
        Some(ManifestValue::Destroying(manifest.inventory()))
    );
    assert_eq!(principal_hashes(test, id).await, hashes);
    set_search_destroy_fail_once(Some(id));
    parked.finish().await;

    assert_destroyed_index(test, admin, id, &hashes).await;
    set_search_destroy_fail_once(None);
    assert_failed_first_destroy(traces, id).await;
}

async fn case_reset_cannot_clear_after_registry_deletion(test: &TestServer, admin: &Account) {
    let key = destroy_fixture(admin, "search-destroy-reset-deleted@example.com").await;
    let id = account_id(&key);
    let document = deliver_one(test, &key, "Reset racing permanent account deletion").await;
    let index_bytes = manifest_bytes(test, id).await.unwrap();
    // A genuine empty index and no links ensure no unlink batch can
    // reject the reset before it reaches the final marker-clear guard.
    assert_eq!(index(test, id).await.inventory(), Inventory::default());
    assert!(principal_hashes(test, id).await.is_empty());
    assert_eq!(message_ids(test, id).await, vec![document]);
    let vault_cas = test.server.za_vault_record(id).await.unwrap().unwrap().cas;
    let reset = park_reset(test, id).await;
    let marker = manifest_bytes(test, id).await.unwrap();
    assert_eq!(
        read_manifest(test, id).await,
        Some(ManifestValue::Destroying(Inventory::default()))
    );

    // Let the real destroy task adopt the marker while range deletion
    // is still unable to make the reset's marker assertion fail.
    let destroy = park_destroy(admin, &key).await;
    let account = ValueKey::from(ValueClass::Registry(RegistryClass::Item {
        object_id: ObjectType::Account.to_id(),
        item_id: u64::from(id),
    }));
    assert!(!test.server.store().key_exists(account).await.unwrap());
    assert_eq!(
        test.server.za_vault_record(id).await.unwrap().unwrap().cas,
        vault_cas
    );
    assert_eq!(
        manifest_bytes(test, id).await.as_deref(),
        Some(marker.as_slice())
    );
    assert!(principal_hashes(test, id).await.is_empty());
    let result = reset.finish().await;
    assert!(
        result.as_ref().is_err_and(|err| err.is_assertion_failure()),
        "reset with a deleted Account Item must refuse"
    );
    assert_eq!(
        manifest_bytes(test, id).await.as_deref(),
        Some(marker.as_slice())
    );
    assert_eq!(
        entry_outcome(read_manifest(test, id).await),
        Err(LoadFailure::NoMap(NoMapReason::Destroying))
    );

    // The would-be manifest creator still has the current vault assertion.
    // It must lose its absence assertion against the retained marker.
    let mut create = BatchBuilder::new();
    create
        .with_account_id(id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .assert_value(
            PrincipalField::ZeroAccessVault,
            AssertValue::Hash(vault_cas),
        )
        .assert_value(PrincipalField::ZeroAccessSearch, AssertValue::None)
        .set(PrincipalField::ZeroAccessSearch, index_bytes);
    assert!(
        test.server
            .store()
            .write_batch(&mut create)
            .await
            .unwrap_err()
            .is_assertion_failure()
    );

    destroy.finish().await;
    assert_destroyed_index(test, admin, id, &[]).await;
}

async fn case_reset_clears_a_live_account(test: &TestServer, admin: &Account) {
    let key = destroy_fixture(admin, "search-destroy-reset-live@example.com").await;
    let id = account_id(&key);
    let document = deliver_one(test, &key, "Live account reset").await;
    advanced(test, &key, true).await;
    let hashes = principal_hashes(test, id).await;
    assert_eq!(hashes.len(), 2);
    let vault_cas = test.server.za_vault_record(id).await.unwrap().unwrap().cas;

    test.server.za_search_destroy(id, true).await.unwrap();
    assert!(principal_hashes(test, id).await.is_empty());
    assert!(read_manifest(test, id).await.is_none());
    assert_eq!(
        test.server.za_vault_record(id).await.unwrap().unwrap().cas,
        vault_cas
    );
    assert_eq!(message_ids(test, id).await, vec![document]);

    admin.destroy_account(key).await;
    assert_destroyed_index(test, admin, id, &hashes).await;
}

async fn case_reset_after_destroy_creates_no_marker(test: &TestServer, admin: &Account) {
    let key = destroy_fixture(admin, "search-destroy-reset-after@example.com").await;
    let id = account_id(&key);
    let server = test.server.clone();
    let (admitted, admission) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let reset = tokio::spawn(async move {
        assert!(server.za_is_key_account(id).await.unwrap());
        admitted.send(()).unwrap();
        released.await.unwrap();
        server.za_search_destroy(id, true).await
    });
    admission.await.unwrap();

    admin.destroy_account(key).await;
    test.wait_for_tasks().await;
    assert!(test.server.za_vault_record(id).await.unwrap().is_none());
    assert!(read_manifest(test, id).await.is_none());
    release.send(()).unwrap();
    let result = reset.await.unwrap();
    assert!(result.as_ref().is_err_and(|err| err.is_assertion_failure()));
    assert!(
        read_manifest(test, id).await.is_none(),
        "the stale reset must not recreate field 152"
    );
    assert_destroyed_index(test, admin, id, &[]).await;
}

async fn case_stale_reset_scan_keeps_new_links(test: &TestServer, admin: &Account) {
    let key = destroy_fixture(admin, "search-destroy-reset-stale@example.com").await;
    let id = account_id(&key);
    let empty_index = manifest_bytes(test, id).await.unwrap();
    write_manifest_bytes(test, id, None).await;
    let reset = park_reset(test, id).await;
    assert_eq!(
        read_manifest(test, id).await,
        Some(ManifestValue::Destroying(Inventory::default()))
    );

    // Supply the starting manifest a different reset/rebuild could leave.
    // The new links and their committed inventory come from the real pass.
    write_manifest_bytes(test, id, Some(empty_index)).await;
    deliver_one(test, &key, "New links after stale reset").await;
    advanced(test, &key, true).await;
    let committed = manifest_bytes(test, id).await;
    let links = principal_blob_links(test, id).await;
    assert_eq!(links.len(), 2);
    let result = reset.finish().await;
    assert!(
        result.as_ref().is_err_and(|err| err.is_assertion_failure()),
        "the stale reset must lose its unlink assertion"
    );
    assert_eq!(manifest_bytes(test, id).await, committed);
    assert_eq!(
        principal_blob_links(test, id).await,
        links,
        "the stale scan must leave new links intact"
    );

    let hashes = links.into_iter().map(|(hash, _)| hash).collect::<Vec<_>>();
    admin.destroy_account(key).await;
    assert_destroyed_index(test, admin, id, &hashes).await;
}

async fn case_stale_permanent_destroy_creates_no_marker(test: &TestServer, admin: &Account) {
    let key = destroy_fixture(admin, "search-destroy-stale-permanent@example.com").await;
    let id = account_id(&key);
    deliver_one(test, &key, "Stale permanent worker fixture").await;
    advanced(test, &key, true).await;
    let hashes = principal_hashes(test, id).await;
    assert_eq!(hashes.len(), 2);
    admin.destroy_account(key).await;
    assert_destroyed_index(test, admin, id, &hashes).await;
    assert!(test.server.za_vault_record(id).await.unwrap().is_none());

    // The duplicate worker has already started the task. It reaches this
    // helper only after the peer removed the account properties and task.
    test.server.za_search_destroy(id, false).await.unwrap();
    assert!(
        read_manifest(test, id).await.is_none(),
        "stale permanent destroy recreated field 152 after task completion"
    );

    set_search_destroy_fail_once(Some(id));
    let result = test.server.za_search_destroy(id, false).await;
    set_search_destroy_fail_once(None);
    assert!(result.as_ref().is_err_and(|err| err.is_assertion_failure()));
    assert!(
        read_manifest(test, id).await.is_none(),
        "failed stale permanent destroy recreated field 152"
    );
}

async fn seed_orphan_link(test: &TestServer, id: u32, bytes: &[u8]) -> BlobHash {
    let hash = BlobHash::generate(bytes);
    let (_, hold) = test
        .server
        .hold_blob(id, hash.clone(), bytes, 60, &BlobOpts::PLAIN)
        .await
        .unwrap();
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Principal)
        .with_document(0);
    hold.release(&mut batch).set(
        BlobOp::Link {
            hash: hash.clone(),
            to: BlobLink::Document,
        },
        BlobLinkValue::new(bytes.len()),
    );
    test.server.store().write_batch(&mut batch).await.unwrap();
    hash
}

async fn passwordless_destroy_fixture(admin: &Account, name: &'static str) -> Account {
    admin
        .create_passwordless_user_account(
            name,
            "Destroy-Search-46!",
            "Search destroy",
            &[],
            user_permissions(),
        )
        .await
}

async fn case_double_absence_removes_only_exact_orphan_links(test: &TestServer, admin: &Account) {
    let key = passwordless_destroy_fixture(admin, "search-destroy-no-vault@example.com").await;
    let neighbor = passwordless_destroy_fixture(admin, "search-destroy-neighbor@example.com").await;
    let id = account_id(&key);
    let bytes = b"double absence orphan fixture";
    let hash = seed_orphan_link(test, id, bytes).await;
    let document_link = BlobOp::Link {
        hash: hash.clone(),
        to: BlobLink::Document,
    };
    let neighbors = [
        (id, Collection::Principal, 1),
        (id, Collection::Email, 0),
        (account_id(&neighbor), Collection::Principal, 0),
    ];
    let mut batch = BatchBuilder::new();
    for (account, collection, document) in neighbors {
        batch
            .with_account_id(account)
            .with_collection(collection)
            .with_document(document)
            .set(document_link.clone(), BlobLinkValue::new(bytes.len()));
    }
    test.server.store().write_batch(&mut batch).await.unwrap();
    assert_eq!(principal_hashes(test, id).await, vec![hash.clone()]);
    assert!(read_manifest(test, id).await.is_none());
    assert!(test.server.za_vault_record(id).await.unwrap().is_none());

    let parked = park_destroy(admin, &key).await;
    assert!(read_manifest(test, id).await.is_none());
    parked.finish().await;
    test.wait_for_tasks().await;
    expire_and_purge_blobs(test, admin).await;
    assert!(principal_hashes(test, id).await.is_empty());
    assert!(read_manifest(test, id).await.is_none());
    assert_eq!(stored_blob(test, &hash).await, bytes);

    let mut clear_neighbors = BatchBuilder::new();
    for (account_id, collection, document_id) in neighbors {
        let key = ValueKey {
            account_id,
            collection: collection as u8,
            document_id,
            class: ValueClass::Blob(document_link.clone()),
        };
        assert!(test.server.store().key_exists(key).await.unwrap());
        // These are deliberately fabricated neighboring links, outside the
        // destroyed account's index. Clear just the three fixture controls.
        clear_neighbors
            .with_account_id(account_id)
            .with_collection(collection)
            .with_document(document_id)
            .clear(document_link.clone());
    }
    test.server
        .store()
        .write_batch(&mut clear_neighbors)
        .await
        .unwrap();
    admin.destroy_account(neighbor).await;
    assert_destroyed_index(test, admin, id, &[hash]).await;
}

async fn case_double_absence_scan_refuses_new_state(
    test: &TestServer,
    admin: &Account,
    vault_appears: bool,
) {
    let name = if vault_appears {
        "search-destroy-vault-appears@example.com"
    } else {
        "search-destroy-manifest-appears@example.com"
    };
    let key = passwordless_destroy_fixture(admin, name).await;
    let id = account_id(&key);
    let hash = seed_orphan_link(test, id, name.as_bytes()).await;
    assert_eq!(principal_hashes(test, id).await, vec![hash.clone()]);
    let server = test.server.clone();
    let parked = HookParked::start(id, set_search_destroy_pause, async move {
        server.za_search_destroy(id, false).await
    })
    .await;
    assert!(read_manifest(test, id).await.is_none());
    assert!(test.server.za_vault_record(id).await.unwrap().is_none());

    let pending = if vault_appears {
        // Use the normal setup-token path to obtain a real PendingSetup
        // record while keeping the search field absent.
        za_setup_token(admin, key.name()).await;
        assert!(read_manifest(test, id).await.is_none());
        Some(test.server.za_vault_record(id).await.unwrap().unwrap())
    } else {
        // The persisted search-field sentinel exercises the assertion
        // separately, with no vault that could make the chunk fail first.
        write_manifest_bytes(test, id, Some(vec![0x55; 40])).await;
        assert!(test.server.za_vault_record(id).await.unwrap().is_none());
        None
    };
    let result = parked.finish().await;
    assert!(
        result.as_ref().is_err_and(|err| err.is_assertion_failure()),
        "{} appearance must stop the old unlink batch",
        if vault_appears { "vault" } else { "manifest" }
    );
    assert_eq!(principal_hashes(test, id).await, vec![hash.clone()]);

    if pending.is_none() {
        write_manifest_bytes(test, id, None).await;
    }
    admin.destroy_account(key).await;
    assert_destroyed_index(test, admin, id, &[hash]).await;

    if let Some(pending) = pending {
        // This is the actual combined vault/manifest transaction used
        // by setup, with the read PendingSetup record from before destroy.
        let mut record = pending.record.clone();
        let extra = ZaVaultExtra {
            field: PrincipalField::ZeroAccessSearch,
            value: za_setup_manifest(&vault::keys::Secret::random(), 0),
        };
        assert!(
            test.server
                .za_vault_commit(id, &mut record, Some(&pending), None, Some(extra))
                .await
                .is_err_and(|err| err.is_assertion_failure())
        );
        assert!(test.server.za_vault_record(id).await.unwrap().is_none());
        assert!(read_manifest(test, id).await.is_none());
    }
}
