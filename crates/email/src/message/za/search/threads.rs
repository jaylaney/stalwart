/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::snapshot::ZaSnapshot;
use crate::cache::email::{MailboxCounters, mailbox_counters_by, thread_keywords_by};
use common::cache::email::MessageRef;
use store::{ahash::AHashMap, roaring::RoaringBitmap};
use types::keyword::Keyword;

pub fn za_mailbox_counters(
    snapshot: &ZaSnapshot,
    mailbox_ids: impl IntoIterator<Item = u32>,
    count_threads: bool,
) -> AHashMap<u32, MailboxCounters> {
    mailbox_counters_by(&snapshot.cache, mailbox_ids, count_threads, |message| {
        snapshot.thread_of(message.document_id())
    })
}

pub fn za_thread_keywords(
    snapshot: &ZaSnapshot,
    keyword: &Keyword,
    match_all: bool,
    documents: Option<&RoaringBitmap>,
    visible: Option<&RoaringBitmap>,
    thread_of: impl Fn(&MessageRef<'_>) -> u32,
) -> RoaringBitmap {
    thread_keywords_by(
        &snapshot.cache,
        keyword,
        match_all,
        documents,
        visible,
        thread_of,
    )
}

pub fn za_group_by_thread<'x>(
    snapshot: &ZaSnapshot,
    messages: impl Iterator<Item = MessageRef<'x>>,
) -> AHashMap<u32, Vec<u32>> {
    let mut threads: AHashMap<u32, Vec<u32>> = AHashMap::new();
    for message in messages {
        let document_id = message.document_id();
        threads
            .entry(snapshot.thread_of(document_id))
            .or_default()
            .push(document_id);
    }
    for members in threads.values_mut() {
        members.sort_unstable();
    }
    threads
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::INBOX_ID;
    use common::{
        MailboxesCache, MessageCache, MessageStoreCache, MessageUid, MessagesCache,
        za::search::cache::{LoadedIndex, ZaSearchCache},
    };
    use std::sync::Arc;
    use store::{
        ahash::AHashSet,
        za_search::{
            manifest::Manifest,
            thread::{ThreadPart, ThreadRecord},
        },
    };
    use types::keyword::{DRAFT, SEEN};

    #[tokio::test]
    async fn counts_and_keyword_threads_group_by_the_map() {
        let cache = ZaSearchCache::new(Ok(Default::default()));
        let entry = cache.entry(1);
        let mut index = LoadedIndex::empty(Manifest::default());
        index.thread = ThreadPart::from_parts(
            [(1, 1), (2, 1), (3, 3)]
                .into_iter()
                .map(|(document_id, thread_id)| ThreadRecord {
                    document_id,
                    thread_id,
                    revision: 1,
                    created: 2,
                    deleted: None,
                    subject_hash: 0,
                    hashes: vec![],
                })
                .collect(),
            vec![],
            vec![],
        );
        *entry.index.write().await = Some(index);
        let index = entry.read_loaded().await.unwrap();
        let messages = [(1, 1 << SEEN), (2, 0), (3, 1 << DRAFT)]
            .into_iter()
            .map(|(id, keywords)| {
                MessageCache::new(
                    id,
                    [MessageUid {
                        mailbox_id: INBOX_ID,
                        uid: id,
                    }]
                    .into_iter()
                    .collect(),
                    keywords,
                    id,
                    2,
                    0,
                    u64::from(id),
                    0,
                )
            })
            .collect();
        let snapshot = ZaSnapshot {
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
        };
        let counters = za_mailbox_counters(&snapshot, [INBOX_ID], true);
        let inbox = &counters[&INBOX_ID];
        assert_eq!((inbox.total, inbox.unread), (3, 1));
        assert_eq!(inbox.threads, [1, 3].into_iter().collect::<AHashSet<_>>());
        assert_eq!(
            inbox.unread_threads,
            [1].into_iter().collect::<AHashSet<_>>()
        );

        let keywords = |all, documents, visible| {
            za_thread_keywords(
                &snapshot,
                &Keyword::Seen,
                all,
                documents,
                visible,
                |message| snapshot.thread_of(message.document_id()),
            )
        };
        let some_seen = |documents, visible| keywords(false, documents, visible);
        assert_eq!(some_seen(None, None), RoaringBitmap::from([1, 2]));
        assert!(keywords(true, None, None).is_empty());
        let documents = RoaringBitmap::from([2]);
        let visible = RoaringBitmap::from([1, 2]);
        assert_eq!(some_seen(Some(&documents), Some(&visible)), documents);
        assert!(some_seen(Some(&documents), Some(&documents)).is_empty());

        let groups = za_group_by_thread(&snapshot, snapshot.cache.emails.iter());
        assert_eq!(groups[&1], [1, 2]);
        assert_eq!(groups[&3], [3]);
    }
}
