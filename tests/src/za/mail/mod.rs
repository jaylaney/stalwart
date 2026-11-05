/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Release 3: mail encryption at rest for key accounts (mail spec). Each
//! module creates the `keymail<n>@example.com` accounts it names and leaves
//! no message, mailbox, app password or queued mail behind.

use super::user_permissions;
use crate::utils::{
    account::Account,
    server::TestServer,
    smtp::SmtpConnection,
    za::{wait_for_delivery, za_unlock},
};
use common::{Server, auth::AccessToken};
use email::{
    cache::MessageCacheFetch,
    mailbox::INBOX_ID,
    message::{
        ingest::{
            IngestEmail, IngestSource,
            group::{IngestGroup, ThreadKey},
        },
        metadata::MetadataRow,
        za::{MAIL_FORMAT_V1, is_stub, open_mail_blob, open_stub, stub_structure},
    },
};
use groupware::calendar::seal::WRAP_MK;
use mail_parser::{Message, MessageParser};
use registry::schema::structs::SpamTrainingSample;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use store::{
    Deserialize, IterateParams, ValueKey,
    write::{BatchBuilder, IndexPropertyClass, ValueClass, serialize::RawValue},
};
use tokio::sync::mpsc;
use trc::{
    Collector, EventType, Key,
    ipc::subscriber::{EventBatch, Interests, SubscriberBuilder},
};
use types::{blob::BlobId, blob_hash::BlobHash, collection::Collection, field::EmailField};
use utils::hash128::Hash128;

pub mod delivery;
pub mod fence;
pub mod gates;
pub mod gateway;
pub mod login;
pub mod reads;
pub mod search;
pub mod traces;
pub mod uploads;
pub mod writes;

/// A fresh key account, registered with the test server.
pub async fn key_mail_account(
    test: &mut TestServer,
    name: &'static str,
    password: &'static str,
) -> Account {
    let account = test
        .account("admin@example.com")
        .create_key_user_account(name, password, "Key mail", &[], user_permissions())
        .await;
    test.insert_account(account.clone());
    account
}

/// One message delivered over LMTP (the real queue and local delivery),
/// waited for until the queue and the index tasks are done.
pub async fn deliver(test: &TestServer, from: &str, to: &str, message: &str) {
    SmtpConnection::connect()
        .await
        .ingest(from, &[to], message)
        .await;
    wait_for_delivery(test).await;
}

/// A minimal RFC 5322 message with a unique `Message-ID`.
pub fn message_with(subject: &str, body: &str) -> String {
    format!(
        "From: Sender <sender@example.org>\r\n\
         To: Recipient <recipient@example.com>\r\n\
         Subject: {subject}\r\n\
         Message-ID: <{:016x}.za-mail@example.org>\r\n\
         Date: Tue, 20 Oct 2026 10:00:00 +0000\r\n\
         \r\n\
         {body}\r\n",
        store::rand::random::<u64>()
    )
}

/// The `SessionKeys` of `account_id` alive in the server's process
/// (instances, not `Arc` references).
pub fn live_keys(account_id: u32) -> usize {
    vault::session::live::session_keys(account_id)
}

/// The message's metadata row (`EmailField::Metadata`, field 71, in the
/// immutable class at 1.0), as stored: a stub for a key account's message.
pub async fn stored_metadata(test: &TestServer, account_id: u32, document_id: u32) -> MetadataRow {
    MetadataRow::deserialize(&stored_metadata_bytes(test, account_id, document_id).await)
        .unwrap_or_else(|err| panic!("message {account_id}/{document_id}: bad row: {err:?}"))
}

/// The message's metadata row's stored bytes (`stored_metadata`).
pub async fn stored_metadata_bytes(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
) -> Vec<u8> {
    test.server
        .store()
        .get_value::<RawValue>(metadata_key(account_id, document_id))
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("no metadata for message {account_id}/{document_id}"))
        .0
}

/// Whether the message has a metadata row at all.
pub async fn has_metadata(test: &TestServer, account_id: u32, document_id: u32) -> bool {
    test.server
        .store()
        .get_value::<RawValue>(metadata_key(account_id, document_id))
        .await
        .unwrap()
        .is_some()
}

/// The key of a message's metadata row (`read_metadata_batch`'s).
pub fn metadata_key(account_id: u32, document_id: u32) -> ValueKey<ValueClass> {
    ValueKey::immutable(
        account_id,
        Collection::Email,
        document_id,
        EmailField::Metadata,
    )
}

/// Every metadata row the account holds, by document id, read from the
/// store (not the message cache): a row a batch left behind without its
/// `MessageData` shows here and not in `message_ids`.
pub async fn metadata_rows(test: &TestServer, account_id: u32) -> Vec<(u32, MetadataRow)> {
    let mut rows = Vec::new();
    test.server
        .store()
        .iterate(
            IterateParams::new(
                metadata_key(account_id, 0),
                metadata_key(account_id, u32::MAX),
            ),
            |key, value| {
                let document_id = u32::from_be_bytes(key[key.len() - 4..].try_into().unwrap());
                rows.push((document_id, MetadataRow::deserialize(value)?));
                Ok(true)
            },
        )
        .await
        .unwrap();
    rows
}

/// The blob hash a stored row names: a stub's is the sealed blob's.
pub async fn stored_hash(test: &TestServer, account_id: u32, document_id: u32) -> BlobHash {
    stored_metadata(test, account_id, document_id)
        .await
        .unarchive()
        .unwrap()
        .blob_hash()
}

/// Whether a stored row is a stub (`is_stub`, the format byte at the head
/// of its header section).
pub fn row_is_stub(row: &MetadataRow) -> bool {
    is_stub(row).unwrap()
}

/// The blob `hash`, decoded as readers decode it.
pub async fn stored_blob(test: &TestServer, hash: &BlobHash) -> Vec<u8> {
    test.server
        .blob_store()
        .get_blob(hash.as_slice(), 0..usize::MAX)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("blob {hash:?} not found"))
}

/// The account's message document ids, from the message cache, ascending.
pub async fn message_ids(test: &TestServer, account_id: u32) -> Vec<u32> {
    let mut ids = test
        .server
        .get_cached_messages(account_id)
        .await
        .unwrap()
        .emails
        .document_ids()
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

/// The one message the account gained since `before` (its `message_ids`).
pub async fn new_message(test: &TestServer, account_id: u32, before: &[u32]) -> u32 {
    let new = message_ids(test, account_id)
        .await
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect::<Vec<_>>();
    assert_eq!(new.len(), 1, "expected one new message, found {new:?}");
    new[0]
}

/// The threading index keys (`EmailField::Threading`, property 90) of one
/// message: every hash of the account's property is scanned. At 1.0 the
/// key ends with the inverted document id (`store/src/write/key.rs`), so
/// the scan runs from the highest document id (inverted: 0) with the
/// lowest hash to the lowest (inverted: `u32::MAX`) with the highest.
pub async fn threading_keys(test: &TestServer, account_id: u32, document_id: u32) -> usize {
    let key = |document_id, hash: u128| ValueKey {
        account_id,
        collection: Collection::Email.into(),
        document_id,
        class: ValueClass::IndexProperty(IndexPropertyClass::Hash {
            property: EmailField::Threading.into(),
            hash: Hash128::from(hash),
        }),
    };
    let suffix = (!document_id).to_be_bytes();
    let mut found = 0;
    test.server
        .store()
        .iterate(
            IterateParams::new(key(u32::MAX, 0), key(0, u128::MAX))
                .ascending()
                .no_values(),
            |key, _| {
                if key.ends_with(&suffix) {
                    found += 1;
                }
                Ok(true)
            },
        )
        .await
        .unwrap();
    found
}

/// Mail spec 5.1, 5.2, 5.4: the message is a stub whose bundle and blob are
/// wrapped as `wrap` (`WRAP_PK` or `WRAP_MK`), and it has no threading key.
pub async fn assert_sealed(test: &TestServer, account_id: u32, document_id: u32, wrap: u8) {
    let stub = stored_metadata(test, account_id, document_id).await;
    assert!(
        row_is_stub(&stub),
        "message {account_id}/{document_id} is stored in the clear"
    );
    // `0x01 || policy || wrap || ..`
    assert_eq!(
        stub.raw_headers().unwrap()[2],
        wrap,
        "the stub's envelope wrap"
    );
    let blob = stored_blob(test, &stub.unarchive().unwrap().blob_hash()).await;
    assert_eq!(blob[0], MAIL_FORMAT_V1, "the blob's format byte");
    assert_eq!(blob[2], wrap, "the blob's envelope wrap");
    assert_eq!(
        threading_keys(test, account_id, document_id).await,
        0,
        "a sealed message has no threading key"
    );
}

/// Mail spec 5.7: the upload `blob_id` is stored as a sealed blob under the
/// session's mail wrapping key (`mk`) that holds none of `plain` and opens,
/// with the account's keys unlocked from `password`, into `plain`.
pub async fn assert_sealed_upload(
    test: &TestServer,
    account_id: u32,
    password: &str,
    blob_id: &str,
    plain: &[u8],
) {
    let hash = BlobId::from_base32(blob_id)
        .unwrap_or_else(|| panic!("invalid blob id {blob_id}"))
        .hash;
    let blob = stored_blob(test, &hash).await;
    assert_eq!(blob[0], MAIL_FORMAT_V1, "the upload is stored in the clear");
    // `0x01 || policy || wrap || ..`
    assert_eq!(blob[2], WRAP_MK, "the upload's envelope wrap");
    assert!(
        !blob.windows(plain.len()).any(|w| w == plain),
        "the sealed upload holds its plaintext"
    );
    let keys = za_unlock(test, account_id, password).await;
    let opened = open_mail_blob(&blob, &keys, None, account_id)
        .unwrap_or_else(|err| panic!("the upload does not open: {err}"));
    assert_eq!(opened.as_slice(), plain, "the opened upload");
}

/// Mail spec 6.1: no message of the account is stored in the clear: every
/// metadata row the account holds (`metadata_rows`, so a row without its
/// `MessageData` counts too) is a stub.
pub async fn assert_no_plaintext(test: &TestServer, account_id: u32) {
    for (document_id, row) in metadata_rows(test, account_id).await {
        assert!(
            row_is_stub(&row),
            "message {account_id}/{document_id} is stored in the clear"
        );
    }
}

/// The message opened with the account's keys, unlocked from `password`:
/// the full metadata row and the raw message.
pub async fn open_message(
    test: &TestServer,
    account_id: u32,
    password: &str,
    document_id: u32,
) -> (MetadataRow, Vec<u8>) {
    let keys = za_unlock(test, account_id, password).await;
    let private_key = test.server.za_private_key(&keys).await.unwrap();
    let stub = stored_metadata(test, account_id, document_id).await;
    let metadata = open_stub(&stub, &keys, Some(private_key), account_id)
        .unwrap_or_else(|err| panic!("the stub does not open: {err}"));
    let blob = stored_blob(test, &stub.unarchive().unwrap().blob_hash()).await;
    let raw = open_mail_blob(&blob, &keys, Some(private_key), account_id)
        .unwrap_or_else(|err| panic!("the blob does not open: {err}"));
    (metadata, raw.to_vec())
}

/// Mail spec 10: the stub rewritten with one byte flipped in its bundle's
/// tag, at the end of its header section, as a well-formed row: the row
/// reads (its structure is rebuilt by `stub_structure` for the same blob
/// and lengths), the bundle does not open.
pub async fn corrupt_stub(test: &TestServer, account_id: u32, document_id: u32) {
    let stub = stored_metadata(test, account_id, document_id).await;
    let mut bundle = stub.raw_headers().unwrap().into_owned();
    assert_eq!(bundle[0], MAIL_FORMAT_V1, "not a stub");
    let structure = stub.unarchive().unwrap();
    let (blob_hash, sealed_len) = (structure.blob_hash(), structure.blob_size());
    let last = bundle.len() - 1;
    bundle[last] ^= 1;
    let row =
        MetadataRow::encode(stub_structure(blob_hash, bundle.len(), sealed_len), &bundle).unwrap();
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Email)
        .with_document(document_id)
        .set(EmailField::Metadata, row);
    test.server.store().write_batch(&mut batch).await.unwrap();
}

/// Mail spec 10: the blob `hash` rewritten with one byte flipped in its
/// sealed body's tag.
pub async fn corrupt_blob(test: &TestServer, hash: &BlobHash) {
    let mut blob = stored_blob(test, hash).await;
    assert_eq!(blob[0], MAIL_FORMAT_V1, "not a sealed blob");
    let last = blob.len() - 1;
    blob[last] ^= 1;
    // A filesystem blob store keeps a file whose length already matches (its
    // blobs are content-addressed), so the old one goes first.
    test.server
        .blob_store()
        .delete_blob(hash.as_slice())
        .await
        .unwrap();
    test.server
        .blob_store()
        .put_blob(hash.as_slice(), &blob, test.server.core.email.compression)
        .await
        .unwrap();
}

/// The parameters IMAP APPEND passes for one message into INBOX
/// (`imap/src/op/append.rs`): `message` is the parsed `raw`, or `None` for
/// a message that did not parse (a rejection, which a group keeps its
/// batch after).
pub fn append_params<'x>(
    raw: &'x str,
    message: Option<&'x Message<'x>>,
    access_token: &'x AccessToken,
) -> IngestEmail<'x> {
    IngestEmail {
        raw_message: raw.as_bytes(),
        message,
        blob_hash: None,
        access_token,
        mailbox_ids: vec![INBOX_ID],
        keywords: vec![],
        received_at: None,
        source: IngestSource::Imap {
            train_classifier: true,
        },
        metadata: None,
        session_id: 0,
    }
}

/// `raw` ingested into `group` as a grouped IMAP APPEND ingests it: parsed,
/// with its thread key (`ThreadKey::new`), so messages of one thread in the
/// group share a pending thread (`IngestGroup::ingest`). Nothing is written
/// until `IngestGroup::flush`.
pub async fn group_ingest(
    server: &Server,
    group: &mut IngestGroup,
    access_token: &AccessToken,
    raw: &str,
) -> trc::Result<()> {
    let message = MessageParser::new().parse(raw.as_bytes());
    let key = message.as_ref().map(ThreadKey::new).unwrap_or_default();
    group
        .ingest(
            server,
            append_params(raw, message.as_ref(), access_token),
            key,
        )
        .await
}

/// A token of `account_id` without keys: an ordinary account's session, or
/// one that classified the account before it was provisioned.
pub fn keyless_token(account_id: u32) -> AccessToken {
    AccessToken::from_id_maybe_invalid(account_id)
}

/// A token of `account_id` holding its keys, unlocked from `password`, as
/// a key account's IMAP or HTTP session holds them.
pub async fn keyed_token(test: &TestServer, account_id: u32, password: &str) -> AccessToken {
    AccessToken::from_id_maybe_invalid(account_id)
        .with_session_keys(za_unlock(test, account_id, password).await)
}

/// A reply to `parent` (a `message_with` message): the same thread name
/// (`Re:` and the parent's subject) and the parent's `Message-ID` in its
/// `In-Reply-To` and `References`, so the two share a thread key.
pub fn reply_to(parent: &str, subject: &str, body: &str) -> String {
    let parent_id = parent
        .lines()
        .find_map(|line| line.strip_prefix("Message-ID: "))
        .expect("the parent has a Message-ID")
        .to_string();
    message_with(&format!("Re: {subject}"), body).replace(
        "Date: ",
        &format!("In-Reply-To: {parent_id}\r\nReferences: {parent_id}\r\nDate: "),
    )
}

/// Spam training samples (`RegistryClass::Item` records), every account's.
pub async fn spam_samples(test: &TestServer) -> usize {
    test.account("admin@example.com")
        .registry_get_all::<SpamTrainingSample>()
        .await
        .len()
}

/// Numbers the subscribers `Traced` registers. Each capture has an id of
/// its own: the collector drops a closed subscriber without removing its id
/// from the active list, so a capture dropped without `finish` (a panicking
/// step) would otherwise let a later capture under the same id return
/// before its own registration was applied (review F6).
static TRACED_SUBSCRIBERS: AtomicU64 = AtomicU64::new(0);

/// Events of the given types emitted while a step runs, captured as
/// `raw_input.rs` captures them: the test server configures no tracer, so
/// nothing is emitted until something declares interest.
pub struct Traced {
    id: String,
    _tx: mpsc::Sender<EventBatch>,
    rx: mpsc::Receiver<EventBatch>,
}

/// One captured event.
#[derive(Debug)]
pub struct TracedEvent {
    pub typ: EventType,
    pub account_id: Option<u64>,
    pub details: String,
    pub reason: String,
    /// The recipient a delivery's error names (`Key::To`).
    pub to: String,
    /// Every key and value, for content checks.
    pub text: String,
    /// The keys and values as emitted.
    pub keys: Vec<(Key, trc::Value)>,
}

impl Traced {
    pub fn start(types: &[EventType]) -> Self {
        let mut interests = Interests::default();
        for event_type in types {
            interests.set(*event_type);
        }
        let id = format!(
            "za-mail-traced-{}",
            TRACED_SUBSCRIBERS.fetch_add(1, Ordering::Relaxed)
        );
        let (_tx, rx, _) = SubscriberBuilder::new(id.clone())
            .set_interests(types.iter().copied())
            .with_lossy(false)
            .register();
        Collector::union_interests(interests);
        Collector::reload();
        wait_for_subscriber(&id, true);
        Traced { id, _tx, rx }
    }

    pub async fn finish(mut self) -> Vec<TracedEvent> {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let mut captured = Vec::new();
        while let Ok(batch) = self.rx.try_recv() {
            for event in batch {
                let mut traced = TracedEvent {
                    typ: event.inner.typ,
                    account_id: None,
                    details: String::new(),
                    reason: String::new(),
                    to: String::new(),
                    text: String::new(),
                    keys: event.keys.clone(),
                };
                for (key, value) in event.keys.iter() {
                    match (key, value) {
                        (Key::AccountId, trc::Value::UInt(id)) => traced.account_id = Some(*id),
                        (Key::Details, trc::Value::String(text)) => {
                            traced.details = text.to_string()
                        }
                        (Key::Reason, trc::Value::String(text)) => traced.reason = text.to_string(),
                        (Key::To, trc::Value::String(text)) => traced.to = text.to_string(),
                        _ => {}
                    }
                    traced.text.push_str(&format!("{key:?}={value:?} "));
                }
                captured.push(traced);
            }
        }
        Collector::remove_subscriber(self.id.clone());
        Collector::reload();
        wait_for_subscriber(&self.id, false);
        captured
    }
}

/// Waits until the collector has applied the subscriber's registration
/// (`present`) or its removal. It applies them on its own thread, once per
/// pass, and an event it collects before then never reaches the subscriber:
/// a step that acts in process right after `start` (`fence.rs` calls
/// `copy_message`) would otherwise race it.
fn wait_for_subscriber(subscriber_id: &str, present: bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while Collector::get_subscribers()
        .iter()
        .any(|id| id == subscriber_id)
        != present
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the collector did not apply the subscriber update"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
