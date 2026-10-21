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
use email::{
    cache::MessageCacheFetch,
    message::{
        metadata::MessageMetadata,
        za::{MAIL_FORMAT_V1, is_stub, open_mail_blob, open_stub},
    },
};
use registry::schema::structs::SpamTrainingSample;
use store::{
    IterateParams, Serialize, ValueKey,
    write::{AlignedBytes, Archive, Archiver, BatchBuilder, IndexPropertyClass, ValueClass},
};
use types::{blob_hash::BlobHash, collection::Collection, field::EmailField};
use utils::cheeky_hash::CheekyHash;

pub mod delivery;
pub mod fence;
pub mod login;
pub mod reads;
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

/// The message's metadata archive (`EmailField::Metadata`, field 71), as
/// stored.
pub async fn stored_metadata(
    test: &TestServer,
    account_id: u32,
    document_id: u32,
) -> Archive<AlignedBytes> {
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
        .unwrap_or_else(|| panic!("no metadata for message {account_id}/{document_id}"))
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
        .items
        .iter()
        .map(|m| m.document_id)
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
/// message: every hash of the account's property is scanned.
pub async fn threading_keys(test: &TestServer, account_id: u32, document_id: u32) -> usize {
    let key = |document_id, hash| ValueKey {
        account_id,
        collection: Collection::Email.into(),
        document_id,
        class: ValueClass::IndexProperty(IndexPropertyClass::Hash {
            property: EmailField::Threading.into(),
            hash,
        }),
    };
    let mut found = 0;
    test.server
        .store()
        .iterate(
            IterateParams::new(key(0, CheekyHash::NULL), key(u32::MAX, CheekyHash::FULL))
                .ascending()
                .no_values(),
            |key, _| {
                if key.ends_with(&document_id.to_be_bytes()) {
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
    let archive = stored_metadata(test, account_id, document_id).await;
    let stub = archive.unarchive::<MessageMetadata>().unwrap();
    assert!(
        is_stub(stub),
        "message {account_id}/{document_id} is stored in the clear"
    );
    // `0x01 || policy || wrap || ..`
    assert_eq!(stub.raw_headers[2], wrap, "the stub's envelope wrap");
    let blob = stored_blob(test, &BlobHash::from(&stub.blob_hash)).await;
    assert_eq!(blob[0], MAIL_FORMAT_V1, "the blob's format byte");
    assert_eq!(blob[2], wrap, "the blob's envelope wrap");
    assert_eq!(
        threading_keys(test, account_id, document_id).await,
        0,
        "a sealed message has no threading key"
    );
}

/// Mail spec 6.1: no message of the account is stored in the clear; every
/// message archive (`stored_metadata`) is a stub.
pub async fn assert_no_plaintext(test: &TestServer, account_id: u32) {
    for document_id in message_ids(test, account_id).await {
        let archive = stored_metadata(test, account_id, document_id).await;
        assert!(
            is_stub(archive.unarchive::<MessageMetadata>().unwrap()),
            "message {account_id}/{document_id} is stored in the clear"
        );
    }
}

/// The message opened with the account's keys, unlocked from `password`:
/// the full metadata and the raw message.
pub async fn open_message(
    test: &TestServer,
    account_id: u32,
    password: &str,
    document_id: u32,
) -> (MessageMetadata, Vec<u8>) {
    let keys = za_unlock(test, account_id, password).await;
    let private_key = test.server.za_private_key(&keys).await.unwrap();
    let archive = stored_metadata(test, account_id, document_id).await;
    let stub = archive.unarchive::<MessageMetadata>().unwrap();
    let metadata = open_stub(stub, &keys, Some(private_key), account_id)
        .unwrap_or_else(|err| panic!("the stub does not open: {err}"));
    let blob = stored_blob(test, &BlobHash::from(&stub.blob_hash)).await;
    let raw = open_mail_blob(&blob, &keys, Some(private_key), account_id)
        .unwrap_or_else(|err| panic!("the blob does not open: {err}"));
    (metadata, raw.to_vec())
}

/// Mail spec 10: the stub rewritten with one byte flipped in its bundle's
/// tag, at the end of `raw_headers`, as a well-formed archive: the archive
/// reads, the bundle does not open.
pub async fn corrupt_stub(test: &TestServer, account_id: u32, document_id: u32) {
    let mut stub = stored_metadata(test, account_id, document_id)
        .await
        .deserialize::<MessageMetadata>()
        .unwrap();
    assert_eq!(stub.raw_headers[0], MAIL_FORMAT_V1, "not a stub");
    let last = stub.raw_headers.len() - 1;
    stub.raw_headers[last] ^= 1;
    let mut batch = BatchBuilder::new();
    batch
        .with_account_id(account_id)
        .with_collection(Collection::Email)
        .with_document(document_id)
        .set(
            EmailField::Metadata,
            Archiver::new(stub).serialize().unwrap(),
        );
    test.server.store().write(batch.build_all()).await.unwrap();
}

/// Mail spec 10: the blob `hash` rewritten with one byte flipped in its
/// sealed body's tag.
pub async fn corrupt_blob(test: &TestServer, hash: &BlobHash) {
    let mut blob = stored_blob(test, hash).await;
    assert_eq!(blob[0], MAIL_FORMAT_V1, "not a sealed blob");
    let last = blob.len() - 1;
    blob[last] ^= 1;
    test.server
        .blob_store()
        .put_blob(hash.as_slice(), &blob, test.server.core.email.compression)
        .await
        .unwrap();
}

/// Spam training samples (`RegistryClass::Item` records), every account's.
pub async fn spam_samples(test: &TestServer) -> usize {
    test.account("admin@example.com")
        .registry_get_all::<SpamTrainingSample>()
        .await
        .len()
}
