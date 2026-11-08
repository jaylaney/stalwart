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
        server::TestServer,
        za::{
            HookParked, expire_and_purge_blobs, index_holds, log_records_since, manifest_bytes,
            principal_blob_links, read_manifest, run_search_pass, simulate_restart, za_unlock,
        },
    },
    za::user_permissions,
};
use common::za::{
    search::{
        ZaSearchConfig,
        cache::EvictReason,
        error::TransientClass,
        seal::za_search_key,
        trace::{ZA_SEARCH_EVENT_TYPE, ZA_SEARCH_SKIPPED},
    },
    test_hooks::{
        reset_za_search_counters, set_search_blob_fail_once, set_search_blob_slow,
        set_search_commit_pause, set_search_config_override, set_search_hold_expire,
        set_search_pass_pause, set_search_swap_pause, za_search_blob_reads,
        za_search_message_opens,
    },
};
use email::{
    cache::MessageCacheFetch,
    message::za::search::pass::{PassOutcome, ZaSearchPass},
};
use imap_proto::ResponseType;
use serde_json::{Value, json};
use std::time::Duration;
use store::{
    query::log::{Change, PartialChange},
    write::{BatchBuilder, BlobLink, BlobOp},
    za_search::{
        BlobHeader, Checkpoint,
        manifest::{FailingDocument, Manifest, ManifestValue, manifest_hash},
    },
};
use trc::{Key, Value as TraceValue};
use types::{
    collection::{Collection, SyncCollection},
    field::PrincipalField,
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

    for account in [&key, &split, &plain] {
        test.destroy_all_mailboxes(account).await;
    }
    test.wait_for_tasks().await;
    for account in [&key, &split] {
        unlink_index_fixture_blobs_until_task16(test, account_id(account)).await;
    }
    println!("Zero-access mail index pass tests finished");
}

async fn unlink_index_fixture_blobs_until_task16(test: &TestServer, id: u32) {
    // Task 16 adds production account-destroy handling for these links.
    // Until then, teardown only the two fixture accounts' index blob links.
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Principal)
        .with_document(0);
    for (hash, _) in principal_blob_links(test, id).await {
        batch.clear(BlobOp::Link {
            hash,
            to: BlobLink::Document,
        });
    }
    test.server.store().write_batch(&mut batch).await.unwrap();
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
    let id = account_id(account);
    let ids = message_ids(test, id)
        .await
        .into_iter()
        .map(email_id)
        .collect::<Vec<_>>();
    destroy(account, &ids).await;
    if !ids.is_empty() {
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
