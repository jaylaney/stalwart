/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use common::Server;
use std::future::Future;
use store::{
    ahash::AHashMap,
    query::log::{Change, ZaChangeRecords},
    roaring::RoaringBitmap,
    za_search::thread::ThreadPart,
};
use types::{collection::SyncCollection, id::Id};

pub trait ZaRetainedBound: Sync + Send {
    fn za_retained_bound(&self, account_id: u32) -> impl Future<Output = trc::Result<u64>> + Send;
}

impl ZaRetainedBound for Server {
    async fn za_retained_bound(&self, account_id: u32) -> trc::Result<u64> {
        let store = self.store();
        let email = store
            .za_log_first_retained(account_id, SyncCollection::Email)
            .await?;
        let thread = store
            .za_log_first_retained(account_id, SyncCollection::Thread)
            .await?;
        Ok(email.max(thread).unwrap_or(0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Created,
    Updated,
    Destroyed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct StreamItem {
    pub document_id: u32,
    pub kind: StreamKind,
    pub old: Option<Id>,
    pub new: Option<Id>,
    pub order: (u64, u32),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ThreadStreamItem {
    pub thread_id: u32,
    pub kind: StreamKind,
    pub order: (u64, u32),
}

pub struct RangeView<'a> {
    pub records: &'a ZaChangeRecords,
    pub later: &'a ZaChangeRecords,
    pub present: &'a dyn Fn(u32) -> bool,
    pub end_is_current: bool,
}

#[derive(Default)]
struct Lifecycle {
    inserted: RoaringBitmap,
    deleted: RoaringBitmap,
}

impl Lifecycle {
    fn record(&mut self, change: Change) {
        match change {
            Change::InsertItem(id) => {
                self.inserted.insert(id as u32);
            }
            Change::DeleteItem(id) => {
                self.deleted.insert(id as u32);
            }
            _ => {}
        }
    }

    fn of(records: &ZaChangeRecords) -> Self {
        let mut lifecycle = Self::default();
        for (_, changes) in &records.records {
            for &change in changes {
                lifecycle.record(change);
            }
        }
        lifecycle
    }

    fn undo(&self, mut present: RoaringBitmap) -> RoaringBitmap {
        present |= &self.deleted;
        present -= &self.inserted;
        present
    }
}

pub fn za_present_at_end(
    present_now: impl Iterator<Item = u32>,
    later: &ZaChangeRecords,
) -> RoaringBitmap {
    Lifecycle::of(later).undo(present_now.collect())
}

pub fn za_initial_stream(
    thread: &ThreadPart,
    present_at_end: impl Iterator<Item = u32>,
    end: u64,
) -> Vec<StreamItem> {
    let revision = thread.revision_at(end);
    let mut items: Vec<_> = present_at_end
        .map(|document_id| StreamItem {
            document_id,
            kind: StreamKind::Created,
            old: None,
            new: Some(Id::from_parts(
                thread.thread_of_revision(document_id, revision),
                document_id,
            )),
            order: (0, document_id),
        })
        .collect();
    items.sort_unstable_by_key(|item| item.order);
    items
}

pub fn za_initial_thread_stream(
    thread: &ThreadPart,
    present_at_end: impl Iterator<Item = u32>,
    end: u64,
) -> Vec<ThreadStreamItem> {
    let revision = thread.revision_at(end);
    let mut first_member = AHashMap::new();
    for id in present_at_end {
        remember_first(
            &mut first_member,
            thread.thread_of_revision(id, revision),
            id,
        );
    }
    let mut items: Vec<_> = first_member
        .into_iter()
        .map(|(thread_id, id)| ThreadStreamItem {
            thread_id,
            kind: StreamKind::Created,
            order: (0, id),
        })
        .collect();
    items.sort_unstable_by_key(|item| item.order);
    items
}

pub fn za_epoch_refuses(
    records_of_kind: &ZaChangeRecords,
    is_container: bool,
    since: u64,
    epoch: u64,
) -> bool {
    since > 0
        && since < epoch
        && records_of_kind.records.iter().any(|(id, changes)| {
            *id > since
                && *id <= epoch
                && changes.iter().any(|change| {
                    if is_container {
                        change.is_container_change()
                    } else {
                        change.is_item_change()
                    }
                })
        })
}

struct History<'a> {
    thread: &'a ThreadPart,
    since: u64,
    before_revision: u32,
    end_revision: u32,
    now: &'a dyn Fn(u32) -> bool,
    end_is_current: bool,
    at_end: Option<&'a RoaringBitmap>,
    lifecycle: Lifecycle,
    later: Lifecycle,
    updated: RoaringBitmap,
    candidates: AHashMap<u32, u64>,
}

impl<'a> History<'a> {
    fn new(
        thread: &'a ThreadPart,
        view: &'a RangeView<'_>,
        since: u64,
        end: u64,
        at_end: Option<&'a RoaringBitmap>,
    ) -> Self {
        let mut history = Self {
            thread,
            since,
            before_revision: thread.revision_at(since),
            end_revision: thread.revision_at(end),
            now: view.present,
            end_is_current: view.end_is_current,
            at_end,
            lifecycle: Lifecycle::default(),
            later: Lifecycle::of(view.later),
            updated: RoaringBitmap::new(),
            candidates: AHashMap::new(),
        };
        for (change_id, changes) in &view.records.records {
            for &change in changes {
                if let Some(id) = change.item_id() {
                    let id = id as u32;
                    remember_first(&mut history.candidates, id, *change_id);
                    history.lifecycle.record(change);
                    if matches!(
                        change,
                        Change::UpdateItem(_) | Change::UpdateItemMetadata(_)
                    ) {
                        history.updated.insert(id);
                    }
                }
            }
        }

        let rows = thread.rows();
        let first = rows.partition_point(|row| row.revision <= history.before_revision);
        let last = rows.partition_point(|row| row.revision <= history.end_revision);
        let revisions = &rows[first..last];
        for record in thread.records() {
            let id = record.document_id;
            let old = history.thread_before(id);
            if old != history.thread_at_end(id) {
                // Assignment replaces an unindexed singleton once. Merges
                // always alias a current root to another current root, so a
                // retired prefix cannot return on this document's alias path.
                let first = revisions
                    .partition_point(|row| thread.thread_of_revision(id, row.revision) == old);
                if let Some(row) = revisions.get(first) {
                    remember_first(&mut history.candidates, id, row.change_id);
                }
            }
            if let Some(deleted) = record.deleted.filter(|&id| id > since && id <= end) {
                remember_first(&mut history.candidates, id, deleted);
            }
        }
        history
    }

    fn thread_before(&self, id: u32) -> u32 {
        self.thread.thread_of_revision(id, self.before_revision)
    }

    fn thread_at_end(&self, id: u32) -> u32 {
        self.thread.thread_of_revision(id, self.end_revision)
    }

    fn existed(&self, id: u32) -> bool {
        if self.lifecycle.inserted.contains(id) {
            false
        } else if self.lifecycle.deleted.contains(id) {
            true
        } else if !(self.now)(id) && !self.later.deleted.contains(id) {
            false
        } else {
            self.thread.exists_at(id, self.since).unwrap_or(true)
        }
    }

    fn exists_at_end(&self, id: u32) -> bool {
        if self.lifecycle.deleted.contains(id) {
            false
        } else if self.end_is_current {
            (self.now)(id)
        } else if let Some(present) = self.at_end {
            present.contains(id)
        } else {
            ((self.now)(id) || self.later.deleted.contains(id)) && !self.later.inserted.contains(id)
        }
    }
}

fn remember_first<T: Copy + Ord>(items: &mut AHashMap<u32, T>, id: u32, value: T) {
    let first = items.entry(id).or_insert(value);
    *first = (*first).min(value);
}

pub fn za_email_stream(
    thread: &ThreadPart,
    view: &RangeView<'_>,
    since: u64,
    end: u64,
) -> Vec<StreamItem> {
    za_email_stream_with_presence(thread, view, since, end, None)
}

pub fn za_email_stream_with_presence(
    thread: &ThreadPart,
    view: &RangeView<'_>,
    since: u64,
    end: u64,
    at_end: Option<&RoaringBitmap>,
) -> Vec<StreamItem> {
    let history = History::new(thread, view, since, end, at_end);
    let mut candidates: Vec<_> = history
        .candidates
        .iter()
        .map(|(&id, &change_id)| (change_id, id))
        .collect();
    candidates.sort_unstable();
    let mut items = Vec::new();
    for order @ (_, document_id) in candidates {
        let old = history
            .existed(document_id)
            .then(|| Id::from_parts(history.thread_before(document_id), document_id));
        let new = history
            .exists_at_end(document_id)
            .then(|| Id::from_parts(history.thread_at_end(document_id), document_id));
        if old != new {
            if old.is_some() {
                items.push(StreamItem {
                    document_id,
                    kind: StreamKind::Destroyed,
                    old,
                    new: None,
                    order,
                });
            }
            if new.is_some() {
                items.push(StreamItem {
                    document_id,
                    kind: StreamKind::Created,
                    old: None,
                    new,
                    order,
                });
            }
        } else if new.is_some() && history.updated.contains(document_id) {
            items.push(StreamItem {
                document_id,
                kind: StreamKind::Updated,
                old,
                new,
                order,
            });
        }
    }
    items
}

pub fn za_thread_stream(
    thread: &ThreadPart,
    view: &RangeView<'_>,
    since: u64,
    end: u64,
) -> Vec<ThreadStreamItem> {
    za_thread_stream_with_presence(thread, view, since, end, None)
}

pub fn za_thread_stream_with_presence(
    thread: &ThreadPart,
    view: &RangeView<'_>,
    since: u64,
    end: u64,
    at_end: Option<&RoaringBitmap>,
) -> Vec<ThreadStreamItem> {
    let history = History::new(thread, view, since, end, at_end);
    let mut touched = AHashMap::new();
    for (&id, &change_id) in &history.candidates {
        remember_first(&mut touched, history.thread_before(id), change_id);
        remember_first(&mut touched, history.thread_at_end(id), change_id);
    }

    // Compare member sets for touched threads. Indexed and raw-only members
    // use the same existence rules at both ends, including lagging tombstones.
    let documents = thread.records().map(|record| record.document_id).chain(
        history
            .candidates
            .keys()
            .copied()
            .filter(|&id| thread.record(id).is_none()),
    );
    let mut before = AHashMap::<_, RoaringBitmap>::new();
    let mut after = AHashMap::<_, RoaringBitmap>::new();
    for id in documents {
        let old_thread = history.thread_before(id);
        if touched.contains_key(&old_thread) && history.existed(id) {
            before.entry(old_thread).or_default().insert(id);
        }
        let new_thread = history.thread_at_end(id);
        if touched.contains_key(&new_thread) && history.exists_at_end(id) {
            after.entry(new_thread).or_default().insert(id);
        }
    }

    let mut items = Vec::new();
    for (thread_id, change_id) in touched {
        let kind = match (before.get(&thread_id), after.get(&thread_id)) {
            (None, Some(_)) => StreamKind::Created,
            (Some(_), None) => StreamKind::Destroyed,
            (Some(before), Some(after)) if before != after => StreamKind::Updated,
            _ => continue,
        };
        items.push(ThreadStreamItem {
            thread_id,
            kind,
            order: (change_id, thread_id),
        });
    }
    items.sort_unstable_by_key(|item| item.order);
    items
}

#[cfg(test)]
mod tests;
