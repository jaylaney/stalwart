/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::embedded::EmbeddedBlobs;
use common::{
    GroupwareResourceRef, GroupwareResources, Server, auth::AccessToken, sharing::EffectiveAcl,
    storage::blob::FetchedBlob,
};
use email::cache::MessageCacheFetch;
use email::cache::email::MessageCacheAccess;
<<<<<<< HEAD
use email::message::metadata::MessageMetadata;
use email::message::za::view::{ZaMailView, Zeroizing, za_readable};
use groupware::cache::GroupwareCache;
use mail_parser::{
    Encoding,
    decoders::{base64::base64_decode, quoted_printable::quoted_printable_decode},
};
=======
use email::cache::mailbox::MailboxCacheAccess;
use email::message::metadata::MetadataRow;
use groupware::cache::GroupwareCache;
use hyper::body::Bytes;
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
use registry::schema::enums::Permission;
use std::{borrow::Cow, future::Future};
use store::ValueKey;
use trc::AddContext;
use types::acl::Acl;
use types::blob::{BlobClass, BlobId, BlobSection};
use types::collection::{Collection, SyncCollection};
use types::field::EmailField;

pub struct DownloadedBlob {
    pub bytes: FetchedBlob,
    pub extra_headers_len: usize,
}

pub enum BlobContents {
    Plain(FetchedBlob),
    Message {
        headers: Vec<u8>,
        body_offset: usize,
        blob: FetchedBlob,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobAccess {
    Denied,
    Allowed { size: Option<u64> },
}

impl BlobAccess {
    pub fn is_allowed(&self) -> bool {
        matches!(self, BlobAccess::Allowed { .. })
    }
}

impl BlobContents {
    pub fn into_downloaded(self) -> trc::Result<DownloadedBlob> {
        match self {
            BlobContents::Plain(bytes) => Ok(DownloadedBlob {
                bytes,
                extra_headers_len: 0,
            }),
            BlobContents::Message {
                headers,
                body_offset,
                mut blob,
            } => {
                blob.replace_prefix(body_offset, &headers)
                    .caused_by(trc::location!())?;
                Ok(DownloadedBlob {
                    bytes: blob,
                    extra_headers_len: headers.len().saturating_sub(body_offset),
                })
            }
        }
    }

    pub fn into_http_parts(self) -> trc::Result<(Option<Bytes>, Bytes)> {
        match self {
            BlobContents::Plain(bytes) => Ok((None, bytes.into())),
            BlobContents::Message {
                headers,
                body_offset,
                mut blob,
            } => {
                blob.try_reserve_extra(headers.capacity())
                    .caused_by(trc::location!())?;
                let body = Bytes::from(blob);
                let body = body.slice(body_offset.min(body.len())..);
                Ok((Some(headers.into()), body))
            }
        }
    }
}

impl DownloadedBlob {
    pub fn stored_len(&self, blob_id: &BlobId) -> Option<usize> {
        (blob_id.section.is_none() && !matches!(blob_id.class, BlobClass::Embedded { .. }))
            .then(|| self.bytes.len().saturating_sub(self.extra_headers_len))
    }
}

pub trait BlobDownload: Sync + Send {
    fn blob_download(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<Option<FetchedBlob>>> + Send;

    fn blob_download_with_extra(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<Option<DownloadedBlob>>> + Send;

    fn blob_contents(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<Option<BlobContents>>> + Send;

    fn has_access_blob(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
    ) -> impl Future<Output = trc::Result<BlobAccess>> + Send;
}

impl BlobDownload for Server {
    async fn blob_download(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
<<<<<<< HEAD
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
=======
    ) -> trc::Result<Option<FetchedBlob>> {
        self.blob_download_with_extra(blob_id, access_token)
            .await
            .map(|blob| blob.map(|blob| blob.bytes))
    }

    async fn blob_download_with_extra(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
    ) -> trc::Result<Option<DownloadedBlob>> {
        self.blob_contents(blob_id, access_token)
            .await?
            .map(BlobContents::into_downloaded)
            .transpose()
    }

    async fn blob_contents(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
    ) -> trc::Result<Option<BlobContents>> {
        let BlobAccess::Allowed { size } = self.has_access_blob(blob_id, access_token).await?
        else {
            return Ok(None);
        };

        if matches!(blob_id.class, BlobClass::Embedded { .. }) {
            return match self
                .embedded_blob(blob_id)
                .await
                .caused_by(trc::location!())?
            {
                Some(bytes) => self
                    .account_blob(bytes)
                    .map(|blob| Some(BlobContents::Plain(blob))),
                None => Ok(None),
            };
        } else if let Some(section) = &blob_id.section {
            return self
                .fetch_blob_section(&blob_id.hash, section, size)
                .await
                .caused_by(trc::location!())
                .map(|blob| blob.map(BlobContents::Plain));
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
        }

        let Some(blob) = self
            .fetch_blob(&blob_id.hash, 0..usize::MAX, size)
            .await
            .caused_by(trc::location!())?
        else {
            return Ok(None);
        };
        if let BlobClass::Linked {
            account_id,
            collection,
            document_id,
        } = &blob_id.class
            && *collection == Collection::Email as u8
            && let Some(row) = self
                .store()
                .get_value::<MetadataRow>(ValueKey::immutable(
                    *account_id,
                    Collection::Email,
                    *document_id,
                    EmailField::Metadata,
                ))
                .await
                .caused_by(trc::location!())?
        {
            let metadata = row.unarchive().caused_by(trc::location!())?;
            let body_offset = metadata.blob_body_offset();
            if metadata.headers_len() != body_offset {
                let headers = row.raw_headers().caused_by(trc::location!())?;
                return Ok(Some(BlobContents::Message {
                    headers: headers.into_owned(),
                    body_offset,
                    blob,
                }));
            }
        }

        Ok(Some(BlobContents::Plain(blob)))
    }

    async fn has_access_blob(
        &self,
        blob_id: &BlobId,
        access_token: &AccessToken,
<<<<<<< HEAD
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
=======
    ) -> trc::Result<BlobAccess> {
        if let BlobClass::Embedded {
            account_id,
            collection,
            document_id,
        } = &blob_id.class
        {
            return self
                .has_access_embedded_blob(access_token, *account_id, *collection, *document_id)
                .await
                .map(|allowed| {
                    if allowed {
                        BlobAccess::Allowed { size: None }
                    } else {
                        BlobAccess::Denied
                    }
                });
        }

        if blob_id.class.is_superuser() && access_token.has_permission(Permission::FetchAnyBlob) {
            return Ok(BlobAccess::Allowed { size: None });
        }

        let Some(size) = self
            .store()
            .blob_link_size(&blob_id.hash, &blob_id.class)
            .await
            .caused_by(trc::location!())?
        else {
            return Ok(BlobAccess::Denied);
        };

        let allowed = match &blob_id.class {
            BlobClass::Linked {
                account_id,
                collection,
                document_id,
            } => {
                if access_token.is_member(*account_id) {
                    true
                } else {
                    match Collection::from(*collection) {
                        Collection::Email => {
                            let cache = self
                                .get_cached_messages(*account_id)
                                .await
                                .caused_by(trc::location!())?;
                            cache.email_by_id(document_id).is_some_and(|message| {
                                message.mailboxes().iter().any(|mailbox| {
                                    cache.mailbox_by_id(&mailbox.mailbox_id).is_some_and(
                                        |mailbox| {
                                            mailbox
                                                .acls
                                                .as_slice()
                                                .effective_acl(access_token)
                                                .contains(Acl::ReadItems)
                                        },
                                    )
                                })
                            })
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
                        }
                        Collection::FileNode => self
                            .fetch_groupware_resources(
                                access_token.account_id(),
                                *account_id,
                                SyncCollection::FileNode,
                            )
                            .await
                            .caused_by(trc::location!())?
                            .file_acl(access_token, *document_id)
                            .contains(Acl::ReadItems),
                        collection @ (Collection::ContactCard | Collection::CalendarEvent) => {
                            let resources = self
                                .fetch_groupware_resources(
                                    access_token.account_id(),
                                    *account_id,
                                    SyncCollection::from(collection),
                                )
                                .await
                                .caused_by(trc::location!())?;
                            resources.item_by_id(*document_id).is_some_and(|item| {
                                is_shared_item_readable(&resources, item, access_token)
                            })
                        }
                        _ => false,
                    }
                }
            }
            BlobClass::Reserved { account_id, .. } => access_token.is_member(*account_id),
            BlobClass::Embedded { .. } => false,
        };

        Ok(if allowed {
            BlobAccess::Allowed {
                size: Some(size).filter(|size| *size != 0),
            }
        } else {
            BlobAccess::Denied
        })
    }
}

<<<<<<< HEAD
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
=======
pub(crate) fn is_shared_item_readable(
    resources: &GroupwareResources,
    item: GroupwareResourceRef<'_>,
    access_token: &AccessToken,
) -> bool {
    item.child_names()
        .iter()
        .any(|name| resources.has_access_to_container(access_token, name.parent_id, Acl::ReadItems))
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
}
