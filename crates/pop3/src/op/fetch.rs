/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::{Session, protocol::response::Response};
use common::{network::SessionStream, za::ZA_MAIL_UNREADABLE};
use email::message::{
    metadata::MessageMetadata,
    za::view::{ZaMailView, za_readable},
};
use registry::schema::enums::Permission;
use std::{borrow::Cow, time::Instant};
use store::{
    ValueKey,
    write::{AlignedBytes, Archive},
};
use trc::AddContext;
use types::{collection::Collection, field::EmailField};
use utils::chained_bytes::ChainedBytes;

impl<T: SessionStream> Session<T> {
    pub async fn handle_fetch(&mut self, msg: u32, lines: Option<u32>) -> trc::Result<()> {
        // Validate access
        self.state
            .access_token()
            .enforce_permission(Permission::Pop3Retr)?;

        let op_start = Instant::now();
        let mailbox = self.state.mailbox();
        if let Some(message) = mailbox.messages.get(msg.saturating_sub(1) as usize) {
            if let Some(metadata_) = self
                .server
                .store()
                .get_value::<Archive<AlignedBytes>>(ValueKey::property(
                    mailbox.account_id,
                    Collection::Email,
                    message.id,
                    EmailField::Metadata,
                ))
                .await
                .caused_by(trc::location!())?
            {
                // Mail spec 7, 10: the unsealed view and blob; an unreadable
                // message answers -ERR.
                let keys = self
                    .server
                    .za_mail_keys(self.state.access_token(), mailbox.account_id)
                    .await?;
                let Some(view) = za_readable(
                    self.server
                        .za_metadata_view(
                            &metadata_,
                            keys.as_deref(),
                            mailbox.account_id,
                            message.id,
                        )
                        .await,
                )?
                else {
                    return Err(unreadable());
                };
                let metadata = view
                    .unarchive::<MessageMetadata>()
                    .caused_by(trc::location!())?;
                if let Some(bytes) = self
                    .server
                    .blob_store()
                    .get_blob(metadata.blob_hash.0.as_slice(), 0..usize::MAX)
                    .await
                    .caused_by(trc::location!())?
                {
                    let Some(bytes) = za_readable(
                        self.server
                            .za_open_message_blob(
                                matches!(view, Cow::Owned(_)),
                                keys.as_deref(),
                                mailbox.account_id,
                                message.id,
                                bytes,
                            )
                            .await,
                    )?
                    else {
                        return Err(unreadable());
                    };
                    trc::event!(
                        Pop3(trc::Pop3Event::Fetch),
                        SpanId = self.session_id,
                        DocumentId = message.id,
                        Elapsed = op_start.elapsed()
                    );

                    let bytes = ChainedBytes::new(metadata.raw_headers.as_ref())
                        .with_last(
                            bytes
                                .get(metadata.blob_body_offset.to_native() as usize..)
                                .unwrap_or_default(),
                        )
                        .get_full_range();

                    self.write_bytes(Response::Message::<u32> { bytes, lines }.serialize())
                        .await
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
