/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{
    Session,
    protocol::response::{MessageWriter, Response},
};
use common::{network::SessionStream, za::ZA_MAIL_UNREADABLE};
use email::message::{
    metadata::MetadataRow,
    za::view::{ZaMailView, ZaMetadataRow, za_readable},
};
use registry::schema::enums::Permission;
use std::time::Instant;
use store::ValueKey;
use trc::AddContext;
use types::{collection::Collection, field::EmailField};
use utils::chained_bytes::ChainedBytes;

const MESSAGE_CHUNK_LEN: usize = 64 * 1024;

impl<T: SessionStream> Session<T> {
    pub async fn handle_fetch(&mut self, msg: u32, lines: Option<u32>) -> trc::Result<()> {
        // Validate access
        self.state
            .access_token()
            .enforce_permission(Permission::Pop3Retr)?;

        let op_start = Instant::now();
        let mailbox = self.state.mailbox();
        if let Some(message) = mailbox.messages.get(msg.saturating_sub(1) as usize) {
            if let Some(row) = self
                .server
                .store()
                .get_value::<MetadataRow>(ValueKey::immutable(
                    mailbox.account_id,
                    Collection::Email,
                    message.id,
                    EmailField::Metadata,
                ))
                .await
                .caused_by(trc::location!())?
            {
                // Mail spec 7, 10: the unsealed row and blob; an unreadable
                // message answers -ERR.
                let keys = self
                    .server
                    .za_mail_keys(self.state.access_token(), mailbox.account_id)
                    .await?;
                let Some(ZaMetadataRow { row, opened }) = za_readable(
                    self.server
                        .za_metadata_view(row, keys.as_deref(), mailbox.account_id, message.id)
                        .await,
                )?
                else {
                    return Err(unreadable());
                };
                let metadata = row.unarchive().caused_by(trc::location!())?;
                let headers = row.raw_headers().caused_by(trc::location!())?;
                if let Some(bytes) = self
                    .server
                    .fetch_blob(
                        &metadata.blob_hash(),
                        0..usize::MAX,
                        Some(metadata.size() as u64),
                    )
                    .await
                    .caused_by(trc::location!())?
                {
                    // Mail spec 7, 10: a stub's sealed blob opened whole and
                    // held for the response under the stored bytes'
                    // reservation.
                    let opened_blob;
                    let _reservation;
                    let bytes: &[u8] = if opened {
                        let (sealed, reservation) = bytes.into_parts();
                        _reservation = reservation;
                        let Some(plain) = za_readable(
                            self.server
                                .za_open_message_blob(
                                    true,
                                    keys.as_deref(),
                                    mailbox.account_id,
                                    message.id,
                                    sealed,
                                )
                                .await,
                        )?
                        else {
                            return Err(unreadable());
                        };
                        opened_blob = plain;
                        &opened_blob
                    } else {
                        &bytes
                    };
                    trc::event!(
                        Pop3(trc::Pop3Event::Fetch),
                        SpanId = self.session_id,
                        DocumentId = message.id,
                        Elapsed = op_start.elapsed()
                    );

                    match Response::<u32>::message(
                        ChainedBytes::from_blob(&headers, bytes, metadata.blob_body_offset()),
                        metadata.headers_len(),
                        lines,
                    ) {
                        Response::Message(bytes, end) => {
                            let mut buf = Vec::new();
                            let mut writer =
                                MessageWriter::new(bytes, end, &mut buf, MESSAGE_CHUNK_LEN);
                            while !writer.fill(&mut buf) {
                                self.write_bytes(&buf).await?;
                                buf.clear();
                            }
                            self.write_bytes(&buf).await
                        }
                        response => self.write_bytes(response.serialize()).await,
                    }
                } else {
                    Err(trc::Pop3Event::Error
                        .into_err()
                        .details("Failed to fetch message. Perhaps another session deleted it?")
                        .caused_by(trc::location!()))
                }
            } else {
                Err(trc::Pop3Event::Error
                    .into_err()
                    .details("Failed to fetch message. Perhaps another session deleted it?")
                    .caused_by(trc::location!()))
            }
        } else {
            Err(trc::Pop3Event::Error.into_err().details("No such message."))
        }
    }
}

/// Mail spec 10: the answer for a message that does not open.
fn unreadable() -> trc::Error {
    trc::Pop3Event::Error.into_err().details(ZA_MAIL_UNREADABLE)
}
