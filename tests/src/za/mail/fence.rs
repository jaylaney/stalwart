/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 6.1: the provisioning fence and the recheck. An ordinary
//! writer (`email_ingest` on the plain path, `copy_message`) commits only
//! while the account holds no vault record, so a delivery that classified
//! the account before `setup-token` and commits after it fails and is
//! retried as a key-account delivery; `setup-token` rechecks for mail after
//! writing its pending record and backs out when a delivery won; issuance
//! is refused on PostgreSQL and MySQL, where the assertion is no fence.

use super::{
    Traced, append_params, assert_no_plaintext, assert_sealed, deliver,
    delivery::{configure, temporarily_failed, unconfigure},
    group_ingest, keyed_token, keyless_token, message_ids, message_with, metadata_key,
    metadata_rows, open_message, reply_to, row_is_stub, stored_blob, stored_hash, stored_metadata,
};
use crate::{
    utils::{
        account::Account,
        imap::ImapConnection,
        server::TestServer,
        smtp::SmtpConnection,
        za::{
            Conn, HookParked, delete_event_records, hold_index_partition, index_search_documents,
            internal_search, partition_status, plant_event, queue_entries, release_index_partition,
            search_keys, stored_event, unindex_search_account, unindex_search_documents,
            wait_for_delivery, wait_for_index_queue, write_stored_event, za_post_as, za_setup,
            za_setup_token,
        },
    },
    za::{
        STRONG, index_task::emulate_superseding_node, raw_input::SUBMISSION_PORT, user_permissions,
    },
};
use base64::{Engine, engine::general_purpose::STANDARD};
use calcard::common::timezone::Tz;
use common::{
    Server,
    auth::{AccessToken, ChangedBy},
    za::{ZA_MAIL_KEYLESS, test_hooks},
};
use email::{
    cache::MessageCacheFetch,
    mailbox::{INBOX_ID, JUNK_ID},
    message::{
        copy::CopySources,
        copy::EmailCopy,
        delete::EmailDeletion,
        delivery::SenderAuthentication,
        ingest::{
            EmailIngest, IngestEmail, IngestSource,
            group::{IngestGroup, ThreadKey},
        },
    },
};
use groupware::calendar::{
    CalendarEventData,
    seal::{WRAP_MK, WRAP_PK},
};
use mail_parser::MessageParser;
use nlp::language::Language;
use registry::{
    schema::{
        enums::TaskStoreMaintenanceType,
        prelude::{Object, ObjectInner, ObjectType},
        structs::{
            self, Credential, PasswordCredential, SpamTrainingSample, Task, TaskMergeThreads,
            TaskStatus, TaskStatusFailed, TaskStoreMaintenance,
        },
    },
    types::{datetime::UTCDateTime, id::ObjectId, map::Map},
};
use serde_json::json;
use std::time::{Duration, Instant};
use store::{
    Deserialize, IterateParams, U32_LEN, U64_LEN, ValueKey,
    registry::write::{RegistryWrite, RegistryWriteResult},
    roaring::RoaringBitmap,
    search::{CalendarSearchField, EmailSearchField, IndexDocument},
    write::{
        BatchBuilder, BlobLink, BlobOp, SearchIndex, SearchIndexClass, TaskId, TaskQueueClass,
        ValueClass, key::DeserializeBigEndian, now, serialize::RawValue,
    },
};
use trc::{EventType, Key, SecurityEvent, StoreEvent};
use types::{
    blob::BlobClass,
    blob_hash::{BLOB_HASH_LEN, BlobHash},
    collection::Collection,
    field::EmailField,
    id::Id,
};
use utils::{cheeky_hash::CheekyHash, snowflake::SnowflakeIdGenerator};
use vault::{
    ZA_MARKER,
    record::{VaultRecord, VaultState},
};

const FENCE1: &str = "fence1@example.com";
const FENCE2: &str = "fence2@example.com";
const FENCE3: &str = "fence3@example.com";
const FENCE4: &str = "fence4@example.com";
const FENCE5: &str = "fence5@example.com";
const FENCE6: &str = "fence6@example.com";
const FENCE7: &str = "fence7@example.com";
const FENCE8: &str = "fence8@example.com";
const FENCE9: &str = "fence9@example.com";
const FENCE10: &str = "fence10@example.com";
const FENCE11: &str = "fence11@example.com";
const FENCE12: &str = "fence12@example.com";
/// `setup_refuses_a_pending_unindex`'s account (ruling R46).
const PENDING_UNINDEX: &str = "keymail14@example.com";
/// `setup_refuses_spam_samples`'s account (ruling R52).
const SAMPLED: &str = "keymail15@example.com";
/// `setup_refuses_an_orphaned_search_document`'s account (ruling R51).
const ORPHANED: &str = "keymail16@example.com";
/// `setup_refuses_an_in_flight_index_write`'s account (ruling R54).
const IN_FLIGHT: &str = "keymail17@example.com";
/// `setup_refuses_an_in_flight_calendar_index_write`'s account (ruling R60).
const IN_FLIGHT_EVENT: &str = "keymail21@example.com";
/// That test's event: no alarm, no organizer, so its only tasks are its
/// index and unindex tasks.
const IN_FLIGHT_ICAL: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Stalwart Labs//za fence//EN\r\n\
BEGIN:VEVENT\r\n\
UID:in-flight-event@example.com\r\n\
DTSTAMP:20261020T100000Z\r\n\
DTSTART:20261021T100000Z\r\n\
DTEND:20261021T110000Z\r\n\
SUMMARY:in-flight summary\r\n\
LOCATION:in-flight location\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";
/// `setup_refuses_a_failed_merge_task`'s account (ruling R54).
const FAILED_MERGE: &str = "keymail18@example.com";
/// `superseded_index_write_loses_to_provisioning`'s account (ruling R58).
const SUPERSEDED: &str = "keymail20@example.com";
/// Its ordinary control, whose index write passes the fence.
const SUPERSEDED_CONTROL: &str = "plainmail17@example.com";
/// `setup_refuses_orphaned_search_terms`'s account (ruling R62).
const ORPHANED_TERMS: &str = "keymail22@example.com";
/// `unindex_between_index_chunks_leaves_no_terms`'s account (ruling R62).
const CHUNKED: &str = "keymail23@example.com";
/// `unindex_between_calendar_index_chunks_leaves_no_terms`'s account
/// (ruling R62).
const CHUNKED_EVENT: &str = "keymail24@example.com";
/// `superseded_index_tail_loses_to_provisioning`'s account (ruling R62).
const SUPERSEDED_TAIL: &str = "keymail25@example.com";
/// Its ordinary control, whose multi-chunk index write passes the fence.
const SUPERSEDED_TAIL_CONTROL: &str = "plainmail18@example.com";
/// `plainmail19`, `plainmail21` and `plainmail22` belonged to the R63 cases
/// retired at 1.0 (part 6d); they stay unallocated.
/// `unindex_between_calendar_merge_chunks_leaves_no_terms`'s account
/// (ruling R63).
const MERGE_UNINDEXED: &str = "keymail26@example.com";
/// `multi_chunk_calendar_merge_is_written_whole`'s account, an ordinary one
/// (ruling R63).
const MERGE_CONTROL: &str = "plainmail20@example.com";
/// The token the merge tests' updated DESCRIPTION ends with: 13 bytes, so
/// stored verbatim, and new to the event.
const MERGE_CANARY: &str = "zzmergecanary";
/// The token `large_body` ends with: 12 bytes, so stored verbatim in its
/// `Term` key (`CheekyHash`), and written after the body's `c`-tokens, in
/// the pass's last commit chunk: 1.0 writes an account's term merges in key
/// order (`account_term_key_order`).
const TAIL_CANARY: &str = "zztailcanary";
/// `grouped_ingest_provisioning_wins_before_flush`'s account (Task 5 of the
/// 1.0 upgrade).
const GROUPED: &str = "keymail27@example.com";
/// Its ordinary control, whose group flushes in the clear.
const GROUPED_CONTROL: &str = "plainmail24@example.com";
/// `grouped_rejection_discards_uncommitted_plain_metadata`'s account, refused
/// in mid-group once provisioned.
const REJECTED: &str = "keymail29@example.com";
/// Its ordinary account, whose group keeps its batch after a rejection.
const REJECTED_CONTROL: &str = "plainmail25@example.com";
/// `train_batch_loses_to_provisioning`'s source account (ruling R55).
const TRAINED: &str = "keymail19@example.com";
/// `train_batch_loses_to_provisioning`'s destination, an ordinary account.
const TRAIN_DESTINATION: &str = "plainmail16@example.com";
/// `setup_refuses_object_metadata`'s account (ruling 66, the Task 6
/// review's F1).
const OBJECT_METADATA: &str = "keymail34@example.com";
/// The ordinary owner of the mailbox it holds a private container on.
const OBJECT_METADATA_OWNER: &str = "plainmail34@example.com";
/// `metadata_writes_lose_to_provisioning`'s account, an ordinary one that
/// logs in.
const METADATA_WRITER: &str = "plainmail35@example.com";
const PLAIN: &str = "plainmail3@example.com";
/// The organizer of `forward_race_imip_side_write`'s invitation, which it
/// sends through authenticated submission.
const ORGANIZER: &str = "plainmail4@example.com";
const SENDER: &str = "sender@example.com";
/// The trace details of `copy_message`'s refusal.
const COPY_REFUSED: &str = "zero-access: mail copy across accounts";
/// The trace details of the JMAP dispatcher's keyless refusal.
const KEYLESS_REFUSED: &str = "zero-access: mail access without session keys";
/// `stress_the_fence`'s runs.
const RUNS: u64 = 64;
/// The key path's temporary failure without a served public key (Task 3).
const NO_SERVED_KEY: &str = "zero-access: delivery without a served public key";
/// Makes every delivery to the script's owner stage its blob before the
/// batch (the message changed, so there is no queue blob to reuse).
const SCRIPT: &str = r#"require ["editheader"];
addheader "X-Sieve-Fence" "1";
keep;
"#;

pub async fn test(test: &mut TestServer) {
    println!("Running zero-access mail provisioning fence tests...");
    let admin = test.account("admin@example.com").clone();
    // As `delivery::test`: one-second retries, so a temporary failure is
    // retried while the test waits.
    let schedule_id = configure(&admin).await;

    let fence1 = forward_race_delivery_loses(test, &admin).await;
    let (fence2, plain) = forward_race_with_a_staged_blob(test, &admin).await;
    let (fence10, organizer) = forward_race_imip_side_write(test, &admin).await;
    let fence12 = forward_race_imip_merge(test, &admin).await;
    let fence3 = reverse_race_setup_token_aborts(test, &admin).await;
    let fence4 = reverse_race_reissue_keeps_the_marker(test, &admin).await;
    let fence11 = reissue_repairs_an_aborted_issuance(test, &admin).await;
    let (grouped, grouped_control) =
        grouped_ingest_provisioning_wins_before_flush(test, &admin).await;
    let (rejected, rejected_control) =
        grouped_rejection_discards_uncommitted_plain_metadata(test, &admin).await;
    let (fence5, fence6) = copy_message_race(test, &admin).await;
    copy_message_refuses_key_accounts(test, &fence1, &fence5, &fence6).await;
    let (trained, train_destination) = train_batch_loses_to_provisioning(test, &admin).await;
    let fence7 = stress_the_fence(test, &admin).await;
    let fence8 = pg_and_mysql_are_refused(test, &admin).await;
    let fence9 = setup_refuses_existing_mail(test, &admin).await;
    let pending_unindex = setup_refuses_a_pending_unindex(test, &admin).await;
    let sampled = setup_refuses_spam_samples(test, &admin).await;
    let orphaned = setup_refuses_an_orphaned_search_document(test, &admin).await;
    let in_flight = setup_refuses_an_in_flight_index_write(test, &admin).await;
    let in_flight_event = setup_refuses_an_in_flight_calendar_index_write(test, &admin).await;
    let failed_merge = setup_refuses_a_failed_merge_task(test, &admin).await;
    let (superseded, superseded_control) =
        superseded_index_write_loses_to_provisioning(test, &admin).await;
    let orphaned_terms = setup_refuses_orphaned_search_terms(test, &admin).await;
    let chunked = unindex_between_index_chunks_leaves_no_terms(test, &admin).await;
    let chunked_event = unindex_between_calendar_index_chunks_leaves_no_terms(test, &admin).await;
    let (superseded_tail, superseded_tail_control) =
        superseded_index_tail_loses_to_provisioning(test, &admin).await;
    // Retired at 1.0 with R62's guard and R63's confirmation read (Task 1's
    // ruling 19, part 6a): `failed_guard_with_its_document_stored_defers_the_task`
    // and `stopped_document_leaves_its_batch_written`.
    let merge_unindexed = unindex_between_calendar_merge_chunks_leaves_no_terms(test, &admin).await;
    let merge_control = multi_chunk_calendar_merge_is_written_whole(test, &admin).await;
    let (object_metadata, object_metadata_owner) =
        setup_refuses_object_metadata(test, &admin).await;
    let metadata_writer = metadata_writes_lose_to_provisioning(test).await;

    // The accounts that log in empty their own mailboxes; the others are
    // pending or credentialless, and the account's destruction removes
    // what they hold.
    for account in [
        &fence1,
        &fence2,
        &plain,
        &fence10,
        &organizer,
        &fence3,
        &fence4,
        &fence11,
        &grouped,
        &grouped_control,
        &rejected,
        &rejected_control,
        &fence5,
        &fence6,
    ] {
        test.destroy_all_mailboxes(account).await;
    }
    for account in [
        fence1,
        fence2,
        plain,
        fence10,
        organizer,
        fence12,
        fence3,
        fence4,
        fence11,
        grouped,
        grouped_control,
        rejected,
        rejected_control,
        fence5,
        fence6,
        trained,
        train_destination,
        fence7,
        fence8,
        fence9,
        pending_unindex,
        sampled,
        orphaned,
        in_flight,
        in_flight_event,
        failed_merge,
        superseded,
        superseded_control,
        orphaned_terms,
        chunked,
        chunked_event,
        superseded_tail,
        superseded_tail_control,
        merge_unindexed,
        merge_control,
        object_metadata,
        object_metadata_owner,
        metadata_writer,
    ] {
        admin.destroy_account(account).await;
    }
    // The grouped ingests train as IMAP APPEND does: the ordinary
    // controls' samples.
    admin
        .registry_destroy_all(ObjectType::SpamTrainingSample)
        .await;
    test.wait_for_tasks().await;
    unconfigure(&admin, schedule_id).await;
    wait_for_delivery(test).await;
}

async fn passwordless(admin: &Account, name: &'static str) -> Account {
    admin
        .create_passwordless_user_account(name, STRONG, "Fence", &[], user_permissions())
        .await
}

/// Waits until the queue holds a message for `address` whose last attempt
/// failed temporarily.
async fn wait_for_temporary_failure(test: &TestServer, address: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !temporarily_failed(test, address).await {
        assert!(
            Instant::now() < deadline,
            "no temporary failure queued for {address}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The first attempt of a delivery to `address` whose ingest held a stale
/// ordinary classification, with `traced` started on
/// `Store(AssertValueFailed)` before it was released: waits until it ended
/// (a temporary failure in the queue, or a stored message), then asserts
/// that the account holds no plaintext and that the attempt lost the fence
/// (a delivery's error names its recipient). The queue may already have
/// retried by then: a retry falls due a second after the attempt started,
/// so a provisioning slower than that makes it immediate.
async fn assert_first_attempt_lost(
    test: &TestServer,
    traced: Traced,
    address: &str,
    account_id: u32,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !temporarily_failed(test, address).await && message_ids(test, account_id).await.is_empty()
    {
        assert!(
            Instant::now() < deadline,
            "the first delivery attempt to {address} did not end"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_no_plaintext(test, account_id).await;
    let events = traced.finish().await;
    assert!(
        events
            .iter()
            .any(|e| e.typ == EventType::Store(StoreEvent::AssertValueFailed) && e.to == address),
        "the stale delivery to {address} did not lose the fence: {events:?}"
    );
}

/// The account's registry object and its user account, read directly.
async fn registry_user(server: &Server, id: Id) -> (Object, structs::UserAccount) {
    let object = server
        .registry()
        .get(ObjectId::new(ObjectType::Account, id))
        .await
        .unwrap()
        .unwrap();
    let ObjectInner::Account(structs::Account::User(user)) = &object.inner else {
        panic!("not a user account");
    };
    let user = user.clone();
    (object, user)
}

/// Adds or removes the marker credential directly in the registry, as
/// `setup-token` writes it, then invalidates the account's caches. Removing
/// it leaves the account without any credential.
pub(super) async fn set_marker(server: &Server, id: Id, marker: bool) {
    let (object, mut user) = registry_user(server, id).await;
    if marker {
        let credential_id = Id::from(user.next_credential_id());
        user.credentials
            .push(Credential::Password(PasswordCredential {
                credential_id,
                secret: ZA_MARKER.to_string(),
                ..Default::default()
            }));
    } else {
        user.credentials = Default::default();
    }
    let old_object = Object::with_revision(object.inner, object.revision);
    let result = server
        .registry()
        .write(RegistryWrite::Update {
            object: &Object::new(ObjectInner::Account(structs::Account::User(user))),
            id,
            old_object: &old_object,
        })
        .await
        .unwrap();
    assert!(
        matches!(result, RegistryWriteResult::Success(_)),
        "the marker write was rejected"
    );
    server
        .za_invalidate_account(id.document_id())
        .await
        .unwrap();
}

/// The hashes the account holds temporary blob links to.
async fn temporary_links(test: &TestServer, account_id: u32) -> Vec<BlobHash> {
    let from_key = ValueKey {
        account_id: 0,
        collection: 0,
        document_id: 0,
        class: ValueClass::Blob(BlobOp::Commit {
            hash: BlobHash::default(),
        }),
    };
    let to_key = ValueKey {
        account_id: u32::MAX,
        collection: u8::MAX,
        document_id: u32::MAX,
        class: ValueClass::Blob(BlobOp::Link {
            hash: BlobHash::new_max(),
            to: BlobLink::Document,
        }),
    };
    let mut hashes = Vec::new();
    test.server
        .store()
        .iterate(
            IterateParams::new(from_key, to_key).ascending().no_values(),
            |key, _| {
                // hash | account id | until
                if key.len() == BLOB_HASH_LEN + U32_LEN + U64_LEN
                    && key.deserialize_be_u32(BLOB_HASH_LEN)? == account_id
                {
                    hashes.push(BlobHash::try_from_hash_slice(&key[..BLOB_HASH_LEN]).unwrap());
                }
                Ok(true)
            },
        )
        .await
        .unwrap();
    hashes
}

/// `message` sent by `from` through the suite's authenticated submission
/// listener, so local delivery sees an authenticated sender.
async fn submit(from: &str, to: &str, message: &str) {
    let mut conn = Conn::open_smtp(SUBMISSION_PORT).await;
    conn.send("EHLO fence.example.com\r\n").await;
    conn.until(&["250 "]).await;
    let response = STANDARD.encode(format!("\0{from}\0{STRONG}"));
    conn.exchange(&format!("AUTH PLAIN {response}\r\n"), "235")
        .await;
    conn.exchange(&format!("MAIL FROM:<{from}>\r\n"), "250")
        .await;
    conn.exchange(&format!("RCPT TO:<{to}>\r\n"), "250").await;
    conn.exchange("DATA\r\n", "354").await;
    conn.exchange(&format!("{message}\r\n.\r\n"), "250").await;
    conn.send("QUIT\r\n").await;
}

/// The queued message for `address`, as local delivery reads it.
async fn queued_blob(test: &TestServer, address: &str) -> Vec<u8> {
    let hash = test
        .read_queued_messages()
        .await
        .into_iter()
        .find(|m| m.message.recipients.iter().any(|r| &*r.address == address))
        .unwrap_or_else(|| panic!("nothing queued for {address}"))
        .message
        .blob_hash;
    stored_blob(test, &hash).await
}

/// The purge as the scheduler runs it, once every temporary link expired.
async fn expire_and_purge_blobs(test: &TestServer, admin: &Account) {
    test.blob_expire_all().await;
    admin
        .registry_create_object(Task::StoreMaintenance(TaskStoreMaintenance {
            maintenance_type: TaskStoreMaintenanceType::PurgeBlob,
            shard_index: None,
            status: TaskStatus::now(),
        }))
        .await;
    test.wait_for_tasks().await;
}

/// The blob hash of the account's message `document_id`.
async fn message_hash(test: &TestServer, account_id: u32, document_id: u32) -> BlobHash {
    stored_hash(test, account_id, document_id).await
}

/// Review F1: a delivery that classified the account as ordinary before
/// `setup-token` and commits after it loses its vault-record assertion; the
/// queue's retry takes the key-account path and stores the message sealed.
async fn forward_race_delivery_loses(test: &TestServer, admin: &Account) -> Account {
    println!("forward_race_delivery_loses");
    let account = passwordless(admin, FENCE1).await;
    let id = account.id().document_id();
    let message = message_with("race", "race-body");
    let parked = HookParked::start(id, test_hooks::set_ingest_pause, async move {
        SmtpConnection::connect()
            .await
            .ingest(SENDER, &[FENCE1], &message)
            .await;
    })
    .await;
    // The ingest holds the ordinary classification it read.
    let token = za_setup_token(admin, FENCE1).await;
    za_setup(FENCE1, &token, STRONG).await;
    let traced = Traced::start(&[EventType::Store(StoreEvent::AssertValueFailed)]);
    parked.finish().await;

    assert_first_attempt_lost(test, traced, FENCE1, id).await;
    wait_for_delivery(test).await;
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "the queue's retry delivered the message");
    assert_sealed(test, id, ids[0], WRAP_PK).await;
    assert_no_plaintext(test, id).await;
    account
}

/// Review F1b: a delivery Sieve modified stages its blob before the batch.
/// When the batch then loses the assertion, the staged blob is left to its
/// temporary link, never deleted: blobs are content-addressed, so the plain
/// account that linked the identical bytes while the delivery was parked
/// keeps them through the purge, which removes only the key account's
/// expired link. The plain account imports (JMAP `Email/import`) the bytes
/// the parked ingest is about to stage: the queued message with the
/// script's header first, where sieve-rs's `addheader` puts it.
async fn forward_race_with_a_staged_blob(test: &TestServer, admin: &Account) -> (Account, Account) {
    println!("forward_race_with_a_staged_blob");
    let account = passwordless(admin, FENCE2).await;
    let id = account.id().document_id();
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    let plain_id = plain.id().document_id();
    let mut client = admin.jmap_client().await;
    client.set_default_account_id(Id::from(id));
    client
        .sieve_script_create("fence", SCRIPT, true)
        .await
        .unwrap();

    let message = message_with("staged", "staged-body");
    let parked = HookParked::start(id, test_hooks::set_ingest_pause, async move {
        SmtpConnection::connect()
            .await
            .ingest(SENDER, &[FENCE2], &message)
            .await;
    })
    .await;
    let staged_bytes = [
        b"X-Sieve-Fence: 1\r\n".as_slice(),
        &queued_blob(test, FENCE2).await,
    ]
    .concat();
    let staged = BlobHash::generate(&staged_bytes);
    plain
        .jmap_client()
        .await
        .email_import(
            staged_bytes.clone(),
            [Id::from(INBOX_ID).to_string()],
            None::<Vec<&str>>,
            None,
        )
        .await
        .unwrap();
    let plain_ids = message_ids(test, plain_id).await;
    assert_eq!(plain_ids.len(), 1, "the plain account's import");
    assert_eq!(
        message_hash(test, plain_id, plain_ids[0]).await,
        staged,
        "the import links the uploaded bytes"
    );
    let token = za_setup_token(admin, FENCE2).await;
    za_setup(FENCE2, &token, STRONG).await;
    let traced = Traced::start(&[EventType::Store(StoreEvent::AssertValueFailed)]);
    parked.finish().await;

    assert_first_attempt_lost(test, traced, FENCE2, id).await;
    assert!(
        temporary_links(test, id).await.contains(&staged),
        "the lost ingest did not stage the bytes the plain account linked"
    );
    let plain_link = BlobClass::Linked {
        account_id: plain_id,
        collection: Collection::Email.into(),
        document_id: plain_ids[0],
    };
    let linked_and_readable = async || {
        assert!(
            test.server
                .store()
                .blob_link_size(&staged, &plain_link)
                .await
                .unwrap()
                .is_some(),
            "the plain account no longer links the staged hash"
        );
        assert_eq!(
            stored_blob(test, &staged).await,
            staged_bytes,
            "the plain account's copy"
        );
    };
    linked_and_readable().await;

    wait_for_delivery(test).await;
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "the queue's retry delivered the message");
    assert_sealed(test, id, ids[0], WRAP_PK).await;
    assert_ne!(
        message_hash(test, id, ids[0]).await,
        staged,
        "the sealed retry wrote a blob of its own"
    );
    assert_no_plaintext(test, id).await;

    expire_and_purge_blobs(test, admin).await;
    linked_and_readable().await;
    assert!(
        temporary_links(test, id).await.is_empty(),
        "a temporary link of the key account survived the purge"
    );
    (account, plain)
}

/// Ruling R25 of plan 13: iMIP's side-write. A delivery whose ingest read
/// the account as ordinary runs upstream's iMIP branch on that reading, and
/// `itip_ingest` commits the event and its inbox item in a batch of its
/// own; that batch takes the fence too, so provisioning in between leaves
/// no plaintext calendar data. The invitation comes from a local organizer
/// through authenticated submission, as upstream's iMIP gate requires (an
/// authenticated sender; a local principal's invitation is auto-added).
async fn forward_race_imip_side_write(test: &TestServer, admin: &Account) -> (Account, Account) {
    println!("forward_race_imip_side_write");
    let account = passwordless(admin, FENCE10).await;
    let id = account.id().document_id();
    let organizer = admin
        .create_user_account(ORGANIZER, STRONG, "Organizer", &[], user_permissions())
        .await;
    let invitation = invitation(FENCE10, "za-fence-imip", 0, "fence-imip-canary");
    let parked = HookParked::start(id, test_hooks::set_ingest_pause, async move {
        submit(ORGANIZER, FENCE10, &invitation).await;
    })
    .await;
    let token = za_setup_token(admin, FENCE10).await;
    za_setup(FENCE10, &token, STRONG).await;
    let traced = Traced::start(&[EventType::Store(StoreEvent::AssertValueFailed)]);
    parked.finish().await;

    assert_first_attempt_lost(test, traced, FENCE10, id).await;
    assert_no_calendar_data(test, id, "the stale delivery's iMIP processing").await;
    wait_for_delivery(test).await;
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "the queue's retry delivered the message");
    assert_sealed(test, id, ids[0], WRAP_PK).await;
    assert_no_plaintext(test, id).await;
    assert_no_calendar_data(test, id, "the sealed retry").await;
    (account, organizer)
}

/// The account holds no calendar event and no scheduling inbox item.
async fn assert_no_calendar_data(test: &TestServer, account_id: u32, after: &str) {
    for collection in [
        Collection::CalendarEvent,
        Collection::CalendarEventNotification,
    ] {
        assert!(
            !test
                .server
                .za_has_documents(account_id, collection)
                .await
                .unwrap(),
            "{after} stored a {collection:?} document"
        );
    }
}

/// An iMIP REQUEST from `ORGANIZER` inviting `to` to the event `uid` at
/// `sequence`, with a unique `Message-ID`.
fn invitation(to: &str, uid: &str, sequence: u32, summary: &str) -> String {
    format!(
        "From: <{ORGANIZER}>\r\n\
         To: <{to}>\r\n\
         Subject: fence invitation\r\n\
         Message-ID: <{:016x}.za-fence@example.com>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/calendar; method=REQUEST; charset=utf-8\r\n\
         \r\n\
         BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//za//fence//EN\r\n\
         METHOD:REQUEST\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         SEQUENCE:{sequence}\r\n\
         DTSTAMP:20261020T100000Z\r\n\
         DTSTART:20261120T100000Z\r\n\
         DTEND:20261120T110000Z\r\n\
         SUMMARY:{summary}\r\n\
         ORGANIZER:mailto:{ORGANIZER}\r\n\
         ATTENDEE;PARTSTAT=NEEDS-ACTION;RSVP=TRUE:mailto:{to}\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR",
        store::rand::random::<u64>()
    )
}

/// The account's archives in `collection`, by document id, as stored.
async fn archives(
    test: &TestServer,
    account_id: u32,
    collection: Collection,
) -> Vec<(u32, Vec<u8>)> {
    let mut archives = Vec::new();
    test.server
        .store()
        .iterate(
            IterateParams::new(
                ValueKey::archive(account_id, collection, 0),
                ValueKey::archive(account_id, collection, u32::MAX),
            ),
            |key, value| {
                archives.push((key.deserialize_be_u32(key.len() - U32_LEN)?, value.to_vec()));
                Ok(true)
            },
        )
        .await
        .unwrap();
    archives
}

/// Ruling R25 of plan 13, the merge side: an iMIP update for an event the
/// account already holds goes through `commit_itip_merge`, whose batch takes
/// the fence too. `setup-token` refuses an account holding calendar data,
/// so the race's record is written directly, as an issuance writes it
/// (`za_vault_commit` of a pending record): the stale attempt leaves the
/// stored event and the scheduling inbox untouched, and once the record is
/// gone the queue's retry merges the update (the control that the update
/// takes the merge path).
async fn forward_race_imip_merge(test: &TestServer, admin: &Account) -> Account {
    println!("forward_race_imip_merge");
    let account = passwordless(admin, FENCE12).await;
    let id = account.id().document_id();
    submit(
        ORGANIZER,
        FENCE12,
        &invitation(FENCE12, "za-fence-merge", 0, "fence merge v1"),
    )
    .await;
    wait_for_delivery(test).await;
    let events = archives(test, id, Collection::CalendarEvent).await;
    assert_eq!(events.len(), 1, "the first REQUEST is imported");
    let items = archives(test, id, Collection::CalendarEventNotification).await;
    let mails = message_ids(test, id).await;

    let update = invitation(FENCE12, "za-fence-merge", 1, "fence merge v2");
    let parked = HookParked::start(id, test_hooks::set_ingest_pause, async move {
        submit(ORGANIZER, FENCE12, &update).await;
    })
    .await;
    let mut record = VaultRecord::pending(vec![3; 32], now() as i64 + 3600);
    test.server
        .za_vault_commit(id, &mut record, None, None)
        .await
        .unwrap();
    parked.finish().await;

    wait_for_temporary_failure(test, FENCE12).await;
    assert_eq!(
        archives(test, id, Collection::CalendarEvent).await,
        events,
        "the stale merge changed the stored event"
    );
    assert_eq!(
        archives(test, id, Collection::CalendarEventNotification).await,
        items,
        "the stale merge wrote a scheduling inbox item"
    );
    assert_eq!(
        message_ids(test, id).await,
        mails,
        "the stale delivery stored"
    );

    let read = test.server.za_vault_record(id).await.unwrap().unwrap();
    test.server.za_vault_delete(id, &read).await.unwrap();
    wait_for_delivery(test).await;
    let merged = archives(test, id, Collection::CalendarEvent).await;
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].0, events[0].0, "the update merges into the event");
    assert_ne!(
        merged[0].1, events[0].1,
        "control: the retry merged the update"
    );
    account
}

/// Ruling R28 of plan 13: an issuance that stopped between its record write
/// and its recheck leaves a record without a marker; once mail arrived
/// before it, every ordinary writer loses the fence. The reissue's first
/// check finds the mail and removes that record as the recheck would have,
/// answering the same 409, so later deliveries are stored again.
async fn reissue_repairs_an_aborted_issuance(test: &TestServer, admin: &Account) -> Account {
    println!("reissue_repairs_an_aborted_issuance");
    let account = passwordless(admin, FENCE11).await;
    let id = account.id().document_id();
    deliver(
        test,
        SENDER,
        FENCE11,
        &message_with("before", "before-body"),
    )
    .await;
    // The crash state: the pending record, no marker.
    let mut record = VaultRecord::pending(vec![4; 32], now() as i64 + 3600);
    test.server
        .za_vault_commit(id, &mut record, None, None)
        .await
        .unwrap();

    let reply = za_post_as(admin, "setup-token", &json!({ "account": FENCE11 })).await;
    assert_eq!(reply.expect(409)["error"], "account already holds mail");
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the reissue left the aborted issuance's record"
    );
    let before = message_ids(test, id).await;
    deliver(test, SENDER, FENCE11, &message_with("after", "after-body")).await;
    let after = super::new_message(test, id, &before).await;
    assert!(
        !row_is_stub(&stored_metadata(test, id, after).await),
        "the ordinary account's delivery is plaintext"
    );

    admin.destroy_all_mailboxes_for_account(id).await;
    test.wait_for_tasks().await;
    let token = za_setup_token(admin, FENCE11).await;
    za_setup(FENCE11, &token, STRONG).await;
    account
}

/// Review F1c: a delivery that commits while `setup-token` sits between its
/// emptiness check and its record write is seen by the recheck, which
/// removes the pending record it wrote and answers the first check's 409;
/// no marker is written, so the account stays ordinary and can be issued a
/// token once emptied.
async fn reverse_race_setup_token_aborts(test: &TestServer, admin: &Account) -> Account {
    println!("reverse_race_setup_token_aborts");
    let account = passwordless(admin, FENCE3).await;
    let id = account.id();
    let caller = admin.clone();
    let parked = HookParked::start(
        id.document_id(),
        test_hooks::set_setup_token_pause,
        async move { za_post_as(&caller, "setup-token", &json!({ "account": FENCE3 })).await },
    )
    .await;
    // No record yet: the delivery's assertion rightly succeeds.
    deliver(
        test,
        SENDER,
        FENCE3,
        &message_with("reverse", "reverse-body"),
    )
    .await;
    let reply = parked.finish().await.expect(409);
    assert_eq!(reply["error"], "account already holds mail");
    assert!(
        test.server
            .za_vault_record(id.document_id())
            .await
            .unwrap()
            .is_none(),
        "the aborted issuance left its pending record"
    );
    assert!(
        registry_user(&test.server, id)
            .await
            .1
            .credentials
            .is_empty(),
        "the aborted issuance wrote the marker"
    );

    admin
        .destroy_all_mailboxes_for_account(id.document_id())
        .await;
    test.wait_for_tasks().await;
    let token = za_setup_token(admin, FENCE3).await;
    za_setup(FENCE3, &token, STRONG).await;
    account
}

/// Ruling R9 of plan 13: an account holding a marker and a pending record
/// (a pending account whose first token expired, reissued) is a key
/// account, so a delivery takes the key path and fails temporarily without
/// a served public key (that failure, traced, not the fence's); nothing is
/// stored, the reissue's recheck finds no mail, answers 200 and keeps the
/// marker, and setup lets the retry deliver.
async fn reverse_race_reissue_keeps_the_marker(test: &TestServer, admin: &Account) -> Account {
    println!("reverse_race_reissue_keeps_the_marker");
    let account = passwordless(admin, FENCE4).await;
    let id = account.id();
    za_setup_token(admin, FENCE4).await;
    za_setup_token(admin, FENCE4).await;

    let traced = Traced::start(&[
        EventType::Store(StoreEvent::UnexpectedError),
        EventType::Store(StoreEvent::AssertValueFailed),
    ]);
    SmtpConnection::connect()
        .await
        .ingest(SENDER, &[FENCE4], &message_with("pending", "pending-body"))
        .await;
    wait_for_temporary_failure(test, FENCE4).await;
    // This account's events: its id, or its address as a delivery's error
    // names the recipient (a store error carries no account id).
    let failures = traced
        .finish()
        .await
        .into_iter()
        .filter(|e| e.account_id == Some(u64::from(id.document_id())) || e.to == FENCE4)
        .collect::<Vec<_>>();
    assert!(
        failures.iter().any(|e| {
            e.typ == EventType::Store(StoreEvent::UnexpectedError) && e.reason == NO_SERVED_KEY
        }),
        "no no-served-key failure traced for the delivery: {failures:?}"
    );
    assert!(
        !failures
            .iter()
            .any(|e| e.typ == EventType::Store(StoreEvent::AssertValueFailed)),
        "the fence refused the delivery: {failures:?}"
    );
    assert!(
        message_ids(test, id.document_id()).await.is_empty(),
        "nothing is stored for a pending account"
    );
    let token = za_setup_token(admin, FENCE4).await;
    assert_eq!(
        registry_user(&test.server, id)
            .await
            .1
            .password_credential()
            .map(|c| c.secret.as_str()),
        Some(ZA_MARKER),
        "the reissue keeps the marker"
    );

    za_setup(FENCE4, &token, STRONG).await;
    wait_for_delivery(test).await;
    let ids = message_ids(test, id.document_id()).await;
    assert_eq!(ids.len(), 1, "the queue's retry delivered the message");
    assert_sealed(test, id.document_id(), ids[0], WRAP_PK).await;
    account
}

/// Task 5 of the 1.0 upgrade: a grouped ingest (`IngestGroup`, IMAP
/// APPEND's and JMAP's) classifies each message when it is ingested into
/// the group and writes nothing until the flush. An account provisioned in
/// between (its `setup-token` and setup complete: the group's messages are
/// in memory only, so the emptiness check and the recheck see nothing)
/// wins: every message the group classified as ordinary carries the
/// provisioning fence in its own commit chunk, so the flush fails its
/// assertion and commits nothing, no message, metadata row, threading key
/// or sample. The plaintext the group staged is left to its temporary
/// links (spec 6.1's allowed residue), which the purge removes. The client's
/// retry, now with the account's keys, stores the same messages sealed.
/// The control, an ordinary account's identical group, flushes in the
/// clear, threaded.
async fn grouped_ingest_provisioning_wins_before_flush(
    test: &TestServer,
    admin: &Account,
) -> (Account, Account) {
    println!("grouped_ingest_provisioning_wins_before_flush");
    let account = passwordless(admin, GROUPED).await;
    let id = account.id().document_id();
    let control = admin
        .create_user_account(GROUPED_CONTROL, STRONG, "Fence", &[], user_permissions())
        .await;
    let control_id = control.id().document_id();
    // The system folders, so INBOX exists for both.
    message_ids(test, id).await;
    message_ids(test, control_id).await;
    let first = message_with("grouped fence", "grouped-first-canary");
    let reply = reply_to(&first, "grouped fence", "grouped-reply-canary");
    let messages = [&first, &reply];

    // The control: the same group for an ordinary account commits both,
    // in the clear, the reply in its parent's pending thread.
    let mut group = IngestGroup::default();
    for raw in messages {
        group_ingest(&test.server, &mut group, &keyless_token(control_id), raw)
            .await
            .unwrap();
    }
    let controls = group.flush(&test.server).await.unwrap();
    assert_eq!(controls.len(), 2, "control: the ordinary group's flush");
    for email in &controls {
        assert!(
            !row_is_stub(&stored_metadata(test, control_id, email.document_id).await),
            "control: the ordinary group stored a stub"
        );
        assert_eq!(
            super::threading_keys(test, control_id, email.document_id).await,
            1,
            "control: the ordinary group's message is threaded"
        );
    }
    assert_eq!(controls[1].thread_id, controls[0].thread_id);

    // The race: classified as ordinary, then provisioned before the flush.
    let stale = keyless_token(id);
    let mut group = IngestGroup::default();
    for raw in messages {
        group_ingest(&test.server, &mut group, &stale, raw)
            .await
            .unwrap();
    }
    let token = za_setup_token(admin, GROUPED).await;
    za_setup(GROUPED, &token, STRONG).await;
    let err = match group.flush(&test.server).await {
        Err(err) => err,
        Ok(ingested) => panic!(
            "the stale group committed {} messages after provisioning",
            ingested.len()
        ),
    };
    assert!(err.is_assertion_failure(), "the flush's failure: {err:?}");
    assert!(
        message_ids(test, id).await.is_empty(),
        "the stale group stored a message"
    );
    assert!(
        metadata_rows(test, id).await.is_empty(),
        "the stale group left a metadata row"
    );
    assert!(
        !test.server.za_has_spam_samples(id).await.unwrap(),
        "the stale group stored a sample"
    );
    assert!(
        group.flush(&test.server).await.unwrap().is_empty(),
        "the failed flush kept its batch"
    );
    let staged = temporary_links(test, id).await;
    for raw in messages {
        assert!(
            staged.contains(&BlobHash::generate(raw.as_bytes())),
            "the stale group did not stage its plaintext"
        );
    }

    // The retry, with the session's keys: sealed under them.
    let keyed = keyed_token(test, id, STRONG).await;
    let mut group = IngestGroup::default();
    for raw in messages {
        group_ingest(&test.server, &mut group, &keyed, raw)
            .await
            .unwrap();
    }
    let ingested = group.flush(&test.server).await.unwrap();
    assert_eq!(ingested.len(), 2, "the keyed retry's flush");
    let mut documents = Vec::new();
    for (email, raw) in ingested.iter().zip(messages) {
        assert_sealed(test, id, email.document_id, WRAP_MK).await;
        assert_eq!(email.thread_id, email.document_id, "a thread of its own");
        let (_, opened) = open_message(test, id, STRONG, email.document_id).await;
        assert_eq!(opened, raw.as_bytes(), "the retried message");
        documents.push(email.document_id);
    }
    documents.sort_unstable();
    assert_eq!(message_ids(test, id).await, documents);
    assert_eq!(
        metadata_rows(test, id).await.len(),
        2,
        "a metadata row without its message"
    );
    assert_no_plaintext(test, id).await;

    expire_and_purge_blobs(test, admin).await;
    let staged = temporary_links(test, id).await;
    for raw in messages {
        assert!(
            !staged.contains(&BlobHash::generate(raw.as_bytes())),
            "the stale group's staged plaintext survived the purge"
        );
    }
    (account, control)
}

/// Task 5 of the 1.0 upgrade: a group discards its batch after an error
/// that is not a rejection (`IngestGroup::ingest`), so a message refused in
/// mid-group leaves none of the plaintext metadata the group already held
/// for an earlier one. The account is classified as ordinary for the first
/// message, then provisioned; the second, ingested with the same keyless
/// token, is now a key account's and is refused (`za_keyless_refusal`,
/// before any operation of the batch). The flush then commits nothing, and
/// the client's retry with the account's keys stores both sealed, with no
/// metadata row left over. The control, an ordinary account: a message that
/// does not parse is a rejection, after which a group keeps its batch, and
/// the rejected message adds nothing to it; its retry, parsed, adds one
/// message and one row.
async fn grouped_rejection_discards_uncommitted_plain_metadata(
    test: &TestServer,
    admin: &Account,
) -> (Account, Account) {
    println!("grouped_rejection_discards_uncommitted_plain_metadata");
    let account = passwordless(admin, REJECTED).await;
    let id = account.id().document_id();
    let control = admin
        .create_user_account(REJECTED_CONTROL, STRONG, "Fence", &[], user_permissions())
        .await;
    let control_id = control.id().document_id();
    message_ids(test, id).await;
    message_ids(test, control_id).await;
    let first = message_with("grouped refusal", "refusal-first-canary");
    let second = message_with("grouped refusal second", "refusal-second-canary");

    let stale = keyless_token(id);
    let mut group = IngestGroup::default();
    group_ingest(&test.server, &mut group, &stale, &first)
        .await
        .unwrap();
    let token = za_setup_token(admin, REJECTED).await;
    za_setup(REJECTED, &token, STRONG).await;
    let err = group_ingest(&test.server, &mut group, &stale, &second)
        .await
        .expect_err("a keyless ingest into a key account");
    assert!(
        err.matches(EventType::Jmap(trc::JmapEvent::Forbidden))
            && err.value_as_str(Key::Details) == Some(ZA_MAIL_KEYLESS),
        "the refusal: {err:?}"
    );
    assert!(
        group.flush(&test.server).await.unwrap().is_empty(),
        "the refusal left the first message in the group's batch"
    );
    assert!(
        message_ids(test, id).await.is_empty() && metadata_rows(test, id).await.is_empty(),
        "the discarded group stored a message or a metadata row"
    );

    let keyed = keyed_token(test, id, STRONG).await;
    let mut group = IngestGroup::default();
    for raw in [&first, &second] {
        group_ingest(&test.server, &mut group, &keyed, raw)
            .await
            .unwrap();
    }
    let ingested = group.flush(&test.server).await.unwrap();
    assert_eq!(ingested.len(), 2, "the keyed retry's flush");
    for (email, raw) in ingested.iter().zip([&first, &second]) {
        assert_sealed(test, id, email.document_id, WRAP_MK).await;
        let (_, opened) = open_message(test, id, STRONG, email.document_id).await;
        assert_eq!(opened, raw.as_bytes(), "the retried message");
    }
    let rows = metadata_rows(test, id).await;
    let mut documents = ingested.iter().map(|e| e.document_id).collect::<Vec<_>>();
    documents.sort_unstable();
    assert_eq!(
        rows.iter()
            .map(|(document_id, _)| *document_id)
            .collect::<Vec<_>>(),
        documents,
        "the account's metadata rows are the retry's messages"
    );
    assert_eq!(message_ids(test, id).await, documents);
    assert_no_plaintext(test, id).await;

    // The control: a rejection keeps the batch, and adds nothing to it.
    let ordinary = keyless_token(control_id);
    let mut group = IngestGroup::default();
    group_ingest(&test.server, &mut group, &ordinary, &first)
        .await
        .unwrap();
    let err = group
        .ingest(
            &test.server,
            append_params(&second, None, &ordinary),
            ThreadKey::default(),
        )
        .await
        .expect_err("an unparsed message is rejected");
    assert!(
        err.matches(EventType::MessageIngest(trc::MessageIngestEvent::Error)),
        "the rejection: {err:?}"
    );
    let kept = group.flush(&test.server).await.unwrap();
    assert_eq!(kept.len(), 1, "control: the rejection discarded the batch");
    let rows = metadata_rows(test, control_id).await;
    assert_eq!(
        rows.iter()
            .map(|(document_id, _)| *document_id)
            .collect::<Vec<_>>(),
        [kept[0].document_id],
        "the rejected message left a metadata row"
    );
    assert!(!row_is_stub(&rows[0].1), "control: an ordinary message");
    let mut group = IngestGroup::default();
    group_ingest(&test.server, &mut group, &ordinary, &second)
        .await
        .unwrap();
    let retried = group.flush(&test.server).await.unwrap();
    assert_eq!(retried.len(), 1, "the control's retry");
    let mut documents = vec![kept[0].document_id, retried[0].document_id];
    documents.sort_unstable();
    assert_eq!(
        metadata_rows(test, control_id)
            .await
            .iter()
            .map(|(document_id, _)| *document_id)
            .collect::<Vec<_>>(),
        documents
    );
    assert_eq!(message_ids(test, control_id).await, documents);
    (account, control)
}

/// Mail spec 6.3 and ruling R3 of plan 13: a cross-account copy that read
/// its destination as ordinary before the destination was provisioned loses
/// its assertion at commit, which surfaces as a method-level server error,
/// and stores nothing.
async fn copy_message_race(test: &TestServer, admin: &Account) -> (Account, Account) {
    println!("copy_message_race");
    let from = admin
        .create_user_account(FENCE5, STRONG, "Fence", &[], user_permissions())
        .await;
    let from_id = from.id().document_id();
    deliver(test, SENDER, FENCE5, &message_with("copy", "copy-body")).await;
    let to = passwordless(admin, FENCE6).await;
    let to_id = to.id().document_id();
    // The destination's system folders, so the copy has an inbox to name.
    message_ids(test, to_id).await;
    let email_id = {
        let cache = test.server.get_cached_messages(from_id).await.unwrap();
        let item = cache.emails.iter().next().unwrap();
        Id::from_parts(item.thread_id(), item.document_id()).to_string()
    };

    let copy = json!({
        "fromAccountId": Id::from(from_id).to_string(),
        "accountId": Id::from(to_id).to_string(),
        "create": {
            "c1": {
                "id": email_id,
                "mailboxIds": { Id::from(INBOX_ID).to_string(): true }
            }
        }
    });
    let caller = admin.clone();
    let parked_copy = copy.clone();
    let parked = HookParked::start(to_id, test_hooks::set_copy_pause, async move {
        caller.jmap_method_call("Email/copy", parked_copy).await
    })
    .await;
    let token = za_setup_token(admin, FENCE6).await;
    za_setup(FENCE6, &token, STRONG).await;
    let response = parked.finish().await;

    let method = &response.0["methodResponses"][0];
    assert_eq!(method[0], "error", "{method}");
    assert_eq!(method[1]["type"], "serverUnavailable", "{method}");
    assert!(
        message_ids(test, to_id).await.is_empty(),
        "the stale copy stored a message"
    );
    assert_no_plaintext(test, to_id).await;

    // Ruling R3 of plan 13: the same copy retried, its destination now a key
    // account, is refused before `copy_message`: the caller's token holds no
    // keys for the destination (mail spec 4, the dispatcher's gate).
    println!("copy_message_race_status");
    let traced = Traced::start(&[EventType::Security(SecurityEvent::Unauthorized)]);
    let response = admin.jmap_method_call("Email/copy", copy).await;
    assert_eq!(
        response.0["methodResponses"][0][1],
        json!({ "type": "forbidden", "description": ZA_MAIL_KEYLESS }),
        "the retried copy: {:?}",
        response.0
    );
    let events = traced.finish().await;
    assert!(
        events
            .iter()
            .any(|e| e.details == KEYLESS_REFUSED && e.account_id == Some(u64::from(to_id))),
        "the retried copy's refusal: {events:?}"
    );
    assert!(
        events.iter().all(|e| e.details != COPY_REFUSED),
        "the retried copy reached copy_message: {events:?}"
    );
    assert!(
        message_ids(test, to_id).await.is_empty(),
        "the retried copy stored a message"
    );
    (from, to)
}

/// Mail spec 6.3: `copy_message` itself refuses a key account on either
/// side, before any read, with the error every caller writes
/// (`za_keyless_refusal`) and its own `zero-access:` trace naming the key
/// account; nothing is stored. `key` holds a sealed message, `plain` a
/// plaintext one, `empty_key` nothing.
async fn copy_message_refuses_key_accounts(
    test: &TestServer,
    key: &Account,
    plain: &Account,
    empty_key: &Account,
) {
    println!("copy_message_refuses_key_accounts");
    let key_id = key.id().document_id();
    let plain_id = plain.id().document_id();
    let empty_id = empty_key.id().document_id();
    let key_doc = message_ids(test, key_id).await[0];
    let plain_before = message_ids(test, plain_id).await;
    let traced = Traced::start(&[EventType::Security(SecurityEvent::Unauthorized)]);
    for (from, document_id, to, what) in [
        (plain_id, plain_before[0], empty_id, "into a key account"),
        (key_id, key_doc, plain_id, "out of a key account"),
    ] {
        // 1.0's `copy_message` takes the source row its caller read
        // (`CopySources`, as `Email/copy` and IMAP COPY read it) and the
        // destination's account; the refusal reads the row's account
        // (`CopySource::account_id`) and the destination, not the row.
        let source = CopySources::new(from, vec![document_id])
            .take(&test.server, document_id)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("no source row for {from}/{document_id}"));
        let to_account = test.server.account(to).await.unwrap();
        let err = match test
            .server
            .copy_message(
                source,
                &to_account,
                // The tenant budget: the parameter exists in an `enterprise`
                // build of `email`, which the tests crate always enables
                // (`tests/Cargo.toml`); the budget is upstream's AGPL type,
                // unlimited as for an account without a tenant.
                &mut common::storage::quota::DiskQuotaBudget::unlimited(),
                vec![INBOX_ID],
                vec![],
                now(),
                None,
                0,
            )
            .await
        {
            Err(err) => err,
            Ok(_) => panic!("a copy {what} was not refused"),
        };
        assert!(
            err.matches(EventType::Jmap(trc::JmapEvent::Forbidden)) && err.should_write_err(),
            "{what}: {err:?}"
        );
        assert_eq!(
            err.value_as_str(Key::Details),
            Some(ZA_MAIL_KEYLESS),
            "{what}"
        );
    }
    let mut refused = traced
        .finish()
        .await
        .into_iter()
        .filter(|e| e.details == COPY_REFUSED)
        .map(|e| e.account_id)
        .collect::<Vec<_>>();
    refused.sort_unstable();
    let mut expected = vec![Some(u64::from(empty_id)), Some(u64::from(key_id))];
    expected.sort_unstable();
    assert_eq!(refused, expected, "one trace per refused copy");
    assert!(
        message_ids(test, empty_id).await.is_empty(),
        "the copy into a key account stored a message"
    );
    assert_eq!(
        message_ids(test, plain_id).await,
        plain_before,
        "the copy out of a key account stored a message"
    );
}

/// Ruling R55 of plan 13 (PR #51 review 1, P2-2): a cross-account
/// `Email/copy` into Junk trains a sample of the source account in a batch
/// of its own, committed after the copies, with no message assertion to
/// fail. A sample whose sender and subject were read from an ordinary source
/// that is emptied and provisioned before that commit loses the provisioning
/// fence each of the batch's chunks carries: the method fails after the copy
/// committed, and no sample or temporary blob link lands under the now key
/// account.
async fn train_batch_loses_to_provisioning(
    test: &TestServer,
    admin: &Account,
) -> (Account, Account) {
    println!("train_batch_loses_to_provisioning");
    let from = passwordless(admin, TRAINED).await;
    let from_id = from.id().document_id();
    deliver(
        test,
        SENDER,
        TRAINED,
        &message_with("trained", "trained-body"),
    )
    .await;
    let to = passwordless(admin, TRAIN_DESTINATION).await;
    let to_id = to.id().document_id();
    // The destination's system folders, so the copy has a Junk to name.
    message_ids(test, to_id).await;
    let (email_id, document_id) = {
        let cache = test.server.get_cached_messages(from_id).await.unwrap();
        let item = cache.emails.iter().next().unwrap();
        (
            Id::from_parts(item.thread_id(), item.document_id()).to_string(),
            item.document_id(),
        )
    };
    let hash = message_hash(test, from_id, document_id).await;
    test.wait_for_tasks().await;

    let copy = json!({
        "fromAccountId": Id::from(from_id).to_string(),
        "accountId": Id::from(to_id).to_string(),
        "create": {
            "c1": {
                "id": email_id,
                "mailboxIds": { Id::from(JUNK_ID).to_string(): true }
            }
        }
    });
    let caller = admin.clone();
    // The copy committed; the sample's sender and subject are read.
    let parked = HookParked::start(from_id, test_hooks::set_train_pause, async move {
        caller.jmap_method_call("Email/copy", copy).await
    })
    .await;
    admin.destroy_all_mailboxes_for_account(from_id).await;
    test.wait_for_tasks().await;
    let token = za_setup_token(admin, TRAINED).await;
    za_setup(TRAINED, &token, STRONG).await;
    let response = parked.finish().await;

    let method = &response.0["methodResponses"][0];
    assert_eq!(method[0], "error", "the stale training commit: {method}");
    assert_eq!(
        message_ids(test, to_id).await.len(),
        1,
        "the copy committed before the training batch"
    );
    assert!(
        !test.server.za_has_spam_samples(from_id).await.unwrap(),
        "the stale training batch stored a sample under the key account"
    );
    assert!(
        !temporary_links(test, from_id).await.contains(&hash),
        "the stale training batch linked the plaintext blob under the key account"
    );
    (from, to)
}

/// Ruling R10 of plan 13: an ingest and a provisioning write race, many
/// times. The writer follows `setup-token`'s order (the pending record, a
/// store-level read for mail, the marker); the read is a one-sided order
/// detector: an ingest that returned `Ok` while it saw no message committed
/// after the record existed. Each run ends in one of three outcomes: the
/// ingest stored plaintext before the record (1), lost its assertion (2,
/// the record came first and its classification was stale), or took the
/// key path and failed without a served key (3, its classification fresh).
/// The start of either side is delayed by 0-7 ms, alternately. The blobs
/// the lost runs staged are linked by nobody, and the purge removes them.
async fn stress_the_fence(test: &TestServer, admin: &Account) -> Account {
    println!("stress_the_fence");
    let account = passwordless(admin, FENCE7).await;
    let id = account.id();
    let account_id = id.document_id();
    // The system folders, before the runs race on their creation.
    message_ids(test, account_id).await;
    let mut outcomes = [0usize; 3];
    let mut staged_only = Vec::new();
    for run in 0..RUNS {
        let (ingest_delay, writer_delay) = if run % 2 == 0 {
            (0, (run / 2) % 8)
        } else {
            ((run / 2) % 8, 0)
        };
        let server = test.server.clone();
        let ingest = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(ingest_delay)).await;
            let message = message_with("stress", "stress-body");
            let access_token = AccessToken::from_id_maybe_invalid(account_id);
            let result = server
                .email_ingest(IngestEmail {
                    raw_message: message.as_bytes(),
                    message: MessageParser::new().parse(message.as_bytes()),
                    blob_hash: None,
                    access_token: &access_token,
                    mailbox_ids: vec![INBOX_ID],
                    keywords: vec![],
                    received_at: None,
                    source: IngestSource::Smtp {
                        deliver_to: FENCE7,
                        envelope_from: SENDER,
                        sender_authentication: SenderAuthentication::DmarcPass,
                        is_spam: false,
                    },
                    metadata: None,
                    session_id: 0,
                })
                .await
                .map(|_| ());
            (result, BlobHash::generate(message.as_bytes()))
        });
        let server = test.server.clone();
        let writer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(writer_delay)).await;
            let mut record = VaultRecord::pending(vec![1; 32], now() as i64 + 3600);
            server
                .za_vault_commit(account_id, &mut record, None, None)
                .await
                .unwrap();
            let saw_message = server
                .za_has_documents(account_id, Collection::Email)
                .await
                .unwrap();
            set_marker(&server, id, true).await;
            saw_message
        });
        let (ingested, staged) = ingest.await.unwrap();
        let saw_message = writer.await.unwrap();

        let stored = message_ids(test, account_id).await;
        match &ingested {
            Ok(()) => {
                assert!(
                    saw_message,
                    "run {run}: the message committed after the vault record"
                );
                assert_eq!(stored.len(), 1, "run {run}");
                assert!(
                    !row_is_stub(&stored_metadata(test, account_id, stored[0]).await),
                    "run {run}: an ordinary ingest stored a stub"
                );
                outcomes[0] += 1;
            }
            Err(err) if err.is_assertion_failure() => {
                assert!(stored.is_empty(), "run {run}: a lost assertion stored");
                staged_only.push(staged);
                outcomes[1] += 1;
            }
            Err(err) if err.value_as_str(trc::Key::Reason) == Some(NO_SERVED_KEY) => {
                assert!(stored.is_empty(), "run {run}: a pending account stored");
                outcomes[2] += 1;
            }
            Err(err) => panic!("run {run}: unexpected ingest failure: {err:?}"),
        }

        // Back to an ordinary account without mail.
        set_marker(&test.server, id, false).await;
        let read = test.server.za_vault_record(account_id).await.unwrap();
        test.server
            .za_vault_delete(account_id, &read.unwrap())
            .await
            .unwrap();
        if !stored.is_empty() {
            let mut batch = BatchBuilder::new();
            test.server
                .emails_delete(
                    account_id,
                    ChangedBy::PrincipalId(account_id),
                    &mut batch,
                    RoaringBitmap::from_iter(stored),
                )
                .await
                .unwrap();
            test.server.commit_batch(batch).await.unwrap();
            // A direct write does not wake the task manager.
            test.server.notify_task_queue();
        }
        test.wait_for_tasks().await;
    }
    println!(
        "stress_the_fence: {RUNS} runs: {} stored before the record, {} lost the assertion, {} took the key path",
        outcomes[0], outcomes[1], outcomes[2]
    );
    if outcomes[1] == 0 {
        println!("stress_the_fence: note: contention never produced a lost assertion");
    }

    let readable = async |hash: &BlobHash| {
        test.server
            .blob_store()
            .get_blob(hash.as_slice(), 0..usize::MAX)
            .await
            .unwrap()
            .is_some()
    };
    for hash in &staged_only {
        assert!(readable(hash).await, "a lost run staged no blob");
    }
    expire_and_purge_blobs(test, admin).await;
    for hash in &staged_only {
        assert!(
            !readable(hash).await,
            "a lost run's staged blob survived the purge"
        );
    }
    account
}

/// Mail spec 6.1: the assertion is no fence on PostgreSQL or MySQL, so
/// issuance is refused there before anything is written.
async fn pg_and_mysql_are_refused(test: &TestServer, admin: &Account) -> Account {
    println!("pg_and_mysql_are_refused");
    let account = passwordless(admin, FENCE8).await;
    test_hooks::set_force_sql_store(true);
    let reply = za_post_as(admin, "setup-token", &json!({ "account": FENCE8 })).await;
    test_hooks::set_force_sql_store(false);
    assert_eq!(
        reply.expect(409)["error"],
        "key accounts are not supported on PostgreSQL or MySQL"
    );
    assert!(
        test.server
            .za_vault_record(account.id().document_id())
            .await
            .unwrap()
            .is_none(),
        "the refused issuance wrote a record"
    );
    za_setup_token(admin, FENCE8).await;
    account
}

/// Mail spec 6.6: issuance is refused for an account that holds mail.
async fn setup_refuses_existing_mail(test: &TestServer, admin: &Account) -> Account {
    println!("setup_refuses_existing_mail");
    let account = passwordless(admin, FENCE9).await;
    let id = account.id().document_id();
    deliver(
        test,
        SENDER,
        FENCE9,
        &message_with("existing", "existing-body"),
    )
    .await;
    let reply = za_post_as(admin, "setup-token", &json!({ "account": FENCE9 })).await;
    assert_eq!(reply.expect(409)["error"], "account already holds mail");
    assert!(test.server.za_vault_record(id).await.unwrap().is_none());

    admin.destroy_all_mailboxes_for_account(id).await;
    test.wait_for_tasks().await;
    za_setup_token(admin, FENCE9).await;
    account
}

/// The metadata row (field 71, immutable at 1.0) of message
/// `document_id`, if stored.
async fn metadata_archive(test: &TestServer, account_id: u32, document_id: u32) -> Option<Vec<u8>> {
    test.server
        .store()
        .get_value::<RawValue>(metadata_key(account_id, document_id))
        .await
        .unwrap()
        .map(|value| value.0)
}

/// Ruling R46 of plan 13 (review F1): destroying a message removes its
/// `MessageData` (field 50) at once and leaves its plaintext metadata row
/// (field 71), blob link and search document to its unindex, which 1.0's
/// index queue runs (`delete_email_metadata`). While that has not run,
/// issuance answers the same 409 as for mail present (spec 6.6's "empty the
/// mailbox, then provision"); it succeeds once it has run. The account's
/// Email partition is held (`hold_index_partition`: 0.16 held the task
/// consumer, which does not hold 1.0's index queue).
async fn setup_refuses_a_pending_unindex(test: &TestServer, admin: &Account) -> Account {
    println!("setup_refuses_a_pending_unindex");
    let account = passwordless(admin, PENDING_UNINDEX).await;
    let id = account.id().document_id();
    deliver(
        test,
        SENDER,
        PENDING_UNINDEX,
        &message_with("pending unindex", "pending-unindex-body"),
    )
    .await;
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "{ids:?}");

    hold_index_partition(test, SearchIndex::Email, id).await;
    admin.destroy_all_mailboxes_for_account(id).await;
    // The window: the message's `MessageData` is gone, its metadata row is
    // not, and its unindex entry is queued.
    assert!(
        !test
            .server
            .za_has_documents(id, Collection::Email)
            .await
            .unwrap(),
        "the destroy left the message's MessageData"
    );
    assert!(
        metadata_archive(test, id, ids[0]).await.is_some(),
        "the metadata row was removed before the unindex ran"
    );
    assert!(
        queue_entries(test, id)
            .await
            .iter()
            .any(|entry| entry.document_id == ids[0] && !entry.insert),
        "the destroy queued no unindex"
    );
    // Ruling R51 would refuse on the message's search document too: it is
    // removed here, ahead of the held unindex, so field 71 alone carries the
    // refusal (R46's pin on the internal search store).
    unindex_search_documents(test, SearchIndex::Email, id, vec![ids[0]]).await;
    assert!(
        !test.server.za_has_email_search_documents(id).await.unwrap(),
        "the message's search document is still stored"
    );
    let reply = za_post_as(admin, "setup-token", &json!({ "account": PENDING_UNINDEX })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account already holds mail",
        "issuance while the destroyed message's metadata row is stored"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    release_index_partition(&test.server, SearchIndex::Email, id).await;
    test.wait_for_tasks().await;
    assert!(
        metadata_archive(test, id, ids[0]).await.is_none(),
        "the unindex left the metadata row"
    );
    za_setup_token(admin, PENDING_UNINDEX).await;
    account
}

/// The ids of the spam training samples indexed under `account_id`.
async fn account_samples(admin: &Account, account_id: u32) -> Vec<Id> {
    admin
        .registry_get_all::<SpamTrainingSample>()
        .await
        .into_iter()
        .filter(|(_, sample)| sample.account_id == Some(Id::from(account_id)))
        .map(|(id, _)| id)
        .collect()
}

/// Ruling R52 of plan 13 (re-review N2): a spam training sample written
/// while the account was plain keeps the message's blob linked and its
/// sender and subject in the registry until it expires, so issuance is
/// refused while one is indexed under the account, also once the mailbox is
/// emptied and unindexed; it succeeds once the sample is destroyed.
async fn setup_refuses_spam_samples(test: &TestServer, admin: &Account) -> Account {
    println!("setup_refuses_spam_samples");
    let account = passwordless(admin, SAMPLED).await;
    let id = account.id().document_id();
    // A master-user session of the credentialless account trains one sample
    // with an APPEND into Junk.
    let mut imap = ImapConnection::connect(b"_s ").await;
    imap.authenticate(&format!("{SAMPLED}%{}", admin.name()), admin.secret())
        .await;
    imap.append("Junk Mail", &message_with("sampled junk", "sampled-body"))
        .await;
    imap.send_ok("LOGOUT").await;
    let samples = account_samples(admin, id).await;
    assert_eq!(samples.len(), 1, "the APPEND into Junk trained no sample");

    admin.destroy_all_mailboxes_for_account(id).await;
    test.wait_for_tasks().await;
    // Only the sample is left: no message, no metadata row.
    assert!(
        !test
            .server
            .za_has_documents(id, Collection::Email)
            .await
            .unwrap()
            && !test
                .server
                .za_has_property(id, Collection::Email, EmailField::Metadata)
                .await
                .unwrap(),
        "the emptied mailbox still holds mail"
    );
    let reply = za_post_as(admin, "setup-token", &json!({ "account": SAMPLED })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account already holds spam training samples",
        "issuance while a sample of the account is stored"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    let destroyed = admin
        .registry_destroy(ObjectType::SpamTrainingSample, &samples)
        .await
        .destroyed()
        .count();
    assert_eq!(destroyed, samples.len(), "the sample was not destroyed");
    za_setup_token(admin, SAMPLED).await;
    account
}

/// Ruling R51 of plan 13 (re-review N1): an Email search document can
/// outlive its message (a deletion that failed or was skipped after the
/// metadata row was cleared). Issuance is refused while the internal search
/// store holds one for the account (planted here through the store's index
/// pass, with no Email archive and no metadata row) and succeeds once it is
/// removed.
async fn setup_refuses_an_orphaned_search_document(test: &TestServer, admin: &Account) -> Account {
    println!("setup_refuses_an_orphaned_search_document");
    let account = passwordless(admin, ORPHANED).await;
    let id = account.id().document_id();
    let mut document = IndexDocument::new(SearchIndex::Email)
        .with_account_id(id)
        .with_document_id(0);
    document.index_text(
        EmailSearchField::Subject,
        "orphaned search document",
        Language::English,
    );
    index_search_documents(test, SearchIndex::Email, id, vec![document]).await;
    assert!(
        !test
            .server
            .za_has_documents(id, Collection::Email)
            .await
            .unwrap()
            && !test
                .server
                .za_has_property(id, Collection::Email, EmailField::Metadata)
                .await
                .unwrap(),
        "the account holds a message"
    );

    let reply = za_post_as(admin, "setup-token", &json!({ "account": ORPHANED })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account already holds mail",
        "issuance while an Email search document of the account is stored"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    unindex_search_documents(test, SearchIndex::Email, id, vec![0]).await;
    za_setup_token(admin, ORPHANED).await;
    account
}

/// The task records of `account_id` that ruling R54 counts in the task
/// queue at 1.0 (thread merge, iTIP, share-revocation cleanup), in queue
/// order. Index work is no longer a task: its queue entries are read by
/// `queue_entries`.
async fn account_tasks(test: &TestServer, account_id: u32) -> Vec<Task> {
    let key = |id| {
        ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task {
            id: TaskId::Assigned(id),
        }))
    };
    let mut tasks = Vec::new();
    test.server
        .store()
        .iterate(
            IterateParams::new(key(0), key(u64::MAX)).ascending(),
            |_, value| {
                let task = Task::deserialize(value)?;
                let owner = match &task {
                    Task::MergeThreads(task) => Some(task.account_id),
                    Task::CalendarItipMessage(task) => Some(task.account_id),
                    Task::ShareRevokeCleanup(task) => Some(task.account_id),
                    _ => None,
                };
                if owner.is_some_and(|owner| owner.document_id() == account_id) {
                    tasks.push(task);
                }
                Ok(true)
            },
        )
        .await
        .unwrap();
    tasks
}

/// Ruling R54 of plan 13 at 1.0, and the index queue's ordering (Task 6,
/// ruling 63): the 0.16 interleaving reproduced. At 0.16 the index worker
/// built an ordinary message's full document and the message's unindex
/// task, in a worker of its own, finished before the build was written, so
/// the document landed afterwards with nothing to remove it. At 1.0 the
/// build is parked (`INDEX_WRITE_PAUSE`, after the pass's documents are
/// built and before the search store's write) and the message destroyed:
/// its unindex entry is queued before the parked write runs, in the same
/// partition, which one worker runs in order. While parked, issuance is
/// refused (the message's row is stored until the unindex runs: R46). Once
/// released, the stale document lands and the partition's next batch
/// removes it: no late document survives, and issuance succeeds without any
/// cleanup. If the reproduction orphaned the document, this case fails.
async fn setup_refuses_an_in_flight_index_write(test: &TestServer, admin: &Account) -> Account {
    println!("setup_refuses_an_in_flight_index_write");
    test.wait_for_tasks().await;
    let account = passwordless(admin, IN_FLIGHT).await;
    let id = account.id().document_id();
    let message = message_with("in flight", "in-flight-body");
    // The delivery commits; its index pass builds the document and parks.
    let parked = HookParked::start(id, test_hooks::set_index_write_pause, async move {
        SmtpConnection::connect()
            .await
            .ingest(SENDER, &[IN_FLIGHT], &message)
            .await;
    })
    .await;
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "{ids:?}");

    admin.destroy_all_mailboxes_for_account(id).await;
    assert!(
        queue_entries(test, id)
            .await
            .iter()
            .any(|entry| entry.document_id == ids[0] && !entry.insert),
        "the unindex was not queued before the parked write"
    );
    assert!(
        !test.server.za_has_email_search_documents(id).await.unwrap(),
        "the parked write landed early"
    );
    let reply = za_post_as(admin, "setup-token", &json!({ "account": IN_FLIGHT })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account already holds mail",
        "issuance while an index write of the account is in flight"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    parked.finish().await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.is_empty(),
        "the late index write outlived the message's unindex (the 0.16 interleaving orphaned it): {keys:?}"
    );
    assert!(
        metadata_archive(test, id, ids[0]).await.is_none(),
        "the unindex left the metadata row"
    );
    za_setup_token(admin, IN_FLIGHT).await;
    account
}

/// Ruling R60 of plan 13, at 1.0: the calendar twin of
/// `setup_refuses_an_in_flight_index_write`. An ordinary event's index pass
/// builds its full document (summary, description, location, participants)
/// and parks; the event is deleted, which queues its unindex entry behind
/// the parked pass. Issuance is refused while the entries are queued
/// (R54's queue scan); once released, the late document lands and the
/// partition's next batch removes it, and issuance succeeds.
async fn setup_refuses_an_in_flight_calendar_index_write(
    test: &TestServer,
    admin: &Account,
) -> Account {
    println!("setup_refuses_an_in_flight_calendar_index_write");
    test.wait_for_tasks().await;
    let account = passwordless(admin, IN_FLIGHT_EVENT).await;
    let id = account.id().document_id();
    // The event is planted while its partition is held; released with the
    // pause set, the partition's pass builds the document and parks.
    hold_index_partition(test, SearchIndex::Calendar, id).await;
    let event_id = plant_event(test, id, "default", "in-flight.ics", IN_FLIGHT_ICAL, None).await;
    let server = test.server.clone();
    let parked = HookParked::start(id, test_hooks::set_index_write_pause, async move {
        release_index_partition(&server, SearchIndex::Calendar, id).await;
    })
    .await;

    delete_event_records(test, id, event_id).await;
    assert!(
        !test
            .server
            .za_has_documents(id, Collection::CalendarEvent)
            .await
            .unwrap()
            && !test
                .server
                .za_has_calendar_search_documents(id)
                .await
                .unwrap(),
        "the account still holds the event or its search document"
    );
    let reply = za_post_as(admin, "setup-token", &json!({ "account": IN_FLIGHT_EVENT })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account has unfinished tasks",
        "issuance while a calendar index write of the account is in flight"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    parked.finish().await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.is_empty(),
        "the late calendar index write outlived the event's unindex: {keys:?}"
    );
    za_setup_token(admin, IN_FLIGHT_EVENT).await;
    account
}

/// Ruling R54 of plan 13 (PR #51 review 1, P2-3): an ordinary APPEND or JMAP
/// create schedules a thread-merge task whose `thread_name` and
/// `message_ids` are `CheekyHash` strings, which hold an input of up to 16
/// bytes verbatim; a task that exhausted its retries is kept as failed and
/// never runs again, and destroying the message leaves it. Issuance is
/// refused while one of the account's is in the queue (planted here as
/// failed, so the task manager never takes it) and succeeds once the
/// operator destroys it through `x:Task/set`.
async fn setup_refuses_a_failed_merge_task(test: &TestServer, admin: &Account) -> Account {
    println!("setup_refuses_a_failed_merge_task");
    let account = passwordless(admin, FAILED_MERGE).await;
    let id = account.id().document_id();
    let task_id = SnowflakeIdGenerator::global_id().unwrap();
    let now = UTCDateTime::now();
    let mut batch = BatchBuilder::new();
    batch.schedule_task_with_id(
        task_id,
        Task::MergeThreads(TaskMergeThreads {
            account_id: Id::from(id),
            thread_name: CheekyHash::new(b"canary-subject").to_string(),
            message_ids: Map::new(vec![
                CheekyHash::new(b"<a@example.org>").to_string(),
                CheekyHash::new(b"<b@example.org>").to_string(),
            ]),
            status: TaskStatus::Failed(TaskStatusFailed {
                created_at: now,
                failed_at: now,
                failed_attempt_number: 3,
                failure_reason: "planted".into(),
            }),
        }),
    );
    test.server.store().write_batch(&mut batch).await.unwrap();
    assert_eq!(account_tasks(test, id).await.len(), 1, "the planted task");

    let reply = za_post_as(admin, "setup-token", &json!({ "account": FAILED_MERGE })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account has unfinished tasks",
        "issuance while a failed merge task of the account is retained"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    // The operator's remedy for a failed task.
    let destroyed = admin
        .registry_destroy(ObjectType::Task, [Id::from(task_id)])
        .await
        .destroyed()
        .count();
    assert_eq!(destroyed, 1, "the failed task was not destroyed");
    assert!(
        account_tasks(test, id).await.is_empty(),
        "the destroyed task is still queued"
    );
    za_setup_token(admin, FAILED_MERGE).await;
    account
}

/// Waits (at most 10 s) until the account's partition in `index` is in
/// retry (its status key set by a failed pass).
async fn wait_for_retry_state(test: &TestServer, index: SearchIndex, account_id: u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while partition_status(test, index, account_id).await.is_none() {
        assert!(
            Instant::now() < deadline,
            "the partition of account {account_id} did not enter its retry state"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Ruling R58 of plan 13 (PR #51 review 2, P2-6) at 1.0: an index pass (A)
/// that built an ordinary message's full document and stalled past its
/// partition lock is superseded on a cluster by another node's execution
/// (B), which runs the message's unindex and clears the partition's entries;
/// R46, R54 and R51 then pass and the account is provisioned. When A
/// resumes, the vault-absence assertion its document carries, in every
/// commit chunk of its pass (ruling 18), fails: nothing of it lands, and the
/// partition enters its retry state, which one reset drains. B is simulated
/// (`index_task::emulate_superseding_node`); the harness has one index
/// manager. An ordinary account's document passes the same fence (the
/// control).
async fn superseded_index_write_loses_to_provisioning(
    test: &TestServer,
    admin: &Account,
) -> (Account, Account) {
    println!("superseded_index_write_loses_to_provisioning");
    test.wait_for_tasks().await;
    let account = passwordless(admin, SUPERSEDED).await;
    let id = account.id().document_id();
    let control = passwordless(admin, SUPERSEDED_CONTROL).await;
    let control_id = control.id().document_id();
    let message = message_with("superseded", "superseded-body");
    // A: the delivery commits; its index pass classifies the account as
    // ordinary, builds the full document and parks before the write.
    let parked = HookParked::start(id, test_hooks::set_index_write_pause, async move {
        SmtpConnection::connect()
            .await
            .ingest(SENDER, &[SUPERSEDED], &message)
            .await;
    })
    .await;
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "{ids:?}");

    admin.destroy_all_mailboxes_for_account(id).await;
    // B, after A's lease expired.
    emulate_superseding_node(test, id, &ids).await;
    assert!(
        queue_entries(test, id).await.is_empty()
            && metadata_archive(test, id, ids[0]).await.is_none(),
        "B's execution left an entry or the row"
    );

    // Every term passes: no message, no metadata row, no entry, no sample,
    // no search key.
    let token = za_setup_token(admin, SUPERSEDED).await;
    za_setup(SUPERSEDED, &token, STRONG).await;
    assert_eq!(
        test.server
            .za_vault_record(id)
            .await
            .unwrap()
            .expect("the vault record")
            .record
            .state,
        VaultState::Active,
        "setup did not complete"
    );

    // A resumes and attempts its write.
    parked.finish().await;
    wait_for_retry_state(test, SearchIndex::Email, id).await;
    assert!(
        search_keys(test, id, SearchIndex::Email).await.is_empty(),
        "the superseded write landed in the key account's index"
    );
    release_index_partition(&test.server, SearchIndex::Email, id).await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;
    assert!(
        search_keys(test, id, SearchIndex::Email).await.is_empty(),
        "the reset's retry wrote into the key account's index"
    );

    // The control's document passes the fence.
    deliver(
        test,
        SENDER,
        SUPERSEDED_CONTROL,
        &message_with("superseded control", "superseded-control-body"),
    )
    .await;
    assert!(
        test.server
            .za_has_email_search_documents(control_id)
            .await
            .unwrap(),
        "the control's document was not written"
    );

    admin.destroy_all_mailboxes_for_account(control_id).await;
    (account, control)
}

/// 2,400 distinct five-byte tokens (`c0000` to `c2399`, 16 to a line) and
/// then `canary`: about 2,400 term merges, so the pass splits after every
/// 1,000 (`build_batch`'s `add_commit_point`) into three commit chunks, its
/// `Document` key in the last. Terms are written in key order, so a canary
/// that starts with `zz` lands in the last chunk.
fn large_body(canary: &str) -> String {
    let mut body = String::new();
    for line in 0..150 {
        let tokens = (0..16)
            .map(|token| format!("c{:04}", line * 16 + token))
            .collect::<Vec<_>>();
        body.push_str(&tokens.join(" "));
        body.push_str("\r\n");
    }
    body.push_str(canary);
    body
}

/// `unindex_between_calendar_index_chunks_leaves_no_terms`'s event: no
/// alarm, no organizer (so its only work is its index and unindex), and a
/// DESCRIPTION of `large_body`'s tokens and the canary, folded at 70 octets.
fn chunked_ical() -> String {
    event_ical(
        "chunked-event@example.com",
        &large_body(TAIL_CANARY).replace("\r\n", " "),
    )
}

/// An event with `uid` and `description`, folded at 70 octets: no alarm,
/// no organizer, so its only work is its index and unindex.
fn event_ical(uid: &str, description: &str) -> String {
    let description = format!("DESCRIPTION:{description}");
    let folded = description
        .as_bytes()
        .chunks(70)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\r\n ");
    format!(
        "BEGIN:VCALENDAR\r\n\
         VERSION:2.0\r\n\
         PRODID:-//Stalwart Labs//za fence//EN\r\n\
         BEGIN:VEVENT\r\n\
         UID:{uid}\r\n\
         DTSTAMP:20261020T100000Z\r\n\
         DTSTART:20261021T100000Z\r\n\
         DTEND:20261021T110000Z\r\n\
         SUMMARY:chunked summary\r\n\
         {folded}\r\n\
         END:VEVENT\r\n\
         END:VCALENDAR\r\n"
    )
}

/// Indexes `document` for the account straight into the internal search
/// store, then clears its `Document` key, leaving the terms it wrote
/// unlisted: the shape of a tail written after its document's unindex, of a
/// calendar merge that lost its clears, and of the lost-lease residual
/// (part 6a).
async fn plant_orphaned_keys(
    test: &TestServer,
    account_id: u32,
    index: SearchIndex,
    document: IndexDocument,
) {
    index_search_documents(test, index, account_id, vec![document]).await;
    let mut batch = BatchBuilder::new();
    batch.clear(ValueClass::SearchIndex(SearchIndexClass::Document {
        index,
        account_id,
        document_id: 0,
    }));
    internal_search(test).write_batch(&mut batch).await.unwrap();
}

/// `setup-token` for the account `name` (id `id`) answers 409 with
/// `reason` and writes no record.
async fn assert_refused(
    test: &TestServer,
    admin: &Account,
    (name, id): (&str, u32),
    reason: &str,
    what: &str,
) {
    let reply = za_post_as(admin, "setup-token", &json!({ "account": name })).await;
    assert_eq!(reply.expect(409)["error"], reason, "issuance while {what}");
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );
}

/// Ruling R62 of plan 13 (PR #51 review 3, P2-8), kept at 1.0 (Task 1's
/// ruling 19): a search document's terms can outlive its `Document` key
/// (the lost-lease residual, a calendar merge that lost its clears), and
/// R51's and R60's scans read the `Document` keys only. Issuance is refused
/// while the internal search store holds any term of the account in the
/// Email index (as for mail) or the Calendar index (as for calendar data);
/// each case is planted alone, and issuance succeeds once the account-wide
/// unindex has removed the keys. Neither scan reads the other index's keys
/// or another account's. 1.0 has no separate `Index` keys (0.16's
/// "ReceivedAt alone" case): every field is a term.
async fn setup_refuses_orphaned_search_terms(test: &TestServer, admin: &Account) -> Account {
    println!("setup_refuses_orphaned_search_terms");
    let account = passwordless(admin, ORPHANED_TERMS).await;
    let id = account.id().document_id();

    // Email terms only.
    let mut document = IndexDocument::new(SearchIndex::Email)
        .with_account_id(id)
        .with_document_id(0);
    document.index_text(EmailSearchField::Body, "orphanterm", Language::English);
    plant_orphaned_keys(test, id, SearchIndex::Email, document).await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.documents.is_empty() && keys.terms > 0,
        "Email terms alone were not planted: {keys:?}"
    );
    assert!(
        !test.server.za_has_email_search_documents(id).await.unwrap()
            && test.server.za_has_email_search_terms(id).await.unwrap()
            && !test.server.za_has_calendar_search_terms(id).await.unwrap(),
        "the scans do not see the orphaned Email terms alone"
    );
    assert_refused(
        test,
        admin,
        (ORPHANED_TERMS, id),
        "account already holds mail",
        "an orphaned Email term of the account is stored",
    )
    .await;
    unindex_search_account(test, SearchIndex::Email, id).await;
    assert!(
        !test.server.za_has_email_search_terms(id).await.unwrap(),
        "the account-wide unindex left Email terms"
    );

    // Calendar terms only.
    let mut document = IndexDocument::new(SearchIndex::Calendar)
        .with_account_id(id)
        .with_document_id(0);
    document.index_text(
        CalendarSearchField::Title,
        "orphaned title",
        Language::English,
    );
    plant_orphaned_keys(test, id, SearchIndex::Calendar, document).await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.documents.is_empty() && keys.terms > 0,
        "Calendar terms alone were not planted: {keys:?}"
    );
    assert!(
        !test
            .server
            .za_has_calendar_search_documents(id)
            .await
            .unwrap()
            && test.server.za_has_calendar_search_terms(id).await.unwrap()
            && !test.server.za_has_email_search_terms(id).await.unwrap(),
        "the scans do not see the orphaned Calendar terms alone"
    );
    assert_refused(
        test,
        admin,
        (ORPHANED_TERMS, id),
        "account already holds calendar data",
        "an orphaned Calendar term of the account is stored",
    )
    .await;
    unindex_search_account(test, SearchIndex::Calendar, id).await;
    assert!(
        !test.server.za_has_calendar_search_terms(id).await.unwrap(),
        "the account-wide unindex left Calendar terms"
    );

    // Orphans of another account, at the adjacent id above (account ids
    // are assigned in order, so none holds it yet), are not the account's:
    // its scans stay false.
    let neighbour = id + 1;
    let mut document = IndexDocument::new(SearchIndex::Email)
        .with_account_id(neighbour)
        .with_document_id(0);
    document.index_text(EmailSearchField::Body, "orphanterm", Language::English);
    plant_orphaned_keys(test, neighbour, SearchIndex::Email, document).await;
    let mut document = IndexDocument::new(SearchIndex::Calendar)
        .with_account_id(neighbour)
        .with_document_id(0);
    document.index_text(
        CalendarSearchField::Title,
        "orphaned title",
        Language::English,
    );
    plant_orphaned_keys(test, neighbour, SearchIndex::Calendar, document).await;
    assert!(
        test.server
            .za_has_email_search_terms(neighbour)
            .await
            .unwrap()
            && test
                .server
                .za_has_calendar_search_terms(neighbour)
                .await
                .unwrap(),
        "the adjacent account's orphans were not planted"
    );
    assert!(
        !test.server.za_has_email_search_terms(id).await.unwrap()
            && !test.server.za_has_calendar_search_terms(id).await.unwrap(),
        "the account's scans see the adjacent account's orphans"
    );
    for index in [SearchIndex::Email, SearchIndex::Calendar] {
        unindex_search_account(test, index, neighbour).await;
    }
    assert!(
        !test
            .server
            .za_has_email_search_terms(neighbour)
            .await
            .unwrap()
            && !test
                .server
                .za_has_calendar_search_terms(neighbour)
                .await
                .unwrap(),
        "the account-wide unindex left the adjacent account's orphans"
    );

    za_setup_token(admin, ORPHANED_TERMS).await;
    account
}

/// Ruling R62 of plan 13 (PR #51 review 3, P2-8), flipped at 1.0 (Task 1's
/// ruling 19, part 6a): at 0.16 the message's unindex task ran in a worker
/// of its own between two commit chunks of the message's index write, so
/// the later chunks wrote terms nothing listed, and R62's per-chunk guard
/// stopped them. At 1.0 the unindex is an entry of the same partition,
/// which one worker runs in order: with the pass parked after chunk 0
/// (`INDEX_CHUNK_PAUSE`) and the message destroyed, the unindex waits;
/// issuance is refused meanwhile (the row is stored: R46). Released, the
/// pass writes its later chunks whole and the next batch's unindex removes
/// the document and every term: nothing is left, and the account
/// provisions. The guard is retired.
async fn unindex_between_index_chunks_leaves_no_terms(
    test: &TestServer,
    admin: &Account,
) -> Account {
    println!("unindex_between_index_chunks_leaves_no_terms");
    test.wait_for_tasks().await;
    let account = passwordless(admin, CHUNKED).await;
    let id = account.id().document_id();
    let message = message_with("chunked subject", &large_body(TAIL_CANARY));
    // The delivery commits; its index pass writes chunk 0 and parks.
    let parked = HookParked::start(id, test_hooks::set_index_chunk_pause, async move {
        SmtpConnection::connect()
            .await
            .ingest(SENDER, &[CHUNKED], &message)
            .await;
    })
    .await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.documents.is_empty() && keys.terms > 0 && !keys.holds_term(TAIL_CANARY),
        "chunk 0 alone was not written: {keys:?}"
    );
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "{ids:?}");

    admin.destroy_all_mailboxes_for_account(id).await;
    assert!(
        queue_entries(test, id)
            .await
            .iter()
            .any(|entry| entry.document_id == ids[0] && !entry.insert),
        "the destroy queued no unindex"
    );
    let reply = za_post_as(admin, "setup-token", &json!({ "account": CHUNKED })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account already holds mail",
        "issuance while the index write of the account is between chunks"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    parked.finish().await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.is_empty(),
        "the later chunks outlived the unindex: {keys:?}"
    );
    za_setup_token(admin, CHUNKED).await;
    account
}

/// Ruling R62 of plan 13, flipped at 1.0: the calendar twin of
/// `unindex_between_index_chunks_leaves_no_terms`. An ordinary event whose
/// description fills three commit chunks is indexed; its pass parks after
/// chunk 0 and the event is deleted, which queues its unindex behind the
/// pass; issuance is refused meanwhile (R54's queue scan). Released, the
/// pass completes and the unindex removes everything.
async fn unindex_between_calendar_index_chunks_leaves_no_terms(
    test: &TestServer,
    admin: &Account,
) -> Account {
    println!("unindex_between_calendar_index_chunks_leaves_no_terms");
    test.wait_for_tasks().await;
    let account = passwordless(admin, CHUNKED_EVENT).await;
    let id = account.id().document_id();
    hold_index_partition(test, SearchIndex::Calendar, id).await;
    let event_id = plant_event(test, id, "default", "chunked.ics", &chunked_ical(), None).await;
    let server = test.server.clone();
    let parked = HookParked::start(id, test_hooks::set_index_chunk_pause, async move {
        release_index_partition(&server, SearchIndex::Calendar, id).await;
    })
    .await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.documents.is_empty() && keys.terms > 0 && !keys.holds_term(TAIL_CANARY),
        "chunk 0 alone was not written: {keys:?}"
    );

    delete_event_records(test, id, event_id).await;
    assert_refused(
        test,
        admin,
        (CHUNKED_EVENT, id),
        "account has unfinished tasks",
        "the calendar index write of the account is between chunks",
    )
    .await;

    parked.finish().await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.is_empty(),
        "the later calendar chunks outlived the unindex: {keys:?}"
    );
    za_setup_token(admin, CHUNKED_EVENT).await;
    account
}

/// Ruling R62's cluster route (P2-8's second route), at 1.0 under ruling
/// 18: an index pass (A) that wrote a large message's chunk 0 and stalled
/// past its partition lock is superseded by another node's execution (B,
/// simulated), which runs the message's unindex: the `Document` key is not
/// written yet (it is in the last chunk), so B's deletion finds no stored
/// document and chunk 0's terms are left, listed by nothing (part 6a's
/// lost-lease residual). R62's kept term scan refuses issuance on them;
/// after the account-wide unindex (the remedy) the account is provisioned.
/// When A resumes, its later chunks fail the vault-absence assertion each
/// carries: nothing of them lands, the partition enters its retry state,
/// and one reset drains it. An ordinary account's multi-chunk document is
/// written whole (the control).
async fn superseded_index_tail_loses_to_provisioning(
    test: &TestServer,
    admin: &Account,
) -> (Account, Account) {
    println!("superseded_index_tail_loses_to_provisioning");
    test.wait_for_tasks().await;
    let account = passwordless(admin, SUPERSEDED_TAIL).await;
    let id = account.id().document_id();
    let control = passwordless(admin, SUPERSEDED_TAIL_CONTROL).await;
    let control_id = control.id().document_id();
    let message = message_with("superseded tail", &large_body(TAIL_CANARY));
    // A: the delivery commits; its index pass writes chunk 0 and parks.
    let parked = HookParked::start(id, test_hooks::set_index_chunk_pause, async move {
        SmtpConnection::connect()
            .await
            .ingest(SENDER, &[SUPERSEDED_TAIL], &message)
            .await;
    })
    .await;
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "{ids:?}");

    admin.destroy_all_mailboxes_for_account(id).await;
    // B, after A's lease expired.
    emulate_superseding_node(test, id, &ids).await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.documents.is_empty() && keys.terms > 0,
        "B's deletion did not leave chunk 0's terms: {keys:?}"
    );
    assert_refused(
        test,
        admin,
        (SUPERSEDED_TAIL, id),
        "account already holds mail",
        "chunk 0's terms outlived the superseding unindex",
    )
    .await;
    unindex_search_account(test, SearchIndex::Email, id).await;

    let token = za_setup_token(admin, SUPERSEDED_TAIL).await;
    za_setup(SUPERSEDED_TAIL, &token, STRONG).await;
    assert_eq!(
        test.server
            .za_vault_record(id)
            .await
            .unwrap()
            .expect("the vault record")
            .record
            .state,
        VaultState::Active,
        "setup did not complete"
    );

    // A resumes: chunk 1 fails its assertion.
    parked.finish().await;
    wait_for_retry_state(test, SearchIndex::Email, id).await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.is_empty(),
        "the superseded tail landed in the key account's index: {keys:?}"
    );
    release_index_partition(&test.server, SearchIndex::Email, id).await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;

    // The control's multi-chunk document, written whole.
    deliver(
        test,
        SENDER,
        SUPERSEDED_TAIL_CONTROL,
        &message_with("superseded tail control", &large_body(TAIL_CANARY)),
    )
    .await;
    let control_keys = search_keys(test, control_id, SearchIndex::Email).await;
    assert!(
        control_keys.documents.len() == 1
            && control_keys.terms > 2000
            && control_keys.holds_term(TAIL_CANARY),
        "the control's multi-chunk document was not written whole: {control_keys:?}"
    );

    admin.destroy_all_mailboxes_for_account(control_id).await;
    (account, control)
}

/// Replaces the stored event's iCalendar data as a PUT does (upstream's
/// full split update over the row and the content), which queues its index
/// entry: a merge with the event's stored `Document` key.
async fn update_event(test: &TestServer, account_id: u32, event_id: u32, ical: &str) {
    let stored = stored_event(test, account_id, event_id).await;
    let mut event = stored.za_event();
    let parsed = match calcard::Parser::new(ical).entry() {
        calcard::Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    };
    event.content.data = CalendarEventData::new(parsed, Tz::Floating, 100);
    write_stored_event(test, account_id, event_id, &stored, event).await;
    test.server.notify_index_queue();
}

/// The merge tests' updated DESCRIPTION: a few tokens, none of the large
/// one's, ending with `MERGE_CANARY`.
fn short_description() -> String {
    format!("short merged text {MERGE_CANARY}")
}

/// Ruling R63 of plan 13 (opus F4, fable F3), flipped at 1.0: a calendar
/// update's index pass merges with the stored document, its clears of the
/// old-only terms spread over several commit chunks. At 0.16 an unindex
/// between two of them stopped the rest and left old-only keys. At 1.0 the
/// event's deletion queues its unindex behind the parked merge (the same
/// partition, in order); issuance is refused meanwhile; released, the merge
/// completes and the unindex removes the document and every term, so the
/// account provisions.
async fn unindex_between_calendar_merge_chunks_leaves_no_terms(
    test: &TestServer,
    admin: &Account,
) -> Account {
    println!("unindex_between_calendar_merge_chunks_leaves_no_terms");
    test.wait_for_tasks().await;
    let account = passwordless(admin, MERGE_UNINDEXED).await;
    let id = account.id().document_id();
    let uid = "merge-unindexed@example.com";
    let event_id = plant_event(
        test,
        id,
        "default",
        "merged.ics",
        &event_ical(uid, &large_body(TAIL_CANARY).replace("\r\n", " ")),
        None,
    )
    .await;
    test.wait_for_tasks().await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.documents.len() == 1 && keys.terms > 2000 && keys.holds_term(TAIL_CANARY),
        "the event was not indexed whole: {keys:?}"
    );

    // The update is committed while the partition is held; released with
    // the pause set, its merge writes chunk 0 and parks before the second.
    hold_index_partition(test, SearchIndex::Calendar, id).await;
    update_event(test, id, event_id, &event_ical(uid, &short_description())).await;
    let server = test.server.clone();
    let parked = HookParked::start(id, test_hooks::set_index_chunk_pause, async move {
        release_index_partition(&server, SearchIndex::Calendar, id).await;
    })
    .await;

    delete_event_records(test, id, event_id).await;
    assert_refused(
        test,
        admin,
        (MERGE_UNINDEXED, id),
        "account has unfinished tasks",
        "the calendar merge of the account is between chunks",
    )
    .await;

    parked.finish().await;
    wait_for_index_queue(test, id, Duration::from_secs(10)).await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.is_empty(),
        "the merge's keys outlived the unindex: {keys:?}"
    );
    za_setup_token(admin, MERGE_UNINDEXED).await;
    account
}

/// Ruling R63 of plan 13 (fable F5): the merge path's positive control. An
/// ordinary event whose description fills three commit chunks is indexed,
/// then updated to a short description; its merge leaves one `Document`
/// key, the new tokens, and none of the old-only ones.
async fn multi_chunk_calendar_merge_is_written_whole(
    test: &TestServer,
    admin: &Account,
) -> Account {
    println!("multi_chunk_calendar_merge_is_written_whole");
    test.wait_for_tasks().await;
    let account = passwordless(admin, MERGE_CONTROL).await;
    let id = account.id().document_id();
    let uid = "merge-control@example.com";
    let event_id = plant_event(
        test,
        id,
        "default",
        "merged.ics",
        &event_ical(uid, &large_body(TAIL_CANARY).replace("\r\n", " ")),
        None,
    )
    .await;
    test.wait_for_tasks().await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.documents.len() == 1 && keys.terms > 2000 && keys.holds_term(TAIL_CANARY),
        "the event was not indexed whole: {keys:?}"
    );

    update_event(test, id, event_id, &event_ical(uid, &short_description())).await;
    test.wait_for_tasks().await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.documents.len() == 1
            && keys.holds_term(MERGE_CANARY)
            && !keys.holds_term(TAIL_CANARY)
            && (0..2400).all(|token| !keys.holds_term(&format!("c{token:04}"))),
        "the multi-chunk merge was not written whole: {keys:?}"
    );

    delete_event_records(test, id, event_id).await;
    test.wait_for_tasks().await;
    account
}

/// Writes `container` (or removes the stored one with `None`) as the
/// object-metadata container of `owner_id`'s document, through upstream's
/// writers (`ContainerChange`, then `MetadataWrite` or, with a viewer,
/// `PrivateMetadataWrite` and its counters), without the presence bits:
/// what an ordinary writer leaves before provisioning, including on a
/// document that no longer exists.
async fn write_object_metadata(
    test: &TestServer,
    owner_id: u32,
    viewer_id: Option<u32>,
    collection: Collection,
    document_id: u32,
    container: Option<types::metadata::EncodedMetadata>,
) {
    use common::storage::metadata::{ContainerChange, MetadataLog, PrivateMetadataCommit};
    use types::metadata::MetadataEdit;
    let previous = match viewer_id {
        Some(viewer_id) => test
            .server
            .private_metadata_container(owner_id, viewer_id, collection, document_id)
            .await
            .unwrap(),
        None => test
            .server
            .metadata_container(owner_id, collection, document_id)
            .await
            .unwrap(),
    };
    let Some(change) = ContainerChange::new(previous.as_ref(), container, MetadataEdit::Write)
    else {
        return;
    };
    let mut batch = BatchBuilder::new();
    let mut commit = PrivateMetadataCommit::default();
    let document = store::write::PendingId::Assigned(document_id);
    match viewer_id {
        Some(viewer_id) => {
            let viewer = test.server.account(viewer_id).await.unwrap();
            change
                .into_private_write(owner_id, &viewer, collection, MetadataLog::None)
                .build(document, &mut batch, &mut commit)
                .unwrap();
        }
        None => {
            let owner = test.server.account(owner_id).await.unwrap();
            change
                .into_write(&owner, collection, MetadataLog::None)
                .build(document, &mut batch)
                .unwrap();
        }
    }
    test.server
        .commit_metadata_batch(batch, commit)
        .await
        .unwrap();
}

/// Ruling 66 (the Task 6 review's F1): issuance is refused while the account
/// holds object metadata in the clear, and proceeds once it is removed. Each
/// container is written through upstream's writers as an ordinary writer
/// leaves it before provisioning (an impersonating operator or a grantee with
/// `ModifyMetadata`): a mailbox's shared container, a server-level IMAP
/// annotation, another account's private container on a destroyed message of
/// the account (Email destruction releases only the shared container), and
/// the account's own private container on another account's mailbox (its
/// `Owner` counter; once removed, the counter stays and counts none). An
/// address book's container, DAV dead properties kept in the clear for every
/// account, is permitted.
async fn setup_refuses_object_metadata(test: &TestServer, admin: &Account) -> (Account, Account) {
    use crate::utils::za::plain_dav_container;
    println!("setup_refuses_object_metadata");
    let account = passwordless(admin, OBJECT_METADATA).await;
    let id = account.id().document_id();
    let owner = admin
        .create_user_account(
            OBJECT_METADATA_OWNER,
            STRONG,
            "Fence",
            &[],
            user_permissions(),
        )
        .await;
    let owner_id = owner.id().document_id();
    let container = || plain_dav_container(&[("urn:za:fence", "note", "object metadata canary")]);
    // (what, the object's owner, the viewer of a private container, the
    // collection, the document).
    let cases = [
        (
            "a mailbox's shared container",
            id,
            None,
            Collection::Mailbox,
            INBOX_ID,
        ),
        (
            "a server-level annotation",
            id,
            None,
            Collection::Principal,
            0,
        ),
        (
            "another account's private container on a destroyed message",
            id,
            Some(owner_id),
            Collection::Email,
            u32::MAX - 7,
        ),
        (
            "its private container on another account's mailbox",
            owner_id,
            Some(id),
            Collection::Mailbox,
            INBOX_ID,
        ),
    ];
    assert!(!test.server.za_has_object_metadata(id).await.unwrap());
    for (what, owner_id, viewer_id, collection, document_id) in cases {
        write_object_metadata(
            test,
            owner_id,
            viewer_id,
            collection,
            document_id,
            Some(container()),
        )
        .await;
        let reply = za_post_as(admin, "setup-token", &json!({ "account": OBJECT_METADATA })).await;
        assert_eq!(
            reply.expect(409)["error"],
            "account already holds object metadata",
            "issuance while the account holds {what}"
        );
        assert!(
            test.server.za_vault_record(id).await.unwrap().is_none(),
            "the refused issuance wrote a record ({what})"
        );
        write_object_metadata(test, owner_id, viewer_id, collection, document_id, None).await;
        assert!(
            !test.server.za_has_object_metadata(id).await.unwrap(),
            "{what} was not removed"
        );
    }
    // Permitted: an address book's dead properties.
    write_object_metadata(
        test,
        id,
        None,
        Collection::AddressBook,
        0,
        Some(container()),
    )
    .await;
    assert!(
        !test.server.za_has_object_metadata(id).await.unwrap(),
        "an address book's container refuses issuance"
    );
    write_object_metadata(test, id, None, Collection::AddressBook, 0, None).await;

    za_setup_token(admin, OBJECT_METADATA).await;
    (account, owner)
}

/// Ruling 66 (the Task 6 review's F1), the writers' fence: a container
/// write whose accounts were read as ordinary commits only while the
/// account holds no vault record, as mail's ordinary writers do (mail spec
/// 6.1). A pending record is written straight to the store, as
/// `setup-token` writes it but without the invalidation, so the account
/// stays classified as ordinary in the caches the writers read: JMAP's
/// `Mailbox/set` with metadata, IMAP `SETMETADATA` on a mailbox and a
/// private server annotation all fail and store nothing. With the record
/// removed, the same writes commit (the controls).
async fn metadata_writes_lose_to_provisioning(test: &TestServer) -> Account {
    use crate::utils::{imap::Type, jmap::JmapUtils};
    use imap_proto::ResponseType;
    println!("metadata_writes_lose_to_provisioning");
    const USING: &[&str] = &[
        "urn:ietf:params:jmap:core",
        "urn:ietf:params:jmap:mail",
        "urn:ietf:params:jmap:metadata",
    ];
    let admin = test.account("admin@example.com").clone();
    let account = admin
        .create_user_account(METADATA_WRITER, STRONG, "Fence", &[], user_permissions())
        .await;
    let id = account.id().document_id();
    let reply = account
        .jmap_request(
            USING,
            json!([["Mailbox/set", {
                "accountId": account.id_string(),
                "create": { "m": { "name": "fence-metadata" } }
            }, "0"]]),
        )
        .await;
    let mailbox = reply.method_response()["created"]["m"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("Mailbox/set: {:?}", reply.0))
        .to_string();
    let mut imap = ImapConnection::connect(b"_f ").await;
    imap.authenticate(METADATA_WRITER, STRONG).await;
    let jmap_write = async |value: serde_json::Value| {
        account
            .jmap_request(
                USING,
                json!([["Mailbox/set", {
                    "accountId": account.id_string(),
                    "update": { (mailbox.as_str()): { "metadata": value } }
                }, "0"]]),
            )
            .await
    };
    let note = || json!({ "example.com": { "note": "fence-canary" } });
    const IMAP_WRITES: [&str; 2] = [
        "SETMETADATA INBOX (/shared/comment \"fence-canary\")",
        "SETMETADATA \"\" (/private/comment \"fence-canary\")",
    ];

    let record = VaultRecord::pending(vec![0x5a; 32], i64::MAX);
    test.server.za_vault_write(id, &record, None).await.unwrap();
    assert!(
        !test.server.account(id).await.unwrap().is_key_account(),
        "the planted record changed the cached classification"
    );
    let reply = jmap_write(note()).await;
    assert!(
        reply.method_response()["updated"]
            .get(mailbox.as_str())
            .is_none(),
        "Mailbox/set with metadata committed while the account holds a vault record: {:?}",
        reply.0
    );
    for command in IMAP_WRITES {
        imap.send(command).await;
        imap.assert_read(Type::Tagged, ResponseType::No).await;
    }
    assert!(
        !test.server.za_has_object_metadata(id).await.unwrap(),
        "a container write committed while the account holds a vault record"
    );

    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(id)
        .with_collection(Collection::Principal)
        .with_document(0)
        .clear(types::field::PrincipalField::ZeroAccessVault);
    test.server.store().write(batch.build_all()).await.unwrap();
    let reply = jmap_write(note()).await;
    assert!(
        reply.method_response()["updated"]
            .get(mailbox.as_str())
            .is_some(),
        "the control's Mailbox/set with metadata: {:?}",
        reply.0
    );
    for command in IMAP_WRITES {
        imap.send_ok(command).await;
    }
    assert!(
        test.server.za_has_object_metadata(id).await.unwrap(),
        "the controls stored no container"
    );
    // The account's destruction removes the mailbox's container.
    imap.send_ok("SETMETADATA INBOX (/shared/comment NIL)")
        .await;
    imap.send_ok("SETMETADATA \"\" (/private/comment NIL)")
        .await;
    imap.send_ok("LOGOUT").await;
    account
}
