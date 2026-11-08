/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{K, ThreadFieldsK};
use crate::ahash::AHashMap;
use std::cmp::Reverse;

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ThreadRecord {
    pub document_id: u32,
    pub thread_id: u32,
    pub revision: u32,
    pub created: u64,
    pub deleted: Option<u64>,
    pub subject_hash: u128,
    pub hashes: Vec<u128>,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct Alias {
    pub source: u32,
    pub target: u32,
    pub revision: u32,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct RevisionRow {
    pub revision: u32,
    pub change_id: u64,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct Tombstone {
    pub document_id: u32,
    pub deleted: u64,
}

pub struct Assignment {
    pub thread_id: u32,
    pub aliases: Vec<Alias>,
}

#[derive(Default, Clone)]
pub struct ThreadPart {
    records: AHashMap<u32, ThreadRecord>,
    aliases: AHashMap<u32, Alias>,
    rows: Vec<RevisionRow>,
    threads: AHashMap<u32, Vec<u32>>,
    members: AHashMap<u32, Vec<u32>>,
    buckets: AHashMap<u128, Vec<u32>>,
}

impl ThreadPart {
    pub fn from_parts(
        records: Vec<ThreadRecord>,
        aliases: Vec<Alias>,
        rows: Vec<RevisionRow>,
    ) -> Self {
        let mut part = Self {
            records: records
                .into_iter()
                .map(|mut record| {
                    if record.deleted.is_some() {
                        record.clear_threading_fields();
                    }
                    (record.document_id, record)
                })
                .collect(),
            aliases: aliases.into_iter().map(|a| (a.source, a)).collect(),
            ..Self::default()
        };
        part.merge_rows(rows);
        part.rebuild_thread_tables();
        for record in part
            .records
            .values()
            .filter(|record| record.deleted.is_none())
        {
            part.buckets
                .entry(record.subject_hash)
                .or_default()
                .push(record.document_id);
        }
        part
    }

    pub fn extend(
        &mut self,
        records: impl IntoIterator<Item = ThreadRecord>,
        aliases: impl IntoIterator<Item = Alias>,
        rows: impl IntoIterator<Item = RevisionRow>,
        tombstones: impl IntoIterator<Item = Tombstone>,
    ) {
        self.add_aliases(aliases);
        for record in records {
            self.insert_record(record);
        }
        self.merge_rows(rows);
        for tombstone in tombstones {
            self.tombstone(tombstone.document_id, tombstone.deleted);
        }
    }

    pub fn record(&self, document_id: u32) -> Option<&ThreadRecord> {
        self.records.get(&document_id)
    }

    pub fn is_present(&self, document_id: u32) -> bool {
        self.record(document_id)
            .is_some_and(|record| record.deleted.is_none())
    }

    pub fn records(&self) -> impl Iterator<Item = &ThreadRecord> {
        self.records.values()
    }

    pub fn aliases(&self) -> impl Iterator<Item = &Alias> {
        self.aliases.values()
    }

    pub fn rows(&self) -> &[RevisionRow] {
        &self.rows
    }

    pub fn newest_revision(&self) -> u32 {
        self.rows.last().map_or(0, |row| row.revision)
    }

    pub fn revision_at(&self, change_id: u64) -> u32 {
        self.rows
            .iter()
            .rev()
            .find(|row| row.change_id <= change_id)
            .map_or(0, |row| row.revision)
    }

    pub fn revision_change_id(&self, revision: u32) -> Option<u64> {
        self.rows
            .binary_search_by_key(&revision, |row| row.revision)
            .ok()
            .map(|pos| self.rows[pos].change_id)
    }

    pub fn thread_of(&self, document_id: u32, change_id: u64) -> u32 {
        self.record(document_id).map_or(document_id, |record| {
            self.thread_at_revision(record, self.revision_at(change_id))
        })
    }

    /// Resolve the assignment through all held aliases, including staged revisions.
    pub fn current_thread(&self, document_id: u32) -> u32 {
        self.record(document_id).map_or(document_id, |record| {
            follow_aliases(&self.aliases, record.thread_id, |_| true).0
        })
    }

    pub fn exists_at(&self, document_id: u32, change_id: u64) -> Option<bool> {
        self.record(document_id)
            .map(|record| record.exists_at(change_id))
    }

    pub fn members_of(&self, thread_id: u32, change_id: u64) -> Vec<u32> {
        let revision = self.revision_at(change_id);
        let mut members: Vec<_> = self
            .records()
            .filter(|record| {
                record.exists_at(change_id)
                    && self.thread_at_revision(record, revision) == thread_id
            })
            .map(|record| record.document_id)
            .collect();
        members.sort_unstable();
        members
    }

    pub fn present_members(&self, thread_id: u32) -> &[u32] {
        self.threads.get(&thread_id).map_or(&[], Vec::as_slice)
    }

    /// Includes retained tombstones so a caller can apply its own snapshot's present set.
    pub fn members(&self, thread_id: u32) -> &[u32] {
        self.members.get(&thread_id).map_or(&[], Vec::as_slice)
    }

    pub fn assign(
        &mut self,
        fields: &ThreadFieldsK,
        document_id: u32,
        revision: u32,
        created: u64,
    ) -> Assignment {
        debug_assert!(fields.hashes.len() <= K);
        let mut counts = AHashMap::<u32, usize>::new();
        for candidate in self.matching_documents(fields) {
            *counts.entry(self.current_thread(candidate)).or_default() += 1;
        }
        let thread_id = counts
            .iter()
            .max_by_key(|&(&id, &count)| (count, Reverse(id)))
            .map_or(document_id, |(&id, _)| id);
        let mut aliases: Vec<_> = counts
            .into_keys()
            .filter(|&source| source != thread_id)
            .map(|source| Alias {
                source,
                target: thread_id,
                revision,
            })
            .collect();
        aliases.sort_unstable_by_key(|alias| alias.source);

        self.add_aliases(aliases.iter().copied());
        self.insert_record(ThreadRecord {
            document_id,
            thread_id,
            revision,
            created,
            deleted: None,
            subject_hash: fields.subject_hash,
            hashes: fields.hashes.clone(),
        });
        Assignment { thread_id, aliases }
    }

    pub fn tombstone(&mut self, document_id: u32, deleted: u64) -> bool {
        let Some(record) = self.records.get_mut(&document_id) else {
            return false;
        };
        let subject_hash = record.subject_hash;
        let was_present = record.deleted.replace(deleted).is_none();
        record.clear_threading_fields();
        if was_present {
            let thread_id = self.current_thread(document_id);
            remove_thread_member(&mut self.threads, thread_id, document_id);
            self.remove_bucket_member(subject_hash, document_id);
        }
        true
    }

    pub fn fold_sub_bound(&mut self, sub_bound: impl Fn(&Alias) -> bool) -> usize {
        let mut moved = 0;
        for record in self.records.values_mut() {
            // All aliases remain available until each record has followed its expired prefix.
            let (thread_id, revision) = follow_aliases(&self.aliases, record.thread_id, &sub_bound);
            if let Some(revision) = revision {
                record.thread_id = thread_id;
                record.revision = revision;
                moved += 1;
            }
        }
        self.aliases.retain(|_, alias| !sub_bound(alias));
        self.rebuild_thread_tables();
        moved
    }

    pub fn loaded_bytes(&self) -> usize {
        self.records()
            .map(|record| 72 + record.hashes.len() * 16)
            .sum::<usize>()
            + self.aliases.len() * 16
            + self.rows.len() * 16
            + self
                .threads
                .values()
                .map(|ids| ids.len() * 8)
                .sum::<usize>()
            + self
                .members
                .values()
                .map(|ids| ids.len() * 8)
                .sum::<usize>()
    }

    fn matching_documents<'a>(
        &'a self,
        fields: &'a ThreadFieldsK,
    ) -> impl Iterator<Item = u32> + 'a {
        self.buckets
            .get(&fields.subject_hash)
            .filter(|_| !fields.hashes.is_empty())
            .into_iter()
            .flatten()
            .copied()
            .filter(|id| {
                self.records[id]
                    .hashes
                    .iter()
                    .any(|hash| fields.hashes.contains(hash))
            })
    }

    fn thread_at_revision(&self, record: &ThreadRecord, revision: u32) -> u32 {
        if record.revision > revision {
            record.document_id
        } else {
            follow_aliases(&self.aliases, record.thread_id, |alias| {
                alias.revision <= revision
            })
            .0
        }
    }

    fn merge_rows(&mut self, rows: impl IntoIterator<Item = RevisionRow>) {
        for row in rows {
            match self
                .rows
                .binary_search_by_key(&row.revision, |row| row.revision)
            {
                Ok(pos) => self.rows[pos] = row,
                Err(pos) => self.rows.insert(pos, row),
            }
        }
    }

    fn add_aliases(&mut self, aliases: impl IntoIterator<Item = Alias>) {
        let mut sources = Vec::new();
        let mut target_replaced = false;
        for alias in aliases {
            if let Some(previous) = self.aliases.insert(alias.source, alias) {
                target_replaced |= previous.target != alias.target;
            } else {
                sources.push(alias.source);
            }
        }
        if target_replaced {
            self.rebuild_thread_tables();
            return;
        }

        // A batch may introduce a chain in any order. Move each newly retired
        // thread directly to its final survivor.
        for source in sources {
            let target = follow_aliases(&self.aliases, source, |_| true).0;
            move_members(&mut self.threads, source, target);
            move_members(&mut self.members, source, target);
        }
    }

    fn insert_record(&mut self, mut record: ThreadRecord) {
        let document_id = record.document_id;
        let thread_id = follow_aliases(&self.aliases, record.thread_id, |_| true).0;
        let subject_hash = record.subject_hash;
        let present = record.deleted.is_none();
        if !present {
            record.clear_threading_fields();
        }

        if let Some(old) = self.records.insert(document_id, record) {
            let old_thread = follow_aliases(&self.aliases, old.thread_id, |_| true).0;
            remove_thread_member(&mut self.members, old_thread, document_id);
            if old.deleted.is_none() {
                remove_thread_member(&mut self.threads, old_thread, document_id);
                self.remove_bucket_member(old.subject_hash, document_id);
            }
        }

        insert_sorted(self.members.entry(thread_id).or_default(), document_id);
        if present {
            insert_sorted(self.threads.entry(thread_id).or_default(), document_id);
            self.buckets
                .entry(subject_hash)
                .or_default()
                .push(document_id);
        }
    }

    fn remove_bucket_member(&mut self, subject_hash: u128, document_id: u32) {
        if let Some(bucket) = self.buckets.get_mut(&subject_hash) {
            if let Some(pos) = bucket.iter().position(|id| *id == document_id) {
                bucket.swap_remove(pos);
            }
            if bucket.is_empty() {
                self.buckets.remove(&subject_hash);
            }
        }
    }

    fn rebuild_thread_tables(&mut self) {
        self.threads.clear();
        self.members.clear();
        for record in self.records.values() {
            let thread_id = follow_aliases(&self.aliases, record.thread_id, |_| true).0;
            self.members
                .entry(thread_id)
                .or_default()
                .push(record.document_id);
            if record.deleted.is_none() {
                self.threads
                    .entry(thread_id)
                    .or_default()
                    .push(record.document_id);
            }
        }
        for ids in self.threads.values_mut().chain(self.members.values_mut()) {
            ids.sort_unstable();
        }
    }
}

impl ThreadRecord {
    fn exists_at(&self, change_id: u64) -> bool {
        self.created <= change_id && self.deleted.is_none_or(|deleted| deleted > change_id)
    }

    fn clear_threading_fields(&mut self) {
        self.hashes = Vec::new();
        self.subject_hash = 0;
    }
}

fn follow_aliases(
    aliases: &AHashMap<u32, Alias>,
    mut thread_id: u32,
    applies: impl Fn(&Alias) -> bool,
) -> (u32, Option<u32>) {
    let mut revision = None;
    for _ in 0..=aliases.len() {
        let Some(alias) = aliases.get(&thread_id).filter(|alias| applies(alias)) else {
            break;
        };
        thread_id = alias.target;
        revision = Some(alias.revision);
    }
    (thread_id, revision)
}

fn insert_sorted(ids: &mut Vec<u32>, id: u32) {
    if let Err(pos) = ids.binary_search(&id) {
        ids.insert(pos, id);
    }
}

fn remove_thread_member(table: &mut AHashMap<u32, Vec<u32>>, thread_id: u32, document_id: u32) {
    if let Some(ids) = table.get_mut(&thread_id) {
        if let Ok(pos) = ids.binary_search(&document_id) {
            ids.remove(pos);
        }
        if ids.is_empty() {
            table.remove(&thread_id);
        }
    }
}

fn move_members(table: &mut AHashMap<u32, Vec<u32>>, source: u32, target: u32) {
    if let Some(mut moved) = table.remove(&source) {
        let members = table.entry(target).or_default();
        members.append(&mut moved);
        members.sort_unstable();
    }
}

#[cfg(test)]
#[path = "thread_tests.rs"]
mod tests;
