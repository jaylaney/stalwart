/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 5 and 6.1: a message delivered to a key account is stored as a
//! stub and a fresh sealed blob, its key wrapped to the account's public key
//! (`pk`); a pending account's delivery waits in the queue; Sieve runs on
//! the plaintext in memory; flags, moves and copies within the account touch
//! no content; no spam sample is written for a key account (6.4); a restore
//! into a key account is refused (6.7).

use super::{
    assert_no_plaintext, assert_sealed, deliver, group_ingest, has_metadata, key_mail_account,
    keyed_token, keyless_token, message_ids, message_with, metadata_rows, new_message,
    open_message, reply_to, row_is_stub, spam_samples, stored_blob, stored_hash, stored_metadata,
    stored_metadata_bytes, threading_keys,
};
use crate::{
    utils::{
        account::Account,
        imap::{ImapConnection, Type},
        server::TestServer,
        smtp::SmtpConnection,
        za::{queued_recipients, temporarily_failed, wait_for_delivery, za_setup, za_setup_token},
    },
    za::{STRONG, user_permissions},
};
use email::{
    cache::MessageCacheFetch,
    mailbox::{INBOX_ID, JUNK_ID},
    message::ingest::group::IngestGroup,
};
use groupware::calendar::seal::{WRAP_MK, WRAP_PK};
use imap_proto::ResponseType;
use registry::{
    schema::{
        enums::ArchivedItemType,
        prelude::{ObjectType, Property},
        structs::{
            MtaDeliveryExpiration, MtaDeliveryExpirationTtl, MtaDeliverySchedule,
            MtaDeliveryScheduleInterval, MtaDeliveryScheduleIntervals,
            MtaDeliveryScheduleIntervalsOrDefault, MtaOutboundStrategy, MtaVirtualQueue,
            SpamClassifier, Task, TaskRestoreArchivedItem, TaskStatus,
        },
    },
    types::{datetime::UTCDateTime, list::List},
};
use serde_json::json;
use std::time::{Duration, Instant};
use store::write::{BatchBuilder, now};
use types::{
    blob::{BlobClass, BlobId},
    collection::Collection,
    id::Id,
};

const KEY: &str = "keymail2@example.com";
const PENDING: &str = "keymail3@example.com";
const PLAIN: &str = "plainmail1@example.com";
/// `grouped_ingest_mixed_key_and_plain`'s key account.
const MIXED_KEY: &str = "keymail28@example.com";
/// Its ordinary account, whose messages share the group's batch.
const MIXED_PLAIN: &str = "plainmail23@example.com";
const SENDER: &str = "sender@example.com";
/// The module's local delivery schedule: retries one second apart, so a
/// temporary failure is retried while the test waits.
const SCHEDULE: &str = "za-retry";
const SCRIPT: &str = r#"require ["fileinto", "mailbox", "vacation"];
if header :contains "subject" "file-me" {
    fileinto :create "Archive";
} elsif header :contains "subject" "redirect-me" {
    redirect "plainmail1@example.com";
    keep;
}
vacation "away text";
"#;

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mail delivery tests...");
    let admin = test.account("admin@example.com").clone();
    let schedule_id = configure(&admin).await;
    let key = key_mail_account(test, KEY, STRONG).await;
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    let key_id = key.id().document_id();
    let plain_id = plain.id().document_id();

    let pending = pending_account_gets_a_temporary_failure(test).await;
    delivery_is_sealed_under_pk(test, key_id).await;
    let plain_doc = same_message_to_a_key_and_a_plain_account(test, key_id, plain_id).await;
    sieve_fileinto_vacation_redirect(test, &key, key_id, plain_id).await;
    imap_and_jmap_flags_and_moves_touch_no_content(test, &key, key_id, plain_id, plain_doc).await;
    restore_is_refused(test, key_id).await;
    let mixed_key = key_mail_account(test, MIXED_KEY, STRONG).await;
    let mixed_plain = admin
        .create_user_account(MIXED_PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    grouped_ingest_mixed_key_and_plain(test, &mixed_key, &mixed_plain).await;

    for account in [&key, &plain, &pending, &mixed_key, &mixed_plain] {
        test.destroy_all_mailboxes(account).await;
    }
    admin
        .registry_destroy_all(ObjectType::SpamTrainingSample)
        .await;
    unconfigure(&admin, schedule_id).await;
    wait_for_delivery(test).await;
}

/// The local delivery schedule of one-second retries, selected for local
/// recipients, and the classifier as `imap_tests` configures it (ruling
/// R8), so samples are written for ordinary accounts.
pub(super) async fn configure(admin: &Account) -> Id {
    let local_queue = admin
        .registry_get_all::<MtaVirtualQueue>()
        .await
        .into_iter()
        .find(|(_, queue)| queue.name == "local")
        .expect("the local virtual queue")
        .0;
    let day = MtaDeliveryScheduleIntervalsOrDefault::Custom(MtaDeliveryScheduleIntervals {
        intervals: List::from_iter([MtaDeliveryScheduleInterval {
            duration: 86_400_000u64.into(),
        }]),
    });
    let schedule_id = admin
        .registry_create_object(MtaDeliverySchedule {
            name: SCHEDULE.into(),
            description: None,
            retry: MtaDeliveryScheduleIntervalsOrDefault::Custom(MtaDeliveryScheduleIntervals {
                intervals: List::from_iter([MtaDeliveryScheduleInterval {
                    duration: 1_000u64.into(),
                }]),
            }),
            notify: day,
            expiry: MtaDeliveryExpiration::Ttl(MtaDeliveryExpirationTtl {
                expire: 86_400_000u64.into(),
            }),
            queue_id: local_queue,
        })
        .await;
    let mut strategy = MtaOutboundStrategy::default();
    let mut rewritten = 0;
    for rule in strategy.schedule.match_.values_mut() {
        if rule.then == "'local'" {
            rule.then = format!("'{SCHEDULE}'");
            rewritten += 1;
        }
    }
    assert!(
        rewritten > 0,
        "no local rule in the default schedule expression: {:?}",
        strategy.schedule
    );
    admin
        .registry_update_setting(strategy, &[Property::Schedule])
        .await;
    admin
        .registry_update_setting(
            SpamClassifier {
                min_ham_samples: 10,
                min_spam_samples: 10,
                ..Default::default()
            },
            &[Property::MinHamSamples, Property::MinSpamSamples],
        )
        .await;
    admin.reload_settings().await;
    schedule_id
}

pub(super) async fn unconfigure(admin: &Account, schedule_id: Id) {
    admin
        .registry_update_setting(MtaOutboundStrategy::default(), &[Property::Schedule])
        .await;
    admin
        .registry_update_setting(
            SpamClassifier::default(),
            &[Property::MinHamSamples, Property::MinSpamSamples],
        )
        .await;
    admin
        .registry_destroy(ObjectType::MtaDeliverySchedule, [schedule_id])
        .await
        .assert_destroyed(&[schedule_id]);
    admin.reload_settings().await;
}

/// `message_with`, addressed to the key account (Sieve's `vacation` answers
/// only a message whose header recipients name the account).
fn addressed(subject: &str, body: &str) -> String {
    message_with(subject, body).replace(
        "To: Recipient <recipient@example.com>",
        &format!("To: <{KEY}>"),
    )
}

/// The DATA content `SmtpConnection` sends for `message`: it puts a CRLF of
/// its own before the terminating dot.
fn received(message: &str) -> String {
    format!("{message}\r\n")
}

/// The header fields the receiving session may prepend to a message it
/// queues: its trace and authentication headers (`write_received`,
/// `add_auth_results`, `add_received_spf`, ARC, `Return-Path`, in
/// `crates/smtp/src/inbound/data.rs`) and the spam filter's score headers.
/// `Date` and `Message-ID` are added only when missing, and `message_with`
/// writes both.
const PREPENDED: &[&str] = &[
    "Received",
    "Return-Path",
    "Authentication-Results",
    "Received-SPF",
    "ARC-Seal",
    "ARC-Message-Signature",
    "ARC-Authentication-Results",
    "X-Spam-Result",
    "X-Spam-Score",
];

/// `raw` is the delivered bytes behind a prefix made only of `PREPENDED`
/// header fields (folded lines included).
pub(super) fn assert_received_as(raw: &[u8], message: &str) {
    let prefix = raw
        .strip_suffix(received(message).as_bytes())
        .unwrap_or_else(|| {
            panic!(
                "the blob is not the delivered message: {}",
                String::from_utf8_lossy(raw)
            )
        });
    let prefix = std::str::from_utf8(prefix).expect("an ASCII header block");
    assert!(prefix.is_empty() || prefix.ends_with("\r\n"), "{prefix:?}");
    for (n, line) in prefix.split_terminator("\r\n").enumerate() {
        if line.starts_with([' ', '\t']) {
            assert!(n > 0, "a continuation line opens the prefix: {prefix:?}");
            continue;
        }
        let name = line
            .split_once(':')
            .map(|(name, _)| name)
            .unwrap_or_else(|| panic!("not a header line: {line:?}"));
        assert!(
            PREPENDED.iter().any(|h| h.eq_ignore_ascii_case(name)),
            "the receiving session prepended {name:?}"
        );
    }
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

/// Mail spec 6.1 step 1: an account without a served public key (state
/// `PendingSetup`) gets a temporary failure and nothing is stored; once
/// setup completes, the queue's retry delivers the message sealed.
async fn pending_account_gets_a_temporary_failure(test: &mut TestServer) -> Account {
    println!("pending_account_gets_a_temporary_failure");
    let admin = test.account("admin@example.com").clone();
    let mut account = admin
        .create_passwordless_user_account(PENDING, STRONG, "Key mail", &[], user_permissions())
        .await;
    let id = account.id().document_id();
    let token = za_setup_token(&admin, PENDING).await;

    SmtpConnection::connect()
        .await
        .ingest(
            SENDER,
            &[PENDING],
            &message_with("pending", "pending-canary"),
        )
        .await;
    test.wait_for_tasks().await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !temporarily_failed(test, PENDING).await {
        assert!(
            Instant::now() < deadline,
            "no temporary failure queued for the pending account"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        queued_recipients(test).await.iter().any(|r| r == PENDING),
        "the queue holds the retry"
    );
    assert!(
        message_ids(test, id).await.is_empty(),
        "nothing is stored for a pending account"
    );

    account.recovery_key = Some(za_setup(PENDING, &token, STRONG).await);
    test.insert_account(account.clone());
    wait_for_delivery(test).await;
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "the queue's retry delivered the message");
    assert_sealed(test, id, ids[0], WRAP_PK).await;
    account
}

/// Mail spec 5.1-5.4, 6.1 step 3.
async fn delivery_is_sealed_under_pk(test: &TestServer, id: u32) {
    println!("delivery_is_sealed_under_pk");
    let before = message_ids(test, id).await;
    let message = message_with("hello subject-canary", "body-canary");
    deliver(test, SENDER, KEY, &message).await;
    let document_id = new_message(test, id, &before).await;
    assert_sealed(test, id, document_id, WRAP_PK).await;
    let stub = stored_metadata_bytes(test, id, document_id).await;
    let blob_hash = stored_hash(test, id, document_id).await;
    for canary in ["subject-canary", "body-canary"] {
        assert!(!contains(&stub, canary), "{canary} in the stub");
        assert!(
            !contains(&stored_blob(test, &blob_hash).await, canary),
            "{canary} in the blob"
        );
    }

    let (meta, raw) = open_message(test, id, STRONG, document_id).await;
    // The blob is the message as received: the queue's copy, whose head the
    // receiving session extended with its trace headers.
    assert_received_as(&raw, &message);
    let raw_headers = meta.raw_headers().unwrap();
    let meta = meta.unarchive().unwrap();
    assert!(
        raw_headers.starts_with(format!("Delivered-To: {KEY}\r\n").as_bytes()),
        "{}",
        String::from_utf8_lossy(&raw_headers)
    );
    assert!(meta.preview().contains("body-canary"), "{}", meta.preview());
    assert_eq!(meta.blob_hash(), blob_hash);
    let cache = test.server.get_cached_messages(id).await.unwrap();
    let item = cache
        .emails
        .by_id(document_id)
        .expect("the message is cached");
    assert_eq!(
        item.size() as usize,
        raw_headers.len() + raw.len() - meta.blob_body_offset(),
        "MessageData.size is the plaintext size"
    );
    assert_eq!(item.thread_id(), document_id, "a thread of its own");
}

/// Mail spec 5.1: a message delivered to two key accounts or to a key and a
/// plain account is sealed per key account; the plain copy is upstream's.
/// Ruling R8: delivery writes no sample.
async fn same_message_to_a_key_and_a_plain_account(
    test: &TestServer,
    key_id: u32,
    plain_id: u32,
) -> u32 {
    println!("same_message_to_a_key_and_a_plain_account");
    let key_before = message_ids(test, key_id).await;
    let plain_before = message_ids(test, plain_id).await;
    let samples = spam_samples(test).await;
    let message = message_with("shared delivery", "shared-canary");
    SmtpConnection::connect()
        .await
        .ingest(SENDER, &[KEY, PLAIN], &message)
        .await;
    wait_for_delivery(test).await;

    let key_doc = new_message(test, key_id, &key_before).await;
    let plain_doc = new_message(test, plain_id, &plain_before).await;
    assert_sealed(test, key_id, key_doc, WRAP_PK).await;
    let plain_row = stored_metadata(test, plain_id, plain_doc).await;
    assert!(!row_is_stub(&plain_row), "the plain copy is not sealed");
    let plain_hash = plain_row.unarchive().unwrap().blob_hash();
    assert_ne!(stored_hash(test, key_id, key_doc).await, plain_hash);
    assert!(
        stored_blob(test, &plain_hash)
            .await
            .ends_with(received(&message).as_bytes()),
        "the plain copy is the queue's blob"
    );
    assert_eq!(
        threading_keys(test, plain_id, plain_doc).await,
        1,
        "control: the plain copy is threaded"
    );
    assert_eq!(spam_samples(test).await, samples, "delivery wrote a sample");
    plain_doc
}

/// Mail spec 6.1 step 2: the Sieve script runs on the plaintext in memory;
/// `fileinto` and `keep` store sealed copies, `redirect` and `vacation` send
/// plaintext on. The sender is the plain account, so the vacation reply is
/// delivered and can be read there.
async fn sieve_fileinto_vacation_redirect(
    test: &TestServer,
    key: &Account,
    key_id: u32,
    plain_id: u32,
) {
    println!("sieve_fileinto_vacation_redirect");
    let client = key.jmap_client().await;
    let script_id = client
        .sieve_script_create("za-mail", SCRIPT, true)
        .await
        .unwrap()
        .take_id();
    let key_before = message_ids(test, key_id).await;
    let plain_before = message_ids(test, plain_id).await;
    for subject in ["file-me sieve", "redirect-me sieve", "kept sieve"] {
        deliver(
            test,
            PLAIN,
            KEY,
            &addressed(subject, &format!("{subject} body")),
        )
        .await;
    }

    let cache = test.server.get_cached_messages(key_id).await.unwrap();
    let archive_id = cache
        .mailboxes
        .items
        .iter()
        .find(|m| &*m.name == "Archive")
        .expect("fileinto :create made the Archive mailbox")
        .document_id;
    let mut filed = Vec::new();
    for item in cache
        .emails
        .iter()
        .filter(|m| !key_before.contains(&m.document_id()))
    {
        assert_sealed(test, key_id, item.document_id(), WRAP_PK).await;
        let (_, raw) = open_message(test, key_id, STRONG, item.document_id()).await;
        let subject = ["file-me sieve", "redirect-me sieve", "kept sieve"]
            .into_iter()
            .find(|s| contains(&raw, &format!("Subject: {s}")))
            .expect("one of the delivered messages");
        let mailboxes = item
            .mailboxes()
            .iter()
            .map(|m| m.mailbox_id)
            .collect::<Vec<_>>();
        filed.push((subject, mailboxes));
    }
    filed.sort();
    assert_eq!(
        filed,
        vec![
            ("file-me sieve", vec![archive_id]),
            ("kept sieve", vec![INBOX_ID]),
            ("redirect-me sieve", vec![INBOX_ID]),
        ]
    );

    // The redirected copy and the vacation reply, in the clear.
    let mut received = Vec::new();
    for document_id in message_ids(test, plain_id)
        .await
        .into_iter()
        .filter(|id| !plain_before.contains(id))
    {
        let row = stored_metadata(test, plain_id, document_id).await;
        assert!(!row_is_stub(&row), "the plain account's copy is not sealed");
        let blob = stored_blob(test, &row.unarchive().unwrap().blob_hash()).await;
        received.push(if contains(&blob, "redirect-me sieve body") {
            "redirect"
        } else if contains(&blob, "away text") {
            "vacation"
        } else {
            "other"
        });
    }
    received.sort_unstable();
    assert_eq!(received, ["redirect", "vacation"]);

    client.sieve_script_deactivate().await.unwrap();
    client.sieve_script_destroy(&script_id).await.unwrap();
}

/// The message's UID in `mailbox_id`, from the message cache.
async fn uid_in(test: &TestServer, account_id: u32, document_id: u32, mailbox_id: u32) -> u32 {
    test.server
        .get_cached_messages(account_id)
        .await
        .unwrap()
        .emails
        .by_id(document_id)
        .and_then(|m| m.uid_in(mailbox_id))
        .unwrap_or_else(|| panic!("message {document_id} is not in mailbox {mailbox_id}"))
}

/// The stored stub is byte-identical to `archive`.
async fn assert_unchanged(test: &TestServer, id: u32, doc: u32, archive: &[u8], after: &str) {
    assert_eq!(
        stored_metadata_bytes(test, id, doc).await,
        archive,
        "the stub changed after {after}"
    );
}

/// Mail spec 6.3, 6.4: STORE, MOVE, keyword updates and COPY within the
/// account edit `MessageData` only; EXPUNGE and Email/set destroy remove the
/// stub and the blob link. Ruling R8: the plain account's `$Junk` flag and
/// move to Junk each write a sample; the key account's write none.
async fn imap_and_jmap_flags_and_moves_touch_no_content(
    test: &TestServer,
    key: &Account,
    key_id: u32,
    plain_id: u32,
    plain_doc: u32,
) {
    println!("imap_and_jmap_flags_and_moves_touch_no_content");
    // The positive control, on the plain copy of the shared delivery.
    let samples = spam_samples(test).await;
    let mut imap = ImapConnection::connect(b"_p ").await;
    imap.authenticate(PLAIN, STRONG).await;
    imap.send_ok("SELECT INBOX").await;
    let uid = uid_in(test, plain_id, plain_doc, INBOX_ID).await;
    imap.send_ok(&format!("UID STORE {uid} +FLAGS ($Junk)"))
        .await;
    assert_eq!(spam_samples(test).await, samples + 1, "control: $Junk");
    imap.send_ok(&format!("UID MOVE {uid} \"Junk Mail\"")).await;
    assert_eq!(spam_samples(test).await, samples + 2, "control: Junk move");
    imap.send_ok("LOGOUT").await;
    let samples = samples + 2;

    let before = message_ids(test, key_id).await;
    deliver(test, SENDER, KEY, &message_with("flags", "flags-canary")).await;
    let doc = new_message(test, key_id, &before).await;
    let archive = stored_metadata_bytes(test, key_id, doc).await;
    let hash = stored_hash(test, key_id, doc).await;

    let mut imap = ImapConnection::connect(b"_k ").await;
    imap.authenticate(KEY, STRONG).await;
    imap.send_ok("SELECT INBOX").await;
    let uid = uid_in(test, key_id, doc, INBOX_ID).await;
    imap.send_ok(&format!("UID STORE {uid} +FLAGS (\\Seen $Junk)"))
        .await;
    assert_eq!(spam_samples(test).await, samples, "a sample from $Junk");
    imap.send_ok(&format!("UID MOVE {uid} \"Junk Mail\"")).await;
    assert_eq!(spam_samples(test).await, samples, "a sample from the move");
    assert_unchanged(test, key_id, doc, &archive, "STORE and MOVE").await;

    let email_id = Id::from_parts(doc, doc).to_string();
    key.jmap_update(
        "Email",
        [(&email_id, json!({ "keywords/$flagged": true }))],
        Vec::<(&str, &str)>::new(),
    )
    .await
    .updated(&email_id);
    assert_unchanged(test, key_id, doc, &archive, "a keyword update").await;

    imap.send_ok("SELECT \"Junk Mail\"").await;
    let junk_uid = uid_in(test, key_id, doc, JUNK_ID).await;
    imap.send_ok(&format!("UID COPY {junk_uid} INBOX")).await;
    assert_unchanged(test, key_id, doc, &archive, "COPY").await;
    assert_eq!(
        message_ids(test, key_id).await.len(),
        before.len() + 1,
        "a copy within the account is the same document"
    );
    uid_in(test, key_id, doc, INBOX_ID).await;

    // EXPUNGE drops the Junk copy, Email/set destroy the message.
    imap.send_ok(&format!("UID STORE {junk_uid} +FLAGS (\\Deleted)"))
        .await;
    imap.send("EXPUNGE").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send_ok("LOGOUT").await;
    assert_unchanged(test, key_id, doc, &archive, "EXPUNGE").await;
    let destroyed = key
        .jmap_destroy("Email", [&email_id], Vec::<(&str, &str)>::new())
        .await;
    assert_eq!(
        destroyed.destroyed().collect::<Vec<_>>(),
        [email_id.as_str()]
    );
    test.wait_for_tasks().await;
    assert!(!has_metadata(test, key_id, doc).await, "the stub is gone");
    assert!(
        test.server
            .store()
            .blob_link_size(
                &hash,
                BlobClass::Linked {
                    account_id: key_id,
                    collection: Collection::Email.into(),
                    document_id: doc,
                },
            )
            .await
            .unwrap()
            .is_none(),
        "the blob link is gone"
    );
}

/// Mail spec 6.7: `restore_item` re-parses the archived bytes, so a restore
/// into a key account is refused, a permanent task failure.
async fn restore_is_refused(test: &TestServer, key_id: u32) {
    println!("restore_is_refused");
    let before = message_ids(test, key_id).await;
    let raw = message_with("restored", "restore-canary");
    let (hash, _) = test
        .server
        .put_temporary_blob(key_id, raw.as_bytes(), 3600, &test.server.core.blob.email)
        .await
        .unwrap();
    let mut batch = BatchBuilder::new();
    batch.schedule_task(Task::RestoreArchivedItem(TaskRestoreArchivedItem {
        blob_id: BlobId::new(hash, BlobClass::default()),
        archived_item_type: ArchivedItemType::Email,
        created_at: UTCDateTime::now(),
        archived_until: UTCDateTime::from_timestamp((now() + 3600) as i64),
        account_id: Id::from(key_id),
        status: TaskStatus::now(),
    }));
    test.server.store().write_batch(&mut batch).await.unwrap();
    test.server.notify_task_queue();
    test.wait_for_tasks_skip_failures().await;
    assert_eq!(message_ids(test, key_id).await, before, "nothing restored");

    let admin = test.account("admin@example.com");
    let failed = admin
        .registry_get_all::<Task>()
        .await
        .into_iter()
        .filter(|(_, task)| {
            matches!(task, Task::RestoreArchivedItem(task) if task.account_id == Id::from(key_id))
        })
        .collect::<Vec<_>>();
    assert_eq!(failed.len(), 1, "{failed:?}");
    match failed[0].1.status() {
        TaskStatus::Failed(status) => assert_eq!(
            status.failure_reason,
            "zero-access: restore not supported for key accounts"
        ),
        status => panic!("the restore did not fail permanently: {status:?}"),
    }
    admin
        .registry_destroy(ObjectType::Task, [failed[0].0])
        .await
        .assert_destroyed(&[failed[0].0]);
}

/// Task 5 of the 1.0 upgrade: one `IngestGroup` batch (IMAP APPEND's and
/// JMAP's) holding a key account's and an ordinary account's messages,
/// interleaved, flushed once. The classification, the wrap and the fence
/// are each message's own (`email_ingest_into`): the key account's messages
/// are sealed under its session's key (`WRAP_MK`), with no threading key and
/// a thread of their own, although the plain message before them recorded
/// the same thread key in the group (a group thread a key message must
/// ignore); the ordinary account's messages are upstream's, plaintext and
/// threaded, its reply joining its parent's pending thread (the control
/// that the group threads at all). The same bytes go to both accounts, so
/// the key account's opened row is compared with the ordinary account's
/// stored row. Each result names its own account (`email_ingest_finish`).
async fn grouped_ingest_mixed_key_and_plain(test: &TestServer, key: &Account, plain: &Account) {
    println!("grouped_ingest_mixed_key_and_plain");
    let key_id = key.id().document_id();
    let plain_id = plain.id().document_id();
    // The system folders, so INBOX exists for both.
    message_ids(test, key_id).await;
    message_ids(test, plain_id).await;
    let key_token = keyed_token(test, key_id, STRONG).await;
    let plain_token = keyless_token(plain_id);

    let first = message_with("grouped mixed", "mixed-first-canary");
    let reply = reply_to(&first, "grouped mixed", "mixed-reply-canary");
    // (account, token, message), in the group's order: the plain message
    // records the thread key first, the key account's copy of it follows.
    let order = [
        (plain_id, &plain_token, &first),
        (key_id, &key_token, &first),
        (plain_id, &plain_token, &reply),
        (key_id, &key_token, &reply),
    ];
    let mut group = IngestGroup::default();
    for (account_id, token, raw) in order {
        group_ingest(&test.server, &mut group, token, raw)
            .await
            .unwrap_or_else(|err| panic!("{account_id}: the grouped ingest failed: {err:?}"));
    }
    assert!(
        message_ids(test, key_id).await.is_empty() && message_ids(test, plain_id).await.is_empty(),
        "the group wrote before its flush"
    );
    let ingested = group.flush(&test.server).await.unwrap();
    assert_eq!(ingested.len(), order.len(), "one result per message");

    for ((account_id, _, raw), email) in order.iter().zip(&ingested) {
        assert_eq!(
            email.blob_id.class,
            BlobClass::Linked {
                account_id: *account_id,
                collection: Collection::Email.into(),
                document_id: email.document_id,
            },
            "the result names its own account and document"
        );
        assert_eq!(email.size, raw.len(), "{account_id}: the ingested size");
        assert_eq!(email.imap_uids.len(), 1, "one UID, in INBOX");
    }
    let (plain_first, key_first, plain_reply, key_reply) =
        (&ingested[0], &ingested[1], &ingested[2], &ingested[3]);
    let mut key_docs = vec![key_first.document_id, key_reply.document_id];
    key_docs.sort_unstable();
    assert_eq!(message_ids(test, key_id).await, key_docs);
    let mut plain_docs = vec![plain_first.document_id, plain_reply.document_id];
    plain_docs.sort_unstable();
    assert_eq!(message_ids(test, plain_id).await, plain_docs);

    // The key account's messages: sealed, unthreaded, each its own thread.
    for (email, raw) in [(key_first, &first), (key_reply, &reply)] {
        assert_sealed(test, key_id, email.document_id, WRAP_MK).await;
        assert_eq!(
            email.thread_id, email.document_id,
            "a key account's grouped message joined a thread"
        );
        let (_, opened) = open_message(test, key_id, STRONG, email.document_id).await;
        assert_eq!(opened, raw.as_bytes(), "the opened blob is the message");
    }
    assert_no_plaintext(test, key_id).await;
    assert_eq!(
        metadata_rows(test, key_id).await.len(),
        2,
        "a metadata row without its message"
    );

    // The ordinary account's messages: upstream's, plaintext and threaded.
    for (email, raw) in [(plain_first, &first), (plain_reply, &reply)] {
        let row = stored_metadata(test, plain_id, email.document_id).await;
        assert!(
            !row_is_stub(&row),
            "the ordinary account's message is sealed"
        );
        assert_eq!(
            stored_blob(test, &row.unarchive().unwrap().blob_hash()).await,
            raw.as_bytes(),
            "the ordinary account's blob is the message"
        );
        assert_eq!(
            threading_keys(test, plain_id, email.document_id).await,
            1,
            "control: the ordinary account's message is threaded"
        );
    }
    assert_eq!(
        plain_reply.thread_id, plain_first.thread_id,
        "control: the ordinary reply joined its parent's pending thread"
    );

    // The key account's opened row is the row the ordinary path stores for
    // the same bytes, but for the blob it names (the sealed blob's hash).
    let (opened, _) = open_message(test, key_id, STRONG, key_first.document_id).await;
    let stored = stored_metadata(test, plain_id, plain_first.document_id).await;
    assert_eq!(
        opened.raw_headers().unwrap(),
        stored.raw_headers().unwrap(),
        "the opened header section"
    );
    let (opened, stored) = (opened.unarchive().unwrap(), stored.unarchive().unwrap());
    assert_eq!(opened.blob_body_offset(), stored.blob_body_offset());
    assert_eq!(opened.size(), stored.size());
    assert_eq!(opened.preview(), stored.preview());
    assert_eq!(opened.parts().len(), stored.parts().len());
    assert_eq!(
        opened.root().envelope().subject(),
        stored.root().envelope().subject(),
        "the opened envelope"
    );
    assert_eq!(
        opened.blob_hash(),
        stored_hash(test, key_id, key_first.document_id).await,
        "the opened row names the sealed blob"
    );

    assert!(
        !test.server.za_has_spam_samples(key_id).await.unwrap(),
        "the group trained a sample of the key account"
    );
}
