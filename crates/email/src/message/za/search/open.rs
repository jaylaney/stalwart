/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::message::{
    messagedata::MessageData,
    metadata::MetadataRow,
    za::view::{ZA_REASON_UNREADABLE, ZaMailView, ZaMetadataRow, Zeroizing},
};
use common::{
    Server,
    za::search::{
        cache::IndexEntry,
        error::{LoadFailure, LossClass, TransientClass},
        load::absence_class,
    },
};
use mail_parser::{Message, MessageParser};
use std::future::Future;
use store::ValueKey;
use trc::{EventType, Key, StoreEvent};
use types::{collection::Collection, field::EmailField};
use vault::session::SessionKeys;

pub struct OpenedMessage {
    pub row: MetadataRow,
    pub raw: Zeroizing<Vec<u8>>,
    pub bytes: usize,
    pub received_at: u64,
}

impl OpenedMessage {
    pub fn parse(&self) -> Option<Message<'_>> {
        MessageParser::new().parse(self.raw.as_slice())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenFailure {
    Loss(LossClass),
    Transient(TransientClass),
}

pub fn classify_open_error(err: &trc::Error) -> OpenFailure {
    match err.event_type() {
        EventType::Store(StoreEvent::NotFound) => OpenFailure::Loss(LossClass::StubAbsent),
        EventType::Store(StoreEvent::DataCorruption)
            if err.value_as_str(Key::Reason) == Some(ZA_REASON_UNREADABLE) =>
        {
            OpenFailure::Loss(LossClass::Aead)
        }
        EventType::Store(StoreEvent::DeserializeError)
            if err.value_as_str(Key::Details) == Some("Archive access failed") =>
        {
            OpenFailure::Loss(LossClass::Parse)
        }
        EventType::Store(StoreEvent::DecryptError) if err.value(Key::Id).is_some() => {
            OpenFailure::Transient(TransientClass::CodecKey)
        }
        EventType::Store(
            StoreEvent::DataCorruption | StoreEvent::DecryptError | StoreEvent::DecompressError,
        ) => OpenFailure::Loss(LossClass::Parse),
        _ => OpenFailure::Transient(TransientClass::Store),
    }
}

pub trait ZaSearchMessageOpen: Sync + Send {
    fn za_open_for_index(
        &self,
        account_id: u32,
        document_id: u32,
        keys: &SessionKeys,
        entry: &IndexEntry,
        window_secs: u64,
        now: u64,
    ) -> impl Future<Output = Result<OpenedMessage, OpenFailure>> + Send;
}

impl ZaSearchMessageOpen for Server {
    async fn za_open_for_index(
        &self,
        account_id: u32,
        document_id: u32,
        keys: &SessionKeys,
        entry: &IndexEntry,
        window_secs: u64,
        now: u64,
    ) -> Result<OpenedMessage, OpenFailure> {
        #[cfg(feature = "test_mode")]
        {
            use common::za::test_hooks::{
                search_open_fail_all, take_search_blob_fail, take_search_open_fail,
            };

            if search_open_fail_all(account_id) || take_search_open_fail(account_id) {
                return Err(OpenFailure::Loss(LossClass::Aead));
            }
            if take_search_blob_fail(account_id) {
                return Err(OpenFailure::Transient(TransientClass::BlobStore));
            }
        }

        let stored = self
            .store()
            .get_value::<MetadataRow>(ValueKey::immutable(
                account_id,
                Collection::Email,
                document_id,
                EmailField::Metadata,
            ))
            .await
            .map_err(|err| classify_open_error(&err))?
            .ok_or(OpenFailure::Loss(LossClass::StubAbsent))?;
        let received_at = self
            .store()
            .get_value::<MessageData>(ValueKey::archive(
                account_id,
                Collection::Email,
                document_id,
            ))
            .await
            .map_err(|err| classify_open_error(&err))?
            .ok_or(OpenFailure::Loss(LossClass::StubAbsent))?
            .received_at;
        let ZaMetadataRow { row, opened } = self
            .za_metadata_view(stored, Some(keys), account_id, document_id)
            .await
            .map_err(|err| classify_open_error(&err))?;
        let hash = row
            .unarchive()
            .map_err(|err| classify_open_error(&err))?
            .blob_hash();

        let Some(bytes) = self
            .get_blob(hash.as_slice(), 0..usize::MAX)
            .await
            .map_err(|err| classify_open_error(&err))?
        else {
            return Err(
                match absence_class(entry.absent_since(&hash, now), now, window_secs) {
                    LoadFailure::Transient(class) => OpenFailure::Transient(class),
                    LoadFailure::Loss(class) => OpenFailure::Loss(class),
                    _ => unreachable!("the absence window yields only transient or loss"),
                },
            );
        };
        entry.seen(&hash);
        let raw = self
            .za_open_message_blob(opened, Some(keys), account_id, document_id, bytes)
            .await
            .map_err(|err| classify_open_error(&err))?;
        let bytes = raw.len();
        #[cfg(feature = "test_mode")]
        common::za::test_hooks::count_za_search_message_open(account_id);

        Ok(OpenedMessage {
            row,
            raw,
            bytes,
            received_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenFailure, classify_open_error};
    use crate::message::za::view::za_unreadable;
    use common::za::search::error::{LossClass, TransientClass};
    use groupware::calendar::seal::SealError;
    use trc::{Key, StoreEvent};

    #[test]
    fn only_checked_archive_deserialization_is_permanent_loss() {
        for (error, expected) in [
            (
                StoreEvent::DeserializeError
                    .into_err()
                    .details("Archive access failed"),
                OpenFailure::Loss(LossClass::Parse),
            ),
            (
                StoreEvent::DeserializeError.into_err(),
                OpenFailure::Transient(TransientClass::Store),
            ),
            (
                StoreEvent::DeserializeError
                    .into_err()
                    .details("Other decoding failure"),
                OpenFailure::Transient(TransientClass::Store),
            ),
            (
                trc::Error::new(trc::EventType::Server(trc::ServerEvent::ThreadError))
                    .details("Archive access failed"),
                OpenFailure::Transient(TransientClass::Store),
            ),
            (
                StoreEvent::DecryptError.into_err().ctx(Key::Id, 3u32),
                OpenFailure::Transient(TransientClass::CodecKey),
            ),
        ] {
            assert_eq!(classify_open_error(&error), expected);
        }
    }

    #[test]
    fn open_errors_are_classified() {
        for (error, expected) in [
            (
                StoreEvent::NotFound.into_err(),
                OpenFailure::Loss(LossClass::StubAbsent),
            ),
            (
                za_unreadable(SealError::Aead, 1, 2),
                OpenFailure::Loss(LossClass::Aead),
            ),
            (
                StoreEvent::DataCorruption.into_err(),
                OpenFailure::Loss(LossClass::Parse),
            ),
            (
                StoreEvent::DecryptError.into_err().ctx(Key::Id, 3u32),
                OpenFailure::Transient(TransientClass::CodecKey),
            ),
            (
                StoreEvent::DecryptError.into_err(),
                OpenFailure::Loss(LossClass::Parse),
            ),
            (
                StoreEvent::DecompressError.into_err(),
                OpenFailure::Loss(LossClass::Parse),
            ),
            (
                StoreEvent::UnexpectedError.into_err(),
                OpenFailure::Transient(TransientClass::Store),
            ),
        ] {
            assert_eq!(classify_open_error(&error), expected);
        }
    }
}
