/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::{Server, auth::AccessToken};
use groupware::calendar::{
    ArchivedCalendarEvent,
    pipeline::{
        context::{ZaReadMemo, za_read_event_view},
        error::ZaPipelineError,
    },
    seal::ZaEventView,
};
use std::collections::{HashMap, hash_map::Entry};
use store::write::{Archive, ArchiveBytes};
use trc::{EventType, StoreEvent};

/// The opened events and calendar contexts live only for one request.
pub(crate) struct ZaEventReader {
    account_id: u32,
    memo: ZaReadMemo,
    views: HashMap<u32, ZaEventView>,
}

impl ZaEventReader {
    /// Ordinary accounts use the upstream reader.
    pub(crate) async fn new(
        server: &Server,
        _access_token: &AccessToken,
        account_id: u32,
    ) -> trc::Result<Option<Self>> {
        Ok(server.za_is_key_account(account_id).await?.then(|| Self {
            account_id,
            memo: ZaReadMemo::default(),
            views: HashMap::new(),
        }))
    }

    pub(crate) async fn view(
        &mut self,
        server: &Server,
        access_token: &AccessToken,
        document_id: u32,
        calendar_ids: &[u32],
        meta: &ArchivedCalendarEvent,
        content: &Archive<ArchiveBytes>,
    ) -> trc::Result<Option<&ZaEventView>> {
        let slot = match self.views.entry(document_id) {
            Entry::Occupied(view) => return Ok(Some(view.into_mut())),
            Entry::Vacant(slot) => slot,
        };

        for &calendar_id in calendar_ids {
            match za_read_event_view(
                server,
                access_token,
                self.account_id,
                document_id,
                calendar_id,
                meta,
                content,
                &mut self.memo,
            )
            .await
            {
                Ok(Ok(Some(view))) => return Ok(Some(slot.insert(view))),
                Ok(Ok(None)) => return Ok(None),
                Ok(Err(error)) if error.matches(EventType::Store(StoreEvent::DataCorruption)) => {
                    // Store diagnostics may carry archive bytes. Trace only the item identity.
                    let error = StoreEvent::DataCorruption
                        .into_err()
                        .account_id(self.account_id)
                        .document_id(document_id)
                        .details("zero-access: unreadable calendar event");
                    trc::error!(error);
                    return Ok(None);
                }
                Ok(Err(error)) | Err(ZaPipelineError::Store(error)) => return Err(error),
                Err(_) => continue,
            }
        }
        Ok(None)
    }
}
