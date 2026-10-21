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
    assert_no_plaintext, assert_sealed, deliver,
    delivery::{configure, temporarily_failed, unconfigure},
    message_ids, message_with, stored_blob, stored_metadata,
};
use crate::{
    utils::{
        account::Account,
        server::TestServer,
        smtp::SmtpConnection,
        za::{HookParked, wait_for_delivery, za_post_as, za_setup, za_setup_token},
    },
    za::{STRONG, user_permissions},
};
use common::{Server, auth::AccessToken, za::test_hooks};
use email::{
    cache::MessageCacheFetch,
    mailbox::INBOX_ID,
    message::{
        delete::EmailDeletion,
        ingest::{EmailIngest, IngestEmail, IngestSource},
        metadata::MessageMetadata,
        za::is_stub,
    },
};
use groupware::calendar::seal::WRAP_PK;
use mail_parser::MessageParser;
use registry::{
    schema::{
        enums::TaskStoreMaintenanceType,
        prelude::{Object, ObjectInner, ObjectType},
        structs::{self, Credential, PasswordCredential, Task, TaskStatus, TaskStoreMaintenance},
    },
    types::id::ObjectId,
};
use serde_json::json;
use std::time::{Duration, Instant};
use store::{
    IterateParams, U32_LEN, U64_LEN, ValueKey,
    registry::write::{RegistryWrite, RegistryWriteResult},
    roaring::RoaringBitmap,
    write::{BatchBuilder, BlobLink, BlobOp, ValueClass, key::DeserializeBigEndian, now},
};
use types::{
    blob::BlobClass,
    blob_hash::{BLOB_HASH_LEN, BlobHash},
    collection::Collection,
    id::Id,
};
use vault::{ZA_MARKER, record::VaultRecord};

const FENCE1: &str = "fence1@example.com";
const FENCE2: &str = "fence2@example.com";
const FENCE3: &str = "fence3@example.com";
const FENCE4: &str = "fence4@example.com";
const FENCE5: &str = "fence5@example.com";
const FENCE6: &str = "fence6@example.com";
const FENCE7: &str = "fence7@example.com";
const FENCE8: &str = "fence8@example.com";
const FENCE9: &str = "fence9@example.com";
const PLAIN: &str = "plainmail3@example.com";
const SENDER: &str = "sender@example.com";
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
    let fence3 = reverse_race_setup_token_aborts(test, &admin).await;
    let fence4 = reverse_race_reissue_keeps_the_marker(test, &admin).await;
    let (fence5, fence6) = copy_message_race(test, &admin).await;
    let fence7 = stress_the_fence(test, &admin).await;
    let fence8 = pg_and_mysql_are_refused(test, &admin).await;
    let fence9 = setup_refuses_existing_mail(test, &admin).await;

    // The accounts that log in empty their own mailboxes; the others are
    // pending or credentialless, and the account's destruction removes
    // what they hold.
    for account in [&fence1, &fence2, &plain, &fence3, &fence4, &fence5, &fence6] {
        test.destroy_all_mailboxes(account).await;
    }
    for account in [
        fence1, fence2, plain, fence3, fence4, fence5, fence6, fence7, fence8, fence9,
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
/// ordinary classification: waits until it either stored a message or left
/// a temporary failure in the queue, then asserts the latter.
async fn assert_first_attempt_lost(test: &TestServer, address: &str, account_id: u32) {
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
    assert!(
        message_ids(test, account_id).await.is_empty(),
        "the stale delivery stored a message"
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
/// `setup-token` writes it, then invalidates the account's caches.
async fn set_marker(server: &Server, id: Id, marker: bool) {
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
    parked.finish().await;

    assert_first_attempt_lost(test, FENCE1, id).await;
    wait_for_delivery(test).await;
    let ids = message_ids(test, id).await;
    assert_eq!(ids.len(), 1, "the queue's retry delivered the message");
    assert_sealed(test, id, ids[0], WRAP_PK).await;
    assert_no_plaintext(test, id).await;
    account
}

/// Review F1b: a delivery Sieve modified stages its blob before the batch.
/// When the batch then loses the assertion, the staged blob is left to its
/// temporary link, never deleted: the plain account that ingested the
/// identical bytes keeps them, and the purge removes only the key account's
/// expired link. One transaction reaches both accounts, whose identical
/// Sieve scripts make the identical bytes; the session delivers recipients
/// in address order, so the plain account's ingest waits behind the parked
/// one and links the hash the lost ingest staged.
async fn forward_race_with_a_staged_blob(test: &TestServer, admin: &Account) -> (Account, Account) {
    println!("forward_race_with_a_staged_blob");
    let account = passwordless(admin, FENCE2).await;
    let id = account.id().document_id();
    let plain = admin
        .create_user_account(PLAIN, STRONG, "Plain mail", &[], user_permissions())
        .await;
    let plain_id = plain.id().document_id();
    plain
        .jmap_client()
        .await
        .sieve_script_create("fence", SCRIPT, true)
        .await
        .unwrap();
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
            .ingest(SENDER, &[PLAIN, FENCE2], &message)
            .await;
    })
    .await;
    let token = za_setup_token(admin, FENCE2).await;
    za_setup(FENCE2, &token, STRONG).await;
    parked.finish().await;

    assert_first_attempt_lost(test, FENCE2, id).await;
    let plain_ids = message_ids(test, plain_id).await;
    assert_eq!(plain_ids.len(), 1, "the plain account's copy");
    let staged = message_hash(test, plain_id, plain_ids[0]).await;
    assert!(
        temporary_links(test, id).await.contains(&staged),
        "the key account's lost ingest staged the plain copy's hash"
    );
    let plain_link = BlobClass::Linked {
        account_id: plain_id,
        collection: Collection::Email.into(),
        document_id: plain_ids[0],
    };
    assert!(
        test.server
            .store()
            .blob_has_access(&staged, &plain_link)
            .await
            .unwrap(),
        "the plain account links the staged hash"
    );
    let staged_bytes = stored_blob(test, &staged).await;
    assert!(
        staged_bytes.windows(16).any(|w| w == b"X-Sieve-Fence: 1"),
        "the staged blob is the Sieve-modified message"
    );

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

    // The purge as the scheduler runs it, once every temporary link expired.
    test.blob_expire_all().await;
    admin
        .registry_create_object(Task::StoreMaintenance(TaskStoreMaintenance {
            maintenance_type: TaskStoreMaintenanceType::PurgeBlob,
            shard_index: None,
            status: TaskStatus::now(),
        }))
        .await;
    test.wait_for_tasks().await;
    assert_eq!(
        stored_blob(test, &staged).await,
        staged_bytes,
        "the plain account's copy survives the purge"
    );
    assert!(
        test.server
            .store()
            .blob_has_access(&staged, &plain_link)
            .await
            .unwrap()
    );
    assert!(
        temporary_links(test, id).await.is_empty(),
        "a temporary link of the key account survived the purge"
    );
    (account, plain)
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
/// a served public key; nothing is stored, the reissue's recheck finds no
/// mail, answers 200 and keeps the marker, and setup lets the retry deliver.
async fn reverse_race_reissue_keeps_the_marker(test: &TestServer, admin: &Account) -> Account {
    println!("reverse_race_reissue_keeps_the_marker");
    let account = passwordless(admin, FENCE4).await;
    let id = account.id();
    za_setup_token(admin, FENCE4).await;
    za_setup_token(admin, FENCE4).await;

    SmtpConnection::connect()
        .await
        .ingest(SENDER, &[FENCE4], &message_with("pending", "pending-body"))
        .await;
    wait_for_temporary_failure(test, FENCE4).await;
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

    let caller = admin.clone();
    let parked = HookParked::start(to_id, test_hooks::set_copy_pause, async move {
        caller
            .jmap_method_call(
                "Email/copy",
                json!({
                    "fromAccountId": Id::from(from_id).to_string(),
                    "accountId": Id::from(to_id).to_string(),
                    "create": {
                        "c1": {
                            "id": email_id,
                            "mailboxIds": { Id::from(INBOX_ID).to_string(): true }
                        }
                    }
                }),
            )
            .await
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
/// The start of either side is delayed by 0-7 ms, alternately.
async fn stress_the_fence(test: &TestServer, admin: &Account) -> Account {
    println!("stress_the_fence");
    let account = passwordless(admin, FENCE7).await;
    let id = account.id();
    let account_id = id.document_id();
    // The system folders, before the runs race on their creation.
    message_ids(test, account_id).await;
    let mut outcomes = [0usize; 3];
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
            server
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
                .map(|_| ())
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
        let ingested = ingest.await.unwrap();
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
