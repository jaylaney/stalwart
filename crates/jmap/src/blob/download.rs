/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::{Server, auth::AccessToken};
use email::cache::MessageCacheFetch;
use email::cache::email::MessageCacheAccess;
use email::message::metadata::MessageMetadata;
use email::message::za::view::{ZaMailView, Zeroizing, za_readable};
use groupware::cache::GroupwareCache;
use mail_parser::{
    Encoding,
    decoders::{base64::base64_decode, quoted_printable::quoted_printable_decode},
};
use registry::schema::enums::Permission;
use std::{borrow::Cow, future::Future};
use store::ValueKey;
use store::write::{AlignedBytes, Archive};
use trc::AddContext;
use types::acl::Acl;
use types::blob::{BlobClass, BlobId, BlobSection};
use types::collection::{Collection, SyncCollection};
use types::field::EmailField;
use utils::chained_bytes::ChainedBytes;

pub trait BlobDownload: Sync + Send {
    fn blob_download(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<Option<Vec<u8>>>> + Send;

    fn has_access_blob(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<bool>> + Send;
}

impl BlobDownload for Server {
    #[allow(clippy::blocks_in_conditions)]
    async fn blob_download(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
    ) -> trc::Result<Option<Vec<u8>>> {
        if self.has_access_blob(blob_id, access_token).await? {
            // Mail spec 5.7, 7: a key account's message or upload, opened
            // with the request's keys.
            if self.za_is_sealed_source(blob_id).await? {
                return match blob_id.class {
                    BlobClass::Linked {
                        account_id,
                        document_id,
                        ..
                    } => {
                        za_download_message(self, blob_id, access_token, account_id, document_id)
                            .await
                    }
                    BlobClass::Reserved { account_id, .. } => {
                        za_download_upload(self, blob_id, access_token, account_id).await
                    }
                };
            }
            if let Some(section) = &blob_id.section {
                self.get_blob_section(&blob_id.hash, section)
                    .await
                    .caused_by(trc::location!())
            } else {
                let blob = self
                    .blob_store()
                    .get_blob(blob_id.hash.as_slice(), 0..usize::MAX)
                    .await
                    .caused_by(trc::location!());
                match (&blob_id.class, blob) {
                    (
                        BlobClass::Linked {
                            account_id,
                            collection,
                            document_id,
                        },
                        Ok(Some(data)),
                    ) if *collection == Collection::Email as u8 => {
                        let Some(archive) = self
                            .store()
                            .get_value::<Archive<AlignedBytes>>(ValueKey::property(
                                *account_id,
                                Collection::Email,
                                *document_id,
                                EmailField::Metadata,
                            ))
                            .await
                            .caused_by(trc::location!())?
                        else {
                            return Ok(Some(data));
                        };
                        let metadata = archive
                            .to_unarchived::<MessageMetadata>()
                            .caused_by(trc::location!())?;
                        let body_offset = metadata.inner.blob_body_offset.to_native();
                        if metadata.inner.root_part().offset_body.to_native() != body_offset {
                            let raw_message = ChainedBytes::new(
                                metadata.inner.raw_headers.as_ref(),
                            )
                            .with_last(data.get(body_offset as usize..).unwrap_or_default());
                            Ok(Some(raw_message.to_bytes()))
                        } else {
                            Ok(Some(data))
                        }
                    }
                    (_, blob) => blob,
                }
            }
        } else {
            Ok(None)
        }
    }

    async fn has_access_blob(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
    ) -> trc::Result<bool> {
        Ok(
            (blob_id.class.is_superuser() && access_token.has_permission(Permission::FetchAnyBlob))
                || (self
                    .store()
                    .blob_has_access(&blob_id.hash, &blob_id.class)
                    .await
                    .caused_by(trc::location!())?
                    && match &blob_id.class {
                        BlobClass::Linked {
                            account_id,
                            collection,
                            document_id,
                        } => {
                            if matches!(
                                Collection::from(*collection),
                                Collection::Calendar | Collection::CalendarEvent
                            ) && self
                                .za_is_key_account(*account_id)
                                .await
                                .caused_by(trc::location!())?
                            {
                                // Spec 9: never for a key account's calendar data.
                                false
                            } else if access_token.is_member(*account_id) {
                                true
                            } else {
                                match Collection::from(*collection) {
                                    Collection::Email => self
                                        .get_cached_messages(*account_id)
                                        .await
                                        .caused_by(trc::location!())?
                                        .shared_messages(access_token, Acl::ReadItems)
                                        .contains(*document_id),
                                    collection @ (Collection::FileNode
                                    | Collection::ContactCard
                                    | Collection::CalendarEvent) => self
                                        .fetch_dav_resources(
                                            access_token.account_id(),
                                            *account_id,
                                            SyncCollection::from(collection),
                                        )
                                        .await
                                        .caused_by(trc::location!())?
                                        .shared_items(access_token, [Acl::ReadItems], true)
                                        .contains(*document_id),
                                    _ => false,
                                }
                            }
                        }
                        BlobClass::Reserved { account_id, .. } => {
                            access_token.is_member(*account_id)
                        }
                    }),
        )
    }
}

/// Mail spec 5.7, 7: a key account's message, or a section of it, read
/// through the unsealed pair as upstream reads the stored one: a section id
/// applies its offsets, which Email/get translated to stored-blob
/// coordinates, to the opened blob and transfer-decodes the section; a
/// whole-message id reconstructs `raw_headers || opened[blob_body_offset..]`.
/// An unreadable message is not found (spec 10).
async fn za_download_message(
    server: &Server,
    blob_id: &BlobId,
    access_token: &AccessToken,
    account_id: u32,
    document_id: u32,
) -> trc::Result<Option<Vec<u8>>> {
    let keys = server.za_mail_keys(access_token, account_id).await?;
    let Some(stored) = server
        .store()
        .get_value::<Archive<AlignedBytes>>(ValueKey::property(
            account_id,
            Collection::Email,
            document_id,
            EmailField::Metadata,
        ))
        .await
        .caused_by(trc::location!())?
    else {
        return Ok(None);
    };
    let Some(view) = za_readable(
        server
            .za_metadata_view(&stored, keys.as_deref(), account_id, document_id)
            .await,
    )?
    else {
        return Ok(None);
    };
    let Some(blob) = server
        .blob_store()
        .get_blob(blob_id.hash.as_slice(), 0..usize::MAX)
        .await
        .caused_by(trc::location!())?
    else {
        return Ok(None);
    };
    let Some(mut opened) = za_readable(
        server
            .za_open_message_blob(
                matches!(view, Cow::Owned(_)),
                keys.as_deref(),
                account_id,
                document_id,
                blob,
            )
            .await,
    )?
    else {
        return Ok(None);
    };
    if let Some(section) = &blob_id.section {
        return Ok(decode_section(&opened, section));
    }
    let metadata = view
        .unarchive::<MessageMetadata>()
        .caused_by(trc::location!())?;
    let body_offset = metadata.blob_body_offset.to_native();
    if metadata.root_part().offset_body.to_native() != body_offset {
        Ok(Some(
            ChainedBytes::new(metadata.raw_headers.as_ref())
                .with_last(opened.get(body_offset as usize..).unwrap_or_default())
                .to_bytes(),
        ))
    } else {
        // The bytes leave in the HTTP body.
        Ok(Some(std::mem::take(&mut *opened)))
    }
}

/// Mail spec 5.7, 7: a key account's upload, opened whole with the
/// request's keys; a section id is cut from the opened bytes and
/// transfer-decoded, as upstream reads a stored one. An unreadable upload
/// is not found (spec 10).
async fn za_download_upload(
    server: &Server,
    blob_id: &BlobId,
    access_token: &AccessToken,
    account_id: u32,
) -> trc::Result<Option<Vec<u8>>> {
    let keys = server.za_mail_keys(access_token, account_id).await?;
    let Some(blob) = server
        .blob_store()
        .get_blob(blob_id.hash.as_slice(), 0..usize::MAX)
        .await
        .caused_by(trc::location!())?
    else {
        return Ok(None);
    };
    let mut opened = match keys {
        Some(keys) => {
            let Some(opened) =
                za_readable(server.za_open_upload_blob(&keys, account_id, blob).await)?
            else {
                return Ok(None);
            };
            opened
        }
        // `za_mail_keys` answers `None` only for an account that is not a
        // key account (any longer): the bytes as stored.
        None => Zeroizing::new(blob),
    };
    Ok(match &blob_id.section {
        Some(section) => decode_section(&opened, section),
        // The bytes leave in the HTTP body.
        None => Some(std::mem::take(&mut *opened)),
    })
}

/// `get_blob_section` on bytes in memory: the blob store's range read (from
/// offset 0 a range is cut at the end of the blob, any other range out of
/// it reads empty), then the section's transfer decoding.
fn decode_section(bytes: &[u8], section: &BlobSection) -> Option<Vec<u8>> {
    let end = section.offset_start.saturating_add(section.size);
    let bytes = if section.offset_start == 0 {
        &bytes[..end.min(bytes.len())]
    } else {
        bytes.get(section.offset_start..end).unwrap_or_default()
    };
    match Encoding::from(section.encoding) {
        Encoding::None => Some(bytes.to_vec()),
        Encoding::Base64 => base64_decode(bytes),
        Encoding::QuotedPrintable => quoted_printable_decode(bytes),
    }
}
