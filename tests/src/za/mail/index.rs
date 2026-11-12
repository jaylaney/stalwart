/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Request-owned indexing against sealed LMTP/IMAP mail and the real store.

use super::{
    Traced, corrupt_blob, deliver, key_mail_account, message_ids, message_with, metadata_key,
    stored_blob, stored_hash,
};
use crate::{
    utils::{
        account::Account,
        imap::{ImapConnection, Type},
        jmap::JmapResponse,
        pop3::{Pop3Connection, ResponseType as Pop3Reply},
        server::TestServer,
        za::{
            HookParked, expire_and_purge_blobs, force_loss, index_holds, log_records_since,
            lower_changes_history, manifest_bytes, pop3_delete, principal_blob_links,
            purge_account, read_manifest, run_search_pass, search_config_guard, simulate_restart,
            temporary_blob_hashes, wait_for_delivery, write_manifest_bytes, za_search_thread_part,
            za_setup_token, za_unlock,
        },
    },
    za::{
        leak::{create_blob_codec_key, recode_blobs, retire_blob_codec_key},
        tracing::{ws_call, ws_close, ws_connect},
        user_permissions,
    },
};
use common::auth::vault::ZaVaultExtra;
use common::za::{
    search::{
        ZaSearchConfig,
        cache::EvictReason,
        error::{LoadFailure, LossClass, NoMapReason, TransientClass},
        load::{ObservedLoadError, entry_outcome},
        seal::{parse_thread_blob, za_search_key},
        setup::za_setup_manifest,
        trace::{
            ZA_SEARCH_EVENT_TYPE, ZA_SEARCH_LOST, ZA_SEARCH_PASS, ZA_SEARCH_REBUILD,
            ZA_SEARCH_SKIPPED,
        },
    },
    test_hooks::{
        SearchBlobTimeoutOnce, reset_za_search_counters, set_search_blob_fail_once,
        set_search_blob_slow, set_search_commit_pause, set_search_config_override,
        set_search_destroy_fail_once, set_search_destroy_pause, set_search_hold_expire,
        set_search_open_fail_all, set_search_pass_pause, set_search_post_unlink_pause,
        set_search_rebuild_create_pause, set_search_recovery_pause, set_search_snapshot_pause,
        set_search_swap_pause, set_search_swap_skip, za_search_blob_reads, za_search_message_opens,
    },
};
use email::{
    cache::MessageCacheFetch,
    mailbox::{DRAFTS_ID, INBOX_ID},
    message::messagedata::EmailMessageData,
    message::za::search::{
        changes::ZaRetainedBound,
        ensure::{Unavailable, ZaSearchEnsure, is_current},
        pass::{PassOutcome, ZaSearchPass},
        rebuild::{RebuildStarted, ZaSearchRebuild, za_search_content_needs_rebuild},
    },
};
use imap_proto::ResponseType;
use jmap_proto::types::state::State;
use registry::{
    schema::{
        enums::{CompressionAlgo, TaskAccountMaintenanceType, TaskBlobRecodeCompression},
        prelude::ObjectType,
        structs::{
            Task, TaskAccountMaintenance, TaskBlobRecodeEncryption, TaskBlobRecodeKey, TaskStatus,
        },
    },
    types::EnumImpl,
};
use serde_json::{Value, json};
use std::{
    str::FromStr,
    time::{Duration, Instant},
};
use store::{
    ValueKey,
    query::log::{Change, PartialChange},
    write::{
        BatchBuilder, BlobLink, BlobOp, RegistryClass, ValueClass,
        assert::AssertValue,
        blob::BlobLinkValue,
        blob_codec::{BlobFormat, BlobOpts},
    },
    za_search::{
        BlobHeader, BlobKind, Checkpoint,
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
    let search_fix =
        key_mail_account(test, "searchfix-fixture@example.com", "Searchfix-Fixture!").await;
    case_in_window_absence_does_not_consume_an_insert(test, &search_fix).await;
    clean_messages(test, &search_fix).await;
    case_warm_content_transient_does_not_block_a_served_map(test, &search_fix).await;
    clean_messages(test, &search_fix).await;
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

    for account in [
        &key,
        &split,
        &plain,
        &triggers,
        &waits,
        &plain_triggers,
        &search_fix,
    ] {
        test.destroy_all_mailboxes(account).await;
    }
    case_blob_hook_does_not_create_mailboxes(test, &triggers, false).await;
    test.wait_for_tasks().await;
    case_i6_reindex_resets_the_index(test, &admin).await;
    case_reindex_without_email_rows(test, &admin).await;
    case_blob_recode_recodes_the_index(test, &admin).await;
    destroy_cases(test, &admin).await;
    test_recovery(test).await;
    test_compaction(test, &admin).await;
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
    let load_flight = entry.content_load_flight.lock().await;
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

async fn append_many(test: &TestServer, key: &Account, count: usize) -> Vec<u32> {
    let id = account_id(key);
    assert!(message_ids(test, id).await.is_empty());
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
    assert_eq!(docs.len(), count);
    docs
}

async fn multiappend(test: &TestServer, key: &Account, count: usize) -> (u64, Vec<u32>) {
    let id = account_id(key);
    let before = change_id(test, id).await;
    let docs = append_many(test, key, count).await;
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
    reset_za_search_counters(id);
    for n in 0..100 {
        set_flag(key, document, n % 2 == 0).await;
        assert_eq!(pass(test, key).await, PassOutcome::Current);
    }
    assert_eq!(manifest_bytes(test, id).await, bytes);
    assert_eq!(principal_blob_links(test, id).await, links);
    assert_eq!(za_search_message_opens(id), 0);
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
    reset_za_search_counters(id);
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
    assert_eq!(za_search_message_opens(id), 0);
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

    let needle = "repeatedtransientrecoveryfixture";
    let next = deliver_one(test, key, needle).await;
    let before = index(test, id).await;
    let links = principal_blob_links(test, id).await;
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    // Cross the former default skip threshold and reload the persisted failure
    // count midway through the outage. None of these attempts consumes mail.
    for attempts in 1..=10 {
        if attempts == 9 {
            simulate_restart(test, id).await;
        }
        set_search_blob_fail_once(Some(id));
        assert_eq!(
            pass(test, key).await,
            PassOutcome::Aborted(TransientClass::BlobStore)
        );
        let failed = index(test, id).await;
        assert_eq!(
            failed.failing,
            Some(FailingDocument {
                document_id: next,
                attempts,
            })
        );
        assert_eq!(failed.indexed_to, before.indexed_to);
        assert_eq!(failed.content_to, before.content_to);
        assert_eq!(failed.cursor, before.cursor);
        assert_eq!(failed.reconciliation, before.reconciliation);
        assert_eq!(failed.epoch, before.epoch);
        assert_eq!(principal_blob_links(test, id).await, links);
        assert!(!index_holds(test, id, next).await);
    }
    set_search_blob_fail_once(None);
    let events = traces.finish().await;
    assert!(
        !events.iter().any(|event| {
            event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_SKIPPED
        }),
        "{events:?}"
    );

    advanced(test, key, true).await;
    let recovered = index(test, id).await;
    assert!(recovered.failing.is_none());
    assert_eq!(recovered.epoch, before.epoch);
    assert!(index_holds(test, id, next).await);
    assert_eq!(
        queried(&recovery_query(key, Some(needle)).await),
        [email_id(next)]
    );
}

async fn case_in_window_absence_does_not_consume_an_insert(test: &TestServer, key: &Account) {
    println!("case_in_window_absence_does_not_consume_an_insert");
    let _config = search_config_guard!(test, absent_retry_secs: 60, segments: 255);
    let id = account_id(key);
    let needle = "absencerecoveryfixture";
    let document = deliver_one(test, key, needle).await;
    let keys = za_unlock(test, id, key.secret()).await;
    let hash = stored_hash(test, id, document).await;
    let saved = stored_blob(test, &hash).await;
    let before = index(test, id).await;
    let links = principal_blob_links(test, id).await;

    let blob_store = test.server.blob_store();
    assert!(blob_store.delete_blob(hash.as_slice()).await.unwrap());
    let start = Instant::now();
    let mut attempts = Vec::new();
    for _ in 0..10 {
        let outcome = test.server.za_search_pass(id, &keys).await;
        let manifest = read_manifest(test, id).await;
        let links = principal_blob_links(test, id).await;
        attempts.push((outcome, manifest, links));
    }
    let elapsed = start.elapsed();
    blob_store
        .put_blob(hash.as_slice(), &saved, &test.server.core.blob.email)
        .await
        .unwrap();

    assert!(elapsed < Duration::from_secs(59), "{elapsed:?}");
    for (outcome, manifest, after_links) in attempts {
        assert_eq!(
            outcome.unwrap(),
            PassOutcome::Aborted(TransientClass::AbsentWithinWindow)
        );
        let Some(ManifestValue::Index(manifest)) = manifest else {
            panic!("aborted pass did not retain the manifest: {manifest:?}");
        };
        assert_eq!(manifest.indexed_to, before.indexed_to);
        assert_eq!(manifest.content_to, before.content_to);
        assert_eq!(manifest.cursor, before.cursor);
        assert_eq!(manifest.reconciliation, before.reconciliation);
        assert_eq!(after_links, links);
        assert_eq!(
            manifest.failing,
            Some(FailingDocument {
                document_id: document,
                attempts: 0,
            })
        );
    }

    advanced(test, key, true).await;
    let after = index(test, id).await;
    assert_eq!(after.epoch, before.epoch);
    assert!(after.failing.is_none());
    assert!(index_holds(test, id, document).await);
    assert_eq!(
        queried(&recovery_query(key, Some(needle)).await),
        [email_id(document)]
    );

    let _expired = search_config_guard!(test, absent_retry_secs: 0);
    let expired = deliver_one(test, key, "Already expired absence fixture").await;
    let hash = stored_hash(test, id, expired).await;
    let saved = stored_blob(test, &hash).await;
    let latest = change_id(test, id).await;
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    assert!(blob_store.delete_blob(hash.as_slice()).await.unwrap());
    let outcome = test.server.za_search_pass(id, &keys).await;
    blob_store
        .put_blob(hash.as_slice(), &saved, &test.server.core.blob.email)
        .await
        .unwrap();
    assert!(matches!(
        outcome.unwrap(),
        PassOutcome::Advanced {
            published: false,
            indexed_to,
        } if indexed_to.complete && indexed_to.change_id == latest
    ));
    let after = index(test, id).await;
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(after.content_to.checkpoint, after.indexed_to);
    assert!(!index_holds(test, id, expired).await);
    let events = traces.finish().await;
    assert!(events.iter().any(|event| {
        event.account_id == Some(u64::from(id))
            && event.details == ZA_SEARCH_SKIPPED
            && event.reason == LossClass::BlobAbsent.as_str()
            && event
                .keys
                .contains(&(Key::DocumentId, TraceValue::UInt(u64::from(expired))))
    }));
}

async fn case_warm_content_transient_does_not_block_a_served_map(test: &TestServer, key: &Account) {
    println!("case_warm_content_transient_does_not_block_a_served_map");
    let _config = search_config_guard!(test, segments: 255);
    let id = account_id(key);
    let root = message_with("Map refresh fixture", "maprefreshrootfixture");
    let reference = root
        .lines()
        .find_map(|line| line.strip_prefix("Message-ID: "))
        .unwrap();
    deliver(test, SENDER, key.name(), &root).await;
    let root_id = super::new_message(test, id, &[]).await;
    advanced(test, key, true).await;
    assert_eq!(
        queried(&recovery_query(key, Some("maprefreshrootfixture")).await),
        [email_id(root_id)]
    );
    let old_hash = {
        let view = loaded(test, id).await;
        assert!(view.content.is_some());
        view.manifest_bytes_hash
    };

    let needle = "maprefreshreplyfixture";
    let reply = message_with("Re: Map refresh fixture", needle).replacen(
        "\r\n\r\n",
        &format!("\r\nIn-Reply-To: {reference}\r\n\r\n"),
        1,
    );
    deliver(test, SENDER, key.name(), &reply).await;
    let reply_id = super::new_message(test, id, &[root_id]).await;
    let keys = za_unlock(test, id, key.secret()).await;
    set_search_swap_skip(Some(id));
    let outcome = test.server.za_search_pass(id, &keys).await;
    set_search_swap_skip(None);
    assert!(matches!(
        outcome.unwrap(),
        PassOutcome::Advanced {
            published: true,
            ..
        }
    ));
    let manifest = index(test, id).await;
    let raw = manifest_bytes(test, id).await.unwrap();
    let committed_hash = manifest_hash(&raw);
    assert_ne!(committed_hash, old_hash);
    let links = principal_blob_links(test, id).await;
    {
        let view = loaded(test, id).await;
        assert_eq!(view.manifest_bytes_hash, old_hash);
        assert!(view.content.is_some());
    }
    let thread_hash = &manifest.thread_blobs.last().unwrap().hash;
    assert_eq!(
        &BlobHash::generate(&stored_blob(test, thread_hash).await),
        thread_hash
    );

    let timeout = SearchBlobTimeoutOnce::new(id, manifest.content.as_ref().unwrap().hash.clone());
    let mapped_reply = Id::from_parts(root_id, reply_id).to_string();
    assert_ne!(mapped_reply, email_id(reply_id));
    reset_za_search_counters(id);
    let answer = recovery_get(key).await;
    assert!(
        !timeout.was_consumed(),
        "the keyed Map path read optional Content: {:?}",
        answer.0
    );
    assert_eq!(answer.name_at(0), "Email/get", "{:?}", answer.0);
    let thread_id = Id::from(root_id).to_string();
    let emails = answer.method_response()["list"].as_array().unwrap();
    assert!(
        emails.iter().any(|email| {
            email["id"].as_str() == Some(mapped_reply.as_str())
                && email["threadId"].as_str() == Some(thread_id.as_str())
        }),
        "{:?}",
        answer.0
    );
    assert_eq!(loaded(test, id).await.manifest_bytes_hash, committed_hash);
    assert_eq!(za_search_blob_reads(id), 1);

    assert_search_unavailable(&recovery_query(key, Some(needle)).await);
    assert!(timeout.was_consumed());
    drop(timeout);
    reset_za_search_counters(id);
    assert_eq!(
        queried(&recovery_query(key, Some(needle)).await),
        [mapped_reply]
    );
    assert_eq!(za_search_blob_reads(id), 1);
    assert_eq!(manifest_bytes(test, id).await.unwrap(), raw);
    assert_eq!(principal_blob_links(test, id).await, links);
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
    {
        let resident = loaded(test, id).await;
        assert!(
            resident.thread.record(document).is_none(),
            "the staged thread record became resident before commit"
        );
    }
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

async fn reindex_account(test: &TestServer, admin: &Account, key: &Account) {
    admin
        .registry_create_object(Task::AccountMaintenance(TaskAccountMaintenance {
            account_id: key.id(),
            maintenance_type: TaskAccountMaintenanceType::Reindex,
            status: TaskStatus::now(),
        }))
        .await;
    tokio::time::timeout(Duration::from_secs(30), test.wait_for_tasks())
        .await
        .expect("search fixture's reindex task did not finish");
}

async fn case_i6_reindex_resets_the_index(test: &TestServer, admin: &Account) {
    use store::write::serialize::RawValue;

    println!("case_i6_reindex_resets_the_index");
    let key = destroy_fixture(admin, "keymail63@example.com").await;
    let id = account_id(&key);
    for number in 1..=4 {
        deliver_one(
            test,
            &key,
            &format!("reindexneedle indexedfixture {number}"),
        )
        .await;
    }
    let indexed = get_recovery_ids(&key).await;
    assert_eq!(indexed.len(), 4);
    assert_eq!(
        queried(&recovery_query(&key, Some("reindexneedle")).await),
        indexed
    );
    let before = index(test, id).await;

    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    set_search_open_fail_all(Some(id));
    let mut skipped = Vec::new();
    for number in 1..=3 {
        skipped.push(
            deliver_one(
                test,
                &key,
                &format!("reindexneedle skippedfixture {number}"),
            )
            .await,
        );
        mailbox_get(&key).await;
    }
    let all = get_recovery_ids(&key).await;
    assert_eq!(all.len(), 7);
    assert!(queried(&recovery_query(&key, Some("skippedfixture")).await).is_empty());
    let threads = key
        .jmap_method_call("Thread/get", json!({"accountId": key.id_string()}))
        .await;
    assert_eq!(threads.name_at(0), "Thread/get", "{:?}", threads.0);
    set_search_open_fail_all(None);

    let events = traces.finish().await;
    let mut skipped_events = events
        .iter()
        .filter(|event| {
            event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_SKIPPED
        })
        .map(|event| {
            event
                .keys
                .iter()
                .find_map(|(key, value)| match (key, value) {
                    (Key::DocumentId, TraceValue::UInt(value)) => {
                        Some(u32::try_from(*value).unwrap())
                    }
                    _ => None,
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    skipped.sort_unstable();
    skipped_events.sort_unstable();
    assert_eq!(skipped_events, skipped);
    let skipped_ids = all
        .iter()
        .filter(|email| skipped.contains(&Id::from_str(email).unwrap().document_id()))
        .collect::<Vec<_>>();
    assert_eq!(skipped_ids.len(), 3);
    for email in skipped_ids {
        assert!(
            threads
                .list()
                .iter()
                .any(|thread| { reply_ids(thread, "emailIds").as_slice() == [email.as_str()] }),
            "missing singleton for skipped message {email}"
        );
    }

    let vault_key = ValueKey::property(
        id,
        Collection::Principal,
        0,
        PrincipalField::ZeroAccessVault,
    );
    let store = test.server.store();
    let vault = store
        .get_value::<RawValue>(vault_key.clone())
        .await
        .unwrap()
        .unwrap()
        .0;
    let mut rows = Vec::new();
    for document in message_ids(test, id).await {
        let row_key = metadata_key(id, document);
        let bytes = store
            .get_value::<RawValue>(row_key.clone())
            .await
            .unwrap()
            .unwrap()
            .0;
        rows.push((row_key, bytes));
    }
    assert_eq!(rows.len(), 7);
    let before_hashes = principal_hashes(test, id).await;
    assert!(!before_hashes.is_empty());
    reindex_account(test, admin, &key).await;

    assert!(
        manifest_bytes(test, id).await.is_none(),
        "reindex must durably clear field 152 before keyed recovery"
    );
    assert!(principal_blob_links(test, id).await.is_empty());
    assert!(
        store
            .get_value::<RawValue>(vault_key)
            .await
            .unwrap()
            .is_some_and(|raw| raw.0 == vault),
        "reindex changed the stored vault record"
    );
    assert_eq!(message_ids(test, id).await.len(), 7);
    for (row_key, before) in rows {
        assert!(
            store
                .get_value::<RawValue>(row_key)
                .await
                .unwrap()
                .is_some_and(|raw| raw.0 == before),
            "reindex changed a stored Email row"
        );
    }

    // The resident view still exists. Only the ordinary keyed request can
    // notice the missing durable generation and recover it.
    let rebuilt_ids = get_recovery_ids(&key).await;
    let mut rebuilt_docs = rebuilt_ids
        .iter()
        .map(|email| Id::from_str(email).unwrap().document_id())
        .collect::<Vec<_>>();
    rebuilt_docs.sort_unstable();
    assert_eq!(rebuilt_docs, message_ids(test, id).await);
    let after_get = index(test, id).await;
    assert_ne!(after_get.epoch, before.epoch);
    assert_eq!(
        queried(&recovery_query(&key, Some("reindexneedle")).await),
        rebuilt_ids
    );
    let rebuilt = index(test, id).await;
    assert_manifest_links(test, id, &rebuilt).await;
    let mut hashes = before_hashes;
    hashes.extend(principal_hashes(test, id).await);
    admin.destroy_account(key).await;
    assert_destroyed_index(test, admin, id, &hashes).await;
}

async fn case_reindex_without_email_rows(test: &TestServer, admin: &Account) {
    println!("case_reindex_without_email_rows");
    let key = destroy_fixture(admin, "search-reindex-empty@example.com").await;
    let id = account_id(&key);
    assert!(message_ids(test, id).await.is_empty());
    let before = index(test, id).await;
    assert!(before.thread_blobs.is_empty() && before.content.is_none());
    assert_manifest_links(test, id, &before).await;

    reindex_account(test, admin, &key).await;
    assert!(
        manifest_bytes(test, id).await.is_none(),
        "reindex skipped the empty key account"
    );
    assert!(principal_blob_links(test, id).await.is_empty());
    assert!(get_recovery_ids(&key).await.is_empty());
    let after = index(test, id).await;
    assert_ne!(after.epoch, before.epoch);
    assert_manifest_links(test, id, &after).await;

    admin.destroy_account(key).await;
    assert_destroyed_index(test, admin, id, &[]).await;
}

async fn case_blob_recode_recodes_the_index(test: &mut TestServer, admin: &Account) {
    println!("case_blob_recode_recodes_the_index");
    let key = destroy_fixture(admin, "keymail62@example.com").await;
    let id = account_id(&key);
    for number in 1..=3 {
        deliver_one(test, &key, &format!("recodeneedle fixture {number}")).await;
    }
    let expected_ids = get_recovery_ids(&key).await;
    assert_eq!(expected_ids.len(), 3);
    let manifest = index(test, id).await;
    assert!(!manifest.thread_blobs.is_empty() && manifest.content.is_some());
    assert_manifest_links(test, id, &manifest).await;
    let hashes = principal_hashes(test, id).await;
    let mut expected_bytes = Vec::new();
    for hash in &hashes {
        expected_bytes.push(stored_blob(test, hash).await);
    }

    let codec_key = create_blob_codec_key(admin).await;
    admin.reload_settings().await;
    test.reload_core();
    recode_blobs(
        test,
        admin,
        TaskBlobRecodeCompression::Zstd,
        TaskBlobRecodeEncryption::Encrypt(TaskBlobRecodeKey { key_id: codec_key }),
    )
    .await;
    for (hash, bytes) in hashes.iter().zip(&expected_bytes) {
        let raw = test
            .server
            .blob_store()
            .get_raw_blob(hash.as_slice())
            .await
            .unwrap()
            .unwrap();
        let format = BlobFormat::of(&raw).unwrap();
        assert_eq!(
            format.key_id,
            Some(codec_key.document_id()),
            "Principal index blob {hash:?} missed the codec sweep"
        );
        // Zstd may leave sealed payloads uncompressed when compression does
        // not reduce their size. Compute that choice from the original bytes.
        let compressed = BlobOpts::compressed(CompressionAlgo::Zstd)
            .encode(hash.as_slice(), bytes)
            .unwrap();
        assert_eq!(
            format.compression,
            BlobFormat::of(&compressed).unwrap().compression
        );
        assert_eq!(&stored_blob(test, hash).await, bytes);
    }
    simulate_restart(test, id).await;
    assert_eq!(get_recovery_ids(&key).await, expected_ids);
    assert_eq!(
        queried(&recovery_query(&key, Some("recodeneedle")).await),
        expected_ids
    );
    assert_eq!(index(test, id).await, manifest);

    // The recode task swept the whole store. Remove the outer encryption
    // before retiring the key, including from the other fixtures' blobs.
    recode_blobs(
        test,
        admin,
        TaskBlobRecodeCompression::Lz4,
        TaskBlobRecodeEncryption::Disable,
    )
    .await;
    admin.destroy_account(key).await;
    assert_destroyed_index(test, admin, id, &hashes).await;
    retire_blob_codec_key(test, admin, codec_key, None).await;
}

async fn destroy_cases(test: &TestServer, admin: &Account) {
    case_stale_permanent_destroy_creates_no_marker(test, admin).await;
    case_double_absence_removes_only_exact_orphan_links(test, admin).await;
    case_double_absence_scan_refuses_new_state(test, admin, false).await;
    case_double_absence_scan_refuses_new_state(test, admin, true).await;
    case_r7_interrupted_transitions_leave_nothing(test, admin).await;
    case_r7_middle_node_break(test, admin, false).await;
    case_r7_middle_node_break(test, admin, true).await;
    case_f23_unparseable_manifest(test, admin, false).await;
    case_f23_pre_release_account_without_a_manifest(test, admin).await;
    case_r19_paused_pass_loses_to_the_marker(test, admin).await;
    case_r19_no_publication_after_unlink(test, admin).await;
    case_r19_absent_manifest_creator_loses_after_destroy(test, admin).await;
    case_g12_destroy_dies_after_the_marker(test, admin).await;
    case_empty_marker_retry_scans(test, admin).await;
    case_reset_clears_a_live_account(test, admin).await;
    case_reset_cannot_clear_after_registry_deletion(test, admin).await;
    case_reset_after_destroy_creates_no_marker(test, admin).await;
    case_stale_reset_scan_keeps_new_links(test, admin).await;
    case_stale_reset_loses_to_reindex_rebuild(test, admin).await;
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

async fn case_r7_middle_node_break(test: &TestServer, admin: &Account, recover: bool) {
    println!("case_r7_middle_node_break recover={recover}");
    let name = if recover {
        "search-destroy-2-recovery@example.com"
    } else {
        "search-destroy-2@example.com"
    };
    let key = destroy_fixture(admin, name).await;
    let id = account_id(&key);
    let _config = search_config_guard!(test, segments: 8, absent_retry_secs: 0);
    for number in 1..=6 {
        deliver_one(test, &key, &format!("brokenchainneedle fixture {number}")).await;
        advanced(test, &key, true).await;
    }

    let mapped = if recover {
        let reply = recovery_get(&key).await;
        assert_eq!(reply.name_at(0), "Email/get", "{:?}", reply.0);
        assert_eq!(reply.list().len(), 6);
        Some(reply)
    } else {
        None
    };
    let manifest = index(test, id).await;
    assert_manifest_links(test, id, &manifest).await;
    let old_content = content_hashes(test, &manifest).await;
    assert_eq!(old_content.len(), 6);
    let middle = &old_content[2];
    let mut hashes = principal_hashes(test, id).await;
    assert!(hashes.len() > 6 && hashes.contains(middle));
    assert!(
        test.server
            .blob_store()
            .delete_blob(middle.as_slice())
            .await
            .unwrap()
    );
    simulate_restart(test, id).await;

    if let Some(mapped) = mapped {
        let mut expected = mapped
            .list()
            .iter()
            .map(|email| email["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        expected.sort_unstable();
        let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
        assert_eq!(
            queried(&recovery_query(&key, Some("brokenchainneedle")).await),
            expected
        );
        let after = index(test, id).await;
        assert_eq!(after.epoch, manifest.epoch);
        assert_eq!(after.thread_blobs, manifest.thread_blobs);
        assert!(after.content_to.cursor.is_none());
        let current = principal_hashes(test, id).await;
        assert!(
            old_content.iter().all(|hash| !current.contains(hash)),
            "content recovery left old content links before destroy"
        );
        assert_manifest_links(test, id, &after).await;
        let after_get = recovery_get(&key).await;
        assert_eq!(after_get.name_at(0), "Email/get", "{:?}", after_get.0);
        assert_eq!(after_get.list().len(), mapped.list().len());
        assert!(
            mapped
                .list()
                .iter()
                .all(|email| after_get.list().contains(email))
        );
        let events = traces.finish().await;
        let scanned = events.iter().any(|event| {
            rebuild_trace(event, id, "content")
                && event.keys.iter().any(|(key, value)| {
                    *key == Key::Source
                        && matches!(value, TraceValue::String(source) if source.as_str() == "scan")
                })
        });
        assert!(
            scanned,
            "content recovery did not use the exact Principal link scan"
        );
        assert!(
            !events
                .iter()
                .any(|event| rebuild_trace(event, id, "rebuild"))
        );
        hashes.extend(current);
    }

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
        server.za_search_reset(id).await
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

async fn case_r19_no_publication_after_unlink(test: &TestServer, admin: &Account) {
    println!("case_r19_no_publication_after_unlink");
    let key = destroy_fixture(admin, "search-destroy-post-unlink@example.com").await;
    let id = account_id(&key);
    deliver_one(test, &key, "Committed before unlink").await;
    advanced(test, &key, true).await;
    let committed = principal_hashes(test, id).await;
    assert_eq!(committed.len(), 2);

    deliver_one(test, &key, "Staged across unlink").await;
    let holds = temporary_blob_hashes(test, id).await;
    let server = test.server.clone();
    let keys = za_unlock(test, id, key.secret()).await;
    let pass = HookParked::start(id, set_search_commit_pause, async move {
        server.za_search_pass(id, &keys).await
    })
    .await;
    let staged = staged_hashes(test, id, &holds).await;

    let admin_request = admin.clone();
    let destroy = HookParked::start(id, set_search_post_unlink_pause, async move {
        admin_request.destroy_account(key).await;
    })
    .await;
    let after_unlink = principal_hashes(test, id).await;
    let holds = temporary_blob_hashes(test, id).await;
    // No expiry or purge until the prepared pass has tried its commit.
    let outcome = pass.finish().await;
    destroy.finish().await;

    // Inspect terminal state before checking the writer result. A successful
    // writer could leave fresh links that were outside the unlink inventory.
    let hashes = committed
        .into_iter()
        .chain(staged.iter().cloned())
        .collect::<Vec<_>>();
    assert_destroyed_index(test, admin, id, &hashes).await;
    assert!(
        after_unlink.is_empty(),
        "destroy had not unlinked its inventory"
    );
    assert!(staged.iter().all(|hash| holds.contains(hash)));
    assert_eq!(outcome.unwrap(), PassOutcome::Lost);
}

async fn case_r19_absent_manifest_creator_loses_after_destroy(test: &TestServer, admin: &Account) {
    println!("case_r19_absent_manifest_creator_loses_after_destroy");
    let key = destroy_fixture(admin, "search-destroy-absent-create@example.com").await;
    let id = account_id(&key);
    deliver_one(test, &key, "Present for absent-manifest creation").await;
    advanced(test, &key, true).await;
    let committed = principal_hashes(test, id).await;
    assert_eq!(committed.len(), 2);
    write_manifest_bytes(test, id, None).await;
    simulate_restart(test, id).await;

    let keys = za_unlock(test, id, key.secret()).await;
    let loss = match test.server.za_search_entry(id, &keys).await {
        Err(ObservedLoadError::MapLoss(loss)) => loss,
        _ => panic!("the cold loader did not observe missing manifest loss"),
    };
    assert_eq!(loss.class(), LossClass::ManifestMissing);
    assert!(loss.raw().is_none());

    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let server = test.server.clone();
    let creator = HookParked::start(id, set_search_rebuild_create_pause, async move {
        server.za_search_rebuild_start(id, &keys, loss, None).await
    })
    .await;
    assert!(test.server.za_vault_record(id).await.unwrap().is_some());
    assert!(manifest_bytes(test, id).await.is_none());
    admin.destroy_account(key).await;
    assert_destroyed_index(test, admin, id, &committed).await;
    assert!(test.server.za_vault_record(id).await.unwrap().is_none());

    let outcome = creator.finish().await;
    assert!(
        manifest_bytes(test, id).await.is_none(),
        "the prepared creator resurrected field 152"
    );
    assert!(test.server.za_vault_record(id).await.unwrap().is_none());
    assert!(principal_blob_links(test, id).await.is_empty());
    for collection in [SyncCollection::Email, SyncCollection::Thread] {
        assert!(log_records_since(test, id, collection, 0).await.is_empty());
    }
    let events = traces.finish().await;
    assert!(
        events.iter().any(|event| {
            event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_LOST
        }),
        "{events:?}"
    );
    assert_eq!(outcome.unwrap(), RebuildStarted::LostToAnotherNode);
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
    let before = index(test, id).await;
    let mut hashes = principal_hashes(test, id).await;
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

    let mapped = get_recovery_ids(&key).await;
    assert_eq!(mapped.len(), 1);
    assert_eq!(Id::from_str(&mapped[0]).unwrap().document_id(), document);
    let after = index(test, id).await;
    assert_ne!(after.epoch, before.epoch);
    assert_manifest_links(test, id, &after).await;
    hashes.extend(principal_hashes(test, id).await);
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

async fn case_stale_reset_loses_to_reindex_rebuild(test: &TestServer, admin: &Account) {
    println!("case_stale_reset_loses_to_reindex_rebuild");
    let key = destroy_fixture(admin, "search-reset-reindex-overlap@example.com").await;
    let id = account_id(&key);
    deliver_one(test, &key, "overlapneedle fixture").await;
    write_manifest_bytes(test, id, None).await;
    assert!(principal_blob_links(test, id).await.is_empty());

    // The single task manager serializes AccountMaintenance. Model a stale
    // execution after lease loss by running A's real reset helper directly.
    // B is an actual queued Reindex and can finish while A stays parked.
    let reset = park_reset(test, id).await;
    assert_eq!(
        read_manifest(test, id).await,
        Some(ManifestValue::Destroying(Inventory::default()))
    );
    reindex_account(test, admin, &key).await;
    assert!(
        manifest_bytes(test, id).await.is_none(),
        "queued reindex did not clear A's empty marker"
    );
    assert!(principal_blob_links(test, id).await.is_empty());

    let mapped = get_recovery_ids(&key).await;
    assert_eq!(mapped.len(), 1);
    let rebuilt = index(test, id).await;
    assert_manifest_links(test, id, &rebuilt).await;
    let committed = manifest_bytes(test, id).await.unwrap();
    let links = principal_blob_links(test, id).await;
    assert!(!links.is_empty());

    let result = reset.finish().await;
    assert_eq!(
        principal_blob_links(test, id).await,
        links,
        "A's stale scan unlinked B's rebuilt index"
    );
    assert!(result.as_ref().is_err_and(|err| err.is_assertion_failure()));
    assert_eq!(manifest_bytes(test, id).await, Some(committed));
    assert_eq!(
        queried(&recovery_query(&key, Some("overlapneedle")).await),
        mapped
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

pub(crate) async fn test_recovery(test: &mut TestServer) {
    let _config = search_config_guard!(test, absent_retry_secs: 0, segments: 255, wait_secs: 15);
    let admin = test.account("admin@example.com").clone();
    let key = key_mail_account(test, "keymail58@example.com", "Keymail-58!").await;
    let content = key_mail_account(test, "keymail59@example.com", "Keymail-59!").await;
    let gap = key_mail_account(test, "keymail60@example.com", "Keymail-60!").await;
    let timeout = key_mail_account(
        test,
        "keymail-thread-timeout@example.com",
        "Thread-timeout!",
    )
    .await;
    case_f3_thread_blob_timeout_keeps_index(test, &timeout).await;
    test.destroy_all_mailboxes(&timeout).await;

    case_zero_row_epoch(test, &key, "fresh").await;
    mailbox_get(&key).await;
    assert!(!mailbox_archives(test, account_id(&key)).await.is_empty());
    test.destroy_all_mailboxes(&key).await;
    case_zero_row_epoch(test, &key, "after mailbox deletion").await;
    case_r3_rebuild_moves_the_epoch(test, &key).await;
    case_f7_newer_format_is_no_map(test, &key).await;
    clean_messages(test, &key).await;
    case_r10_cursor_finishes_across_restarts(test, &key).await;
    clean_messages(test, &key).await;
    case_pending_rebuild_cursor_overrides_scanned_state(test, &key).await;
    clean_messages(test, &key).await;
    case_r22_vanished_cursor_checkpoints(test, &key).await;
    clean_messages(test, &key).await;
    case_h11_missing_manifest_on_an_empty_account(test, &key).await;

    case_h23_content_loss_is_content_only(test, &content).await;
    clean_messages(test, &content).await;
    case_pl5_content_chain_break_keeps_the_thread_blobs(test, &content, &admin).await;
    clean_messages(test, &content).await;
    case_f6_r23_over_the_cap_then_raised(test, &content).await;
    clean_messages(test, &content).await;

    case_f9_reconciliation_renders_nothing_seen(test, &gap, &admin).await;
    clean_messages(test, &gap).await;
    case_pl6_unreadable_gap_and_retained_inserts_finish(test, &gap, &admin).await;
    clean_messages(test, &gap).await;
    case_f9_truncation_during_a_pending_rebuild(test, &gap, &admin).await;
    clean_messages(test, &gap).await;
    case_r24_g2_retained_deletion_rendered_once(test, &gap, &admin).await;
    clean_messages(test, &gap).await;

    case_r27_empty_rebuild_keeps_clients(test, &key).await;
    case_g3_loss_with_all_deleted_refuses_holders(test, &key).await;
    case_r2_old_epoch_map_never_served(test, &key).await;
    clean_messages(test, &key).await;
    for (name, loss) in [
        ("keymail-task14-stale-thread@example.com", StaleLoss::Thread),
        (
            "keymail-task14-stale-content@example.com",
            StaleLoss::Content,
        ),
        (
            "keymail-task14-stale-missing@example.com",
            StaleLoss::Missing,
        ),
        ("keymail-task14-stale-older@example.com", StaleLoss::Older),
        ("keymail-task14-stale-tag@example.com", StaleLoss::Tag),
        ("keymail-task14-stale-parse@example.com", StaleLoss::Parse),
    ] {
        let fixture = key_mail_account(test, name, "Keymail-Recovery!").await;
        case_stale_loss_keeps_successor(test, &fixture, loss).await;
        test.destroy_all_mailboxes(&fixture).await;
    }
    super::threads::test_recovery(test, &key).await;
    for account in [&key, &content, &gap] {
        test.destroy_all_mailboxes(account).await;
    }
}

async fn mailbox_archives(test: &TestServer, id: u32) -> Vec<u32> {
    let mut ids = Vec::new();
    test.server
        .archives(id, Collection::Mailbox, Field::ARCHIVE, &(), |id, _| {
            ids.push(id);
            Ok(true)
        })
        .await
        .unwrap();
    ids
}

async fn upload_contact(account: &Account) {
    let reply = account.jmap_method_call("Blob/upload", json!({
        "accountId": account.id_string(),
        "create": { "vcard": { "data": [{ "data:asText": "BEGIN:VCARD\r\nEND:VCARD\r\n" }] }},
    })).await;
    assert!(
        reply.0["methodResponses"][0][1]["created"]["vcard"]["id"].is_string(),
        "{:?}",
        reply.0
    );
}

async fn email_boundary(test: &TestServer, id: u32) -> u64 {
    test.server
        .store()
        .get_last_change_id(id, SyncCollection::Email.into())
        .await
        .unwrap()
        .unwrap_or_default()
}

async fn case_zero_row_epoch(test: &TestServer, key: &Account, phase: &str) {
    use common::auth::vault::RawValue;
    use store::write::LogCollection;
    println!("case_zero_row_epoch: {phase}");
    let id = account_id(key);
    assert!(mailbox_archives(test, id).await.is_empty());
    test.server
        .message_datas(id, &(), |_, _| {
            panic!("zero-row fixture contains an Email row")
        })
        .await
        .unwrap();
    let original = manifest_bytes(test, id).await.unwrap();
    let previous = email_boundary(test, id).await;
    upload_contact(key).await;
    assert_eq!(manifest_bytes(test, id).await.unwrap(), original);
    assert_eq!(email_boundary(test, id).await, previous);

    write_manifest_bytes(test, id, None).await;
    simulate_restart(test, id).await;
    upload_contact(key).await;
    assert!(mailbox_archives(test, id).await.is_empty());
    assert!(
        manifest_bytes(test, id).await.is_some(),
        "{phase}: keyed upload left the manifest absent"
    );

    let rebuilt = index(test, id).await;
    assert_eq!(rebuilt.revision, 1);
    assert!(rebuilt.epoch > previous);
    assert_eq!(rebuilt.epoch, rebuilt.newest_row.change_id);
    assert_eq!(rebuilt.newest_row.revision, rebuilt.revision);
    assert_eq!(rebuilt.epoch, email_boundary(test, id).await);
    assert!(rebuilt.thread_blobs.is_empty());
    assert!(rebuilt.cursor.is_none() && rebuilt.content.is_none());
    let store = test.server.store();
    assert_eq!(
        store
            .get_counter(ValueKey {
                account_id: id,
                collection: Collection::Email.into(),
                document_id: 0,
                class: ValueClass::ChangeId(SyncCollection::Email.change_group()),
            })
            .await
            .unwrap() as u64,
        rebuilt.epoch
    );
    let bytes = store
        .get_value::<RawValue>(
            LogCollection::Sync(SyncCollection::Email).log_key(id, rebuilt.epoch),
        )
        .await
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(bytes, [0]);
    let records = store
        .za_changes_records(
            id,
            SyncCollection::Email,
            Some(Checkpoint {
                change_id: previous,
                position: 0,
                complete: true,
            }),
            usize::MAX,
        )
        .await
        .unwrap();
    assert!(!records.truncated && !records.resumed_record_lost && records.first_retained.is_none());
    assert_eq!(records.records, [(rebuilt.epoch, vec![])]);
    assert!(
        store
            .get_value::<RawValue>(
                LogCollection::Sync(SyncCollection::Thread).log_key(id, rebuilt.epoch)
            )
            .await
            .unwrap()
            .is_none()
    );

    let stable = manifest_bytes(test, id).await.unwrap();
    upload_contact(key).await;
    assert_eq!(manifest_bytes(test, id).await.unwrap(), stable);
    assert_eq!(email_boundary(test, id).await, rebuilt.epoch);
    assert!(mailbox_archives(test, id).await.is_empty());
}

async fn get_recovery_ids(key: &Account) -> Vec<String> {
    let reply = recovery_get(key).await;
    assert_eq!(reply.name_at(0), "Email/get", "{:?}", reply.0);
    let mut ids: Vec<_> = reply.method_response()["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value["id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort_unstable();
    ids
}

async fn recovery_get(key: &Account) -> JmapResponse {
    key.jmap_method_call(
        "Email/get",
        json!({
            "accountId": key.id_string(), "ids": null, "properties": ["id", "threadId"]
        }),
    )
    .await
}

async fn recovery_query(key: &Account, text: Option<&str>) -> JmapResponse {
    let mut request = json!({"accountId": key.id_string(), "limit": 500});
    if let Some(text) = text {
        request["filter"] = json!({"text": text});
    }
    key.jmap_method_call("Email/query", request).await
}

fn queried(reply: &JmapResponse) -> Vec<String> {
    assert_eq!(reply.name_at(0), "Email/query", "{:?}", reply.0);
    let mut ids = reply_ids(reply.method_response(), "ids");
    ids.sort_unstable();
    ids
}

fn reply_ids(body: &Value, field: &str) -> Vec<String> {
    body[field]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect()
}

async fn recovery_changes(key: &Account, method: &str, since: &State) -> JmapResponse {
    key.jmap_method_call(
        method,
        json!({
            "accountId": key.id_string(), "sinceState": since.to_string(), "maxChanges": 500
        }),
    )
    .await
}

fn recovery_state(reply: &JmapResponse) -> State {
    State::parse(reply.method_response()["newState"].as_str().unwrap()).unwrap()
}

fn assert_no_changes(reply: &JmapResponse, method: &str) {
    assert_eq!(reply.name_at(0), method, "{:?}", reply.0);
    for field in ["created", "updated", "destroyed"] {
        assert!(
            reply_ids(reply.method_response(), field).is_empty(),
            "{:?}",
            reply.0
        );
    }
}

async fn append_recovery_messages(test: &TestServer, key: &Account, count: usize) -> Vec<u32> {
    let id = account_id(key);
    let before = message_ids(test, id).await;
    // Each command commits a distinct Email record. Suppress the
    // opportunistic hooks to model arrivals before the next keyed pass.
    let entry = test.server.inner.cache.za_search.entry(id);
    let pass = entry.pass_lock.lock().await;
    let mut imap = ImapConnection::connect(b"_recovery ").await;
    imap.authenticate(key.name(), key.secret()).await;
    imap.send_ok("SELECT INBOX").await;
    for n in 0..count {
        let message = message_with(&format!("Recovery fixture {n}"), "recoveryneedle");
        imap.send(&format!("APPEND INBOX {{{}}}", message.len()))
            .await;
        imap.assert_read(Type::Continuation, ResponseType::Ok).await;
        imap.send_untagged(&message).await;
        imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    }
    drop(imap);
    drop(pass);
    test.wait_for_tasks().await;
    let ids: Vec<_> = message_ids(test, id)
        .await
        .into_iter()
        .filter(|d| !before.contains(d))
        .collect();
    assert_eq!(ids.len(), count);
    ids
}

async fn case_f3_thread_blob_timeout_keeps_index(test: &TestServer, key: &Account) {
    println!("case_f3_thread_blob_timeout_keeps_index");
    let id = account_id(key);
    let first = deliver_one(test, key, "Thread load before the timed-out segment").await;
    advanced(test, key, true).await;
    let second = deliver_one(test, key, "Thread load with a timed-out segment").await;
    advanced(test, key, true).await;
    let before = index(test, id).await;
    assert_eq!(before.thread_blobs.len(), 2);
    let hash = &before.thread_blobs[1].hash;
    assert_ne!(hash, &before.thread_blobs[0].hash);
    assert_manifest_links(test, id, &before).await;
    assert_eq!(&BlobHash::generate(&stored_blob(test, hash).await), hash);
    let raw = manifest_bytes(test, id).await;
    let links = principal_blob_links(test, id).await;

    simulate_restart(test, id).await;
    assert!(test.server.inner.cache.za_search.get(id).is_none());
    reset_za_search_counters(id);
    let timeout = SearchBlobTimeoutOnce::new(id, hash.clone());
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);

    // Mailbox/query takes the real request pre-pass, including map recovery
    // on loss. The handler reads the mailbox cache without loading the map.
    let query = async || {
        let reply = key
            .jmap_method_call("Mailbox/query", json!({ "accountId": key.id_string() }))
            .await;
        assert_eq!(
            reply.0["methodResponses"][0][0], "Mailbox/query",
            "{:?}",
            reply.0
        );
    };
    query().await;
    assert_eq!(
        manifest_bytes(test, id).await,
        raw,
        "the timeout changed the index manifest"
    );
    assert_eq!(principal_blob_links(test, id).await, links);
    assert!(
        timeout.was_consumed(),
        "the selected thread blob was not read"
    );
    assert_eq!(
        za_search_blob_reads(id),
        2,
        "both thread segments were fetched"
    );
    let entry = test.server.inner.cache.za_search.get(id).unwrap();
    assert!(
        entry.read_loaded().await.is_none(),
        "a partial map was installed"
    );
    let events = traces.finish().await;
    assert!(
        !events.iter().any(|event| {
            event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_REBUILD
        }),
        "{events:?}"
    );

    query().await;
    assert_eq!(
        za_search_blob_reads(id),
        4,
        "the retry fetched both segments again"
    );
    assert!(index_holds(test, id, first).await);
    assert!(index_holds(test, id, second).await);
    assert_eq!(manifest_bytes(test, id).await, raw);
    assert_eq!(principal_blob_links(test, id).await, links);
}

async fn case_r3_rebuild_moves_the_epoch(test: &TestServer, key: &Account) {
    println!("case_r3_rebuild_moves_the_epoch");
    let id = account_id(key);
    append_recovery_messages(test, key, 3).await;
    assert_eq!(get_recovery_ids(key).await.len(), 3);
    let mut since = recovery_state(&recovery_changes(key, "Email/changes", &State::Initial).await);
    let original = index(test, id).await;
    let mut prior = original.epoch;
    for mode in ["thread", "missing", "older"] {
        match mode {
            "thread" => force_loss(test, id).await,
            "missing" => {
                write_manifest_bytes(test, id, None).await;
                simulate_restart(test, id).await;
            }
            "older" => {
                let mut bytes = manifest_bytes(test, id).await.unwrap();
                bytes[0] = 0;
                write_manifest_bytes(test, id, Some(bytes)).await;
                simulate_restart(test, id).await;
            }
            _ => unreachable!(),
        }
        assert_eq!(get_recovery_ids(key).await.len(), 3);
        let rebuilt = index(test, id).await;
        assert!(rebuilt.epoch > prior, "{mode}: {rebuilt:?}");
        assert_eq!(
            rebuilt.revision,
            if mode == "thread" {
                original.revision + 2
            } else {
                2
            }
        );
        prior = rebuilt.epoch;
        assert_eq!(
            recovery_changes(key, "Email/changes", &since)
                .await
                .error_type_at(0),
            Some("cannotCalculateChanges"),
        );
        let query = recovery_query(key, Some("recoveryneedle")).await;
        assert_eq!(queried(&query).len(), 3);
        since = State::parse(query.method_response()["queryState"].as_str().unwrap()).unwrap();
        assert_no_changes(
            &recovery_changes(key, "Email/changes", &since).await,
            "Email/changes",
        );
    }
}

async fn case_f7_newer_format_is_no_map(test: &TestServer, key: &Account) {
    println!("case_f7_newer_format_is_no_map");
    let id = account_id(key);
    let original = manifest_bytes(test, id).await.unwrap();
    let mut newer = original.clone();
    newer[0] = store::za_search::SEARCH_FORMAT_V1 + 1;
    write_manifest_bytes(test, id, Some(newer.clone())).await;
    simulate_restart(test, id).await;
    assert_eq!(
        recovery_get(key).await.error_type_at(0),
        Some("serverUnavailable")
    );
    assert_search_unavailable(&recovery_query(key, Some("recoveryneedle")).await);
    assert_eq!(manifest_bytes(test, id).await.unwrap(), newer);
    write_manifest_bytes(test, id, Some(original)).await;
    simulate_restart(test, id).await;
}

async fn case_r10_cursor_finishes_across_restarts(test: &TestServer, key: &Account) {
    println!("case_r10_cursor_finishes_across_restarts");
    let id = account_id(key);
    append_many(test, key, 200).await;
    mailbox_get(key).await;
    force_loss(test, id).await;
    for step in 1..=6 {
        mailbox_get(key).await;
        let manifest = index(test, id).await;
        if step == 1 {
            assert_eq!(loaded(test, id).await.thread.records().count(), 48);
        }
        simulate_restart(test, id).await;
        if manifest.cursor.is_none() {
            assert_eq!(
                queried(&recovery_query(key, Some("grouped")).await).len(),
                200
            );
            return;
        }
    }
    panic!("the rebuild cursor did not finish in six keyed requests");
}

async fn case_pending_rebuild_cursor_overrides_scanned_state(test: &TestServer, key: &Account) {
    println!("case_pending_rebuild_cursor_overrides_scanned_state");
    let id = account_id(key);
    let documents = append_many(test, key, 2).await;
    advanced(test, key, true).await;
    force_loss(test, id).await;

    let keys = za_unlock(test, id, key.secret()).await;
    let loss = match test.server.za_search_entry(id, &keys).await {
        Err(ObservedLoadError::MapLoss(loss)) => loss,
        _ => panic!("the cold loader did not observe the lost thread blob"),
    };
    assert_eq!(
        test.server
            .za_search_rebuild_start(id, &keys, loss, None)
            .await
            .unwrap(),
        RebuildStarted::Written
    );
    let pending = index(test, id).await;
    let cursor = pending.cursor.as_ref().expect("pending identity rebuild");
    let state = email_boundary(test, id).await;
    assert_eq!(change_id(test, id).await, state);

    let _config = search_config_guard!(test, pass_cap: 1);
    let entry = test.server.za_search_entry(id, &keys).await.unwrap();
    {
        let mut guard = entry.index.write().await;
        let resident = guard.as_mut().unwrap();
        assert_eq!(resident.manifest, pending);
        // A reload can preserve a newer scan watermark from the previous map.
        // Seed that resident state directly to make the shortcut deterministic.
        resident.scanned_to = resident.scanned_to.max(state);
        assert!(state <= resident.scanned_to);
        assert!(resident.manifest.indexed_to.complete);
        assert!(resident.manifest.reconciliation.is_none());
        assert!(resident.manifest.content_to.cursor.is_none());
        assert!(
            za_search_content_needs_rebuild(&resident.manifest, &test.server.za_search_config())
                .is_none()
        );
    }

    advanced(test, key, true).await;
    let partial = index(test, id).await;
    let next = partial
        .cursor
        .as_ref()
        .expect("second document still pending");
    assert!(next.next > cursor.next);
    assert_eq!(next.next, documents[0] + 1);
    assert_eq!(next.end, cursor.end);
    assert_held(test, id, &documents, 1).await;

    advanced(test, key, true).await;
    assert!(index(test, id).await.cursor.is_none());
    simulate_restart(test, id).await;
    let rebuilt = za_search_thread_part(test, id, key.secret()).await;
    let mut indexed = rebuilt
        .records()
        .filter(|record| record.deleted.is_none())
        .map(|record| record.document_id)
        .collect::<Vec<_>>();
    indexed.sort_unstable();
    assert_eq!(indexed, documents);
}

async fn case_r22_vanished_cursor_checkpoints(test: &TestServer, key: &Account) {
    println!("case_r22_vanished_cursor_checkpoints");
    let id = account_id(key);
    let documents = append_many(test, key, 60).await;
    mailbox_get(key).await;
    force_loss(test, id).await;
    mailbox_get(key).await;
    let before = index(test, id).await;
    assert!(before.cursor.is_some());
    assert_held(test, id, &documents, 48).await;
    for &document in &documents[48..] {
        pop3_delete(test, key, document).await;
    }
    let after_expunge = email_boundary(test, id).await;
    assert!(after_expunge > before.newest_row.change_id);
    simulate_restart(test, id).await;
    mailbox_get(key).await;
    let after = index(test, id).await;
    assert!(after.cursor.is_none());
    assert_eq!(after.thread_blobs, before.thread_blobs);
    assert_eq!(email_boundary(test, id).await, after_expunge);
}

async fn case_h11_missing_manifest_on_an_empty_account(test: &TestServer, key: &Account) {
    println!("case_h11_missing_manifest_on_an_empty_account");
    let id = account_id(key);
    let mut mailboxes = mailbox_archives(test, id).await;
    assert!(!mailboxes.is_empty());
    assert!(message_ids(test, id).await.is_empty());
    let previous = email_boundary(test, id).await;
    write_manifest_bytes(test, id, None).await;
    simulate_restart(test, id).await;
    mailbox_get(key).await;
    let after = index(test, id).await;
    assert!(after.epoch > previous && after.thread_blobs.is_empty() && after.cursor.is_none());
    let records = log_records_since(test, id, SyncCollection::Email, previous).await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0, after.epoch);
    let mut witnessed = records[0]
        .1
        .iter()
        .map(|change| match *change {
            Change::UpdateContainerPartial(id, changes) if changes.has_properties() => id as u32,
            _ => panic!("unexpected boundary: {records:?}"),
        })
        .collect::<Vec<_>>();
    mailboxes.sort_unstable();
    witnessed.sort_unstable();
    assert_eq!(witnessed, mailboxes);
    let raw = manifest_bytes(test, id).await.unwrap();
    mailbox_get(key).await;
    assert_eq!(manifest_bytes(test, id).await.unwrap(), raw);
}

async fn content_hashes(test: &TestServer, manifest: &Manifest) -> Vec<BlobHash> {
    let mut next = manifest.content.as_ref().map(|head| head.hash.clone());
    let mut hashes = Vec::new();
    while let Some(hash) = next {
        let bytes = stored_blob(test, &hash).await;
        next = BlobHeader::decode(&bytes)
            .unwrap()
            .0
            .predecessor
            .map(|(hash, _)| hash);
        hashes.push(hash);
    }
    hashes
}

async fn case_h23_content_loss_is_content_only(test: &TestServer, key: &Account) {
    println!("case_h23_content_loss_is_content_only");
    let _config = search_config_guard!(test, absent_retry_secs: 0);
    let id = account_id(key);
    append_recovery_messages(test, key, 4).await;
    let mapped = get_recovery_ids(key).await;
    let before = index(test, id).await;
    let hashes = content_hashes(test, &before).await;
    test.server
        .blob_store()
        .delete_blobs(&[hashes[0].as_slice()])
        .await
        .unwrap();
    simulate_restart(test, id).await;
    assert_eq!(get_recovery_ids(key).await, mapped);
    assert_eq!(index(test, id).await.epoch, before.epoch);
    assert_eq!(
        queried(&recovery_query(key, Some("recoveryneedle")).await),
        mapped
    );
    let after = index(test, id).await;
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(after.newest_row, before.newest_row);
    assert!(after.content_to.cursor.is_none());
    let linked = principal_hashes(test, id).await;
    assert!(hashes.iter().all(|hash| !linked.contains(hash)));
}

fn rebuild_trace(event: &super::TracedEvent, id: u32, kind: &str) -> bool {
    event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_REBUILD
        && event.keys.iter().any(|(key, value)| {
            matches!((key, value), (Key::Type, TraceValue::String(value)) if value.as_str() == kind)
        })
}

async fn case_pl5_content_chain_break_keeps_the_thread_blobs(
    test: &TestServer,
    key: &Account,
    admin: &Account,
) {
    println!("case_pl5_content_chain_break_keeps_the_thread_blobs");
    let _config = search_config_guard!(test, segments: 64, absent_retry_secs: 0);
    let id = account_id(key);
    for _ in 0..3 {
        append_recovery_messages(test, key, 1).await;
        mailbox_get(key).await;
    }
    let mapped = get_recovery_ids(key).await;
    let before = index(test, id).await;
    let hashes = content_hashes(test, &before).await;
    assert!(hashes.len() >= 3 && before.thread_blobs.len() >= 3);
    test.server
        .blob_store()
        .delete_blobs(&[hashes[1].as_slice()])
        .await
        .unwrap();
    simulate_restart(test, id).await;
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    assert_eq!(
        queried(&recovery_query(key, Some("recoveryneedle")).await),
        mapped
    );
    expire_and_purge_blobs(test, admin).await;
    simulate_restart(test, id).await;
    assert_eq!(get_recovery_ids(key).await, mapped);
    let after = index(test, id).await;
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(after.thread_blobs, before.thread_blobs);
    let linked = principal_hashes(test, id).await;
    for blob in &before.thread_blobs {
        assert!(linked.contains(&blob.hash));
        assert_eq!(
            stored_blob(test, &blob.hash).await.len(),
            blob.size as usize
        );
    }
    assert!(hashes.iter().all(|hash| !linked.contains(hash)));
    let events = traces.finish().await;
    assert!(
        !events
            .iter()
            .any(|event| rebuild_trace(event, id, "rebuild"))
    );
    assert!(
        events
            .iter()
            .any(|event| rebuild_trace(event, id, "content"))
    );
}

fn assert_search_unavailable(reply: &JmapResponse) {
    assert_eq!(
        reply.error_type_at(0),
        Some("unsupportedFilter"),
        "{:?}",
        reply.0
    );
    assert!(
        reply
            .method_response()
            .to_string()
            .contains(common::za::ZA_MAIL_SEARCH_UNAVAILABLE),
        "{:?}",
        reply.0
    );
}

async fn deliver_large_recovery(test: &TestServer, key: &Account, ordinal: usize) -> u32 {
    use std::fmt::Write;
    let id = account_id(key);
    let before = message_ids(test, id).await;
    let mut body = String::with_capacity(220_000);
    for n in 0..16_000 {
        let mut number = n + ordinal * 16_000;
        let mut word = [b'a'; 6];
        for byte in &mut word {
            *byte += (number % 26) as u8;
            number /= 26;
        }
        write!(body, " term{}recover", std::str::from_utf8(&word).unwrap()).unwrap();
        if n % 16 == 15 {
            body.push_str("\r\n");
        }
    }
    body.push_str("recoveryneedle\r\n");
    deliver(
        test,
        SENDER,
        key.name(),
        &message_with(&format!("Cap fixture {ordinal}"), &body),
    )
    .await;
    super::new_message(test, id, &before).await
}

async fn case_f6_r23_over_the_cap_then_raised(test: &TestServer, key: &Account) {
    println!("case_f6_r23_over_the_cap_then_raised");
    let id = account_id(key);
    let held = search_config_guard!(test, account_max_mb: 1, segments: 255);
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let mut first_crossing = false;
    let mut next = 0;
    let suspended = loop {
        assert!(next < 15, "large documents did not cross the cap");
        deliver_large_recovery(test, key, next).await;
        next += 1;
        mailbox_get(key).await;
        let manifest = index(test, id).await;
        if manifest.content_to.suspended {
            break manifest;
        }
        if !first_crossing
            && manifest
                .content
                .as_ref()
                .is_some_and(|head| head.size > 1 << 20)
        {
            first_crossing = true;
            assert_search_unavailable(
                &tokio::time::timeout(
                    Duration::from_secs(2),
                    recovery_query(key, Some("recoveryneedle")),
                )
                .await
                .unwrap(),
            );
        }
    };
    assert!(first_crossing);
    let length = suspended.content.as_ref().unwrap().length;
    let links = principal_hashes(test, id).await;
    let thread = key
        .jmap_method_call("Thread/get", json!({"accountId": key.id_string()}))
        .await;
    assert_eq!(thread.name_at(0), "Thread/get", "{:?}", thread.0);
    assert_search_unavailable(
        &tokio::time::timeout(
            Duration::from_secs(2),
            recovery_query(key, Some("recoveryneedle")),
        )
        .await
        .unwrap(),
    );
    let after = index(test, id).await;
    assert!(after.content_to.cursor.is_none());
    assert_eq!(after.content.as_ref().unwrap().length, length);
    assert_eq!(principal_hashes(test, id).await, links);
    assert!(
        !traces
            .finish()
            .await
            .iter()
            .any(|event| event.account_id == Some(u64::from(id))
                && event.details == ZA_SEARCH_REBUILD)
    );

    let mut missing = 1;
    for _ in 0..2 {
        deliver_large_recovery(test, key, next).await;
        next += 1;
        mailbox_get(key).await;
        missing += 1;
        assert_eq!(
            index(test, id).await.content.as_ref().unwrap().length,
            length
        );
    }
    let mapped = get_recovery_ids(key).await;
    let prior_links = principal_hashes(test, id).await;
    drop(held);
    let raised = search_config_guard!(test, segments: 255);
    simulate_restart(test, id).await;
    mailbox_get(key).await;
    let started = index(test, id).await;
    assert!(started.content_to.cursor.is_some());
    assert_eq!(started.content, suspended.content);
    assert_eq!(principal_hashes(test, id).await, prior_links);

    simulate_restart(test, id).await;
    let new_document = append_recovery_messages(test, key, 1).await[0];
    let pre_request_threads = index(test, id).await.thread_blobs.len();
    let delivered = email_boundary(test, id).await;
    reset_za_search_counters(id);
    mailbox_get(key).await;
    assert!(index_holds(test, id, new_document).await);
    let before_backfill = index(test, id).await;
    assert!(before_backfill.indexed_to.change_id >= delivered);
    assert_eq!(za_search_blob_reads(id), pre_request_threads as u64);
    let expected = get_recovery_ids(key).await;
    assert!(mapped.iter().all(|id| expected.contains(id)));

    // A pre-backfill load uses the caller's original deadline while another
    // loader owns the flight.
    let keys = za_unlock(test, id, key.secret()).await;
    let entry = test.server.za_search_entry(id, &keys).await.unwrap();
    let flight = entry.load_flight.lock().await;
    let budget = test.server.inner.cache.za_search.content_used();
    let raw = manifest_bytes(test, id).await.unwrap();
    let start = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        test.server
            .za_search_ensure_current(id, &keys, start + Duration::from_millis(150)),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(Unavailable::NotCurrent)));
    assert!(start.elapsed() >= Duration::from_millis(140));
    assert_eq!(test.server.inner.cache.za_search.content_used(), budget);
    assert_eq!(manifest_bytes(test, id).await.unwrap(), raw);
    drop(flight);

    reset_za_search_counters(id);
    assert_eq!(
        queried(&recovery_query(key, Some("recoveryneedle")).await),
        expected
    );
    assert_eq!(za_search_message_opens(id), missing);
    let backfilled = index(test, id).await;
    assert_eq!(
        backfilled.content.as_ref().unwrap().length,
        before_backfill.content.as_ref().unwrap().length + 1
    );
    assert!(backfilled.content_to.cursor.is_none());
    assert_eq!(get_recovery_ids(key).await, expected);

    drop(raised);
    let held = search_config_guard!(test, account_max_mb: 1, segments: 255);
    deliver_large_recovery(test, key, next).await;
    mailbox_get(key).await;
    assert!(index(test, id).await.content_to.suspended);
    destroy(key, &expected[..1]).await;
    let deleted = email_boundary(test, id).await;
    mailbox_get(key).await;
    let complete = index(test, id).await;
    assert_eq!(complete.indexed_to.change_id, deleted);
    assert_eq!(email_boundary(test, id).await, deleted);
    assert!(complete.indexed_to.complete && complete.content_to.cursor.is_none());
    drop(held);
    let _raised = search_config_guard!(test, segments: 255);
    simulate_restart(test, id).await;
    mailbox_get(key).await;
    assert!(index(test, id).await.content_to.cursor.is_some());
    let expected = get_recovery_ids(key).await;
    assert_eq!(
        queried(&recovery_query(key, Some("recoveryneedle")).await),
        expected
    );
}

async fn finish_reconciliation(test: &TestServer, key: &Account, attempts: usize, restart: bool) {
    let id = account_id(key);
    for turn in 0..attempts {
        mailbox_get(key).await;
        if index(test, id).await.reconciliation.is_none() {
            return;
        }
        if restart && turn % 3 == 2 {
            simulate_restart(test, id).await;
        }
    }
    panic!("reconciliation did not finish for {}", key.name());
}

async fn case_f9_reconciliation_renders_nothing_seen(
    test: &TestServer,
    key: &Account,
    admin: &Account,
) {
    println!("case_f9_reconciliation_renders_nothing_seen");
    let id = account_id(key);
    append_recovery_messages(test, key, 30).await;
    let before = recovery_changes(key, "Email/changes", &State::Initial).await;
    assert_eq!(reply_ids(before.method_response(), "created").len(), 30);
    let state = recovery_state(&before);
    let epoch = index(test, id).await.epoch;
    append_recovery_messages(test, key, 20).await;
    lower_changes_history(admin, Some(5)).await;
    purge_account(test, admin, key).await;
    finish_reconciliation(test, key, 8, false).await;
    assert_eq!(index(test, id).await.epoch, epoch);
    assert_eq!(
        recovery_changes(key, "Email/changes", &state)
            .await
            .error_type_at(0),
        Some("cannotCalculateChanges")
    );
    let reply = recovery_changes(key, "Email/changes", &State::Initial).await;
    let mut created = reply_ids(reply.method_response(), "created");
    created.sort_unstable();
    assert_eq!(created, get_recovery_ids(key).await);
    assert_eq!(created.len(), 50);
    lower_changes_history(admin, None).await;
}

async fn case_pl6_unreadable_gap_and_retained_inserts_finish(
    test: &TestServer,
    key: &Account,
    admin: &Account,
) {
    println!("case_pl6_unreadable_gap_and_retained_inserts_finish");
    let id = account_id(key);
    append_recovery_messages(test, key, 30).await;
    mailbox_get(key).await;
    let base = index(test, id).await;
    let documents = append_recovery_messages(test, key, 20).await;
    let skipped = [documents[2], documents[18]];
    for document in skipped {
        corrupt_blob(test, &stored_hash(test, id, document).await).await;
    }
    lower_changes_history(admin, Some(5)).await;
    purge_account(test, admin, key).await;
    let retained = test
        .server
        .store()
        .za_changes_records(id, SyncCollection::Email, None, usize::MAX)
        .await
        .unwrap();
    let inserts = retained
        .records
        .iter()
        .flat_map(|(_, changes)| changes)
        .filter_map(|change| match change {
            Change::InsertItem(id) => Some(*id as u32),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!inserts.contains(&skipped[0]) && inserts.contains(&skipped[1]));
    assert!(retained.first_retained.unwrap() > base.indexed_to.change_id);

    let _cap = search_config_guard!(test, pass_cap: 4);
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    finish_reconciliation(test, key, 20, true).await;
    let results = queried(&recovery_query(key, Some("recoveryneedle")).await);
    let results = results
        .iter()
        .map(|id| id.parse::<Id>().unwrap().document_id())
        .collect::<std::collections::BTreeSet<_>>();
    let expected = message_ids(test, id)
        .await
        .into_iter()
        .filter(|id| !skipped.contains(id))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(results, expected);
    let mut events = traces
        .finish()
        .await
        .into_iter()
        .filter(|event| {
            event.details == ZA_SEARCH_SKIPPED && event.account_id == Some(u64::from(id))
        })
        .map(|event| {
            event
                .keys
                .iter()
                .find_map(|(key, value)| match (key, value) {
                    (Key::DocumentId, TraceValue::UInt(id)) => Some(*id as u32),
                    _ => None,
                })
                .unwrap()
        })
        .collect::<Vec<_>>();
    events.sort_unstable();
    assert_eq!(events, skipped);
    let next = append_recovery_messages(test, key, 1).await[0];
    mailbox_get(key).await;
    assert!(index_holds(test, id, next).await);
    lower_changes_history(admin, None).await;
}

async fn case_f9_truncation_during_a_pending_rebuild(
    test: &TestServer,
    key: &Account,
    admin: &Account,
) {
    println!("case_f9_truncation_during_a_pending_rebuild");
    let id = account_id(key);
    append_many(test, key, 100).await;
    mailbox_get(key).await;
    force_loss(test, id).await;
    mailbox_get(key).await;
    let pending = index(test, id).await;
    assert!(pending.cursor.is_some());
    let records = log_records_since(
        test,
        id,
        SyncCollection::Email,
        pending.indexed_to.change_id,
    )
    .await;
    assert_eq!(records.len(), 2, "{records:?}");
    assert_eq!(records[0].0, pending.epoch);
    lower_changes_history(admin, Some(1)).await;
    purge_account(test, admin, key).await;
    assert_eq!(
        test.server.za_retained_bound(id).await.unwrap(),
        pending.epoch
    );
    mailbox_get(key).await;
    let after = index(test, id).await;
    assert!(after.cursor.is_none() && after.reconciliation.is_some());
    assert_eq!(after.epoch, pending.epoch);
    finish_reconciliation(test, key, 8, false).await;
    assert_eq!(queried(&recovery_query(key, None).await).len(), 100);
    lower_changes_history(admin, None).await;
}

async fn case_r24_g2_retained_deletion_rendered_once(
    test: &TestServer,
    key: &Account,
    admin: &Account,
) {
    println!("case_r24_g2_retained_deletion_rendered_once");
    let id = account_id(key);
    let doc = append_recovery_messages(test, key, 1).await[0];
    let held = get_recovery_ids(key).await.pop().unwrap();
    let indexed = index(test, id).await.indexed_to.change_id;
    append_recovery_messages(test, key, 12).await;
    // Sample an actual retained JMAP state before the deletion without
    // letting its hook advance the durable index checkpoint.
    let entry = test.server.inner.cache.za_search.entry(id);
    let pass = entry.pass_lock.lock().await;
    let before = recovery_changes(key, "Email/changes", &State::Initial).await;
    assert!(reply_ids(before.method_response(), "created").contains(&held));
    let since = recovery_state(&before);
    drop(pass);
    assert_eq!(index(test, id).await.indexed_to.change_id, indexed);
    pop3_delete(test, key, doc).await;
    lower_changes_history(admin, Some(8)).await;
    purge_account(test, admin, key).await;
    let bound = test.server.za_retained_bound(id).await.unwrap();
    let State::Exact(s) = since else {
        panic!("expected exact state");
    };
    assert!(indexed < bound && bound <= s);
    finish_reconciliation(test, key, 8, false).await;
    let reply = recovery_changes(key, "Email/changes", &since).await;
    assert_eq!(reply.name_at(0), "Email/changes", "{:?}", reply.0);
    assert_eq!(reply_ids(reply.method_response(), "destroyed"), [held]);
    assert_no_changes(
        &recovery_changes(key, "Email/changes", &recovery_state(&reply)).await,
        "Email/changes",
    );
    lower_changes_history(admin, None).await;
}

async fn case_r27_empty_rebuild_keeps_clients(test: &TestServer, key: &Account) {
    println!("case_r27_empty_rebuild_keeps_clients");
    let id = account_id(key);
    append_recovery_messages(test, key, 2).await;
    mailbox_get(key).await;
    clean_messages(test, key).await;
    let email = recovery_state(&recovery_changes(key, "Email/changes", &State::Initial).await);
    let thread = recovery_state(&recovery_changes(key, "Thread/changes", &State::Initial).await);
    force_loss(test, id).await;
    mailbox_get(key).await;
    assert_no_changes(
        &recovery_changes(key, "Email/changes", &email).await,
        "Email/changes",
    );
    assert_no_changes(
        &recovery_changes(key, "Thread/changes", &thread).await,
        "Thread/changes",
    );
}

async fn case_g3_loss_with_all_deleted_refuses_holders(test: &TestServer, key: &Account) {
    println!("case_g3_loss_with_all_deleted_refuses_holders");
    let id = account_id(key);
    append_recovery_messages(test, key, 2).await;
    let ids = get_recovery_ids(key).await;
    assert_eq!(ids.len(), 2);
    let email = recovery_state(&recovery_changes(key, "Email/changes", &State::Initial).await);
    let thread = recovery_state(&recovery_changes(key, "Thread/changes", &State::Initial).await);
    destroy(key, &ids).await;
    force_loss(test, id).await;
    mailbox_get(key).await;
    assert!(index(test, id).await.thread_blobs.is_empty());
    for (method, since) in [("Email/changes", email), ("Thread/changes", thread)] {
        assert_eq!(
            recovery_changes(key, method, &since).await.error_type_at(0),
            Some("cannotCalculateChanges")
        );
        assert_no_changes(
            &recovery_changes(key, method, &State::Initial).await,
            method,
        );
    }
}

async fn case_r2_old_epoch_map_never_served(test: &TestServer, key: &Account) {
    println!("case_r2_old_epoch_map_never_served");
    let id = account_id(key);
    let root = message_with("Rebuild snapshot era", "recoveryneedle");
    deliver(test, SENDER, key.name(), &root).await;
    let root_id = message_ids(test, id).await[0];
    let child = super::reply_to(&root, "Rebuild snapshot era", "recoveryneedle");
    deliver(test, SENDER, key.name(), &child).await;
    let before = get_recovery_ids(key).await;
    assert_eq!(before.len(), 2);
    let survivor = before
        .iter()
        .find(|mail| mail.parse::<Id>().unwrap().document_id() != root_id)
        .unwrap();
    let old_epoch = index(test, id).await.epoch;
    let b = key.clone();
    let parked = HookParked::start(id, set_search_snapshot_pause, async move {
        recovery_get(&b).await
    })
    .await;
    pop3_delete(test, key, root_id).await;
    force_loss(test, id).await;
    let successor = get_recovery_ids(key).await;
    assert_eq!(successor.len(), 1);
    assert_ne!(&successor[0], survivor);
    assert!(index(test, id).await.epoch > old_epoch);
    let result = parked.finish().await;
    if result.error_type_at(0).is_some() {
        assert_eq!(
            result.error_type_at(0),
            Some("serverUnavailable"),
            "{:?}",
            result.0
        );
    } else {
        assert_eq!(result.name_at(0), "Email/get");
        let mut returned = result.method_response()["list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        returned.sort_unstable();
        assert_eq!(returned, successor);
    }
}

#[derive(Clone, Copy, Debug)]
enum StaleLoss {
    Thread,
    Content,
    Missing,
    Older,
    Tag,
    Parse,
}

async fn case_stale_loss_keeps_successor(test: &TestServer, key: &Account, loss: StaleLoss) {
    use store::za_search::{SEARCH_FORMAT_V1, manifest::ManifestError};
    match loss {
        StaleLoss::Thread => println!("case_stale_thread_loss_keeps_successor"),
        StaleLoss::Content => println!("case_stale_content_loss_keeps_successor"),
        StaleLoss::Missing => println!("case_stale_missing_manifest_keeps_successor"),
        _ => println!("case_stale_invalid_manifest_keeps_successor: {loss:?}"),
    }
    let id = account_id(key);
    append_recovery_messages(test, key, 3).await;
    let original_ids = get_recovery_ids(key).await;
    let original = index(test, id).await;
    let mut raw = manifest_bytes(test, id).await.unwrap();
    match loss {
        StaleLoss::Thread => force_loss(test, id).await,
        StaleLoss::Content => {
            let head = original.content.as_ref().unwrap();
            test.server
                .blob_store()
                .delete_blobs(&[head.hash.as_slice()])
                .await
                .unwrap();
            simulate_restart(test, id).await;
        }
        StaleLoss::Missing => {
            write_manifest_bytes(test, id, None).await;
            simulate_restart(test, id).await;
            assert!(manifest_bytes(test, id).await.is_none());
        }
        StaleLoss::Older | StaleLoss::Tag | StaleLoss::Parse => {
            let expected = match loss {
                StaleLoss::Older => {
                    raw[0] = SEARCH_FORMAT_V1 - 1;
                    ManifestError::OlderFormat(raw[0])
                }
                StaleLoss::Tag => {
                    *raw.last_mut().unwrap() ^= 1;
                    ManifestError::Tag
                }
                StaleLoss::Parse => {
                    raw.truncate(1);
                    ManifestError::Parse
                }
                _ => unreachable!(),
            };
            write_manifest_bytes(test, id, Some(raw.clone())).await;
            let keys = za_unlock(test, id, key.secret()).await;
            assert_eq!(
                manifest_bytes(test, id).await.as_deref(),
                Some(raw.as_slice())
            );
            assert_eq!(
                ManifestValue::decode(&raw, &za_search_key(keys.swk())),
                Err(expected)
            );
            drop(keys);
            simulate_restart(test, id).await;
        }
    }
    let content = matches!(loss, StaleLoss::Content);
    let b = key.clone();
    let start = Instant::now();
    let parked = HookParked::start(id, set_search_recovery_pause, async move {
        if content {
            recovery_query(&b, Some("recoveryneedle")).await
        } else {
            recovery_get(&b).await
        }
    })
    .await;
    // This is a second wire request. The one-shot slot was already consumed
    // by B, before A is admitted into the production recovery path.
    let successor_ids = queried(&recovery_query(key, Some("recoveryneedle")).await);
    let successor = index(test, id).await;
    let successor_raw = manifest_bytes(test, id).await.unwrap();
    let successor_links = principal_blob_links(test, id).await;
    let email = log_records_since(test, id, SyncCollection::Email, 0).await;
    let thread = log_records_since(test, id, SyncCollection::Thread, 0).await;
    assert_eq!(successor_ids.len(), 3);
    assert!(successor.cursor.is_none() && successor.content_to.cursor.is_none());
    assert_eq!(successor.content_to.checkpoint, successor.indexed_to);
    assert_manifest_links(test, id, &successor).await;
    if content {
        assert_eq!(successor_ids, original_ids);
        assert_eq!(successor.epoch, original.epoch);
        assert_eq!(successor.newest_row, original.newest_row);
        assert_eq!(successor.thread_blobs, original.thread_blobs);
        assert_ne!(successor.content, original.content);
    } else {
        assert!(successor.epoch > original.epoch);
        assert_ne!(successor.thread_blobs, original.thread_blobs);
    }

    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let reply = tokio::time::timeout(Duration::from_secs(5), parked.finish())
        .await
        .unwrap();
    if content {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "B exceeded its original content wait"
        );
        if reply.error_type_at(0).is_none() {
            assert_eq!(queried(&reply), successor_ids);
        } else {
            assert_search_unavailable(&reply);
        }
    } else if reply.error_type_at(0).is_some() {
        assert_eq!(
            reply.error_type_at(0),
            Some("serverUnavailable"),
            "{:?}",
            reply.0
        );
    } else {
        assert_eq!(reply.name_at(0), "Email/get");
    }
    assert!(
        manifest_bytes(test, id).await.unwrap() == successor_raw,
        "B changed the exact successor manifest for {loss:?}"
    );
    let events = traces.finish().await;
    assert!(events.iter().any(|event| event.account_id == Some(u64::from(id))
        && event.details == ZA_SEARCH_LOST), "B did not reach the moved-base decision");
    assert!(
        !events
            .iter()
            .any(|event| event.account_id == Some(u64::from(id))
                && event.details == ZA_SEARCH_REBUILD)
    );
    assert_eq!(index(test, id).await, successor);
    assert_eq!(principal_blob_links(test, id).await, successor_links);
    assert_eq!(
        log_records_since(test, id, SyncCollection::Email, 0).await,
        email
    );
    assert_eq!(
        log_records_since(test, id, SyncCollection::Thread, 0).await,
        thread
    );
    assert_eq!(get_recovery_ids(key).await, successor_ids);
    assert_eq!(
        queried(&recovery_query(key, Some("recoveryneedle")).await),
        successor_ids
    );
}

async fn test_compaction(test: &mut TestServer, admin: &Account) {
    let bounded = key_mail_account(test, "keymail61@example.com", "Keymail-61!").await;
    case_r8_thread_compaction_bounds_the_manifest(test, admin, &bounded).await;
    test.destroy_all_mailboxes(&bounded).await;

    let retained = key_mail_account(
        test,
        "keymail-compact-shorter-thread@example.com",
        "Compaction-retained!",
    )
    .await;
    case_compaction_uses_shorter_thread_log(test, &retained).await;
    test.destroy_all_mailboxes(&retained).await;

    let allowance = key_mail_account(
        test,
        "keymail-compact-allowance@example.com",
        "Compaction-allowance!",
    )
    .await;
    case_thread_allowance_fallback_checks_the_final_inventory(test, &allowance).await;
    test.destroy_all_mailboxes(&allowance).await;

    let saturated =
        key_mail_account(test, "keymail-compact-full@example.com", "Compaction-full!").await;
    case_full_merge_saturates_repeatedly(test, &saturated).await;
    test.destroy_all_mailboxes(&saturated).await;

    for (name, arm) in [
        ("keymail-compact-map@example.com", PendingArm::Map),
        ("keymail-compact-content@example.com", PendingArm::Content),
        ("keymail-compact-marker@example.com", PendingArm::Marker),
    ] {
        let key = key_mail_account(test, name, "Compaction-pending!").await;
        case_pending_merge_leaves_tier_255_root(test, &key, arm).await;
        test.destroy_all_mailboxes(&key).await;
    }

    let overflow = key_mail_account(
        test,
        "keymail-compact-overflow@example.com",
        "Compaction-overflow!",
    )
    .await;
    case_tier_overflow_refuses_the_pass(test, &overflow).await;
    test.destroy_all_mailboxes(&overflow).await;

    let rebuild = key_mail_account(
        test,
        "keymail-compact-rebuild@example.com",
        "Compaction-rebuild!",
    )
    .await;
    case_g17_h7_r32_tiered_merges_during_a_rebuild(test, &rebuild).await;
    test.destroy_all_mailboxes(&rebuild).await;

    let budget = key_mail_account(
        test,
        "keymail-compact-budget@example.com",
        "Compaction-budget!",
    )
    .await;
    case_warm_content_compaction_releases_its_own_budget(test, &budget).await;
    test.destroy_all_mailboxes(&budget).await;

    for (name, compressed) in [
        ("keymail-compact-small-frames@example.com", false),
        ("keymail-compact-compressed-frames@example.com", true),
    ] {
        let key = key_mail_account(test, name, "Compaction-bounds!").await;
        case_content_frame_budget(test, &key, compressed).await;
        test.destroy_all_mailboxes(&key).await;
    }

    test_warm_content_admission(test).await;

    let loss = key_mail_account(test, "keymail-compact-loss@example.com", "Compaction-loss!").await;
    case_compaction_hash_loss_keeps_the_map(test, &loss).await;
    test.destroy_all_mailboxes(&loss).await;
    test.wait_for_tasks().await;
}

async fn case_r8_thread_compaction_bounds_the_manifest(
    test: &TestServer,
    admin: &Account,
    key: &Account,
) {
    let _config = search_config_guard!(test, segments: 4);
    let id = account_id(key);
    let mut previous_links = Vec::new();
    let mut retired = Vec::new();
    let mut previous_revision = index(test, id).await.revision;
    for pass_no in 1..=12 {
        deliver_one(test, key, &format!("Bounded pass {pass_no}")).await;
        mailbox_get(key).await;
        let manifest = index(test, id).await;
        assert_eq!(manifest.revision, previous_revision + 1);
        previous_revision = manifest.revision;
        let count = if pass_no <= 3 {
            pass_no
        } else {
            1 + (pass_no - 4) % 3
        };
        assert_eq!(manifest.thread_blobs.len(), count as usize);
        let head = manifest.content.as_ref().unwrap();
        assert_eq!(head.length, count);
        if [4, 7, 10].contains(&pass_no) {
            let header = content_header(test, &head.hash).await;
            assert_eq!(header.tier, ((pass_no - 1) / 3) as u8);
            assert!(header.predecessor.is_none());
        }
        assert!(loaded(test, id).await.content.is_none(), "thread-only node");
        assert_manifest_links(test, id, &manifest).await;
        let links = principal_hashes(test, id).await;
        retired.extend(
            previous_links
                .into_iter()
                .filter(|hash| !links.contains(hash)),
        );
        previous_links = links;
    }
    assert!(!retired.is_empty());
    expire_and_purge_blobs(test, admin).await;
    for hash in retired {
        assert!(
            test.server
                .get_blob(hash.as_slice(), 0..1)
                .await
                .unwrap()
                .is_none()
        );
    }
    assert_manifest_links(test, id, &index(test, id).await).await;
}

async fn case_compaction_uses_shorter_thread_log(test: &TestServer, key: &Account) {
    println!("case_compaction_uses_shorter_thread_log");
    let _setup = search_config_guard!(test, segments: 255, checkpoint: 1);
    let id = account_id(key);

    let older = deliver_one(test, key, "Older retention control").await;
    advanced(test, key, true).await;
    set_flag(key, older, true).await;
    let older_checkpoint = advanced(test, key, false).await;
    let email_bound = test
        .server
        .truncate_change_log(id, SyncCollection::Email.into(), 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(email_bound, older_checkpoint.change_id);

    let expired = deliver_one(test, key, "Deleted singleton inside the retention gap").await;
    advanced(test, key, true).await;
    let assignment = index(test, id).await.newest_row;
    pop3_delete(test, key, expired).await;
    let deletion = change_id(test, id).await;
    advanced(test, key, false).await;

    let newer = deliver_one(test, key, "Newer thread retention control").await;
    advanced(test, key, true).await;
    let thread_bound = test
        .server
        .truncate_change_log(id, SyncCollection::Thread.into(), 0)
        .await
        .unwrap()
        .unwrap();
    let store = test.server.store();
    assert_eq!(
        store
            .za_log_first_retained(id, SyncCollection::Email)
            .await
            .unwrap(),
        Some(email_bound)
    );
    assert_eq!(
        store
            .za_log_first_retained(id, SyncCollection::Thread)
            .await
            .unwrap(),
        Some(thread_bound)
    );
    assert!(thread_bound > email_bound);
    assert!(
        email_bound <= assignment.change_id
            && assignment.change_id < deletion
            && deletion < thread_bound,
        "expected Email bound <= assignment < deletion < Thread bound, got {email_bound}, {}, {deletion}, {thread_bound}",
        assignment.change_id
    );

    // Put the durable checkpoint beyond both markers without a thread delta.
    set_flag(key, newer, true).await;
    advanced(test, key, false).await;
    let before = index(test, id).await;
    assert!(before.indexed_to.complete && before.indexed_to.change_id > thread_bound);
    assert!(before.reconciliation.is_none());
    assert!(before.thread_blobs.len() > 1);
    {
        let view = loaded(test, id).await;
        let record = view
            .thread
            .record(expired)
            .expect("the deleted singleton's retained record");
        assert_eq!(record.thread_id, expired);
        assert_eq!(record.revision, assignment.revision);
        assert_eq!(
            view.thread.revision_change_id(record.revision),
            Some(assignment.change_id)
        );
        assert_eq!(
            view.thread.members_of(expired, assignment.change_id),
            [expired]
        );
        assert_eq!(record.deleted, Some(deletion));
    }

    let _compact = search_config_guard!(test, segments: before.thread_blobs.len() + 1);
    let newest = deliver_one(test, key, "Real delta triggers retention compaction").await;
    advanced(test, key, true).await;
    let after = index(test, id).await;
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(after.thread_blobs.len(), 1);
    let compacted = &after.thread_blobs[0];
    assert!(
        !before
            .thread_blobs
            .iter()
            .any(|blob| blob.hash == compacted.hash)
    );
    assert_manifest_links(test, id, &after).await;

    simulate_restart(test, id).await;
    assert!(test.server.inner.cache.za_search.get(id).is_none());
    let part = za_search_thread_part(test, id, key.secret()).await;
    assert!(
        part.record(expired).is_none(),
        "compaction retained the expired singleton"
    );
    for live in [older, newer, newest] {
        assert!(part.is_present(live));
    }
    let bytes = stored_blob(test, &compacted.hash).await;
    let keys = za_unlock(test, id, key.secret()).await;
    let header = BlobHeader {
        kind: BlobKind::Thread,
        tier: 0,
        segment: compacted.segment,
        predecessor: None,
    };
    let (_, public) = parse_thread_blob(keys.swk(), id, &header, &bytes).unwrap();
    assert!(
        public
            .tombstones
            .iter()
            .all(|tombstone| tombstone.document_id != expired)
    );
}

async fn content_header(test: &TestServer, hash: &BlobHash) -> BlobHeader {
    let bytes = test
        .server
        .get_blob(hash.as_slice(), 0..BlobHeader::LEN)
        .await
        .unwrap()
        .unwrap();
    BlobHeader::decode(&bytes).unwrap().0
}

async fn seed_content_tiers(
    test: &TestServer,
    key: &Account,
    manifest: Manifest,
    tiers: &[u8],
) -> (Manifest, Vec<BlobHash>) {
    seed_content_tiers_with_frame(test, key, manifest, tiers, None).await
}

async fn seed_content_tiers_with_frame(
    test: &TestServer,
    key: &Account,
    mut manifest: Manifest,
    tiers: &[u8],
    extra_frame: Option<&[u8]>,
) -> (Manifest, Vec<BlobHash>) {
    use common::za::search::seal::{build_content_blob, parse_content_blob};
    use store::za_search::{BlobKind, SegmentId, manifest::ContentHead};
    use types::field::PrincipalField;
    use vault::Zeroizing;

    let id = account_id(key);
    let keys = za_unlock(test, id, key.secret()).await;
    let old = manifest.content.as_ref().unwrap();
    assert_eq!(old.length, 1, "the seeded root comes from a real pass");
    let bytes = stored_blob(test, &old.hash).await;
    let (_, records) = parse_content_blob(keys.swk(), id, Some(old.segment), None, &bytes).unwrap();
    assert_eq!(records.len(), 1);
    let frames = records
        .iter()
        .map(|record| Zeroizing::new(record.encode()))
        .collect::<Vec<_>>();
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .clear(BlobOp::Link {
            hash: old.hash.clone(),
            to: BlobLink::Document,
        });
    let mut predecessor = None;
    let mut size = 0u64;
    let mut hashes = Vec::new();
    for (pos, &tier) in tiers.iter().rev().enumerate() {
        let header = BlobHeader {
            kind: BlobKind::Content,
            tier,
            segment: SegmentId::random(),
            predecessor,
        };
        let slices = if pos == 0 {
            frames
                .iter()
                .map(|frame| frame.as_slice())
                .chain(extra_frame)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let bytes = build_content_blob(keys.swk(), id, &header, &slices);
        let (hash, hold) = test
            .server
            .hold_blob(id, BlobHash::generate(&bytes), &bytes, 60, &BlobOpts::PLAIN)
            .await
            .unwrap();
        hold.release(&mut batch).set(
            BlobOp::Link {
                hash: hash.clone(),
                to: BlobLink::Document,
            },
            BlobLinkValue::new(bytes.len()),
        );
        size += bytes.len() as u64;
        hashes.push(hash.clone());
        predecessor = Some((hash, header.segment));
    }
    let (hash, segment) = predecessor.unwrap();
    manifest.content = Some(ContentHead {
        hash,
        segment,
        size,
        length: tiers.len() as u32,
    });
    batch.set(
        PrincipalField::ZeroAccessSearch,
        ManifestValue::Index(manifest.clone()).encode(&za_search_key(keys.swk())),
    );
    test.server.store().write_batch(&mut batch).await.unwrap();
    simulate_restart(test, id).await;
    assert_manifest_links(test, id, &manifest).await;
    hashes.reverse();
    (manifest, hashes)
}

async fn content_frames(
    test: &TestServer,
    key: &Account,
) -> std::collections::BTreeMap<u32, Vec<u8>> {
    let id = account_id(key);
    let keys = za_unlock(test, id, key.secret()).await;
    let entry = test.server.za_search_entry(id, &keys).await.unwrap();
    test.server
        .za_search_load_content(&entry, &keys)
        .await
        .unwrap();
    let view = entry.read_loaded().await.unwrap();
    let content = view.content.as_ref().unwrap();
    content
        .part
        .document_ids()
        .iter()
        .map(|id| (id, content.part.record_bytes(id).unwrap().to_vec()))
        .collect()
}

async fn case_full_merge_saturates_repeatedly(test: &TestServer, key: &Account) {
    let _config = search_config_guard!(test, segments: 2);
    let id = account_id(key);
    let first = deliver_one(test, key, "Saturating root terms").await;
    advanced(test, key, true).await;
    let (mut previous, _) = seed_content_tiers(test, key, index(test, id).await, &[254]).await;
    let mut frames = content_frames(test, key).await;
    assert!(frames.contains_key(&first));
    for turn in 0..3 {
        let document = deliver_one(test, key, &format!("Saturation terms {turn}")).await;
        let progress = advanced(test, key, true).await;
        let after = index(test, id).await;
        let head = after.content.as_ref().unwrap();
        let old = previous.content.as_ref().unwrap();
        assert!(progress.change_id > previous.indexed_to.change_id);
        assert_eq!(head.length, 1);
        assert_ne!(head.hash, old.hash);
        assert_ne!(head.segment, old.segment);
        let header = content_header(test, &head.hash).await;
        assert_eq!(header.tier, 255);
        assert!(header.predecessor.is_none());
        assert_manifest_links(test, id, &after).await;
        assert!(!principal_hashes(test, id).await.contains(&old.hash));
        {
            let view = loaded(test, id).await;
            let resident = view
                .content
                .as_ref()
                .expect("compaction kept its warm part");
            assert_eq!(resident.segments, [head.segment]);
            assert_eq!(resident.bytes, resident.part.loaded_bytes() as u64);
        }
        let warm = content_frames(test, key).await;
        assert_eq!(warm.len(), frames.len() + 1);
        assert!(warm.contains_key(&document));
        for (id, frame) in &frames {
            assert_eq!(warm.get(id), Some(frame));
        }
        simulate_restart(test, id).await;
        assert_eq!(content_frames(test, key).await, warm);
        frames = warm;
        previous = after;
    }
}

#[derive(Debug, Clone, Copy)]
enum PendingArm {
    Map,
    Content,
    Marker,
}

async fn case_pending_merge_leaves_tier_255_root(
    test: &TestServer,
    key: &Account,
    arm: PendingArm,
) {
    use store::za_search::manifest::{ContentCursor, RebuildCursor, ReconciliationMarker};

    let _config = search_config_guard!(test, segments: 2, pass_cap: 1);
    let id = account_id(key);
    deliver_one(test, key, "Older root").await;
    advanced(test, key, true).await;
    let mut manifest = index(test, id).await;
    let ids = append_recovery_messages(test, key, 2).await;
    match arm {
        PendingArm::Map => {
            manifest.cursor = Some(RebuildCursor {
                next: ids[0],
                end: ids[1],
            })
        }
        PendingArm::Content => {
            manifest.content_to.cursor = Some(ContentCursor {
                next_below: u32::MAX,
            })
        }
        PendingArm::Marker => {
            manifest.reconciliation = Some(ReconciliationMarker {
                bound: 1,
                gap_next: 0,
            })
        }
    }
    let (_, hashes) = seed_content_tiers(test, key, manifest, &[0, 255]).await;

    advanced(test, key, true).await;
    let after = index(test, id).await;
    match arm {
        PendingArm::Map => assert!(after.cursor.is_some()),
        PendingArm::Content => assert!(after.content_to.cursor.is_some()),
        PendingArm::Marker => assert!(after.reconciliation.is_some()),
    }
    let chain = content_hashes(test, &after).await;
    assert_eq!(chain.len(), 2, "{arm:?}");
    assert_eq!(chain[1], hashes[1]);
    assert_eq!(content_header(test, &chain[0]).await.tier, 1);
    assert_eq!(content_header(test, &chain[1]).await.tier, 255);
    assert!(!principal_hashes(test, id).await.contains(&hashes[0]));
    assert_manifest_links(test, id, &after).await;
}

struct ObserveContentWrites;

impl ObserveContentWrites {
    fn start(id: u32) -> Self {
        common::za::test_hooks::set_search_content_writes(Some(id));
        Self
    }
}

impl Drop for ObserveContentWrites {
    fn drop(&mut self) {
        common::za::test_hooks::set_search_content_writes(None);
    }
}

async fn case_tier_overflow_refuses_the_pass(test: &TestServer, key: &Account) {
    use common::za::{
        search::trace::ZA_SEARCH_ABORTED,
        test_hooks::{SearchContentWrite, take_search_content_writes},
    };
    use store::za_search::manifest::RebuildCursor;

    let _config = search_config_guard!(test, segments: 2, pass_cap: 1);
    let id = account_id(key);
    let original = deliver_one(test, key, "Overflow original terms").await;
    advanced(test, key, true).await;
    let mut manifest = index(test, id).await;
    let ids = append_recovery_messages(test, key, 2).await;
    manifest.cursor = Some(RebuildCursor {
        next: ids[0],
        end: ids[1],
    });
    manifest.failing = Some(FailingDocument {
        document_id: ids[0],
        attempts: 1,
    });
    let tiers = (0..=255).collect::<Vec<_>>();
    let (before, hashes) = seed_content_tiers(test, key, manifest, &tiers).await;
    let warm = content_frames(test, key).await;
    let raw = manifest_bytes(test, id).await;
    let links = principal_blob_links(test, id).await;
    let holds = temporary_blob_hashes(test, id).await;
    let email = log_records_since(test, id, SyncCollection::Email, 0).await;
    let thread = log_records_since(test, id, SyncCollection::Thread, 0).await;
    let _observer = ObserveContentWrites::start(id);
    use common::za::search::cache::PartKind;
    let cache = &test.server.inner.cache.za_search;
    let resident_bytes = loaded(test, id).await.content.as_ref().unwrap().bytes;
    // Room for the staged record and one input, but not for the entire
    // descriptor plan. Preflight must neither evict its base nor encode.
    {
        let _budget = cache.override_content_budget_for_test(resident_bytes + 4096);
        assert_eq!(cache.content_budget(), resident_bytes + 4096);
        drop(
            cache
                .admit(PartKind::Content, 4096, id)
                .expect("room for one input"),
        );
        assert_eq!(cache.content_used(), resident_bytes);
        assert_eq!(
            pass(test, key).await,
            PassOutcome::Aborted(TransientClass::Budget)
        );
        assert_eq!(
            cache.content_used(),
            resident_bytes,
            "preflight refusal released its charge"
        );
        assert_eq!(manifest_bytes(test, id).await, raw);
        assert_eq!(principal_blob_links(test, id).await, links);
        assert_eq!(temporary_blob_hashes(test, id).await, holds);
        assert!(take_search_content_writes(id).is_empty());
        assert_eq!(content_frames(test, key).await, warm);
    }
    let used = cache.content_used();
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    for _ in 0..2 {
        assert_eq!(
            pass(test, key).await,
            PassOutcome::Aborted(TransientClass::Validation)
        );
        assert!(
            take_search_content_writes(id).is_empty(),
            "overflow encoded or uploaded output"
        );
        assert_eq!(
            cache.content_used(),
            used,
            "tier overflow released all preflight charges"
        );
        assert_eq!(manifest_bytes(test, id).await, raw);
        assert_eq!(principal_blob_links(test, id).await, links);
        assert_eq!(temporary_blob_hashes(test, id).await, holds);
        let view = loaded(test, id).await;
        assert_eq!(view.manifest, before);
        assert!(!view.thread.is_present(ids[0]));
        assert_eq!(view.content.as_ref().unwrap().segments.len(), 256);
        drop(view);
        assert_eq!(content_frames(test, key).await, warm);
    }
    assert_eq!(
        log_records_since(test, id, SyncCollection::Email, 0).await,
        email
    );
    assert_eq!(
        log_records_since(test, id, SyncCollection::Thread, 0).await,
        thread
    );
    let events = traces.finish().await;
    let own = events
        .iter()
        .filter(|event| event.account_id == Some(u64::from(id)))
        .collect::<Vec<_>>();
    assert_eq!(
        own.iter()
            .filter(|event| event.details == ZA_SEARCH_ABORTED && event.reason == "tier-overflow")
            .count(),
        2
    );
    assert!(
        !own.iter()
            .any(|event| event.details == ZA_SEARCH_PASS || event.details == ZA_SEARCH_REBUILD)
    );

    // The exact same inventory and first staged document become a full
    // merge when staging consumes the final eligible rebuild document.
    let mut companion = before;
    companion.cursor.as_mut().unwrap().end = ids[0];
    let keys = za_unlock(test, id, key.secret()).await;
    write_manifest_bytes(
        test,
        id,
        Some(ManifestValue::Index(companion).encode(&za_search_key(keys.swk()))),
    )
    .await;
    simulate_restart(test, id).await;
    let raw = manifest_bytes(test, id).await;
    let links_before_commit = principal_blob_links(test, id).await;
    let used_before_commit = cache.content_used();
    for expire in [true, false] {
        let _expiry = expire
            .then(|| ScopedSearchAccountHook::set(&common::za::test_hooks::SEARCH_HOLD_EXPIRE, id));
        let parked = park(test, key, set_search_commit_pause).await;
        let writes = take_search_content_writes(id);
        let stage = writes
            .iter()
            .find_map(|event| match event {
                SearchContentWrite::StagedMemory(bytes) => Some(*bytes),
                _ => None,
            })
            .unwrap();
        let retirement = writes
            .iter()
            .find_map(|event| match event {
                SearchContentWrite::RetirementMemory(bytes) => Some(*bytes),
                _ => None,
            })
            .unwrap();
        assert!(retirement >= (256 * size_of::<BlobHash>()) as u64);
        assert_eq!(
            cache.content_used(),
            used_before_commit + stage + retirement
        );
        {
            let _budget = cache.override_content_budget_for_test(stage + retirement);
            assert!(
                cache.admit(PartKind::Content, retirement, id + 1).is_none(),
                "unrelated work cannot spend retirement memory still owned by the batch",
            );
        }
        let outcome = parked.finish().await;
        assert_eq!(cache.content_used(), used_before_commit);
        if expire {
            assert_eq!(outcome, PassOutcome::Aborted(TransientClass::BlobStore));
            assert_eq!(manifest_bytes(test, id).await, raw);
            assert_eq!(principal_blob_links(test, id).await, links_before_commit);
        } else {
            assert!(matches!(
                outcome,
                PassOutcome::Advanced {
                    published: true,
                    ..
                }
            ));
        }
        println!("Many-node commit retained {retirement} retirement bytes; expiry={expire}");
    }
    let after = index(test, id).await;
    assert!(after.cursor.is_none());
    assert!(after.failing.is_none());
    let head = after.content.as_ref().unwrap();
    assert_eq!(head.length, 1);
    let header = content_header(test, &head.hash).await;
    assert_eq!(header.tier, 255);
    assert!(header.predecessor.is_none());
    assert_manifest_links(test, id, &after).await;
    let links = principal_hashes(test, id).await;
    assert!(hashes.iter().all(|hash| !links.contains(hash)));
    let frames = content_frames(test, key).await;
    assert_eq!(frames.get(&original), warm.get(&original));
    assert!(frames.contains_key(&ids[0]));
    let writes = take_search_content_writes(id);
    assert!(
        writes
            .iter()
            .any(|event| matches!(event, SearchContentWrite::Linked { tier: 255, .. }))
    );
}

async fn case_g17_h7_r32_tiered_merges_during_a_rebuild(test: &TestServer, key: &Account) {
    use common::za::test_hooks::{SearchContentWrite, take_search_content_writes};
    let _config = search_config_guard!(test, segments: 4, pass_cap: 8, absent_retry_secs: 0);
    let id = account_id(key);
    append_many(test, key, 200).await;
    mailbox_get(key).await;
    force_loss(test, id).await;
    let _observer = ObserveContentWrites::start(id);
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let mut original = 0;
    let mut rewritten = 0;
    let mut measurements = Vec::new();
    for pass_no in 1..=25 {
        mailbox_get(key).await;
        let manifest = index(test, id).await;
        let head = manifest.content.as_ref().unwrap();
        assert!(head.length <= 10, "pass {pass_no}: {}", head.length);
        let writes = take_search_content_writes(id);
        let input = writes
            .iter()
            .find_map(|event| match event {
                SearchContentWrite::Original(bytes) => Some(*bytes),
                _ => None,
            })
            .unwrap();
        let outputs = writes
            .iter()
            .filter_map(|event| match event {
                SearchContentWrite::Encoded { tier, bytes } => Some((*tier, *bytes)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let (tier, bytes) = *outputs.last().unwrap();
        let work = if tier == 0 {
            0
        } else {
            outputs.iter().map(|(_, bytes)| bytes).sum::<u64>()
        };
        measurements.push((bytes, work));
        assert!(writes.contains(&SearchContentWrite::Uploaded { tier, bytes }));
        assert!(writes.contains(&SearchContentWrite::Linked { tier, bytes }));
        assert_eq!(outputs.len(), if pass_no == 16 { 2 } else { 1 });
        if manifest.cursor.is_some() {
            original += input;
            rewritten += work;
        } else {
            assert_eq!(pass_no, 25);
            assert_eq!(head.length, 1);
        }
    }
    assert!(rewritten > 0);
    assert!(rewritten <= 3 * original, "{rewritten} > 3 × {original}");
    let events = traces.finish().await;
    let passes = events
        .iter()
        .filter(|event| event.details == ZA_SEARCH_PASS && event.account_id == Some(u64::from(id)))
        .collect::<Vec<_>>();
    assert_eq!(passes.len(), 25);
    for (event, (written, merged)) in passes.into_iter().zip(measurements) {
        assert!(event.keys.contains(&(Key::Size, TraceValue::UInt(written))));
        assert!(event.keys.contains(&(Key::Used, TraceValue::UInt(merged))));
    }
    println!("Tiered rebuild encoded {rewritten} bytes over {original} original input bytes");
    assert_manifest_links(test, id, &index(test, id).await).await;
}

async fn case_compaction_hash_loss_keeps_the_map(test: &TestServer, key: &Account) {
    use common::za::search::error::LossClass;
    let _config = search_config_guard!(test, segments: 2);
    let id = account_id(key);
    let first = deliver_one(test, key, "Compaction old hash").await;
    advanced(test, key, true).await;
    let before = index(test, id).await;
    let raw = manifest_bytes(test, id).await.unwrap();
    let links = principal_blob_links(test, id).await;
    let hash = &before.content.as_ref().unwrap().hash;
    corrupt_blob(test, hash).await;
    deliver_one(test, key, "Compaction discovers hash loss").await;
    match pass(test, key).await {
        PassOutcome::ContentLoss(loss) => {
            assert_eq!(loss.class(), LossClass::Hash);
            assert!(loss.matches(&raw, &before));
        }
        other => panic!("expected observed content hash loss, got {other:?}"),
    }
    assert_eq!(manifest_bytes(test, id).await.unwrap(), raw);
    assert_eq!(principal_blob_links(test, id).await, links);
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    mailbox_get(key).await;
    let events = traces.finish().await;
    assert!(
        events
            .iter()
            .any(|event| rebuild_trace(event, id, "content") && event.reason == "hash")
    );
    assert!(
        !events
            .iter()
            .any(|event| rebuild_trace(event, id, "rebuild"))
    );
    let after = index(test, id).await;
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(after.thread_blobs, before.thread_blobs);
    assert_eq!(after.newest_row, before.newest_row);
    assert!(loaded(test, id).await.thread.is_present(first));
    assert!(!principal_hashes(test, id).await.contains(hash));
    assert_manifest_links(test, id, &after).await;
}

async fn case_warm_content_compaction_releases_its_own_budget(test: &TestServer, key: &Account) {
    use common::za::{
        search::cache::PartKind,
        test_hooks::{SEARCH_HOLD_EXPIRE, SearchContentWrite, take_search_content_writes},
    };

    let _small_cap = search_config_guard!(test, account_max_mb: 1, segments: 255);
    let id = account_id(key);
    let mut previous = None;
    for _ in 0..14 {
        // Keep the terms shared so postings fit in the warm budget while
        // the repeated frames bring the sealed chain close to its cap.
        deliver_large_recovery(test, key, 0).await;
        advanced(test, key, true).await;
        let manifest = index(test, id).await;
        let head = manifest.content.as_ref().unwrap();
        assert!(
            head.size < 1 << 20,
            "warm chain crossed its account cap: {}",
            head.size
        );
        if head.size >= 750_000 {
            previous = Some(manifest);
            break;
        }
    }
    let previous = previous.expect("fixture approached the account cap");
    let head = previous.content.as_ref().unwrap();
    let cache = &test.server.inner.cache.za_search;
    let boot_budget = cache.content_budget();
    let process_budget = 3 << 20;
    let small_budget = cache.override_content_budget_for_test(process_budget);
    assert_eq!(cache.content_budget(), process_budget);
    let frames = content_frames(test, key).await;
    let warm_bytes = loaded(test, id).await.content.as_ref().unwrap().bytes;
    println!(
        "Warm budget fixture sealed={} resident={} process={process_budget}",
        head.size, warm_bytes
    );
    assert!(head.size * 3 + warm_bytes > process_budget);
    assert!(head.size * 3 < process_budget, "cold reservation must fit");
    assert!(
        cache.admit(PartKind::Content, head.size * 3, id).is_none(),
        "the original whole-head reservation must fail under real warm pressure",
    );
    assert_eq!(
        loaded(test, id).await.content.as_ref().unwrap().bytes,
        warm_bytes
    );

    let _compact = search_config_guard!(test, segments: 2);
    let added = deliver_one(test, key, "Compaction under the warm budget").await;
    let raw = manifest_bytes(test, id).await;
    let links = principal_blob_links(test, id).await;
    let _observer = ObserveContentWrites::start(id);

    for expire in [true, false] {
        let _expiry = expire.then(|| ScopedSearchAccountHook::set(&SEARCH_HOLD_EXPIRE, id));
        let parked = park(test, key, set_search_commit_pause).await;
        assert!(
            loaded(test, id).await.content.is_none(),
            "the pass releases only its warm content"
        );
        assert_eq!(manifest_bytes(test, id).await, raw);
        assert_eq!(principal_blob_links(test, id).await, links);

        let writes = take_search_content_writes(id);
        let original = writes
            .iter()
            .find_map(|event| match event {
                SearchContentWrite::Original(bytes) => Some(*bytes),
                _ => None,
            })
            .unwrap();
        assert!(
            writes
                .iter()
                .any(|event| matches!(event, SearchContentWrite::Uploaded { .. }))
        );
        let staged_bytes = writes
            .iter()
            .find_map(|event| match event {
                SearchContentWrite::StagedMemory(bytes) => Some(*bytes),
                _ => None,
            })
            .expect("estimated staged-frame charge");
        assert!(
            staged_bytes > original,
            "charge includes frame-container metadata"
        );
        let retirement = writes
            .iter()
            .find_map(|event| match event {
                SearchContentWrite::RetirementMemory(bytes) => Some(*bytes),
                _ => None,
            })
            .unwrap();
        let charged = cache.content_used();
        assert!(
            charged >= staged_bytes,
            "staged frames remain charged after upload"
        );
        let outcome = parked.finish().await;
        let after = cache.content_used();
        assert_eq!(charged.checked_sub(after), Some(staged_bytes + retirement));
        println!(
            "Warm budget {} retained {staged_bytes} staged bytes through commit ({charged} to {after})",
            if expire { "abort" } else { "success" }
        );
        if expire {
            assert_eq!(outcome, PassOutcome::Aborted(TransientClass::BlobStore));
            assert_eq!(manifest_bytes(test, id).await, raw);
            assert_eq!(principal_blob_links(test, id).await, links);
            assert_eq!(content_frames(test, key).await, frames);
        } else {
            assert!(
                matches!(outcome, PassOutcome::Advanced { published: true, indexed_to }
                if indexed_to.change_id > previous.indexed_to.change_id)
            );
        }
    }

    let merged = index(test, id).await;
    assert_eq!(merged.content.as_ref().unwrap().length, 1);
    assert!(loaded(test, id).await.content.is_none());
    assert_manifest_links(test, id, &merged).await;
    let reloaded = content_frames(test, key).await;
    assert_eq!(reloaded.len(), frames.len() + 1);
    assert!(reloaded.contains_key(&added));
    for (document, frame) in frames {
        assert_eq!(reloaded.get(&document), Some(&frame));
    }
    drop(small_budget);
    assert_eq!(cache.content_budget(), boot_budget);
}

struct ScopedSearchAccountHook {
    slot: &'static std::sync::Mutex<Option<u32>>,
    prior: Option<u32>,
}

impl ScopedSearchAccountHook {
    fn set(slot: &'static std::sync::Mutex<Option<u32>>, id: u32) -> Self {
        let prior = slot
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .replace(id);
        Self { slot, prior }
    }
}

impl Drop for ScopedSearchAccountHook {
    fn drop(&mut self) {
        *self.slot.lock().unwrap_or_else(|error| error.into_inner()) = self.prior;
    }
}

async fn case_thread_allowance_fallback_checks_the_final_inventory(
    test: &TestServer,
    key: &Account,
) {
    use common::za::{
        search::{seal::build_thread_blob, trace::ZA_SEARCH_ABORTED},
        test_hooks::SEARCH_COMPACTION_FORCE_ALLOWANCE,
    };
    use store::za_search::{
        BlobKind, SegmentId,
        blob::{PublicSection, SEAL_OVERHEAD},
        manifest::ThreadBlobEntry,
    };

    const PAD: usize = 65_537;
    let _config = search_config_guard!(test, segments: 255, pad_min: PAD);
    let id = account_id(key);
    let initial = deliver_one(test, key, "Allowance base").await;
    advanced(test, key, true).await;
    let mut seeded = index(test, id).await;
    assert_eq!(seeded.thread_blobs.len(), 1);
    let keys = za_unlock(test, id, key.secret()).await;
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Principal)
        .with_document(0);
    for _ in 1..254 {
        let header = BlobHeader {
            kind: BlobKind::Thread,
            tier: 0,
            segment: SegmentId::random(),
            predecessor: None,
        };
        let bytes = build_thread_blob(
            keys.swk(),
            id,
            &header,
            None,
            &PublicSection::default(),
            PAD,
        )
        .unwrap();
        let (hash, hold) = test
            .server
            .hold_blob(id, BlobHash::generate(&bytes), &bytes, 60, &BlobOpts::PLAIN)
            .await
            .unwrap();
        hold.release(&mut batch).set(
            BlobOp::Link {
                hash: hash.clone(),
                to: BlobLink::Document,
            },
            BlobLinkValue::new(bytes.len()),
        );
        seeded.thread_blobs.push(ThreadBlobEntry {
            segment: header.segment,
            hash,
            size: u32::try_from(bytes.len()).unwrap(),
        });
    }
    batch.set(
        PrincipalField::ZeroAccessSearch,
        ManifestValue::Index(seeded.clone()).encode(&za_search_key(keys.swk())),
    );
    test.server.store().write_batch(&mut batch).await.unwrap();
    simulate_restart(test, id).await;
    assert_manifest_links(test, id, &seeded).await;

    let allowance = ScopedSearchAccountHook::set(&SEARCH_COMPACTION_FORCE_ALLOWANCE, id);
    let traces = Traced::start(&[ZA_SEARCH_EVENT_TYPE]);
    let fallback = deliver_one(test, key, "Valid fallback at the inventory limit").await;
    assert!(advanced(test, key, true).await.change_id > seeded.indexed_to.change_id);
    let at_limit = index(test, id).await;
    assert_eq!(at_limit.thread_blobs.len(), 255);
    assert_eq!(
        &at_limit.thread_blobs[..254],
        seeded.thread_blobs.as_slice()
    );
    assert_manifest_links(test, id, &at_limit).await;
    let bytes = stored_blob(test, &at_limit.thread_blobs.last().unwrap().hash).await;
    let (_, body) = BlobHeader::decode(&bytes).unwrap();
    let hidden_size = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    assert_eq!(
        hidden_size,
        PAD + SEAL_OVERHEAD,
        "publishing fallback uses exactly M"
    );

    let pending = deliver_one(test, key, "Would overflow the thread inventory").await;
    let raw = manifest_bytes(test, id).await;
    let links = principal_blob_links(test, id).await;
    let holds = temporary_blob_hashes(test, id).await;
    let email = log_records_since(test, id, SyncCollection::Email, 0).await;
    let thread = log_records_since(test, id, SyncCollection::Thread, 0).await;
    let resident_frames = content_frames(test, key).await;
    assert_eq!(
        pass(test, key).await,
        PassOutcome::Aborted(TransientClass::Validation)
    );
    assert_eq!(manifest_bytes(test, id).await, raw);
    assert_eq!(principal_blob_links(test, id).await, links);
    assert_eq!(temporary_blob_hashes(test, id).await, holds);
    assert_eq!(
        log_records_since(test, id, SyncCollection::Email, 0).await,
        email
    );
    assert_eq!(
        log_records_since(test, id, SyncCollection::Thread, 0).await,
        thread
    );
    let resident = loaded(test, id).await;
    assert_eq!(resident.manifest, at_limit);
    assert!(resident.thread.is_present(initial) && resident.thread.is_present(fallback));
    assert!(!resident.thread.is_present(pending));
    drop(resident);
    assert_eq!(content_frames(test, key).await, resident_frames);
    let events = traces.finish().await;
    let refusals = events
        .iter()
        .filter(|event| {
            event.account_id == Some(u64::from(id)) && event.details == ZA_SEARCH_ABORTED
        })
        .map(|event| event.reason.as_str())
        .collect::<Vec<_>>();
    assert_eq!(refusals, ["allowance", "allowance", "segments"]);

    drop(allowance);
    assert!(advanced(test, key, true).await.change_id > at_limit.indexed_to.change_id);
    let compacted = index(test, id).await;
    assert_eq!(compacted.thread_blobs.len(), 1);
    assert_manifest_links(test, id, &compacted).await;
    let installed = principal_hashes(test, id).await;
    assert!(
        at_limit
            .thread_blobs
            .iter()
            .all(|blob| !installed.contains(&blob.hash))
    );
    simulate_restart(test, id).await;
    let part = crate::utils::za::za_search_thread_part(test, id, key.secret()).await;
    assert!(
        [initial, fallback, pending]
            .into_iter()
            .all(|id| part.is_present(id))
    );
    println!("Thread allowance fallback accepted 254→255, refused 255→256, and compacted 255→1");
}

async fn case_content_frame_budget(test: &TestServer, key: &Account, compressed: bool) {
    use common::za::search::seal::{build_content_blob, build_thread_blob};
    use store::{
        search::IndexDocument,
        write::{Dictionary, SearchIndex, compress::compress},
        za_search::{
            BlobKind, PadTarget, SegmentId,
            blob::{HiddenSection, PublicSection},
            content::{ContentRecord, SortKeys},
            manifest::{ContentHead, ThreadBlobEntry},
            thread::ThreadRecord,
        },
    };
    use utils::codec::leb128::Leb128Writer;

    let _config = search_config_guard!(test, segments: 2);
    let id = account_id(key);
    let keys = za_unlock(test, id, key.secret()).await;
    let original = index(test, id).await;
    assert!(original.content.is_none() && original.thread_blobs.is_empty());
    let count = if compressed { 1 } else { 1024 };
    let ids = 10_000..10_000 + count;
    let records = ids
        .clone()
        .map(|document_id| {
            let mut frame = ContentRecord::new(
                IndexDocument::new(SearchIndex::Email),
                document_id,
                SortKeys::default(),
                None,
                false,
            )
            .encode();
            if compressed {
                // One term, one positional field, and a valid highly compressible
                // sequence of occurrences of ordinal zero.
                let mut terms = vec![1, 1, b'x', 1, 1, 0];
                terms.push_leb128(200_000usize);
                terms.resize(terms.len() + 200_000, 0);
                let mut stored = compress(Some(Dictionary::Term), &terms, 1).unwrap();
                stored.push(1); // DOCUMENT_ZSTD
                frame.truncate(16);
                frame.extend_from_slice(&(stored.len() as u32).to_le_bytes());
                frame.extend_from_slice(&stored);
                assert!(ContentRecord::decode(&frame).is_ok());
            }
            frame
        })
        .collect::<Vec<_>>();
    let frame_slices = records.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let hidden = HiddenSection {
        records: ids
            .clone()
            .map(|document_id| ThreadRecord {
                document_id,
                thread_id: document_id,
                revision: original.revision,
                created: original.indexed_to.change_id,
                deleted: None,
                subject_hash: document_id.into(),
                hashes: vec![],
            })
            .collect(),
        aliases: Vec::new(),
    };
    let content_header = BlobHeader {
        kind: BlobKind::Content,
        tier: 0,
        segment: SegmentId::random(),
        predecessor: None,
    };
    let thread_header = BlobHeader {
        kind: BlobKind::Thread,
        tier: 0,
        segment: SegmentId::random(),
        predecessor: None,
    };
    let content_bytes = build_content_blob(keys.swk(), id, &content_header, &frame_slices);
    if !compressed {
        assert_eq!(content_bytes.len(), 27_759);
    }
    let thread_bytes = build_thread_blob(
        keys.swk(),
        id,
        &thread_header,
        Some((
            &hidden,
            PadTarget::Compaction {
                n_records: count as usize,
            },
        )),
        &PublicSection::default(),
        test.server.za_search_config().pad_min,
    )
    .unwrap();
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Principal)
        .with_document(0);
    let mut hashes = Vec::new();
    for bytes in [&content_bytes, &thread_bytes] {
        let (hash, hold) = test
            .server
            .hold_blob(id, BlobHash::generate(bytes), bytes, 60, &BlobOpts::PLAIN)
            .await
            .unwrap();
        hold.release(&mut batch).set(
            BlobOp::Link {
                hash: hash.clone(),
                to: BlobLink::Document,
            },
            BlobLinkValue::new(bytes.len()),
        );
        hashes.push(hash);
    }
    let mut seeded = original;
    seeded.content = Some(ContentHead {
        hash: hashes[0].clone(),
        segment: content_header.segment,
        size: content_bytes.len() as u64,
        length: 1,
    });
    seeded.thread_blobs.push(ThreadBlobEntry {
        hash: hashes[1].clone(),
        segment: thread_header.segment,
        size: thread_bytes.len() as u32,
    });
    let raw = ManifestValue::Index(seeded.clone()).encode(&za_search_key(keys.swk()));
    batch.set(PrincipalField::ZeroAccessSearch, raw.clone());
    test.server.store().write_batch(&mut batch).await.unwrap();
    simulate_restart(test, id).await;
    let document = deliver_one(test, key, "Frame budget delta").await;
    let links = principal_blob_links(test, id).await;
    let cache = &test.server.inner.cache.za_search;

    let pressure = if compressed { 64 * 1024 } else { 128 * 1024 };
    {
        let _budget = cache.override_content_budget_for_test(pressure);
        let entry = test.server.za_search_entry(id, &keys).await.unwrap();
        let load = test
            .server
            .za_search_load_content(&entry, &keys)
            .await
            .map_err(|error| error.classification());
        drop(entry);
        simulate_restart(test, id).await;
        let merge = pass(test, key).await;
        assert_eq!(
            (load, merge),
            (
                Err(LoadFailure::Transient(TransientClass::Budget)),
                PassOutcome::Aborted(TransientClass::Budget)
            ),
            "compressed={compressed}",
        );
        assert_eq!(
            manifest_bytes(test, id).await.as_deref(),
            Some(raw.as_slice())
        );
        assert_eq!(principal_blob_links(test, id).await, links);
        assert!(loaded(test, id).await.content.is_none());
    }
    {
        let budget = if compressed { 1024 * 1024 } else { 256 * 1024 };
        let _budget = cache.override_content_budget_for_test(budget);
        if !compressed {
            let entry = test.server.za_search_entry(id, &keys).await.unwrap();
            test.server
                .za_search_load_content(&entry, &keys)
                .await
                .unwrap();
            let resident = entry
                .read_loaded()
                .await
                .unwrap()
                .content
                .as_ref()
                .unwrap()
                .bytes;
            println!(
                "Warm frame budget cap={budget} resident={resident} sealed={}",
                content_bytes.len()
            );
            assert!(
                resident + 3 * content_bytes.len() as u64 + 4096 < budget,
                "the initial seal reservation fits beside the warm part"
            );
        }
        advanced(test, key, true).await;
        if !compressed {
            assert!(
                loaded(test, id).await.content.is_none(),
                "the larger assembly phase evicts the exact warm base when it fits cold"
            );
        }
        let after = index(test, id).await;
        assert_eq!(after.content.as_ref().unwrap().length, 1);
        assert_manifest_links(test, id, &after).await;
    }
    simulate_restart(test, id).await;
    {
        let _budget = cache.override_content_budget_for_test(pressure);
        let entry = test.server.za_search_entry(id, &keys).await.unwrap();
        assert_eq!(
            test.server
                .za_search_load_content(&entry, &keys)
                .await
                .map_err(|error| error.classification()),
            Err(LoadFailure::Transient(TransientClass::Budget)),
        );
        assert!(entry.read_loaded().await.unwrap().content.is_none());
    }
    let _budget = cache.override_content_budget_for_test(1024 * 1024);
    let reloaded = content_frames(test, key).await;
    assert_eq!(reloaded.len(), records.len() + 1);
    assert!(reloaded.contains_key(&document));
    for (doc, frame) in ids.zip(records) {
        assert_eq!(reloaded.get(&doc), Some(&frame));
    }
    println!(
        "Frame budget merge and cold reload validated compressed={compressed} records={count}"
    );
}

async fn test_warm_content_admission(test: &mut TestServer) {
    for (name, cap) in [
        ("keymail-warm-reuse@example.com", 192 * 1024),
        ("keymail-warm-room@example.com", 1024 * 1024),
    ] {
        let key = key_mail_account(test, name, "Warm-admission!").await;
        case_warm_staged_insertion(test, &key, cap).await;
        test.destroy_all_mailboxes(&key).await;
    }

    let key = key_mail_account(
        test,
        "keymail-warm-tombstone@example.com",
        "Warm-admission!",
    )
    .await;
    case_warm_tombstone_budget(test, &key).await;
    test.destroy_all_mailboxes(&key).await;

    for (name, pressure) in [
        ("keymail-warm-missing@example.com", true),
        ("keymail-warm-prune@example.com", false),
    ] {
        let key = key_mail_account(test, name, "Warm-admission!").await;
        case_warm_full_merge_missing_budget(test, &key, pressure).await;
        test.destroy_all_mailboxes(&key).await;
    }
}

const WARM_NEEDLE: &str = "zawarmneedle";
const WARM_POSITIONS: usize = 120_000;
const WARM_REMOVE_CAP: u64 = 64 * 1024;

fn repeated_warm_body() -> String {
    let line = format!(
        "{}{}\r\n",
        format!("{WARM_NEEDLE} ").repeat(31),
        WARM_NEEDLE
    );
    line.repeat(WARM_POSITIONS / 32)
}

async fn deliver_repeated_warm(test: &TestServer, key: &Account) -> u32 {
    let id = account_id(key);
    let before = message_ids(test, id).await;
    let message = message_with("Warm admission", &repeated_warm_body());
    let (headers, body) = message.split_once("\r\n\r\n").unwrap();
    let mime = format!(
        "{headers}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\
         Content-Language: en\r\n\r\n{body}"
    );
    deliver(test, SENDER, key.name(), &mime).await;
    super::new_message(test, id, &before).await
}

async fn read_warm_root_frames(
    test: &TestServer,
    key: &Account,
    manifest: &Manifest,
    reserve: impl FnMut(usize) -> bool,
) -> Result<
    Vec<store::za_search::content::OwnedContentFrame>,
    store::za_search::blob::ContentFramesError,
> {
    use common::za::search::seal::open_section;
    use store::za_search::{BlobKind, SectionKind, blob::decode_content_frames};

    let id = account_id(key);
    let keys = za_unlock(test, id, key.secret()).await;
    let head = manifest.content.as_ref().unwrap();
    assert_eq!(head.length, 1);
    let bytes = stored_blob(test, &head.hash).await;
    assert_eq!(BlobHash::generate(&bytes), head.hash);
    assert_eq!(bytes.len() as u64, head.size);
    let (header, sealed) = BlobHeader::decode(&bytes).unwrap();
    assert_eq!(header.kind, BlobKind::Content);
    assert_eq!(header.segment, head.segment);
    assert!(header.predecessor.is_none());
    let plaintext = open_section(keys.swk(), id, &header, SectionKind::Content, sealed).unwrap();
    decode_content_frames(&plaintext, reserve)
}

async fn case_warm_staged_insertion(test: &TestServer, key: &Account, cap: u64) {
    use common::za::test_hooks::{SearchContentWrite, take_search_content_writes};
    use store::za_search::blob::ContentFramesError;

    let _config = search_config_guard!(test, segments: 2);
    let id = account_id(key);
    let cache = &test.server.inner.cache.za_search;
    let first = deliver_one(test, key, "Warm admission first").await;
    advanced(test, key, true).await;
    let original_frames = content_frames(test, key).await;
    let original = index(test, id).await;
    assert!(original.cursor.is_none());
    assert!(original.content_to.cursor.is_none());
    assert!(original.reconciliation.is_none());

    let added = deliver_repeated_warm(test, key).await;
    let budget = cache.override_content_budget_for_test(cap);
    let _observer = ObserveContentWrites::start(id);
    let parked = park(test, key, set_search_swap_pause).await;
    let (old_bytes, old_scratch) = {
        let view = loaded(test, id).await;
        let warm = view.content.as_ref().expect("merge kept its warm base");
        assert_eq!(warm.segments, [original.content.as_ref().unwrap().segment]);
        assert_eq!(
            warm.part.record_bytes(first),
            Some(original_frames[&first].as_slice()),
        );
        (warm.bytes, warm.part.removal_scratch(first))
    };
    let committed = index(test, id).await;
    let raw = manifest_bytes(test, id).await.unwrap();
    let links = principal_blob_links(test, id).await;
    assert_ne!(committed.content, original.content);
    let mut declared = 0;
    assert!(matches!(
        read_warm_root_frames(test, key, &committed, |bytes| {
            declared = bytes;
            false
        })
        .await,
        Err(ContentFramesError::Budget)
    ));
    assert!(declared >= WARM_POSITIONS && declared > old_scratch);
    assert!(committed.content.as_ref().unwrap().size < 4096);
    if cap == 192 * 1024 {
        assert!(old_bytes + 2 * declared as u64 > cap);
    }
    let writes = take_search_content_writes(id);
    let estimate = |find: fn(&SearchContentWrite) -> Option<u64>| {
        writes.iter().find_map(find).expect("compaction accounting")
    };
    let staged = estimate(|event| match event {
        SearchContentWrite::StagedMemory(bytes) => Some(*bytes),
        _ => None,
    });
    let original_size = estimate(|event| match event {
        SearchContentWrite::Original(bytes) => Some(*bytes),
        _ => None,
    });
    assert!(
        writes
            .iter()
            .any(|event| matches!(event, SearchContentWrite::Linked { .. }))
    );
    let paused_usage = cache.content_used();
    assert_eq!(
        parked.finish().await,
        PassOutcome::Advanced {
            published: true,
            indexed_to: committed.indexed_to,
        }
    );
    assert!(
        staged >= declared as u64,
        "retained staging charge {staged} must cover the {declared}-byte term buffer",
    );
    assert!(staged > original_size + declared as u64 / 2);
    assert!(old_bytes + staged <= paused_usage && paused_usage <= cap);
    let frames = read_warm_root_frames(test, key, &committed, |_| true)
        .await
        .unwrap();
    assert_eq!(frames.len(), 2);
    {
        let view = loaded(test, id).await;
        let content = view
            .content
            .as_ref()
            .expect("staging capacity was reusable");
        assert_eq!(
            content.segments,
            [committed.content.as_ref().unwrap().segment]
        );
        assert_eq!(content.bytes, content.part.loaded_bytes() as u64);
        assert_eq!(view.manifest_bytes_hash, manifest_hash(&raw));
        assert!(view.thread.is_present(added));
        for frame in &frames {
            assert_eq!(
                content.part.record_bytes(frame.document_id()),
                Some(frame.bytes())
            );
        }
    }
    assert_eq!(
        manifest_bytes(test, id).await.as_deref(),
        Some(raw.as_slice())
    );
    assert_eq!(principal_blob_links(test, id).await, links);
    assert_manifest_links(test, id, &committed).await;

    drop(budget);
    let _budget = cache.override_content_budget_for_test(1024 * 1024);
    simulate_restart(test, id).await;
    let reloaded = content_frames(test, key).await;
    assert_eq!(reloaded.len(), frames.len());
    for frame in frames {
        assert_eq!(reloaded[&frame.document_id()], frame.bytes());
    }
    assert_eq!(reloaded[&first], original_frames[&first]);
    let ids = queried(&recovery_query(key, Some(WARM_NEEDLE)).await);
    assert_eq!(ids.len(), 1);
    assert_eq!(Id::from_str(&ids[0]).unwrap().document_id(), added);
    assert_eq!(pass(test, key).await, PassOutcome::Current);
    println!(
        "Warm insertion reused stage at cap={cap}: stage={staged}, scratch={declared}, paused={paused_usage}"
    );
}

async fn case_warm_tombstone_budget(test: &TestServer, key: &Account) {
    let id = account_id(key);
    let cache = &test.server.inner.cache.za_search;
    let survivor = deliver_one(test, key, "Warm tombstone survivor").await;
    let removed = deliver_repeated_warm(test, key).await;
    advanced(test, key, true).await;
    let original = index(test, id).await;
    let frames = content_frames(test, key).await;
    pop3_delete(test, key, removed).await;

    let budget = cache.override_content_budget_for_test(WARM_REMOVE_CAP);
    let parked = park(test, key, set_search_swap_pause).await;
    let scratch = {
        let view = loaded(test, id).await;
        let warm = view.content.as_ref().expect("warm before tombstone");
        assert!(warm.bytes < WARM_REMOVE_CAP);
        assert_eq!(
            warm.part.record_bytes(removed),
            Some(frames[&removed].as_slice())
        );
        warm.part.removal_scratch(removed)
    };
    assert!(scratch >= WARM_POSITIONS);
    assert!(scratch as u64 > WARM_REMOVE_CAP);
    let committed = index(test, id).await;
    let raw = manifest_bytes(test, id).await.unwrap();
    assert_eq!(committed.content, original.content);
    assert_eq!(committed.revision, original.revision);
    assert!(committed.indexed_to.change_id > original.indexed_to.change_id);
    assert_eq!(
        parked.finish().await,
        PassOutcome::Advanced {
            published: false,
            indexed_to: committed.indexed_to,
        }
    );
    {
        let view = loaded(test, id).await;
        assert!(
            view.content.is_none(),
            "warm tombstone scratch must be admitted"
        );
        assert_eq!(view.manifest_bytes_hash, manifest_hash(&raw));
        assert_eq!(view.thread.exists_at(removed, u64::MAX), Some(false));
    }
    assert_eq!(
        manifest_bytes(test, id).await.as_deref(),
        Some(raw.as_slice())
    );
    assert_manifest_links(test, id, &committed).await;

    drop(budget);
    let _budget = cache.override_content_budget_for_test(1024 * 1024);
    simulate_restart(test, id).await;
    let reloaded = content_frames(test, key).await;
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[&survivor], frames[&survivor]);
    assert!(queried(&recovery_query(key, Some(WARM_NEEDLE)).await).is_empty());
    assert_eq!(pass(test, key).await, PassOutcome::Current);
    println!("Warm tombstone evicted only content before scratch={scratch}");
}

async fn case_warm_full_merge_missing_budget(test: &TestServer, key: &Account, pressure: bool) {
    use nlp::language::Language;
    use store::{
        search::{EmailSearchField, IndexDocument},
        write::SearchIndex,
        za_search::content::{ContentRecord, SortKeys},
    };

    let _config = search_config_guard!(test, segments: 2);
    let id = account_id(key);
    let cache = &test.server.inner.cache.za_search;
    let _room = cache.override_content_budget_for_test(1024 * 1024);
    let first = deliver_one(test, key, "Warm missing survivor").await;
    advanced(test, key, true).await;

    // The authenticated old root has an extra content frame with no thread row.
    let missing = u32::MAX;
    let mut document = IndexDocument::new(SearchIndex::Email);
    document.index_text(
        EmailSearchField::Body,
        &repeated_warm_body(),
        Language::English,
    );
    let extra = ContentRecord::new(document, missing, SortKeys::default(), None, false).encode();
    let (original, _) =
        seed_content_tiers_with_frame(test, key, index(test, id).await, &[0], Some(&extra)).await;
    let frames = content_frames(test, key).await;
    assert_eq!(frames[&missing], extra);
    let added = deliver_one(test, key, "Warm missing addition").await;

    let parked = park(test, key, set_search_swap_pause).await;
    let scratch = {
        let view = loaded(test, id).await;
        let warm = view.content.as_ref().expect("warm before missing prune");
        assert_eq!(warm.segments, [original.content.as_ref().unwrap().segment]);
        assert!(warm.bytes < WARM_REMOVE_CAP);
        assert!(warm.part.contains(missing));
        assert!(view.thread.record(missing).is_none());
        warm.part.removal_scratch(missing)
    };
    assert!(scratch >= WARM_POSITIONS && scratch as u64 > WARM_REMOVE_CAP);
    let committed = index(test, id).await;
    let raw = manifest_bytes(test, id).await.unwrap();
    let merged = read_warm_root_frames(test, key, &committed, |_| true)
        .await
        .unwrap();
    assert_eq!(merged.len(), 2);
    assert!(merged.iter().any(|frame| frame.document_id() == added));
    assert!(merged.iter().all(|frame| frame.document_id() != missing));
    let budget = pressure.then(|| cache.override_content_budget_for_test(WARM_REMOVE_CAP));
    assert_eq!(
        parked.finish().await,
        PassOutcome::Advanced {
            published: true,
            indexed_to: committed.indexed_to,
        }
    );
    {
        let view = loaded(test, id).await;
        assert_eq!(view.manifest_bytes_hash, manifest_hash(&raw));
        assert!(view.thread.is_present(added));
        if pressure {
            assert!(
                view.content.is_none(),
                "warm missing-ID scratch must be admitted"
            );
        } else {
            let warm = view.content.as_ref().expect("warm pruning fits");
            assert!(warm.part.contains(first) && warm.part.contains(added));
            assert!(!warm.part.contains(missing));
        }
    }
    assert_eq!(
        manifest_bytes(test, id).await.as_deref(),
        Some(raw.as_slice())
    );
    assert_manifest_links(test, id, &committed).await;

    drop(budget);
    simulate_restart(test, id).await;
    let reloaded = content_frames(test, key).await;
    assert_eq!(reloaded.len(), 2);
    assert_eq!(reloaded[&first], frames[&first]);
    for frame in merged {
        assert_eq!(reloaded[&frame.document_id()], frame.bytes());
    }
    assert_eq!(pass(test, key).await, PassOutcome::Current);
    println!("Warm missing-ID pruning at pressure={pressure}: scratch={scratch}");
}
