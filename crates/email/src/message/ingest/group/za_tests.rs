/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Mail spec 5.4, ruling 59: a key account's message takes no part in an
//! `IngestGroup`'s pending threads.

use super::{IngestGroup, ThreadKey};
use crate::message::ingest::{PendingEmail, PendingIngest, thread_hash};
use common::storage::metadata::PrivateMetadataCommit;
use std::time::Instant;
use store::write::{BatchBuilder, PendingId, SlotRange};
use trc::MessageIngestEvent;
use types::{blob_hash::BlobHash, collection::Collection};

fn queued(thread_id: PendingId, za_key: bool) -> PendingIngest {
    PendingIngest::Queued(PendingEmail {
        account_id: 1,
        document_slot: Default::default(),
        uid_slots: SlotRange::default(),
        thread_id,
        blob_hash: BlobHash::default(),
        size: 0,
        thread_recheck: None,
        private_commit: PrivateMetadataCommit::default(),
        event: MessageIngestEvent::ImapAppend,
        encrypted: false,
        mailbox_ids: Vec::new(),
        message_id: None,
        session_id: 0,
        start_time: Instant::now(),
        za_key,
    })
}

fn first() -> ThreadKey {
    ThreadKey {
        hash: thread_hash("grouped mixed"),
        message_ids: vec![1],
    }
}

fn reply() -> ThreadKey {
    ThreadKey {
        hash: thread_hash("grouped mixed"),
        message_ids: vec![1, 2],
    }
}

/// Two distinct pending threads, as two messages of a group create.
fn two_threads() -> (PendingId, PendingId) {
    let mut batch = BatchBuilder::new();
    let a = batch.reserve_document_id(1, Collection::Email);
    let b = batch.reserve_document_id(2, Collection::Email);
    assert_ne!(a, b);
    (PendingId::Slot(a), PendingId::Slot(b))
}

#[test]
fn a_key_message_is_recorded_under_no_thread() {
    let (plain, key) = two_threads();
    let mut group = IngestGroup::default();
    group.record_thread(&queued(key, true), first());
    assert!(group.threads.is_empty(), "a key message recorded a thread");
    assert_eq!(group.pending_thread(&reply()), None);
    // A plain message recorded after it is the reply's one thread.
    group.record_thread(&queued(plain, false), first());
    assert_eq!(group.pending_thread(&reply()), Some(plain));
}

#[test]
fn a_plain_reply_joins_its_parent_past_a_key_copy() {
    // The order of `grouped_ingest_mixed_key_and_plain`: the plain message,
    // then the key account's copy of it, then the plain reply.
    let (plain, key) = two_threads();
    let mut group = IngestGroup::default();
    group.record_thread(&queued(plain, false), first());
    group.record_thread(&queued(key, true), first());
    assert_eq!(group.threads.len(), 1);
    assert_eq!(group.pending_thread(&reply()), Some(plain));
}

#[test]
fn control_two_plain_threads_under_one_key_join_neither() {
    // Upstream's rule, the one a recorded key message would trigger: two
    // pending threads under the reply's key leave the reply to the store.
    let (plain, other) = two_threads();
    let mut group = IngestGroup::default();
    group.record_thread(&queued(plain, false), first());
    group.record_thread(&queued(other, false), first());
    assert_eq!(group.pending_thread(&reply()), None);
}
