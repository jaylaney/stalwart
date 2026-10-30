/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::message::{
    index::{IndexMessage, PendingMessageData},
    metadata::{ArchivedMessageMetadata, ExtraHeaders, MessageMetadata},
    sortkeys::MessageSortKeys,
};
use common::storage::index::ObjectIndexBuilder;
use store::{
    write::{BatchBuilder, BlobLink, BlobOp, IndexPropertyClass, ValueClass, blob::BlobLinkValue},
    xxhash_rust::xxh3::xxh3_128,
};
use trc::AddContext;
use types::{
    blob_hash::BlobHash,
    field::EmailField,
    keyword::{HASATTACHMENT, HASNOATTACHMENT},
};
use utils::hash128::Hash128;

impl ArchivedMessageMetadata {
    pub fn index_verbatim(&self, batch: &mut BatchBuilder, metadata: Vec<u8>, sort_keys: Vec<u8>) {
        batch
            .set(
                BlobOp::Link {
                    hash: self.blob_hash(),
                    to: BlobLink::Document,
                },
                BlobLinkValue::new(self.blob_size()),
            )
            .set(EmailField::SortKeys, sort_keys)
            .set(EmailField::Metadata, metadata);
    }

    pub fn unindex(&self, batch: &mut BatchBuilder, thread_name: &str) {
        batch
            .clear(EmailField::Metadata)
            .clear(EmailField::SortKeys)
            .clear(ValueClass::IndexProperty(IndexPropertyClass::Hash {
                property: EmailField::Threading.into(),
                hash: Hash128::from(xxh3_128(
                    if !thread_name.is_empty() {
                        thread_name
                    } else {
                        "!"
                    }
                    .as_bytes(),
                )),
            }))
            .clear(BlobOp::Link {
                hash: self.blob_hash(),
                to: BlobLink::Document,
            });
    }
}

<<<<<<< HEAD
impl MessageMetadata {
    /// The metadata archive upstream stores for `message`: every part, the
    /// parsed headers with `extra_headers` prepended to the root part, the
    /// preview, the raw header block and the attachment bit. The first half
    /// of `index_message`, shared with the key-account stub (mail spec 5.2).
    pub fn build<'x>(
        mut message: mail_parser::Message<'x>,
        extra_headers: Vec<u8>,
        mut extra_headers_parsed: Vec<mail_parser::Header<'x>>,
        blob_hash: BlobHash,
        received_at: u64,
    ) -> Self {
        let mut has_attachments = false;
        let mut preview = None;
        let preview_part_id = message
            .text_body
            .first()
            .or_else(|| message.html_body.first())
            .copied()
            .unwrap_or(u32::MAX);

        for (part_id, part) in message.parts.iter().take(MAX_MESSAGE_PARTS).enumerate() {
            let part_id = part_id as u32;
            match &part.body {
                mail_parser::PartType::Text(text) => {
                    if part_id == preview_part_id {
                        preview =
                            preview_text(text.replace('\r', "").into(), PREVIEW_LENGTH).into();
                    }

                    if !message.text_body.contains(&part_id)
                        && !message.html_body.contains(&part_id)
                    {
                        has_attachments = true;
                    }
                }
                mail_parser::PartType::Html(html) => {
                    let text = html_to_text(html);
                    if part_id == preview_part_id {
                        preview =
                            preview_text(text.replace('\r', "").into(), PREVIEW_LENGTH).into();
                    }

                    if !message.text_body.contains(&part_id)
                        && !message.html_body.contains(&part_id)
                    {
                        has_attachments = true;
                    }
                }
                mail_parser::PartType::Binary(_) | mail_parser::PartType::Message(_)
                    if !has_attachments =>
                {
                    has_attachments = true;
                }
                _ => {}
            }
        }

        // Build raw headers
        let root_part = message.root_part();
        let mut raw_headers = Vec::with_capacity(
            (root_part.offset_body - root_part.offset_header) as usize + extra_headers.len(),
        );
        raw_headers.extend_from_slice(&extra_headers);
        raw_headers.extend_from_slice(
            message
                .raw_message
                .as_ref()
                .get(root_part.offset_header as usize..root_part.offset_body as usize)
                .unwrap_or_default(),
        );

        // Add additional headers to message
        let blob_body_offset = if !extra_headers.is_empty() {
            // Add extra headers to root part
            let offset_start = extra_headers.len() as u32;
            let mut part_iter_stack = Vec::new();
            let mut part_iter = message.parts.iter_mut();

            loop {
                if let Some(part) = part_iter.next() {
                    // Increment header offsets
                    for header in part.headers.iter_mut() {
                        header.offset_field += offset_start;
                        header.offset_start += offset_start;
                        header.offset_end += offset_start;
                    }

                    // Adjust part offsets
                    part.offset_body += offset_start;
                    part.offset_end += offset_start;
                    part.offset_header += offset_start;

                    if let PartType::Message(sub_message) = &mut part.body
                        && sub_message.root_part().offset_header != 0
                    {
                        part_iter_stack.push(part_iter);
                        part_iter = sub_message.parts.iter_mut();
                    }
                } else if let Some(iter) = part_iter_stack.pop() {
                    part_iter = iter;
                } else {
                    break;
                }
            }

            // Add extra headers to root part
            let root_part = &mut message.parts[0];
            extra_headers_parsed.append(&mut root_part.headers);
            root_part.offset_header = 0;
            root_part.headers = extra_headers_parsed;
            root_part.offset_body - offset_start
        } else {
            message.root_part().offset_body
        };

        // Build metadata
        MessageMetadata {
            preview: preview.unwrap_or_default().into_owned().into_boxed_str(),
            raw_headers: raw_headers.into_boxed_slice(),
            contents: build_metadata_contents(message),
            blob_hash,
            blob_body_offset,
            rcvd_attach: (if has_attachments {
                MESSAGE_HAS_ATTACHMENT
            } else {
                0
            }) | (received_at & MESSAGE_RECEIVED_MASK),
        }
    }
}

impl IndexMessage for BatchBuilder {
    fn index_message<'x>(
        &mut self,
        tenant_id: Option<u32>,
        message: mail_parser::Message<'x>,
        extra_headers: Vec<u8>,
        extra_headers_parsed: Vec<mail_parser::Header<'x>>,
        blob_hash: BlobHash,
        data: MessageData,
        received_at: u64,
    ) -> trc::Result<&mut Self> {
        let metadata = MessageMetadata::build(
            message,
            extra_headers,
            extra_headers_parsed,
            blob_hash,
            received_at,
        );
        self.index_metadata(tenant_id, metadata, data)
    }

    fn index_metadata(
        &mut self,
        tenant_id: Option<u32>,
        metadata: MessageMetadata,
        data: MessageData,
    ) -> trc::Result<&mut Self> {
=======
impl IndexMessage for BatchBuilder {
    fn index_message(
        &mut self,
        // SPDX-SnippetBegin
        // SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
        // SPDX-License-Identifier: LicenseRef-SEL
        #[cfg(feature = "enterprise")] tenant_id: Option<u32>,
        // SPDX-SnippetEnd
        message: &mail_parser::Message<'_>,
        extra_headers: &ExtraHeaders,
        blob_hash: BlobHash,
        mut data: PendingMessageData,
    ) -> trc::Result<&mut Self> {
        let built = MessageMetadata::build(message, extra_headers, blob_hash.clone());
        if built.has_attachments {
            data.data.keywords |= 1 << HASATTACHMENT;
        } else {
            data.data.keywords |= 1 << HASNOATTACHMENT;
        }

        let index_builder = ObjectIndexBuilder::<(), _>::new();
        // SPDX-SnippetBegin
        // SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
        // SPDX-License-Identifier: LicenseRef-SEL
        #[cfg(feature = "enterprise")]
        let index_builder = index_builder.with_tenant_id(tenant_id);
        // SPDX-SnippetEnd
>>>>>>> 23b69755cb3cd8ecd952fe0443c5ac067a506184
        self.set(
            BlobOp::Link {
                hash: blob_hash,
                to: BlobLink::Document,
            },
            BlobLinkValue::new(message.raw().len()),
        )
        .custom(index_builder.with_changes(data))
        .caused_by(trc::location!())?
        .set(
            EmailField::SortKeys,
            MessageSortKeys::from_message(message).serialize(),
        )
        .set(
            EmailField::Metadata,
            built.encode().caused_by(trc::location!())?,
        );

        Ok(self)
    }
}
