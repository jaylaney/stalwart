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
    Traced, assert_no_plaintext, assert_sealed, deliver,
    delivery::{configure, temporarily_failed, unconfigure},
    message_ids, message_with, stored_blob, stored_metadata,
};
use crate::{
    utils::{
        account::Account,
        imap::ImapConnection,
        server::TestServer,
        smtp::SmtpConnection,
        za::{
            Conn, HookParked, hold_task_consumer, plant_event, wait_for_delivery, za_post_as,
            za_setup, za_setup_token,
        },
    },
    za::{STRONG, raw_input::SUBMISSION_PORT, user_permissions},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use calcard::common::timezone::Tz;
use common::{
    Server,
    auth::AccessToken,
    za::{ZA_MAIL_KEYLESS, test_hooks},
};
use email::{
    cache::MessageCacheFetch,
    mailbox::{INBOX_ID, JUNK_ID},
    message::{
        copy::EmailCopy,
        delete::EmailDeletion,
        ingest::{EmailIngest, IngestEmail, IngestSource},
        metadata::MessageMetadata,
        za::is_stub,
    },
};
use groupware::{
    DestroyArchive,
    calendar::{CalendarEvent, CalendarEventData, seal::WRAP_PK},
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
    Deserialize, IterateParams, SUBSPACE_SEARCH_INDEX, U32_LEN, U64_LEN, ValueKey,
    registry::write::{RegistryWrite, RegistryWriteResult},
    roaring::RoaringBitmap,
    search::{
        CalendarSearchField, EmailSearchField, IndexDocument, SearchField, SearchFilter,
        SearchQuery,
    },
    write::{
        AlignedBytes, AnyKey, Archive, BatchBuilder, BlobLink, BlobOp, SearchIndex,
        SearchIndexClass, SearchIndexId, SearchIndexType, TaskQueueClass, ValueClass,
        key::DeserializeBigEndian, now,
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
/// Its ordinary control, whose multi-chunk index write passes the guard.
const SUPERSEDED_TAIL_CONTROL: &str = "plainmail18@example.com";
/// `failed_guard_with_its_document_stored_defers_the_task`'s account, an
/// ordinary one (ruling R63).
const GUARD_FAILED: &str = "plainmail19@example.com";
/// `unindex_between_calendar_merge_chunks_leaves_no_terms`'s account
/// (ruling R63).
const MERGE_UNINDEXED: &str = "keymail26@example.com";
/// `multi_chunk_calendar_merge_is_written_whole`'s account, an ordinary one
/// (ruling R63).
const MERGE_CONTROL: &str = "plainmail20@example.com";
/// `stopped_document_leaves_its_batch_written`'s account whose document
/// stops (ruling R63).
const BATCH_STOPPED: &str = "plainmail21@example.com";
/// The account of that batch's other document, which the pause does not
/// park.
const BATCH_OTHER: &str = "plainmail22@example.com";
/// Under the smallest deferral of an index task's failed first attempt:
/// the default retry strategy (exponential backoff from a 60 s initial
/// delay, a jitter factor from 0.5 to 1.5) defers it by at least 30 s from
/// the whole second of the failure, so by more than 29 s.
const UNDER_THE_DEFERRAL: Duration = Duration::from_secs(15);
/// The token the merge tests' updated DESCRIPTION ends with: 13 bytes, so
/// stored verbatim, and new to the event.
const MERGE_CANARY: &str = "zzmergecanary";
/// The token `large_body` ends with: 12 bytes, so stored verbatim in its
/// `Term` key (`CheekyHash`), and longer than the body's five-byte tokens,
/// so written in a later commit chunk than the first.
const TAIL_CANARY: &str = "zztailcanary";
/// `train_batch_loses_to_provisioning`'s source account (ruling R55).
const TRAINED: &str = "keymail19@example.com";
/// `train_batch_loses_to_provisioning`'s destination, an ordinary account.
const TRAIN_DESTINATION: &str = "plainmail16@example.com";
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
    let guard_failed = failed_guard_with_its_document_stored_defers_the_task(test, &admin).await;
    let merge_unindexed = unindex_between_calendar_merge_chunks_leaves_no_terms(test, &admin).await;
    let merge_control = multi_chunk_calendar_merge_is_written_whole(test, &admin).await;
    let (batch_stopped, batch_other) =
        stopped_document_leaves_its_batch_written(test, &admin).await;

    // The accounts that log in empty their own mailboxes; the others are
    // pending or credentialless, and the account's destruction removes
    // what they hold.
    for account in [
        &fence1, &fence2, &plain, &fence10, &organizer, &fence3, &fence4, &fence11, &fence5,
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
        guard_failed,
        merge_unindexed,
        merge_control,
        batch_stopped,
        batch_other,
    ] {
        admin.destroy_account(account).await;
    }
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
    BlobHash::from(
        &stored_metadata(test, account_id, document_id)
            .await
            .unarchive::<MessageMetadata>()
            .unwrap()
            .blob_hash,
    )
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
                .blob_has_access(&staged, &plain_link)
                .await
                .unwrap(),
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
        !is_stub(
            stored_metadata(test, id, after)
                .await
                .unarchive::<MessageMetadata>()
                .unwrap()
        ),
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
        let item = &cache.emails.items[0];
        Id::from_parts(item.thread_id, item.document_id).to_string()
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
        let err = match test
            .server
            .copy_message(from, document_id, to, vec![INBOX_ID], vec![], None, 0)
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
        let item = &cache.emails.items[0];
        (
            Id::from_parts(item.thread_id, item.document_id).to_string(),
            item.document_id,
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
                        is_sender_authenticated: true,
                        is_spam: false,
                    },
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
                let archive = stored_metadata(test, account_id, stored[0]).await;
                assert!(
                    !is_stub(archive.unarchive::<MessageMetadata>().unwrap()),
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
                    None,
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

/// The metadata archive (field 71) of message `document_id`, if stored.
async fn metadata_archive(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
) -> Option<Archive<AlignedBytes>> {
    test.server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::property(
            account_id,
            Collection::Email,
            document_id,
            EmailField::Metadata,
        ))
        .await
        .unwrap()
}

/// Ruling R46 of plan 13 (review F1): destroying a message removes its
/// `MessageData` (field 50) at once and leaves its plaintext metadata
/// archive (field 71), blob link and search document to the scheduled
/// unindex task. While that task has not run, issuance answers the same 409
/// as for mail present (spec 6.6's "empty the mailbox, then provision"); it
/// succeeds once the task has run.
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

    let held = hold_task_consumer(test).await;
    admin.destroy_all_mailboxes_for_account(id).await;
    // The window: the message's `MessageData` is gone, its metadata archive
    // is not.
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
        "the metadata archive was removed before the unindex task ran"
    );
    // Ruling R51 would refuse on the message's search document too: it is
    // removed here, ahead of the held unindex task, so field 71 alone
    // carries the refusal (R46's pin on the internal search store).
    let mut query = SearchQuery::new(SearchIndex::Email);
    query
        .add_filter(SearchFilter::eq(SearchField::AccountId, id))
        .add_filter(SearchFilter::eq(SearchField::DocumentId, ids[0]));
    test.server.search_store().unindex(query).await.unwrap();
    assert!(
        !test.server.za_has_email_search_documents(id).await.unwrap(),
        "the message's search document is still stored"
    );
    let reply = za_post_as(admin, "setup-token", &json!({ "account": PENDING_UNINDEX })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account already holds mail",
        "issuance while the destroyed message's metadata archive is stored"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    drop(held);
    test.wait_for_tasks().await;
    assert!(
        metadata_archive(test, id, ids[0]).await.is_none(),
        "the unindex task left the metadata archive"
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
    // Only the sample is left: no message, no metadata archive.
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

/// Ruling R51 of plan 13 (re-review N1): the unindex task deletes a
/// message's search document in a write after the one that clears its
/// metadata archive, and that write can fail or be skipped, leaving an
/// Email search document that outlived its message. Issuance is refused
/// while the internal search store holds one for the account (planted here
/// through the search store, with no Email archive and no metadata archive)
/// and succeeds once it is removed.
async fn setup_refuses_an_orphaned_search_document(test: &TestServer, admin: &Account) -> Account {
    println!("setup_refuses_an_orphaned_search_document");
    let account = passwordless(admin, ORPHANED).await;
    let id = account.id().document_id();
    let search = test.server.search_store();
    assert!(
        search.internal_fts().is_some(),
        "the suite runs on the internal search store"
    );
    let mut document = IndexDocument::new(SearchIndex::Email)
        .with_account_id(id)
        .with_document_id(0);
    document.index_text(
        EmailSearchField::Subject,
        "orphaned search document",
        Language::English,
    );
    document.index_unsigned(EmailSearchField::ReceivedAt, now());
    search.index(vec![document]).await.unwrap();
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

    let mut query = SearchQuery::new(SearchIndex::Email);
    query
        .add_filter(SearchFilter::eq(SearchField::AccountId, id))
        .add_filter(SearchFilter::eq(SearchField::DocumentId, 0u32));
    search.unindex(query).await.unwrap();
    za_setup_token(admin, ORPHANED).await;
    account
}

/// The task records of `account_id` that ruling R54 counts (index, unindex,
/// thread merge, iTIP), in queue order.
async fn account_tasks(test: &TestServer, account_id: u32) -> Vec<Task> {
    let key = |id| ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task { id }));
    let mut tasks = Vec::new();
    test.server
        .store()
        .iterate(
            IterateParams::new(key(0), key(u64::MAX)).ascending(),
            |_, value| {
                let task = Task::deserialize(value)?;
                let owner = match &task {
                    Task::IndexDocument(task) | Task::UnindexDocument(task) => {
                        Some(task.account_id)
                    }
                    Task::MergeThreads(task) => Some(task.account_id),
                    Task::CalendarItipMessage(task) => Some(task.account_id),
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

/// Ruling R54 of plan 13 (PR #51 review 1, P2-1): the index worker builds an
/// ordinary message's full document, then writes it; the message's unindex
/// task runs in a worker of its own and can finish in between, so neither
/// the metadata archive (R46) nor the search document (R51) is there when
/// issuance checks, and the document lands afterwards. Issuance is refused
/// while the index task's record is in the queue; once the document is
/// written, R51's scan refuses on it; after it is removed, issuance succeeds.
async fn setup_refuses_an_in_flight_index_write(test: &TestServer, admin: &Account) -> Account {
    println!("setup_refuses_an_in_flight_index_write");
    test.wait_for_tasks().await;
    let account = passwordless(admin, IN_FLIGHT).await;
    let id = account.id().document_id();
    let message = message_with("in flight", "in-flight-body");
    // The delivery commits; its index task builds the document and parks.
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
    // The unindex task runs and clears its record while the index task
    // stays parked: only the index task's record is left.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let tasks = account_tasks(test, id).await;
        if metadata_archive(test, id, ids[0]).await.is_none()
            && matches!(tasks.as_slice(), [Task::IndexDocument(_)])
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the unindex task did not finish: {tasks:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !test
            .server
            .za_has_documents(id, Collection::Email)
            .await
            .unwrap()
            && !test.server.za_has_email_search_documents(id).await.unwrap(),
        "the account still holds the message or its search document"
    );
    let reply = za_post_as(admin, "setup-token", &json!({ "account": IN_FLIGHT })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account has unfinished tasks",
        "issuance while an index write of the account is in flight"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    parked.finish().await;
    test.wait_for_tasks().await;
    assert!(
        test.server.za_has_email_search_documents(id).await.unwrap(),
        "the released index task wrote no document"
    );
    let reply = za_post_as(admin, "setup-token", &json!({ "account": IN_FLIGHT })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account already holds mail",
        "issuance while the late document is stored"
    );
    let mut query = SearchQuery::new(SearchIndex::Email);
    query
        .add_filter(SearchFilter::eq(SearchField::AccountId, id))
        .add_filter(SearchFilter::eq(SearchField::DocumentId, ids[0]));
    test.server.search_store().unindex(query).await.unwrap();
    za_setup_token(admin, IN_FLIGHT).await;
    account
}

/// Ruling R60 of plan 13 (PR #51 review 2, the adjacent gap): the calendar
/// twin of `setup_refuses_an_in_flight_index_write`. An ordinary event's
/// index task builds its full document (summary, description, location,
/// participants) and parks; the event is deleted and its unindex task, in a
/// worker of its own, finds nothing to delete. Issuance is refused while the
/// index task's record is in the queue (R54); once the document is written
/// and the task cleared, R51's scan, which covers calendar documents too,
/// refuses on it; after it is removed, issuance succeeds.
async fn setup_refuses_an_in_flight_calendar_index_write(
    test: &TestServer,
    admin: &Account,
) -> Account {
    println!("setup_refuses_an_in_flight_calendar_index_write");
    test.wait_for_tasks().await;
    let account = passwordless(admin, IN_FLIGHT_EVENT).await;
    let id = account.id().document_id();
    // The event is planted while the consumer is held; released with the
    // pause set, the consumer dispatches its index task, which builds the
    // document and parks.
    let held = hold_task_consumer(test).await;
    let event_id = plant_event(test, id, "default", "in-flight.ics", IN_FLIGHT_ICAL, None).await;
    let parked = HookParked::start(id, test_hooks::set_index_write_pause, async move {
        drop(held);
    })
    .await;

    // The event is deleted: an unindex task, which runs and clears its
    // record while the index task stays parked.
    let account_info = test.server.account_info(id).await.unwrap();
    let archive = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            id,
            Collection::CalendarEvent,
            event_id,
        ))
        .await
        .unwrap()
        .expect("the event's archive");
    let mut batch = BatchBuilder::new();
    DestroyArchive(archive.to_unarchived::<CalendarEvent>().unwrap())
        .delete_all(&account_info, id, event_id, false, &mut batch)
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_task_queue();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let tasks = account_tasks(test, id).await;
        if matches!(tasks.as_slice(), [Task::IndexDocument(_)]) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the unindex task did not finish: {tasks:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
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
    test.wait_for_tasks().await;
    assert!(
        test.server
            .za_has_calendar_search_documents(id)
            .await
            .unwrap(),
        "the released index task wrote no document"
    );
    let reply = za_post_as(admin, "setup-token", &json!({ "account": IN_FLIGHT_EVENT })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account already holds calendar data",
        "issuance while the late calendar document is stored"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );
    let mut query = SearchQuery::new(SearchIndex::Calendar);
    query
        .add_filter(SearchFilter::eq(SearchField::AccountId, id))
        .add_filter(SearchFilter::eq(SearchField::DocumentId, event_id));
    test.server.search_store().unindex(query).await.unwrap();
    za_setup_token(admin, IN_FLIGHT_EVENT).await;
    account
}

/// Ruling R54 of plan 13 (PR #51 review 1, P2-3): an ordinary APPEND or JMAP
/// create schedules a thread-merge task whose `thread_name` and
/// `message_ids` are `CheekyHash` strings, which hold an input of up to 15
/// bytes verbatim; a task that exhausted its retries is kept as failed and
/// never runs again, and destroying the message leaves it. Issuance is
/// refused while one of the account's is in the queue (planted here as
/// failed, so the consumer never takes it) and succeeds once the operator
/// destroys it through `x:Task/set`.
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
    test.server.store().write(batch.build_all()).await.unwrap();
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

/// Ruling R58 of plan 13 (PR #51 review 2, P2-6): an index worker (A) that
/// built an ordinary message's full document and stalled past its task
/// lease is superseded on a cluster by another node's execution (B), which
/// finds the message gone and clears the task's record; R54 and R51 then
/// pass, and the account is provisioned. When A resumes, the internal search
/// store's write of its document asserts in its first commit chunk that the
/// account holds no vault record, so nothing of it lands; A's update is
/// dropped (its task record is gone) and nothing is rescheduled. B is
/// simulated by clearing the task's `Due` and `Task` records, what its
/// `Ignored` update clears; the harness has one task manager. An ordinary
/// account's document passes the same fence (the control), and its task
/// clearing shows that A's batch ended: the index worker runs one batch at a
/// time.
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
    // A: the delivery commits; its index task classifies the account as
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
    // The unindex task runs and clears its record while A stays parked.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let tasks = account_tasks(test, id).await;
        if metadata_archive(test, id, ids[0]).await.is_none()
            && matches!(tasks.as_slice(), [Task::IndexDocument(_)])
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the unindex task did not finish: {tasks:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // B, after A's lease expired: its retries find no message, and its
    // `Ignored` update clears the task's `Due` and `Task` records.
    let mut records = Vec::new();
    test.server
        .store()
        .iterate(
            IterateParams::new(
                ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task { id: 0 })),
                ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task { id: u64::MAX })),
            )
            .ascending(),
            |key, value| {
                if let Task::IndexDocument(task) = Task::deserialize(value)?
                    && task.account_id.document_id() == id
                {
                    let due = Task::IndexDocument(task).due_timestamp();
                    records.push((key.deserialize_be_u64(U64_LEN)?, due));
                }
                Ok(true)
            },
        )
        .await
        .unwrap();
    assert_eq!(records.len(), 1, "A's task record: {records:?}");
    let mut batch = BatchBuilder::new();
    for (task_id, due) in records {
        batch
            .clear(ValueClass::TaskQueue(TaskQueueClass::Due {
                id: task_id,
                due,
            }))
            .clear(ValueClass::TaskQueue(TaskQueueClass::Task { id: task_id }));
    }
    test.server.store().write(batch.build_all()).await.unwrap();
    assert!(
        account_tasks(test, id).await.is_empty(),
        "B's update left the task record"
    );

    // Every term passes: no message, no metadata archive, no task, no
    // sample, no search document.
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

    // A resumes and attempts its write; the control's index task runs after
    // A's batch and its update.
    parked.finish().await;
    deliver(
        test,
        SENDER,
        SUPERSEDED_CONTROL,
        &message_with("superseded control", "superseded-control-body"),
    )
    .await;
    test.wait_for_tasks().await;
    assert!(
        test.server
            .za_has_email_search_documents(control_id)
            .await
            .unwrap(),
        "the control's document was not written"
    );
    assert!(
        !test.server.za_has_email_search_documents(id).await.unwrap(),
        "the superseded write landed in the key account's index"
    );
    assert!(
        account_tasks(test, id).await.is_empty(),
        "the superseded execution was rescheduled"
    );

    admin.destroy_all_mailboxes_for_account(control_id).await;
    (account, control)
}

/// The account's raw keys in one search index of the internal search store
/// (`store/src/write/key.rs`): its `Document` keys, its `Term` keys and its
/// `Index` keys, and each `Term` key's `CheekyHash` payload, which is the
/// token itself for one of up to 15 bytes.
#[derive(Default)]
struct SearchKeys {
    documents: usize,
    terms: usize,
    fields: usize,
    term_payloads: Vec<Vec<u8>>,
}

impl SearchKeys {
    fn is_empty(&self) -> bool {
        self.documents == 0 && self.terms == 0 && self.fields == 0
    }

    fn holds_term(&self, token: &str) -> bool {
        self.term_payloads
            .iter()
            .any(|payload| payload == token.as_bytes())
    }
}

/// Counts only: the payloads of a large document would flood a failure.
impl std::fmt::Debug for SearchKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} document, {} term ({} canary) and {} field keys",
            self.documents,
            self.terms,
            if self.holds_term(TAIL_CANARY) {
                "with the"
            } else {
                "no"
            },
            self.fields
        )
    }
}

/// The keys of `account_id` in `index`, read raw from the search index
/// subspace. Key layout: the class (the index code, Email 0 and Calendar 1,
/// with the type in its top bits: `Term` 0, `Index` 0x40, `Document` 0x80),
/// the account id (u32 BE), then for a `Term` key the payload, its length
/// byte, the field and the document id (u32 BE).
async fn search_keys(test: &TestServer, account_id: u32, index: SearchIndex) -> SearchKeys {
    let store = test
        .server
        .search_store()
        .internal_fts()
        .expect("the suite runs on the internal search store");
    let code = match index {
        SearchIndex::Email => 0u8,
        SearchIndex::Calendar => 1u8,
        _ => panic!("no account keys are read for this index"),
    };
    let mut keys = SearchKeys::default();
    for class in [code, code | 0x40, code | 0x80] {
        let mut from = vec![class];
        from.extend_from_slice(&account_id.to_be_bytes());
        let mut to = from.clone();
        to.extend_from_slice(&[u8::MAX; 64]);
        let mut found: Vec<Vec<u8>> = Vec::new();
        store
            .iterate(
                IterateParams::new(
                    AnyKey {
                        subspace: SUBSPACE_SEARCH_INDEX,
                        key: from,
                    },
                    AnyKey {
                        subspace: SUBSPACE_SEARCH_INDEX,
                        key: to,
                    },
                )
                .no_values(),
                |key, _| {
                    found.push(key.to_vec());
                    Ok(true)
                },
            )
            .await
            .unwrap();
        match class & 0xC0 {
            0x00 => {
                keys.terms += found.len();
                keys.term_payloads
                    .extend(found.into_iter().filter_map(|key| {
                        key.get(1 + U32_LEN..key.len().checked_sub(2 + U32_LEN)?)
                            .map(<[u8]>::to_vec)
                    }));
            }
            0x40 => keys.fields += found.len(),
            _ => keys.documents += found.len(),
        }
    }
    keys
}

/// 2,400 distinct five-byte tokens (`c0000` to `c2399`, 16 to a line) and
/// then `canary`: with the headers indexed in test mode, about 2,500 index
/// operations, so three commit chunks (`BatchBuilder::commit_point` splits
/// above 1,000). `write_index` writes the terms in `CheekyHash` order, by
/// length first, so a canary longer than five bytes lands in a later chunk
/// than the first, and the `Index` keys (the subject and the addresses) in
/// the last.
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
/// alarm, no organizer (so its only tasks are its index and unindex
/// tasks), and a DESCRIPTION of `large_body`'s tokens and the canary,
/// folded at 70 octets.
fn chunked_ical() -> String {
    event_ical(
        "chunked-event@example.com",
        &large_body(TAIL_CANARY).replace("\r\n", " "),
    )
}

/// An event with `uid` and `description`, folded at 70 octets: no alarm,
/// no organizer, so its only tasks are its index and unindex tasks.
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

/// Waits (at most 10 s) until the account's unindex task has run and
/// cleared its record while its index task stays parked: the message's
/// metadata archive is gone, when `message_id` names one, and only the
/// index task's record is left.
async fn wait_for_the_unindex(test: &TestServer, account_id: u32, message_id: Option<u32>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let tasks = account_tasks(test, account_id).await;
        let unindexed = match message_id {
            Some(message_id) => metadata_archive(test, account_id, message_id)
                .await
                .is_none(),
            None => true,
        };
        if unindexed && matches!(tasks.as_slice(), [Task::IndexDocument(_)]) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the unindex task did not finish: {tasks:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Indexes `document` for the account straight into the search store, then
/// clears its `Document` key, leaving the `Term` and `Index` keys it wrote
/// unlisted: the shape of an orphaned tail, of a calendar merge that lost
/// its clears and of an Email re-index whose terms changed.
async fn plant_orphaned_keys(
    test: &TestServer,
    account_id: u32,
    index: SearchIndex,
    document: IndexDocument,
) {
    let search = test.server.search_store();
    search.index(vec![document]).await.unwrap();
    let mut batch = BatchBuilder::new();
    batch.clear(ValueClass::SearchIndex(SearchIndexClass {
        index,
        id: SearchIndexId::Account {
            account_id,
            document_id: 0,
        },
        typ: SearchIndexType::Document,
    }));
    search
        .internal_fts()
        .expect("the suite runs on the internal search store")
        .write(batch.build_all())
        .await
        .unwrap();
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

/// Ruling R62 of plan 13 (PR #51 review 3, P2-8): a search document's
/// `Term` and `Index` keys can outlive its `Document` key (a later chunk
/// written before R62's guard, a calendar merge that lost its clears, an
/// Email re-index whose terms changed), and R51's and R60's scans read the
/// `Document` keys only. Issuance is refused while the internal search
/// store holds any `Term` or `Index` key of the account in the Email index
/// (as for mail) or the Calendar index (as for calendar data); each case is
/// planted alone, and issuance succeeds once the account-wide unindex has
/// removed the keys. Neither scan reads the other index's keys or another
/// account's (ruling R63).
async fn setup_refuses_orphaned_search_terms(test: &TestServer, admin: &Account) -> Account {
    println!("setup_refuses_orphaned_search_terms");
    let account = passwordless(admin, ORPHANED_TERMS).await;
    let id = account.id().document_id();
    let search = test.server.search_store();
    assert!(
        search.internal_fts().is_some(),
        "the suite runs on the internal search store"
    );

    // Email `Term` keys only: Body is a text field, not an indexed one.
    let mut document = IndexDocument::new(SearchIndex::Email)
        .with_account_id(id)
        .with_document_id(0);
    document.index_text(EmailSearchField::Body, "orphanterm", Language::English);
    plant_orphaned_keys(test, id, SearchIndex::Email, document).await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.documents == 0 && keys.terms > 0 && keys.fields == 0,
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
    search
        .unindex(SearchQuery::new(SearchIndex::Email).with_account_id(id))
        .await
        .unwrap();
    assert!(
        !test.server.za_has_email_search_terms(id).await.unwrap(),
        "the account-wide unindex left Email terms"
    );

    // An Email `Index` key only: ReceivedAt is indexed and not text.
    let mut document = IndexDocument::new(SearchIndex::Email)
        .with_account_id(id)
        .with_document_id(0);
    document.index_unsigned(EmailSearchField::ReceivedAt, now());
    plant_orphaned_keys(test, id, SearchIndex::Email, document).await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.documents == 0 && keys.terms == 0 && keys.fields == 1,
        "an Email field alone was not planted: {keys:?}"
    );
    assert!(
        test.server.za_has_email_search_terms(id).await.unwrap()
            && !test.server.za_has_calendar_search_terms(id).await.unwrap(),
        "the scans do not see the orphaned Email field alone"
    );
    assert_refused(
        test,
        admin,
        (ORPHANED_TERMS, id),
        "account already holds mail",
        "an orphaned Email field of the account is stored",
    )
    .await;
    search
        .unindex(SearchQuery::new(SearchIndex::Email).with_account_id(id))
        .await
        .unwrap();
    assert!(
        !test.server.za_has_email_search_terms(id).await.unwrap(),
        "the account-wide unindex left an Email field"
    );

    // Calendar `Term` keys only: Title is a text field, not an indexed one.
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
        keys.documents == 0 && keys.terms > 0 && keys.fields == 0,
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
    search
        .unindex(SearchQuery::new(SearchIndex::Calendar).with_account_id(id))
        .await
        .unwrap();
    assert!(
        !test.server.za_has_calendar_search_terms(id).await.unwrap(),
        "the account-wide unindex left Calendar terms"
    );

    // Ruling R63: orphans of another account, at the adjacent id above
    // (account ids are assigned in order, so none holds it yet), are not
    // the account's: its scans stay false.
    let neighbour = id + 1;
    let mut document = IndexDocument::new(SearchIndex::Email)
        .with_account_id(neighbour)
        .with_document_id(0);
    document.index_text(EmailSearchField::Body, "orphanterm", Language::English);
    document.index_unsigned(EmailSearchField::ReceivedAt, now());
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
        search
            .unindex(SearchQuery::new(index).with_account_id(neighbour))
            .await
            .unwrap();
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

/// Ruling R62 of plan 13 (PR #51 review 3, P2-8): `Store::index` writes a
/// large document in several commit chunks, its `Document` key in the
/// first, and the message's unindex task, in a worker of its own, can run
/// between two of them: it deletes the `Document` key and every key that
/// key lists. Before R62 the later chunks then wrote `Term` and `Index`
/// keys that nothing lists (short tokens, the subject and the addresses
/// verbatim), the task cleared, and issuance passed R51's document scan.
/// With R62's guard every later chunk of a fenced document asserts that its
/// `Document` key is stored, so the rest of the document is not written,
/// the task ends without a retry (ruling R63: within `UNDER_THE_DEFERRAL`
/// of the release), and the account provisions cleanly.
async fn unindex_between_index_chunks_leaves_no_terms(
    test: &TestServer,
    admin: &Account,
) -> Account {
    println!("unindex_between_index_chunks_leaves_no_terms");
    test.wait_for_tasks().await;
    let account = passwordless(admin, CHUNKED).await;
    let id = account.id().document_id();
    let message = message_with("chunked subject", &large_body(TAIL_CANARY));
    // The delivery commits; its index task writes the document's first
    // chunk and parks before the second.
    let parked = HookParked::start(id, test_hooks::set_index_chunk_pause, async move {
        SmtpConnection::connect()
            .await
            .ingest(SENDER, &[CHUNKED], &message)
            .await;
    })
    .await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.documents == 1 && keys.terms > 0 && !keys.holds_term(TAIL_CANARY),
        "chunk 0 alone was not written: {keys:?}"
    );
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "{ids:?}");

    admin.destroy_all_mailboxes_for_account(id).await;
    wait_for_the_unindex(test, id, Some(ids[0])).await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(keys.is_empty(), "the unindex left chunk 0's keys: {keys:?}");
    let reply = za_post_as(admin, "setup-token", &json!({ "account": CHUNKED })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account has unfinished tasks",
        "issuance while the index write of the account is between chunks"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    let released = Instant::now();
    parked.finish().await;
    test.wait_for_tasks().await;
    let elapsed = released.elapsed();
    assert!(
        elapsed < UNDER_THE_DEFERRAL,
        "the index task ended {elapsed:?} after the release: deferred, not ended without an error"
    );
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.is_empty(),
        "the later chunks outlived the unindex: {keys:?}"
    );
    assert!(
        account_tasks(test, id).await.is_empty(),
        "the index task is still queued"
    );
    za_setup_token(admin, CHUNKED).await;
    account
}

/// Ruling R62 of plan 13: the calendar twin of
/// `unindex_between_index_chunks_leaves_no_terms`, built like
/// `setup_refuses_an_in_flight_calendar_index_write`. An ordinary event
/// whose description fills three commit chunks is indexed; its first chunk
/// is written, and the event is deleted before the second, so its unindex
/// task removes the `Document` key and what it lists. The later chunks
/// (the description's other tokens, the canary, the `Uid` verbatim in an
/// `Index` key) are not written.
async fn unindex_between_calendar_index_chunks_leaves_no_terms(
    test: &TestServer,
    admin: &Account,
) -> Account {
    println!("unindex_between_calendar_index_chunks_leaves_no_terms");
    test.wait_for_tasks().await;
    let account = passwordless(admin, CHUNKED_EVENT).await;
    let id = account.id().document_id();
    // The event is planted while the consumer is held; released with the
    // pause set, the consumer dispatches its index task, which writes the
    // document's first chunk and parks before the second.
    let held = hold_task_consumer(test).await;
    let event_id = plant_event(test, id, "default", "chunked.ics", &chunked_ical(), None).await;
    let parked = HookParked::start(id, test_hooks::set_index_chunk_pause, async move {
        drop(held);
    })
    .await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.documents == 1 && keys.terms > 0 && !keys.holds_term(TAIL_CANARY),
        "chunk 0 alone was not written: {keys:?}"
    );

    // The event is deleted: an unindex task, which runs and clears its
    // record while the index task stays parked.
    let account_info = test.server.account_info(id).await.unwrap();
    let archive = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            id,
            Collection::CalendarEvent,
            event_id,
        ))
        .await
        .unwrap()
        .expect("the event's archive");
    let mut batch = BatchBuilder::new();
    DestroyArchive(archive.to_unarchived::<CalendarEvent>().unwrap())
        .delete_all(&account_info, id, event_id, false, &mut batch)
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_task_queue();
    wait_for_the_unindex(test, id, None).await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(keys.is_empty(), "the unindex left chunk 0's keys: {keys:?}");
    let reply = za_post_as(admin, "setup-token", &json!({ "account": CHUNKED_EVENT })).await;
    assert_eq!(
        reply.expect(409)["error"],
        "account has unfinished tasks",
        "issuance while the calendar index write of the account is between chunks"
    );
    assert!(
        test.server.za_vault_record(id).await.unwrap().is_none(),
        "the refused issuance wrote a record"
    );

    parked.finish().await;
    test.wait_for_tasks().await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.is_empty(),
        "the later calendar chunks outlived the unindex: {keys:?}"
    );
    assert!(
        account_tasks(test, id).await.is_empty(),
        "the index task is still queued"
    );
    za_setup_token(admin, CHUNKED_EVENT).await;
    account
}

/// Ruling R62 of plan 13 (P2-8's second route): on a cluster, an index
/// worker (A) that wrote a large document's first chunk and stalled past
/// its task lease before the second is superseded by another node's
/// execution (B), which finds the message gone and clears the task's
/// record; the message's unindex has removed the `Document` key and what it
/// lists, so every term passes and the account is provisioned. R58's
/// assertion is in the first chunk only; when A resumes, its later chunks
/// assert that the `Document` key is stored (R62), so nothing of them lands
/// in the key account's index. B is simulated as in
/// `superseded_index_write_loses_to_provisioning`. An ordinary account's
/// multi-chunk document passes the guard (the control), and its task
/// clearing shows that A's batch ended.
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
    // A: the delivery commits; its index task writes the document's first
    // chunk and parks before the second.
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
    wait_for_the_unindex(test, id, Some(ids[0])).await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(keys.is_empty(), "the unindex left chunk 0's keys: {keys:?}");

    // B, after A's lease expired: its retries find no message, and its
    // `Ignored` update clears the task's `Due` and `Task` records.
    let mut records = Vec::new();
    test.server
        .store()
        .iterate(
            IterateParams::new(
                ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task { id: 0 })),
                ValueKey::from(ValueClass::TaskQueue(TaskQueueClass::Task { id: u64::MAX })),
            )
            .ascending(),
            |key, value| {
                if let Task::IndexDocument(task) = Task::deserialize(value)?
                    && task.account_id.document_id() == id
                {
                    let due = Task::IndexDocument(task).due_timestamp();
                    records.push((key.deserialize_be_u64(U64_LEN)?, due));
                }
                Ok(true)
            },
        )
        .await
        .unwrap();
    assert_eq!(records.len(), 1, "A's task record: {records:?}");
    let mut batch = BatchBuilder::new();
    for (task_id, due) in records {
        batch
            .clear(ValueClass::TaskQueue(TaskQueueClass::Due {
                id: task_id,
                due,
            }))
            .clear(ValueClass::TaskQueue(TaskQueueClass::Task { id: task_id }));
    }
    test.server.store().write(batch.build_all()).await.unwrap();
    assert!(
        account_tasks(test, id).await.is_empty(),
        "B's update left the task record"
    );

    // Every term passes: no message, no metadata archive, no task, no
    // sample, no search key.
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

    // A resumes and attempts its later chunks; the control's index task
    // runs after A's batch and its update.
    parked.finish().await;
    deliver(
        test,
        SENDER,
        SUPERSEDED_TAIL_CONTROL,
        &message_with("superseded tail control", &large_body(TAIL_CANARY)),
    )
    .await;
    test.wait_for_tasks().await;
    let control_keys = search_keys(test, control_id, SearchIndex::Email).await;
    assert!(
        control_keys.documents == 1
            && control_keys.terms > 2000
            && control_keys.holds_term(TAIL_CANARY)
            && control_keys.fields > 0,
        "the control's multi-chunk document was not written whole: {control_keys:?}"
    );
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.is_empty(),
        "the superseded tail landed in the key account's index: {keys:?}"
    );
    assert!(
        account_tasks(test, id).await.is_empty(),
        "the superseded execution was rescheduled"
    );

    admin.destroy_all_mailboxes_for_account(control_id).await;
    (account, control)
}

/// Replaces the stored event's iCalendar data as a PUT does
/// (`CalendarEvent::update`), which schedules its index task: a merge with
/// the event's stored `Document` key.
async fn update_event(test: &TestServer, account_id: u32, event_id: u32, ical: &str) {
    let archive = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEvent,
            event_id,
        ))
        .await
        .unwrap()
        .expect("the event's archive");
    let current = archive.to_unarchived::<CalendarEvent>().unwrap();
    let mut event = current.deserialize::<CalendarEvent>().unwrap();
    let parsed = match calcard::Parser::new(ical).entry() {
        calcard::Entry::ICalendar(ical) => ical,
        other => panic!("{other:?}"),
    };
    let mut next_alarm = None;
    event.size = ical.len() as u32;
    event.data = CalendarEventData::new(parsed, Tz::Floating, 100, &mut next_alarm);
    let account_info = test.server.account_info(account_id).await.unwrap();
    let mut batch = BatchBuilder::new();
    event
        .update(
            account_info.account_tenant_ids(),
            current,
            account_id,
            event_id,
            &mut batch,
        )
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_task_queue();
}

/// Deletes the stored event, which schedules its unindex task.
async fn delete_event(test: &TestServer, account_id: u32, event_id: u32) {
    let account_info = test.server.account_info(account_id).await.unwrap();
    let archive = test
        .server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::archive(
            account_id,
            Collection::CalendarEvent,
            event_id,
        ))
        .await
        .unwrap()
        .expect("the event's archive");
    let mut batch = BatchBuilder::new();
    DestroyArchive(archive.to_unarchived::<CalendarEvent>().unwrap())
        .delete_all(&account_info, account_id, event_id, false, &mut batch)
        .unwrap();
    test.server.commit_batch(batch).await.unwrap();
    test.server.notify_task_queue();
}

/// The merge tests' updated DESCRIPTION: a few tokens, none of the large
/// one's, ending with `MERGE_CANARY`.
fn short_description() -> String {
    format!("short merged text {MERGE_CANARY}")
}

/// Ruling R63 of plan 13 (round 5's re-reviews, opus F1 and fable F1):
/// FoundationDB evaluates an assertion's read error as a mismatch
/// (`AssertValueFailed`), so a transient read error at a later chunk's
/// guard stopped an ordinary document as R62 stops one whose `Document`
/// key an unindex deleted: its later chunks were never written and its
/// task cleared. `Store::index` now re-reads the `Document` key after a
/// later chunk's failed guard and returns the error while the key is
/// stored, so the worker defers the batch and the retry writes the
/// document whole. The read error is simulated by `INDEX_GUARD_FAIL_ONCE`
/// (the suites run no FoundationDB): an ordinary account's multi-chunk
/// message whose second chunk is not written and fails its guard. The
/// retry waits out the default deferral (at least 30 s).
async fn failed_guard_with_its_document_stored_defers_the_task(
    test: &TestServer,
    admin: &Account,
) -> Account {
    println!("failed_guard_with_its_document_stored_defers_the_task");
    test.wait_for_tasks().await;
    let account = passwordless(admin, GUARD_FAILED).await;
    let id = account.id().document_id();
    test_hooks::set_index_guard_fail_once(Some(id));
    SmtpConnection::connect()
        .await
        .ingest(
            SENDER,
            &[GUARD_FAILED],
            &message_with("guard failed", &large_body(TAIL_CANARY)),
        )
        .await;

    // The message and its index task commit together; the task writes chunk
    // 0, fails chunk 1's guard and ends deferred, or cleared if the
    // failure stopped the document.
    let deadline = Instant::now() + Duration::from_secs(10);
    let deferred = loop {
        let index_tasks = account_tasks(test, id)
            .await
            .into_iter()
            .filter(|task| matches!(task, Task::IndexDocument(_)))
            .collect::<Vec<_>>();
        match index_tasks.as_slice() {
            [task] if matches!(task.status(), TaskStatus::Retry(_)) => break true,
            [] if !message_ids(test, id).await.is_empty() => break false,
            _ => {}
        }
        assert!(
            Instant::now() < deadline,
            "the index task did not end: {index_tasks:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert!(
        deferred,
        "the failed guard stopped the document and its task was cleared"
    );
    assert!(
        test_hooks::INDEX_GUARD_FAIL_ONCE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none(),
        "the guard failure was not taken"
    );
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.documents == 1 && keys.terms > 0 && !keys.holds_term(TAIL_CANARY),
        "chunk 0 alone was not written: {keys:?}"
    );

    // The retry, once due, writes the document whole.
    test.wait_for_tasks().await;
    let keys = search_keys(test, id, SearchIndex::Email).await;
    assert!(
        keys.documents == 1 && keys.terms > 2000 && keys.holds_term(TAIL_CANARY) && keys.fields > 0,
        "the retry did not write the document whole: {keys:?}"
    );
    assert!(
        account_tasks(test, id).await.is_empty(),
        "the index task is still queued"
    );

    admin.destroy_all_mailboxes_for_account(id).await;
    account
}

/// Ruling R63 of plan 13 (opus F4, fable F3): a calendar update's index
/// write merges with the stored document: chunk 0 sets the new `Document`
/// key and the new-only keys, then come the clears of the old-only keys,
/// in later chunks when there are many. An unindex between two of those
/// chunks deletes the new `Document` key and what it lists; R62's guard
/// then stopped the rest, clears included, so the old version's keys that
/// the later chunks clear stayed, listed by nothing, and issuance answered
/// 409 for good. `Store::index` now writes a stopped document's remaining
/// clears without the guard and drops its sets, so nothing of the account
/// is left in the Calendar index and the account provisions.
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
        keys.documents == 1 && keys.terms > 2000 && keys.holds_term(TAIL_CANARY),
        "the event was not indexed whole: {keys:?}"
    );

    // The update is committed while the consumer is held; released with the
    // pause set, its merge writes chunk 0 (the new `Document` key, the
    // new-only keys, the first clears) and parks before the second.
    let held = hold_task_consumer(test).await;
    update_event(test, id, event_id, &event_ical(uid, &short_description())).await;
    let parked = HookParked::start(id, test_hooks::set_index_chunk_pause, async move {
        drop(held);
    })
    .await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.documents == 1 && keys.holds_term(MERGE_CANARY) && keys.terms > 1000,
        "chunk 0 of a merge whose clears span later chunks was not written alone: {keys:?}"
    );

    // The event is deleted: its unindex removes the new `Document` key and
    // what it lists, and leaves the old-only keys not yet cleared.
    delete_event(test, id, event_id).await;
    wait_for_the_unindex(test, id, None).await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.documents == 0 && !keys.holds_term(MERGE_CANARY) && keys.terms > 0,
        "the unindex left no old-only key for the later chunks to clear: {keys:?}"
    );

    parked.finish().await;
    test.wait_for_tasks().await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.is_empty(),
        "the stopped merge's old-only keys outlived the unindex: {keys:?}"
    );
    assert!(
        account_tasks(test, id).await.is_empty(),
        "the index task is still queued"
    );
    za_setup_token(admin, MERGE_UNINDEXED).await;
    account
}

/// Ruling R63 of plan 13 (fable F5): the merge path's positive control. An
/// ordinary event whose description fills three commit chunks is indexed,
/// then updated to a short description; its merge (the new `Document` key
/// and the new-only keys in chunk 0, the old-only keys' clears in later
/// chunks, each later chunk past the guard) leaves one `Document` key, the
/// new tokens, and none of the old-only ones.
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
        keys.documents == 1 && keys.terms > 2000 && keys.holds_term(TAIL_CANARY),
        "the event was not indexed whole: {keys:?}"
    );

    update_event(test, id, event_id, &event_ical(uid, &short_description())).await;
    test.wait_for_tasks().await;
    let keys = search_keys(test, id, SearchIndex::Calendar).await;
    assert!(
        keys.documents == 1
            && keys.holds_term(MERGE_CANARY)
            && !keys.holds_term(TAIL_CANARY)
            && (0..2400).all(|token| !keys.holds_term(&format!("c{token:04}"))),
        "the multi-chunk merge was not written whole: {keys:?}"
    );

    delete_event(test, id, event_id).await;
    test.wait_for_tasks().await;
    account
}

/// Ruling R63 of plan 13 (opus F2): a document that stops at a later
/// chunk's failed guard ends without an error, and the batch's other
/// documents are still written. One `Store::index` call holds two fenced
/// multi-chunk Email documents of two ordinary accounts, fenced as the
/// index worker fences them (`za_index_fence`); the worker's own batches
/// cannot be made to hold both deterministically, since its dispatcher
/// locks each task between two sends. The first document parks after its
/// chunk 0 and is unindexed before it resumes: the call returns `Ok`, the
/// first document leaves nothing, and the second, of an account the pause
/// does not park, is written whole.
async fn stopped_document_leaves_its_batch_written(
    test: &TestServer,
    admin: &Account,
) -> (Account, Account) {
    println!("stopped_document_leaves_its_batch_written");
    test.wait_for_tasks().await;
    let stopped = passwordless(admin, BATCH_STOPPED).await;
    let stopped_id = stopped.id().document_id();
    let other = passwordless(admin, BATCH_OTHER).await;
    let other_id = other.id().document_id();
    let document = |account_id: u32| {
        let mut document = IndexDocument::new(SearchIndex::Email)
            .with_account_id(account_id)
            .with_document_id(0);
        document.index_text(
            EmailSearchField::Body,
            &large_body(TAIL_CANARY),
            Language::English,
        );
        document.index_unsigned(EmailSearchField::ReceivedAt, now());
        test.server.za_index_fence(document, account_id)
    };
    let documents = vec![document(stopped_id), document(other_id)];
    let search = test.server.search_store().clone();
    let parked = HookParked::start(stopped_id, test_hooks::set_index_chunk_pause, async move {
        search.index(documents).await
    })
    .await;
    let keys = search_keys(test, stopped_id, SearchIndex::Email).await;
    assert!(
        keys.documents == 1 && keys.terms > 0 && !keys.holds_term(TAIL_CANARY),
        "chunk 0 alone was not written: {keys:?}"
    );
    let keys = search_keys(test, other_id, SearchIndex::Email).await;
    assert!(
        keys.is_empty(),
        "the batch's other document was written first: {keys:?}"
    );

    // The first document's unindex, as its task runs it.
    let mut query = SearchQuery::new(SearchIndex::Email);
    query
        .add_filter(SearchFilter::eq(SearchField::AccountId, stopped_id))
        .add_filter(SearchFilter::eq(SearchField::DocumentId, 0u32));
    test.server.search_store().unindex(query).await.unwrap();
    let keys = search_keys(test, stopped_id, SearchIndex::Email).await;
    assert!(keys.is_empty(), "the unindex left chunk 0's keys: {keys:?}");

    let result = parked.finish().await;
    assert!(
        result.is_ok(),
        "the stopped document ended its batch with an error: {result:?}"
    );
    let keys = search_keys(test, stopped_id, SearchIndex::Email).await;
    assert!(
        keys.is_empty(),
        "the stopped document's later chunks were written: {keys:?}"
    );
    let keys = search_keys(test, other_id, SearchIndex::Email).await;
    assert!(
        keys.documents == 1
            && keys.terms > 2000
            && keys.holds_term(TAIL_CANARY)
            && keys.fields == 1,
        "the batch's other document was not written whole: {keys:?}"
    );

    test.server
        .search_store()
        .unindex(SearchQuery::new(SearchIndex::Email).with_account_id(other_id))
        .await
        .unwrap();
    (stopped, other)
}
