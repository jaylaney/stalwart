/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::view::{ViewError, ViewNeed, ZaView, za_view_with_minimum};
use crate::cache::email::MessageCacheAccess;
use common::{
    Server,
    za::search::{
        error::NoMapReason,
        trace::{ZA_SEARCH_UNAVAILABLE, za_search_trace},
    },
};
use std::future::Future;
use types::id::Id;
use vault::session::SessionKeys;

pub type ZaSnapshot = ZaView;

pub trait ZaSnapshotFetch: Sync + Send {
    fn za_snapshot(
        &self,
        account_id: u32,
        keys: &SessionKeys,
    ) -> impl Future<Output = Result<ZaSnapshot, NoMapReason>> + Send {
        self.za_response_snapshot(account_id, keys, None)
    }

    fn za_response_snapshot(
        &self,
        account_id: u32,
        keys: &SessionKeys,
        write_floor: Option<u64>,
    ) -> impl Future<Output = Result<ZaSnapshot, NoMapReason>> + Send;
}

impl ZaSnapshotFetch for Server {
    async fn za_response_snapshot(
        &self,
        account_id: u32,
        keys: &SessionKeys,
        write_floor: Option<u64>,
    ) -> Result<ZaSnapshot, NoMapReason> {
        za_view_with_minimum(self, account_id, keys, ViewNeed::Map, write_floor)
            .await
            .map_err(|error| match error {
                ViewError::NoMap(reason) => reason,
                ViewError::Unavailable(_) => unreachable!("a map view returns only no-map errors"),
            })
    }
}

impl ZaView {
    pub fn map_id(&self, document_id: u32) -> Id {
        Id::from_parts(self.thread_of(document_id), document_id)
    }

    pub fn thread_of(&self, document_id: u32) -> u32 {
        self.index.thread.current_thread(document_id)
    }

    pub fn check_id(&self, id: Id) -> Option<u32> {
        let document_id = id.document_id();
        (self.cache.has_email_id(&document_id) && self.thread_of(document_id) == id.prefix_id())
            .then_some(document_id)
    }

    pub fn thread_members(&self, thread_id: u32) -> Vec<u32> {
        let mut members: Vec<_> = self
            .index
            .thread
            .members(thread_id)
            .iter()
            .copied()
            .filter(|id| self.cache.has_email_id(id))
            .collect();
        if self.index.thread.record(thread_id).is_none() && self.cache.has_email_id(&thread_id) {
            members.push(thread_id);
        }
        members
    }

    pub fn state(&self) -> u64 {
        self.cache.last_change_id
    }
}

pub fn za_no_map_error(reason: NoMapReason, account_id: u32) -> trc::Error {
    za_search_trace!(
        ZA_SEARCH_UNAVAILABLE,
        AccountId = account_id,
        Reason = reason.as_str()
    );
    trc::StoreEvent::UnexpectedError
        .into_err()
        .details(ZA_SEARCH_UNAVAILABLE)
        .ctx(trc::Key::Reason, reason.as_str())
}

pub fn za_submission_prefix(document_id: u32) -> u32 {
    document_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{
        MailboxesCache, MessageCache, MessageStoreCache, MessagesCache,
        za::search::cache::{LoadedIndex, ZaSearchCache},
    };
    use std::sync::Arc;
    use store::za_search::{
        manifest::Manifest,
        thread::{ThreadPart, ThreadRecord},
    };

    async fn snapshot_with(present: &[u32]) -> ZaSnapshot {
        let cache = ZaSearchCache::new(Ok(Default::default()));
        let entry = cache.entry(1);
        let mut index = LoadedIndex::empty(Manifest::default());
        index.thread = ThreadPart::from_parts(
            [(2, None), (5, None), (8, Some(3))]
                .into_iter()
                .map(|(document_id, deleted)| ThreadRecord {
                    document_id,
                    thread_id: 2,
                    revision: 1,
                    created: 2,
                    deleted,
                    subject_hash: 0,
                    hashes: vec![],
                })
                .collect(),
            vec![],
            vec![],
        );
        *entry.index.write().await = Some(index);
        let index = entry.read_loaded().await.unwrap();
        let messages = present
            .iter()
            .map(|&doc| MessageCache::new(doc, Default::default(), 0, doc, 2, 0, 0, 0))
            .collect();
        ZaView {
            entry,
            index,
            epoch: 0,
            cache: Arc::new(MessageStoreCache {
                emails: Arc::new(MessagesCache::new(2, messages, vec![])),
                mailboxes: Arc::new(MailboxesCache {
                    change_id: 3,
                    index: Default::default(),
                    items: Default::default(),
                    size: 0,
                }),
                update_lock: Default::default(),
                refresh: Default::default(),
                last_change_id: 3,
                size: 0,
                verification: Default::default(),
            }),
        }
    }

    #[tokio::test]
    async fn check_id_accepts_the_current_prefix_only() {
        let snapshot = snapshot_with(&[2, 5, 7]).await;
        assert_eq!(snapshot.check_id(Id::from_parts(2, 5)), Some(5));
        assert_eq!(snapshot.check_id(Id::from_parts(5, 5)), None);
        assert_eq!(snapshot.check_id(Id::from_parts(2, 9)), None);
        assert_eq!(snapshot.map_id(7), Id::from_parts(7, 7));
        assert_eq!(snapshot.thread_members(7), [7]);
        assert_eq!(snapshot.state(), 3);
    }

    #[tokio::test]
    async fn thread_members_intersect_the_present_set() {
        let snapshot = snapshot_with(&[2, 5]).await;
        assert_eq!(snapshot.thread_members(2), [2, 5]);
        assert_eq!(snapshot.check_id(Id::from_parts(2, 8)), None);
        let snapshot = snapshot_with(&[2, 5, 8]).await;
        assert_eq!(snapshot.thread_members(2), [2, 5, 8]);
        assert_eq!(snapshot.map_id(8), Id::from_parts(2, 8));
        assert_eq!(snapshot.check_id(Id::from_parts(2, 8)), Some(8));
    }
}
