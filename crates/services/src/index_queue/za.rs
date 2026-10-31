/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access fork: the index worker's key-account gates and its test
//! hooks. The worker never holds a key: a key account's message is indexed
//! from its stored stub without a blob read (mail spec 5.5), a key
//! account's event is not indexed (calendar spec 9), and a document built
//! from an account read as ordinary carries the vault-absence assertion
//! that `account_batch` places in every commit chunk of its pass (mail
//! ruling R58, Task 1's ruling 18).

use common::Server;
use email::message::{
    metadata::{ArchivedMessageMetadata, MetadataRow},
    za::{is_stub, is_stub_headers},
};
use groupware::calendar::{ArchivedCalendarEventContent, seal::archived_event_is_sealed};
use std::borrow::Cow;
use store::{search::IndexDocument, write::SearchIndex};
use trc::AddContext;

/// Mail spec 5.5: the document of a key account's message, or of a stub
/// whose account no longer exists (the destroyed-account window), built
/// from what is stored in the clear without a key and without reading the
/// blob; `None` for every other message, which takes upstream's builder
/// (an existing ordinary account's message keeps upstream's document
/// whatever its header section starts with, ruling R47).
///
/// 0.16 indexed the arrival time and the size here. 1.0's Email index has
/// no field for either (`EmailSearchField` holds text fields only; both are
/// served from the message cache), so the document holds the account and
/// document ids and nothing else: no term is derived from a key account's
/// message.
pub(crate) async fn za_email_document(
    server: &Server,
    account_id: u32,
    document_id: u32,
    row: &MetadataRow,
) -> trc::Result<Option<IndexDocument>> {
    let visible = server.za_is_key_account(account_id).await?
        || (is_stub(row).caused_by(trc::location!())?
            && server
                .try_account(account_id)
                .await
                .caused_by(trc::location!())?
                .is_none());
    Ok(visible.then(|| {
        IndexDocument::new(SearchIndex::Email)
            .with_account_id(account_id)
            .with_document_id(document_id)
    }))
}

/// Calendar spec 9: nothing readable to index for a key account.
pub(crate) async fn za_skips_calendar(server: &Server, account_id: u32) -> trc::Result<bool> {
    server.za_is_key_account(account_id).await
}

/// Calendar spec 9: a sealed archive has nothing to index. Reached when the
/// account was destroyed after the key-account check (plan 3 ruling R8), or
/// for a plain event whose root ends with a client-written `X-ZA-KEY`.
pub(crate) fn za_is_sealed_event(content: &ArchivedCalendarEventContent) -> bool {
    archived_event_is_sealed(content)
}

/// The un-index path's thread subject (mail spec 5.2): a stub's header
/// section is its sealed bundle, which names no `Subject`. A stub's
/// structure has no header entry, so `thread_subject()` resolves it without
/// reading the header section; this keeps a header section that starts
/// with the stub's format byte from being read as headers if one reaches
/// here anyway.
pub(crate) fn za_thread_subject_in<'m>(
    metadata: &'m ArchivedMessageMetadata,
    headers: &[u8],
) -> Cow<'m, str> {
    if is_stub_headers(headers) {
        Cow::Borrowed("")
    } else {
        metadata.thread_subject_in(headers)
    }
}

/// Test hooks of the index worker, compiled only under `test_mode`. None
/// changes behaviour while unset.
///
/// - `INDEX_BUILD_FAIL_ONCE`: the named account's next Email or Calendar
///   document build fails, so its partition fails and retries with backoff
///   (`index_queue::manager`), its queue entries kept: the provisioning
///   fence's 409 while a partition is in retry, and its remedy, an
///   `x:IndexQueueStatus/set` reset of the partition's retry state.
#[cfg(feature = "test_mode")]
pub mod test_hooks {
    use std::sync::Mutex;

    pub static INDEX_BUILD_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

    pub fn set_index_build_fail_once(account_id: Option<u32>) {
        *INDEX_BUILD_FAIL_ONCE
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = account_id;
    }

    /// Fails once for the account `INDEX_BUILD_FAIL_ONCE` names, and clears
    /// it.
    pub(crate) fn index_build_fault(account_id: u32) -> trc::Result<()> {
        let mut slot = INDEX_BUILD_FAIL_ONCE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if *slot == Some(account_id) {
            *slot = None;
            Err(trc::StoreEvent::UnexpectedError
                .into_err()
                .details("Injected index build failure"))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use email::message::za::{MAIL_FORMAT_V1, stub_structure};
    use store::Deserialize;
    use types::blob_hash::BlobHash;

    fn stub_row() -> MetadataRow {
        let mut bundle = vec![MAIL_FORMAT_V1];
        bundle.extend_from_slice(b"Subject: sealed bytes that are not headers\r\n");
        let structure = stub_structure(BlobHash::generate(b"sealed blob"), bundle.len(), 64);
        let bytes = MetadataRow::encode(structure, &bundle).unwrap();
        MetadataRow::deserialize(&bytes).unwrap()
    }

    #[test]
    fn a_stub_is_recognized_by_its_format_byte() {
        let row = stub_row();
        assert!(is_stub(&row).unwrap());
        let headers = row.raw_headers().unwrap();
        assert!(is_stub_headers(&headers));
        assert!(!is_stub_headers(b"Subject: plain\r\n\r\n"));
    }

    #[test]
    fn a_stubs_thread_subject_never_reads_its_bundle() {
        let row = stub_row();
        let metadata = row.unarchive().unwrap();
        // The stub's structure has no header entry: resolved, and empty.
        assert_eq!(
            metadata.thread_subject(),
            email::message::thread::ThreadSubject::Resolved("")
        );
        let headers = row.raw_headers().unwrap();
        assert_eq!(za_thread_subject_in(metadata, &headers), "");
    }
}
