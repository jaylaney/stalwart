/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::za_search::za_imap_content_guard;
use crate::{
    core::{SelectedMailbox, Session, SessionData},
    spawn_op,
};
use ahash::AHashMap;
use common::network::SessionStream;
use email::{
    cache::{MessageCacheFetch, email::MessageCacheAccess},
    message::za::search::view::ZaView,
};
use imap_proto::{
    Command, StatusResponse,
    protocol::thread::{Arguments, Response},
    receiver::Request,
};
use registry::schema::enums::Permission;
use std::{sync::Arc, time::Instant};
use tokio::sync::OwnedSemaphorePermit;
use trc::AddContext;

impl<T: SessionStream> Session<T> {
    pub async fn handle_thread(
        &mut self,
        request: Request<Command>,
        is_uid: bool,
        permit: Option<OwnedSemaphorePermit>,
    ) -> trc::Result<()> {
        // Validate access
        self.assert_has_permission(Permission::ImapThread)?;

        let op_start = Instant::now();
        let command = request.command;
        let mut arguments = request.parse_thread()?;
        let (data, mailbox) = self.state.mailbox_state();

        spawn_op!(permit, data, {
            let tag = std::mem::take(&mut arguments.tag);

            match data.thread(arguments, mailbox, is_uid, op_start).await {
                Ok(response) => {
                    data.write_bytes(
                        StatusResponse::completed(command)
                            .with_tag(tag)
                            .serialize_after(&response),
                    )
                    .await
                }
                Err(err) => Err(err.id(tag)),
            }
        })
    }
}

impl<T: SessionStream> SessionData<T> {
    pub async fn thread(
        &self,
        arguments: Arguments,
        mailbox: Arc<SelectedMailbox>,
        is_uid: bool,
        op_start: Instant,
    ) -> trc::Result<Response> {
        // Mail spec 6.3, ruling R34: THREAD reads a key account's mailbox
        // only in its own sessions, checked before its message cache is read.
        self.server
            .za_assert_mail_route(&self.access_token, mailbox.id.account_id)
            .await?;
        let mut saved_search = None;
        let za = za_imap_content_guard(self, &mailbox, &arguments.filter, &[], &mut saved_search)
            .await?;
        let cache = match &za {
            Some(view) => view.cache.clone(),
            None => self
                .server
                .get_cached_messages(mailbox.id.account_id)
                .await
                .caused_by(trc::location!())?,
        };
        self.sync_view(&mailbox, &cache, None)
            .await
            .caused_by(trc::location!())?;

        // Run query
        let (result_set, _) = self
            .query(
                arguments.filter,
                vec![],
                &mailbox,
                &cache,
                &saved_search,
                za.as_ref().map(ZaView::content),
            )
            .await?;

        if result_set.is_empty() {
            return Ok(Response {
                is_uid,
                threads: vec![],
            });
        }

        // Group messages by thread
        let mut threads: AHashMap<u32, Vec<u32>> = AHashMap::new();
        {
            let view = mailbox.view.lock();
            for document_id in result_set {
                if let Some(item) = cache.email_by_id(&document_id)
                    && let Some(resolved) = view.map_result(document_id)
                {
                    threads
                        .entry(item.thread_id())
                        .or_default()
                        .push(resolved.imap_id(is_uid));
                }
            }
        }

        let mut threads = threads
            .into_values()
            .map(|mut messages| {
                messages.sort_unstable();
                messages
            })
            .collect::<Vec<_>>();
        threads.sort_unstable();

        trc::event!(
            Imap(trc::ImapEvent::Thread),
            SpanId = self.session_id,
            AccountId = mailbox.id.account_id,
            MailboxId = mailbox.id.mailbox_id,
            Total = threads.len(),
            Elapsed = op_start.elapsed()
        );

        // Build response
        Ok(Response { is_uid, threads })
    }
}
