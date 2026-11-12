/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Test-mode hooks of invitations spec 11, compiled only under `test_mode`
//! like release 1's auth-cache pauses (`za_test` in
//! `crates/http/src/auth/authenticate.rs`), whose `Pause` shape this copies.
//! None changes behaviour while unset. A pause is one-shot and keyed by
//! account id: only the first request for that account reaching the point
//! takes it; requests for other accounts pass and leave it set.
//!
//! - `FAN_OUT_PAUSE`: in the fan-out, between an item's document-id
//!   assignment and its commit, keyed by the recipient
//!   (`groupware::calendar::fanout`).
//! - `PUT_PAUSE`: in the CalDAV PUT handler right after the resource
//!   snapshot (`fetch_dav_resources`), keyed by the account
//!   (`dav::calendar::update`).
//! - `COMMIT_PAUSE`: in the CalDAV PUT handler immediately before the key
//!   account's leased create commits (`za_commit_create`), after the fan-out
//!   and the seal.
//! - `UPDATE_COMMIT_PAUSE`: immediately before an event PUT update commit,
//!   after fan-out and sealing, keyed by the event owner.
//! - `DELETE_COMMIT_PAUSE`: immediately before an event DELETE commit,
//!   after fan-out, keyed by the event owner.
//! - `COMMIT_FAIL_ONCE`: the commit of a key account's PUT create fails once
//!   with an internal error after its fan-out, keyed by the account
//!   (`dav::calendar::za_commit_create`).
//! - `UPDATE_COMMIT_FAIL_ONCE`: the first commit of a key account's PUT
//!   update fails once with an internal error after its fan-out, in place of
//!   `commit_batch`'s result, keyed by the event owner
//!   (`dav::calendar::update`, iMIP spec 8.2).
//! - `DELETE_COMMIT_FAIL_ONCE`: the commit of a key account's event or
//!   collection DELETE fails once with an internal error after its
//!   fan-outs, in place of `commit_batch`'s result, keyed by the event owner
//!   (`dav::calendar::delete`, iMIP spec 8.2).
//! - `IMIP_SUBMIT_FAIL_ONCE`: the account's next iMIP submission answers
//!   `Failed` without reaching the submitter, keyed by the event's account
//!   (`common::za::imip::za_imip_submit`, iMIP spec 5.3).
//! - `PIN_READ_FAIL_ONCE`: the account's next pin-state read
//!   (`Server::za_pin_state`) fails once with a store error, keyed by the
//!   session's account (`common::auth::vault`; the drain's REFRESH answer,
//!   iMIP spec 7's transient class, PR review ruling O-4).
//! - `TASK_CONSUMER_PAUSE`: the task manager's consumer loop takes no task
//!   while it is held; global (`services::task_manager::manager`).
//! - The lease-duration override: stands in for `ZA_UID_LEASE_SECS`, which
//!   is read once per process.
//! - The redelivery batch override: stands in for `REDELIVER_BATCH`, the
//!   entries one `contacts/pin` call attempts (`http::api::contacts`,
//!   through `common::za::redeliver_batch_override`).
//! - `DRAIN_LOOKUP_PAUSE`: in the drain's import, after the first UID
//!   lookup found nothing and before the creation lease, and in its mail
//!   arm after the UID lookup found the event and before the merge (iMIP
//!   spec question 3), keyed by the draining account
//!   (`groupware::calendar::drain`).
//! - `DRAIN_LEASE_PAUSE`: in the drain's import, holding the lease, after
//!   the second lookup and immediately before the commit, keyed by the
//!   draining account.
//! - `DRAIN_FAIL_ONCE`: the account's next drain commit fails with an
//!   internal error (a transient item error, spec 6.5).
//! - `PRIVATE_KEY_FAIL_ONCE`: the account's next private-key unwrap records
//!   a failure, as a corrupt wrap would (`common::auth::vault`).
//! - `DRAIN_ENUMERATE_FAIL_ONCE`: the account's next drain pass fails with an
//!   internal error while enumerating, before any item is read.
//! - `DRAIN_FAIL_ITEM`: every drain commit of the named (account, document
//!   id) fails with an internal error until the slot is cleared (not
//!   one-shot).
//! - `DRAIN_READ_FAIL_ITEM`: the drain's re-read of the named (account,
//!   document id) fails with an internal error until the slot is cleared
//!   (not one-shot).
//! - The drain skip: while an account is listed (or `DRAIN_SKIP_ALL` is),
//!   its drain does not run; the za suite lists every account after setup
//!   and the drain modules clear it for their duration.
//! - The drain retry override: stands in for `DRAIN_RETRY_SECS`.
//! - `PIN_DECIDE_PAUSE`: in the fan-out after its phase 1 reads (the
//!   recipients' served keys and the sender's pin record), before the
//!   phase 2 decision and write, keyed by the sender
//!   (`groupware::calendar::fanout`, key pinning spec 5.2).
//! - `RETIRE_PAUSE`: in the fan-out after phase 3's items and before the
//!   phase 4 retirement, keyed by the sender; `contacts/pin`'s inner
//!   fan-out reaches it too.
//! - `PIN_WRITE_FAIL_ONCE`: the account's next pin-record write by a
//!   fan-out's phase 2 (as the sender) or by `contacts/pin`'s step 5
//!   retirement (through `common::za::take_pin_write_fail`) fails once with
//!   an internal error, whichever takes it first; set while `contacts/pin`
//!   is parked at `REDELIVER_CLEANUP_PAUSE`, it reaches step 5.
//! - `REDELIVER_CLEANUP_PAUSE`: in `contacts/pin` after its last event
//!   commit, before the step 5 retirement, keyed by the account
//!   (`http::api::contacts`, key pinning spec 6.4).
//! - `INGEST_PAUSE`: in `email_ingest` right after the account read (the
//!   classification), keyed by the account (`email::message::ingest`, mail
//!   spec 6.1's provisioning fence).
//! - `COPY_PAUSE`: in `copy_message` right after the destination account
//!   read, keyed by the destination (`email::message::copy`).
//! - `SETUP_TOKEN_PAUSE`: in `setup-token` after its first emptiness check
//!   and immediately before the pending record's write, keyed by the
//!   account (`http::api::vault`, mail spec 6.1's recheck).
//! - `FORCE_SQL_STORE`: `setup-token` answers as on PostgreSQL or MySQL
//!   (mail spec 6.1).
//! - `INDEX_WRITE_PAUSE`: in the index worker after a pass's documents are
//!   built and immediately before the search store's write, keyed by the
//!   partition's account, for a pass that holds a document to write (mail
//!   ruling R54). At 0.16 its call site was `services::task_manager::index`;
//!   at 1.0 it is `services::index_queue::index::process_items`, before
//!   `Sink::flush`. The pass's queue entries stay stored while it is held.
//! - `TRAIN_PAUSE`: in `add_account_spam_sample` after the message's
//!   metadata read and before the sample is added to the batch, keyed by
//!   the sample's account (`email::message::ingest`, mail ruling R55).
//! - `INDEX_CHUNK_PAUSE`: in `Store::index_account_documents` after an
//!   index pass's first commit chunk and before its second, keyed by the
//!   pass's account (`store::search::index`); defined in `store::za_test`
//!   and re-exported here. 0.16's `INDEX_GUARD_FAIL_ONCE` is retired with
//!   R62's per-chunk guard and R63's confirmation read (Task 6, ruling 19).
//! - `FORCE_COMMIT_SPLIT` (ruling 18): while it names an account, every
//!   `BatchBuilder::commit_point()` of a batch whose current account is that
//!   account, or of a batch with no current account, adds a commit point.
//!   The second kind also covers batches that are not the test's: other
//!   accounts' search-index passes, the index queue's bookkeeping batch and
//!   registry-mapping batches split too while it is set. Defined in
//!   `store::za_test`; set it through `ForceCommitSplit`, whose drop clears
//!   it, so a test that panics while it is set does not leave every later
//!   batch splitting.
//! - `SECOND_CHUNK_FAIL_ONCE` (iMIP spec 8.2): `Store::write_batch` fails a
//!   batch whose current account is the named account before its second
//!   commit chunk, after the first is written; one-shot. Defined in
//!   `store::za_test`; its setter is re-exported here.
//! - `PROPFIND_ADMITTED_PAUSE` (upgrade ruling 142): in a foreign key
//!   account's calendar PROPFIND, after `foreign_calendar_contexts` admitted
//!   the calendars and before any item is read, keyed by the requesting
//!   principal (`dav::common::propfind::za`).

use super::search::ZaSearchConfig;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
pub use store::za_test::{
    FORCE_COMMIT_SPLIT, Pause, set_index_chunk_pause, set_second_chunk_fail_once,
};
use types::blob_hash::BlobHash;

/// Sets `FORCE_COMMIT_SPLIT` to `account_id` for the guard's lifetime; the
/// drop clears it, also when the test panics.
#[must_use = "dropping the guard clears FORCE_COMMIT_SPLIT"]
pub struct ForceCommitSplit(());

impl ForceCommitSplit {
    pub fn new(account_id: u32) -> Self {
        store::za_test::set_force_commit_split(Some(account_id));
        ForceCommitSplit(())
    }
}

impl Drop for ForceCommitSplit {
    fn drop(&mut self) {
        store::za_test::set_force_commit_split(None);
    }
}

pub static FAN_OUT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static PUT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static COMMIT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static UPDATE_COMMIT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static DELETE_COMMIT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// Global: its `account_id` is ignored.
pub static TASK_CONSUMER_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// The account whose next key-account PUT create fails its commit.
pub static COMMIT_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

/// The account whose next key-account PUT update fails its first commit.
pub static UPDATE_COMMIT_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

/// The account whose next key-account DELETE fails its commit.
pub static DELETE_COMMIT_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

/// The account whose next iMIP submission fails.
pub static IMIP_SUBMIT_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

/// The account whose next pin-state read fails.
pub static PIN_READ_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

pub static DRAIN_LOOKUP_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static DRAIN_LEASE_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// The account whose next drain commit fails.
pub static DRAIN_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

pub static PRIVATE_KEY_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

pub static DRAIN_ENUMERATE_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

pub static DRAIN_FAIL_ITEM: Mutex<Option<(u32, u32)>> = Mutex::new(None);

pub static DRAIN_READ_FAIL_ITEM: Mutex<Option<(u32, u32)>> = Mutex::new(None);

pub static PIN_DECIDE_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static RETIRE_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// The account whose next fan-out or step 5 pin-record write fails.
pub static PIN_WRITE_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

pub static REDELIVER_CLEANUP_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static INGEST_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static COPY_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static SETUP_TOKEN_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static INDEX_WRITE_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static TRAIN_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// Whether `setup-token` treats the data store as PostgreSQL or MySQL.
pub static FORCE_SQL_STORE: AtomicBool = AtomicBool::new(false);

/// Accounts whose drain does not run; `DRAIN_SKIP_ALL` stands for every
/// account.
static DRAIN_SKIP: Mutex<Vec<u32>> = Mutex::new(Vec::new());

pub const DRAIN_SKIP_ALL: u32 = u32::MAX;

/// Seconds before a pass that left an item pending is retried; `-1` unset.
static DRAIN_RETRY_OVERRIDE: AtomicI64 = AtomicI64::new(-1);

/// Seconds a new creation lease lasts; 0 when unset.
static LEASE_SECS_OVERRIDE: AtomicU64 = AtomicU64::new(0);

/// Entries one `contacts/pin` call attempts; 0 when unset.
static REDELIVER_BATCH_OVERRIDE: AtomicUsize = AtomicUsize::new(0);

fn set_slot(slot: &Mutex<Option<Arc<Pause>>>, pause: Option<Arc<Pause>>) {
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = pause;
}

pub fn set_fan_out_pause(pause: Option<Arc<Pause>>) {
    set_slot(&FAN_OUT_PAUSE, pause);
}

pub fn set_put_pause(pause: Option<Arc<Pause>>) {
    set_slot(&PUT_PAUSE, pause);
}

pub fn set_commit_pause(pause: Option<Arc<Pause>>) {
    set_slot(&COMMIT_PAUSE, pause);
}

pub fn set_update_commit_pause(pause: Option<Arc<Pause>>) {
    set_slot(&UPDATE_COMMIT_PAUSE, pause);
}
pub fn set_delete_commit_pause(pause: Option<Arc<Pause>>) {
    set_slot(&DELETE_COMMIT_PAUSE, pause);
}

pub fn set_task_consumer_pause(pause: Option<Arc<Pause>>) {
    set_slot(&TASK_CONSUMER_PAUSE, pause);
}

pub fn set_commit_fail_once(account_id: Option<u32>) {
    *COMMIT_FAIL_ONCE.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_update_commit_fail_once(account_id: Option<u32>) {
    *UPDATE_COMMIT_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_delete_commit_fail_once(account_id: Option<u32>) {
    *DELETE_COMMIT_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_imip_submit_fail_once(account_id: Option<u32>) {
    *IMIP_SUBMIT_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_pin_read_fail_once(account_id: Option<u32>) {
    *PIN_READ_FAIL_ONCE.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_lease_secs_override(secs: Option<u64>) {
    LEASE_SECS_OVERRIDE.store(secs.unwrap_or(0), Ordering::SeqCst);
}

pub fn lease_secs_override() -> Option<u64> {
    Some(LEASE_SECS_OVERRIDE.load(Ordering::SeqCst)).filter(|secs| *secs > 0)
}

pub fn set_redeliver_batch_override(entries: Option<usize>) {
    REDELIVER_BATCH_OVERRIDE.store(entries.unwrap_or(0), Ordering::SeqCst);
}

pub fn redeliver_batch_override() -> Option<usize> {
    Some(REDELIVER_BATCH_OVERRIDE.load(Ordering::SeqCst)).filter(|entries| *entries > 0)
}

pub fn set_drain_lookup_pause(pause: Option<Arc<Pause>>) {
    set_slot(&DRAIN_LOOKUP_PAUSE, pause);
}

pub fn set_drain_lease_pause(pause: Option<Arc<Pause>>) {
    set_slot(&DRAIN_LEASE_PAUSE, pause);
}

pub fn set_drain_fail_once(account_id: Option<u32>) {
    *DRAIN_FAIL_ONCE.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_private_key_fail(account_id: Option<u32>) {
    *PRIVATE_KEY_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_drain_enumerate_fail(account_id: Option<u32>) {
    *DRAIN_ENUMERATE_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_drain_fail_item(item: Option<(u32, u32)>) {
    *DRAIN_FAIL_ITEM.lock().unwrap_or_else(|e| e.into_inner()) = item;
}

pub fn set_drain_read_fail_item(item: Option<(u32, u32)>) {
    *DRAIN_READ_FAIL_ITEM
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = item;
}

pub fn set_pin_decide_pause(pause: Option<Arc<Pause>>) {
    set_slot(&PIN_DECIDE_PAUSE, pause);
}

pub fn set_retire_pause(pause: Option<Arc<Pause>>) {
    set_slot(&RETIRE_PAUSE, pause);
}

pub fn set_redeliver_cleanup_pause(pause: Option<Arc<Pause>>) {
    set_slot(&REDELIVER_CLEANUP_PAUSE, pause);
}

pub fn set_ingest_pause(pause: Option<Arc<Pause>>) {
    set_slot(&INGEST_PAUSE, pause);
}

pub fn set_copy_pause(pause: Option<Arc<Pause>>) {
    set_slot(&COPY_PAUSE, pause);
}

pub fn set_setup_token_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SETUP_TOKEN_PAUSE, pause);
}

pub fn set_index_write_pause(pause: Option<Arc<Pause>>) {
    set_slot(&INDEX_WRITE_PAUSE, pause);
}

pub fn set_train_pause(pause: Option<Arc<Pause>>) {
    set_slot(&TRAIN_PAUSE, pause);
}

pub fn set_force_sql_store(force: bool) {
    FORCE_SQL_STORE.store(force, Ordering::SeqCst);
}

pub fn force_sql_store() -> bool {
    FORCE_SQL_STORE.load(Ordering::SeqCst)
}

pub fn set_pin_write_fail(account_id: Option<u32>) {
    *PIN_WRITE_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn set_drain_skip(account_id: u32) {
    let mut skipped = DRAIN_SKIP.lock().unwrap_or_else(|e| e.into_inner());
    if !skipped.contains(&account_id) {
        skipped.push(account_id);
    }
}

pub fn clear_drain_skip(account_id: u32) {
    DRAIN_SKIP
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|skipped| *skipped != account_id);
}

/// Whether `account_id`'s drain is skipped: listed itself, or everyone is.
pub fn drain_skipped(account_id: u32) -> bool {
    let skipped = DRAIN_SKIP.lock().unwrap_or_else(|e| e.into_inner());
    skipped.contains(&account_id) || skipped.contains(&DRAIN_SKIP_ALL)
}

pub fn set_drain_retry_override(secs: Option<u64>) {
    DRAIN_RETRY_OVERRIDE.store(
        secs.map_or(-1, |secs| i64::try_from(secs).unwrap_or(i64::MAX)),
        Ordering::SeqCst,
    );
}

pub fn drain_retry_override() -> Option<u64> {
    let secs = DRAIN_RETRY_OVERRIDE.load(Ordering::SeqCst);
    (secs >= 0).then_some(secs as u64)
}

fn take_for(slot: &Mutex<Option<Arc<Pause>>>, account_id: u32) -> Option<Arc<Pause>> {
    let mut pause = slot.lock().unwrap_or_else(|e| e.into_inner());
    if pause.as_ref().is_some_and(|p| p.account_id == account_id) {
        pause.take()
    } else {
        None
    }
}

/// Whether `slot` names `account_id`; clears it if so.
fn take_flag(slot: &Mutex<Option<u32>>, account_id: u32) -> bool {
    let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
    if *slot == Some(account_id) {
        *slot = None;
        true
    } else {
        false
    }
}

/// The guard is gone before this await: `take_for` returns the pause.
async fn wait(pause: Option<Arc<Pause>>) {
    if let Some(pause) = pause {
        pause.arrived.notify_one();
        pause.release.notified().await;
    }
}

pub async fn fan_out_pause_point(recipient_id: u32) {
    wait(take_for(&FAN_OUT_PAUSE, recipient_id)).await;
}

pub async fn put_pause_point(account_id: u32) {
    wait(take_for(&PUT_PAUSE, account_id)).await;
}

pub async fn commit_pause_point(account_id: u32) {
    wait(take_for(&COMMIT_PAUSE, account_id)).await;
}

pub async fn update_commit_pause_point(account_id: u32) {
    wait(take_for(&UPDATE_COMMIT_PAUSE, account_id)).await;
}
pub async fn delete_commit_pause_point(account_id: u32) {
    wait(take_for(&DELETE_COMMIT_PAUSE, account_id)).await;
}

pub async fn task_consumer_pause_point() {
    let pause = TASK_CONSUMER_PAUSE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    wait(pause).await;
}

pub async fn drain_lookup_pause_point(account_id: u32) {
    wait(take_for(&DRAIN_LOOKUP_PAUSE, account_id)).await;
}

pub async fn drain_lease_pause_point(account_id: u32) {
    wait(take_for(&DRAIN_LEASE_PAUSE, account_id)).await;
}

pub async fn pin_decide_pause_point(sender_id: u32) {
    wait(take_for(&PIN_DECIDE_PAUSE, sender_id)).await;
}

pub async fn retire_pause_point(sender_id: u32) {
    wait(take_for(&RETIRE_PAUSE, sender_id)).await;
}

pub async fn redeliver_cleanup_pause_point(account_id: u32) {
    wait(take_for(&REDELIVER_CLEANUP_PAUSE, account_id)).await;
}

pub async fn ingest_pause_point(account_id: u32) {
    wait(take_for(&INGEST_PAUSE, account_id)).await;
}

pub async fn copy_pause_point(to_account_id: u32) {
    wait(take_for(&COPY_PAUSE, to_account_id)).await;
}

pub async fn setup_token_pause_point(account_id: u32) {
    wait(take_for(&SETUP_TOKEN_PAUSE, account_id)).await;
}

pub async fn index_write_pause_point(account_id: u32) {
    wait(take_for(&INDEX_WRITE_PAUSE, account_id)).await;
}

pub async fn train_pause_point(account_id: u32) {
    wait(take_for(&TRAIN_PAUSE, account_id)).await;
}

/// Whether the account's fan-out or step 5 pin-record write must fail now;
/// clears the flag.
pub fn take_pin_write_fail(account_id: u32) -> bool {
    take_flag(&PIN_WRITE_FAIL_ONCE, account_id)
}

/// Whether the drain commit of `account_id` must fail now; clears the flag.
pub fn take_drain_fail(account_id: u32) -> bool {
    take_flag(&DRAIN_FAIL_ONCE, account_id)
}

/// Whether the commit of `account_id` must fail now; clears the flag.
pub fn take_commit_fail(account_id: u32) -> bool {
    take_flag(&COMMIT_FAIL_ONCE, account_id)
}

/// Whether the PUT update commit of `account_id` must fail now; clears the
/// flag.
pub fn take_update_commit_fail(account_id: u32) -> bool {
    take_flag(&UPDATE_COMMIT_FAIL_ONCE, account_id)
}

/// Whether the DELETE commit of `account_id` must fail now; clears the flag.
pub fn take_delete_commit_fail(account_id: u32) -> bool {
    take_flag(&DELETE_COMMIT_FAIL_ONCE, account_id)
}

/// Whether the iMIP submission of `account_id` must fail now; clears the flag.
pub fn take_imip_submit_fail(account_id: u32) -> bool {
    take_flag(&IMIP_SUBMIT_FAIL_ONCE, account_id)
}

/// Whether the account's pin-state read must fail now; clears the flag.
pub fn take_pin_read_fail(account_id: u32) -> bool {
    take_flag(&PIN_READ_FAIL_ONCE, account_id)
}

/// Whether the account's next private-key unwrap must fail; clears the flag.
pub fn take_private_key_fail(account_id: u32) -> bool {
    take_flag(&PRIVATE_KEY_FAIL_ONCE, account_id)
}

/// Whether the account's next drain enumeration must fail; clears the flag.
pub fn take_drain_enumerate_fail(account_id: u32) -> bool {
    take_flag(&DRAIN_ENUMERATE_FAIL_ONCE, account_id)
}

/// Whether the drain commit of this item must fail; the slot stays set.
pub fn drain_item_fails(account_id: u32, document_id: u32) -> bool {
    *DRAIN_FAIL_ITEM.lock().unwrap_or_else(|e| e.into_inner()) == Some((account_id, document_id))
}

/// Whether the drain's re-read of this item must fail; the slot stays set.
pub fn drain_read_fails(account_id: u32, document_id: u32) -> bool {
    *DRAIN_READ_FAIL_ITEM
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        == Some((account_id, document_id))
}

/// Independent calendar rename and completion hooks, keyed by the owner account.
pub static SHARE_RENAME_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SHARE_BOUNDARY_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub fn set_share_rename_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SHARE_RENAME_PAUSE, pause);
}
pub fn set_share_boundary_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SHARE_BOUNDARY_PAUSE, pause);
}
pub async fn share_rename_pause_point(owner: u32) {
    wait(take_for(&SHARE_RENAME_PAUSE, owner)).await;
}
pub async fn share_boundary_pause_point(owner: u32) {
    wait(take_for(&SHARE_BOUNDARY_PAUSE, owner)).await;
}
pub static MOVE_OVERWRITE_OBSERVATION: Mutex<Vec<(u32, u32, bool)>> = Mutex::new(Vec::new());
pub fn record_move_overwrite_message(owner: u32, event: u32, has_delete_result: bool) {
    let mut values = MOVE_OVERWRITE_OBSERVATION
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    values.retain(|(o, e, _)| (*o, *e) != (owner, event));
    values.push((owner, event, has_delete_result));
}
pub fn take_move_overwrite_message(owner: u32, event: u32) -> Option<bool> {
    let mut values = MOVE_OVERWRITE_OBSERVATION
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let idx = values
        .iter()
        .position(|(o, e, _)| (*o, *e) == (owner, event))?;
    Some(values.swap_remove(idx).2)
}

/// First-grant conversion seams, independent of ordinary DAV write hooks.
pub static SHARE_PRECHECK_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SHARE_REWRAP_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SHARE_REWRAP_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);
pub fn set_share_precheck_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SHARE_PRECHECK_PAUSE, pause);
}
pub fn set_share_rewrap_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SHARE_REWRAP_PAUSE, pause);
}
pub fn set_share_rewrap_fail_once(owner: Option<u32>) {
    *SHARE_REWRAP_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = owner;
}
pub async fn share_precheck_pause_point(owner: u32) {
    wait(take_for(&SHARE_PRECHECK_PAUSE, owner)).await;
}
pub async fn share_rewrap_pause_point(owner: u32) {
    wait(take_for(&SHARE_REWRAP_PAUSE, owner)).await;
}
pub fn take_share_rewrap_fail_once(owner: u32) -> bool {
    take_flag(&SHARE_REWRAP_FAIL_ONCE, owner)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShareAssertFailure {
    pub owner_id: u32,
    pub calendar_id: u32,
    pub remaining: u32,
}
fn take_share_assert(slot: &Mutex<Option<ShareAssertFailure>>, owner: u32, calendar: u32) -> bool {
    let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(failure) = slot.as_mut()
        && failure.owner_id == owner
        && failure.calendar_id == calendar
        && failure.remaining > 0
    {
        failure.remaining -= 1;
        true
    } else {
        false
    }
}
pub static SHARE_INIT_ASSERT_FAIL: Mutex<Option<ShareAssertFailure>> = Mutex::new(None);
pub fn set_share_init_assert_fail(failure: Option<ShareAssertFailure>) {
    *SHARE_INIT_ASSERT_FAIL
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = failure;
}
pub fn take_share_init_assert_fail(owner_id: u32, calendar_id: u32) -> bool {
    take_share_assert(&SHARE_INIT_ASSERT_FAIL, owner_id, calendar_id)
}
pub static SHARE_REWRAP_ASSERT_FAIL: Mutex<Option<ShareAssertFailure>> = Mutex::new(None);
pub fn set_share_rewrap_assert_fail(failure: Option<ShareAssertFailure>) {
    *SHARE_REWRAP_ASSERT_FAIL
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = failure;
}
pub fn take_share_rewrap_assert_fail(owner_id: u32, calendar_id: u32) -> bool {
    take_share_assert(&SHARE_REWRAP_ASSERT_FAIL, owner_id, calendar_id)
}
pub static SHARE_REVERT_ASSERT_FAIL: Mutex<Option<ShareAssertFailure>> = Mutex::new(None);
pub fn set_share_revert_assert_fail(failure: Option<ShareAssertFailure>) {
    *SHARE_REVERT_ASSERT_FAIL
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = failure;
}
pub fn take_share_revert_assert_fail(owner_id: u32, calendar_id: u32) -> bool {
    take_share_assert(&SHARE_REVERT_ASSERT_FAIL, owner_id, calendar_id)
}
pub static SHARE_ROLLBACK_ASSERT_FAIL: Mutex<Option<ShareAssertFailure>> = Mutex::new(None);
pub fn set_share_rollback_assert_fail(failure: Option<ShareAssertFailure>) {
    *SHARE_ROLLBACK_ASSERT_FAIL
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = failure;
}
pub fn take_share_rollback_assert_fail(owner_id: u32, calendar_id: u32) -> bool {
    take_share_assert(&SHARE_ROLLBACK_ASSERT_FAIL, owner_id, calendar_id)
}
pub static SHARE_RESTORE_ASSERT_FAIL: Mutex<Option<ShareAssertFailure>> = Mutex::new(None);
pub fn set_share_restore_assert_fail(failure: Option<ShareAssertFailure>) {
    *SHARE_RESTORE_ASSERT_FAIL
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = failure;
}
pub fn take_share_restore_assert_fail(owner_id: u32, calendar_id: u32) -> bool {
    take_share_assert(&SHARE_RESTORE_ASSERT_FAIL, owner_id, calendar_id)
}

/// One authoritative sharing mutation, after all decisions and before commit.
pub static SHARE_COMMIT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub fn set_share_commit_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SHARE_COMMIT_PAUSE, pause);
}
pub async fn share_commit_pause_point(owner: u32) {
    wait(take_for(&SHARE_COMMIT_PAUSE, owner)).await;
}

/// Sharing adapter seams for authenticated mark ordering and terminal-loss classification.
pub static SHARE_PIN_MARK_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SHARE_CONFLICT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub fn set_share_pin_mark_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SHARE_PIN_MARK_PAUSE, pause);
}
pub fn set_share_conflict_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SHARE_CONFLICT_PAUSE, pause);
}
pub async fn share_pin_mark_pause_point(owner: u32) {
    wait(take_for(&SHARE_PIN_MARK_PAUSE, owner)).await;
}
pub async fn share_conflict_pause_point(owner: u32) {
    wait(take_for(&SHARE_CONFLICT_PAUSE, owner)).await;
}

/// One request-local recipient lookup failure, matched by sender and normalized address.
pub static DELEGATION_LOOKUP_FAIL_ONCE: Mutex<Option<(u32, String)>> = Mutex::new(None);

pub fn set_delegation_lookup_fail_once(value: Option<(u32, String)>) {
    *DELEGATION_LOOKUP_FAIL_ONCE
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = value;
}

pub fn take_delegation_lookup_fail_once(sender_id: u32, address: &str) -> bool {
    let mut slot = DELEGATION_LOOKUP_FAIL_ONCE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if slot
        .as_ref()
        .is_some_and(|(id, key)| *id == sender_id && key == address)
    {
        slot.take();
        true
    } else {
        false
    }
}

/// Calendar PROPPATCH after its initial archive read, before fresh mutation resolution.
/// One-shot and owner-keyed, so a competing request can commit during that interval.
pub static CALENDAR_PROPPATCH_READ_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub fn set_calendar_proppatch_read_pause(pause: Option<Arc<Pause>>) {
    set_slot(&CALENDAR_PROPPATCH_READ_PAUSE, pause);
}
pub async fn calendar_proppatch_read_pause_point(owner: u32) {
    wait(take_for(&CALENDAR_PROPPATCH_READ_PAUSE, owner)).await;
}

/// Share listing after enumeration, before fresh archive resolution.
pub static SHARE_LIST_OWN_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SHARE_LIST_FOREIGN_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub fn set_share_list_own_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SHARE_LIST_OWN_PAUSE, pause);
}
pub fn set_share_list_foreign_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SHARE_LIST_FOREIGN_PAUSE, pause);
}
pub async fn share_list_own_pause_point(owner: u32) {
    wait(take_for(&SHARE_LIST_OWN_PAUSE, owner)).await;
}
pub async fn share_list_foreign_pause_point(reader: u32) {
    wait(take_for(&SHARE_LIST_FOREIGN_PAUSE, reader)).await;
}

/// Owner-scoped conversion assertion losses, excluding final grant decision losses.
static SHARE_CONVERSION_LOSSES: Mutex<Vec<u32>> = Mutex::new(Vec::new());
pub fn record_share_conversion_loss(owner: u32) {
    SHARE_CONVERSION_LOSSES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(owner);
}
pub fn take_share_conversion_losses(owner: u32) -> usize {
    let mut losses = SHARE_CONVERSION_LOSSES
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let count = losses.iter().filter(|id| **id == owner).count();
    losses.retain(|id| *id != owner);
    count
}

/// Collection MOVE after final fresh source/target resolution and delete assembly.
pub static CALENDAR_MOVE_DELETE_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub fn set_calendar_move_delete_pause(pause: Option<Arc<Pause>>) {
    set_slot(&CALENDAR_MOVE_DELETE_PAUSE, pause);
}
pub async fn calendar_move_delete_pause_point(owner: u32) {
    wait(take_for(&CALENDAR_MOVE_DELETE_PAUSE, owner)).await;
}

/// Admission metrics contain only owner/counts, never paths or archive payloads.
static CALENDAR_MOVE_PLANS: Mutex<Vec<(u32, usize, usize, usize)>> = Mutex::new(Vec::new());
pub fn record_calendar_move_plan(owner: u32, operations: usize, bytes: usize, slices: usize) {
    CALENDAR_MOVE_PLANS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((owner, operations, bytes, slices));
}
pub fn take_calendar_move_plans(owner: u32) -> Vec<(usize, usize, usize)> {
    let mut plans = CALENDAR_MOVE_PLANS
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut result = Vec::new();
    plans.retain(|(id, operations, bytes, slices)| {
        if *id == owner {
            result.push((*operations, *bytes, *slices));
            false
        } else {
            true
        }
    });
    result
}

/// Foreign calendar sync after raw log read, before identity sidecar read.
/// Only owner id is retained; paths and content never enter the hook.
pub static CALENDAR_SYNC_LOG_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub fn set_calendar_sync_log_pause(pause: Option<Arc<Pause>>) {
    set_slot(&CALENDAR_SYNC_LOG_PAUSE, pause);
}
pub async fn calendar_sync_log_pause_point(owner: u32) {
    wait(take_for(&CALENDAR_SYNC_LOG_PAUSE, owner)).await;
}

/// Calendar retention after both vanished ranges have been removed.
/// Only owner ID enters this test-mode-only publication-order seam.
pub static CALENDAR_HISTORY_PURGE_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub fn set_calendar_history_purge_pause(pause: Option<Arc<Pause>>) {
    set_slot(&CALENDAR_HISTORY_PURGE_PAUSE, pause);
}
pub async fn calendar_history_purge_pause_point(owner: u32) {
    wait(take_for(&CALENDAR_HISTORY_PURGE_PAUSE, owner)).await;
}

/// Foreign calendar PROPFIND after `foreign_calendar_contexts`' fresh own-id
/// admission and before any item is read (upgrade ruling 142), keyed by the
/// requesting principal (the grantee), so the owner's own requests pass.
pub static PROPFIND_ADMITTED_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub fn set_propfind_admitted_pause(pause: Option<Arc<Pause>>) {
    set_slot(&PROPFIND_ADMITTED_PAUSE, pause);
}
pub async fn propfind_admitted_pause_point(reader: u32) {
    wait(take_for(&PROPFIND_ADMITTED_PAUSE, reader)).await;
}

// Zero-access search hooks. The consuming tasks place the pause points and flags.
pub static SEARCH_COMMIT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SEARCH_RETRY_OBSERVATION_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SEARCH_SWAP_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SEARCH_SNAPSHOT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SEARCH_VIEW_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
static SEARCH_PAGE_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SEARCH_SWEEP_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
static SEARCH_RECOVERY_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SEARCH_PASS_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static SEARCH_DESTROY_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
static SEARCH_POST_UNLINK_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
static SEARCH_REBUILD_CREATE_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
static SEARCH_BLOB_TIMEOUT_ONCE: Mutex<Option<BlobTimeout>> = Mutex::new(None);
pub static SEARCH_HOLD_EXPIRE: Mutex<Option<u32>> = Mutex::new(None);
pub static SEARCH_CACHE_HOLD: Mutex<Option<u32>> = Mutex::new(None);
pub static SEARCH_SWAP_SKIP: Mutex<Option<u32>> = Mutex::new(None);
pub static SEARCH_OPEN_FAIL_ALL: Mutex<Option<u32>> = Mutex::new(None);
pub static SEARCH_BLOB_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);
pub static SEARCH_OPEN_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);
pub static SEARCH_MANIFEST_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);
pub static SEARCH_DESTROY_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);
pub static SEARCH_BLOB_SLOW: Mutex<Option<(u32, u64)>> = Mutex::new(None);
pub static SEARCH_CONFIG_OVERRIDE: Mutex<Option<ZaSearchConfig>> = Mutex::new(None);
static ZA_SEARCH_COUNTERS: std::sync::LazyLock<Mutex<HashMap<u32, (u64, u64, usize)>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn set_search_commit_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_COMMIT_PAUSE, pause);
}

pub async fn search_commit_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_COMMIT_PAUSE, account_id)).await;
}

pub fn set_search_retry_observation_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_RETRY_OBSERVATION_PAUSE, pause);
}

pub async fn search_retry_observation_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_RETRY_OBSERVATION_PAUSE, account_id)).await;
}

pub fn set_search_swap_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_SWAP_PAUSE, pause);
}

pub async fn search_swap_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_SWAP_PAUSE, account_id)).await;
}

pub fn set_search_snapshot_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_SNAPSHOT_PAUSE, pause);
}

pub async fn search_snapshot_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_SNAPSHOT_PAUSE, account_id)).await;
}

pub fn set_search_view_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_VIEW_PAUSE, pause);
}

pub async fn search_view_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_VIEW_PAUSE, account_id)).await;
}

pub fn set_search_page_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_PAGE_PAUSE, pause);
}

pub(super) async fn search_page_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_PAGE_PAUSE, account_id)).await;
}

pub fn set_search_sweep_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_SWEEP_PAUSE, pause);
}

pub async fn search_sweep_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_SWEEP_PAUSE, account_id)).await;
}

/// A single request takes the slot after observing loss, before it attempts
/// recovery. It holds no pass or loader guard while parked.
pub fn set_search_recovery_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_RECOVERY_PAUSE, pause);
}

pub async fn search_recovery_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_RECOVERY_PAUSE, account_id)).await;
}

pub fn set_search_pass_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_PASS_PAUSE, pause);
}

pub async fn search_pass_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_PASS_PAUSE, account_id)).await;
}

pub fn set_search_destroy_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_DESTROY_PAUSE, pause);
}

pub async fn search_destroy_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_DESTROY_PAUSE, account_id)).await;
}

/// Park destruction after it has unlinked its selected inventory.
pub fn set_search_post_unlink_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_POST_UNLINK_PAUSE, pause);
}

pub(super) async fn search_post_unlink_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_POST_UNLINK_PAUSE, account_id)).await;
}

/// Park a missing-manifest creator after preparing its batch and before CAS.
pub fn set_search_rebuild_create_pause(pause: Option<Arc<Pause>>) {
    set_slot(&SEARCH_REBUILD_CREATE_PAUSE, pause);
}

pub async fn search_rebuild_create_pause_point(account_id: u32) {
    wait(take_for(&SEARCH_REBUILD_CREATE_PAUSE, account_id)).await;
}

/// Account-scoped attempted work, including failed operations. The fault is
/// one-shot, and RAII clears it even when a regression assertion fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchPassWork {
    Manifest,
    Open,
    Upload,
    Commit,
    Mailboxes,
}

struct PassFault {
    account_id: u32,
    work: SearchPassWork,
    remaining: usize,
    consumed: bool,
}
static SEARCH_PASS_FAULT: Mutex<Option<PassFault>> = Mutex::new(None);
static SEARCH_PASS_COUNTS: std::sync::LazyLock<Mutex<HashMap<u32, [u64; 5]>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct SearchPassFaultOnce(());
impl SearchPassFaultOnce {
    pub fn new(account_id: u32, work: SearchPassWork, occurrence: usize) -> Self {
        let mut slot = SEARCH_PASS_FAULT.lock().unwrap();
        assert!(slot.is_none() && occurrence > 0);
        *slot = Some(PassFault {
            account_id,
            work,
            remaining: occurrence,
            consumed: false,
        });
        Self(())
    }
    pub fn was_consumed(&self) -> bool {
        SEARCH_PASS_FAULT
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|fault| fault.consumed)
    }
}
impl Drop for SearchPassFaultOnce {
    fn drop(&mut self) {
        *SEARCH_PASS_FAULT.lock().unwrap() = None;
    }
}
pub fn take_search_pass_fault(account_id: u32, work: SearchPassWork) -> bool {
    let mut slot = SEARCH_PASS_FAULT.lock().unwrap();
    if let Some(fault) = slot.as_mut()
        && fault.account_id == account_id
        && fault.work == work
        && !fault.consumed
    {
        fault.remaining -= 1;
        if fault.remaining == 0 {
            fault.consumed = true;
            return true;
        }
    }
    false
}
pub fn count_search_pass_work(account_id: u32, work: SearchPassWork) {
    if let Some(counts) = SEARCH_PASS_COUNTS.lock().unwrap().get_mut(&account_id) {
        counts[work as usize] += 1;
    }
}
pub struct SearchPassCounter(u32);
impl SearchPassCounter {
    pub fn start(account_id: u32) -> Self {
        assert!(
            SEARCH_PASS_COUNTS
                .lock()
                .unwrap()
                .insert(account_id, [0; 5])
                .is_none()
        );
        Self(account_id)
    }
    pub fn counts(&self) -> [u64; 5] {
        SEARCH_PASS_COUNTS.lock().unwrap()[&self.0]
    }
}
impl Drop for SearchPassCounter {
    fn drop(&mut self) {
        SEARCH_PASS_COUNTS.lock().unwrap().remove(&self.0);
    }
}

struct BlobTimeout {
    account_id: u32,
    hash: BlobHash,
    consumed: bool,
}

/// Replace one matching index-blob fetch result with an S3 request timeout.
#[must_use = "dropping the guard disarms the timeout"]
pub struct SearchBlobTimeoutOnce(());

impl SearchBlobTimeoutOnce {
    pub fn new(account_id: u32, hash: BlobHash) -> Self {
        let mut slot = SEARCH_BLOB_TIMEOUT_ONCE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert!(slot.is_none(), "an index-blob timeout is already armed");
        *slot = Some(BlobTimeout {
            account_id,
            hash,
            consumed: false,
        });
        Self(())
    }

    pub fn was_consumed(&self) -> bool {
        SEARCH_BLOB_TIMEOUT_ONCE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|timeout| timeout.consumed)
    }
}

impl Drop for SearchBlobTimeoutOnce {
    fn drop(&mut self) {
        *SEARCH_BLOB_TIMEOUT_ONCE
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }
}

pub(super) fn take_search_blob_timeout(account_id: u32, hash: &BlobHash) -> bool {
    let mut slot = SEARCH_BLOB_TIMEOUT_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(timeout) = slot.as_mut()
        && timeout.account_id == account_id
        && &timeout.hash == hash
        && !timeout.consumed
    {
        timeout.consumed = true;
        return true;
    }
    false
}

pub fn set_search_hold_expire(account_id: Option<u32>) {
    *SEARCH_HOLD_EXPIRE.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn search_hold_expire(account_id: u32) -> bool {
    *SEARCH_HOLD_EXPIRE.lock().unwrap_or_else(|e| e.into_inner()) == Some(account_id)
}

pub fn set_search_cache_hold(account_id: Option<u32>) {
    *SEARCH_CACHE_HOLD.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn search_cache_hold(account_id: u32) -> bool {
    *SEARCH_CACHE_HOLD.lock().unwrap_or_else(|e| e.into_inner()) == Some(account_id)
}

pub fn set_search_swap_skip(account_id: Option<u32>) {
    *SEARCH_SWAP_SKIP.lock().unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn search_swap_skip(account_id: u32) -> bool {
    *SEARCH_SWAP_SKIP.lock().unwrap_or_else(|e| e.into_inner()) == Some(account_id)
}

pub fn set_search_open_fail_all(account_id: Option<u32>) {
    *SEARCH_OPEN_FAIL_ALL
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn search_open_fail_all(account_id: u32) -> bool {
    *SEARCH_OPEN_FAIL_ALL
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        == Some(account_id)
}

pub fn set_search_blob_fail_once(account_id: Option<u32>) {
    *SEARCH_BLOB_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn take_search_blob_fail(account_id: u32) -> bool {
    take_flag(&SEARCH_BLOB_FAIL_ONCE, account_id)
}

pub fn set_search_open_fail_once(account_id: Option<u32>) {
    *SEARCH_OPEN_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn take_search_open_fail(account_id: u32) -> bool {
    take_flag(&SEARCH_OPEN_FAIL_ONCE, account_id)
}

pub fn set_search_manifest_fail_once(account_id: Option<u32>) {
    *SEARCH_MANIFEST_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn take_search_manifest_fail(account_id: u32) -> bool {
    take_flag(&SEARCH_MANIFEST_FAIL_ONCE, account_id)
}

pub fn set_search_destroy_fail_once(account_id: Option<u32>) {
    *SEARCH_DESTROY_FAIL_ONCE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = account_id;
}

pub fn take_search_destroy_fail(account_id: u32) -> bool {
    take_flag(&SEARCH_DESTROY_FAIL_ONCE, account_id)
}

pub fn set_search_blob_slow(delay: Option<(u32, u64)>) {
    *SEARCH_BLOB_SLOW.lock().unwrap_or_else(|e| e.into_inner()) = delay;
}

pub fn search_blob_slow(account_id: u32) -> Option<Duration> {
    SEARCH_BLOB_SLOW
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .filter(|(id, _)| *id == account_id)
        .map(|(_, millis)| Duration::from_millis(millis))
}

pub fn set_search_config_override(config: Option<ZaSearchConfig>) {
    *SEARCH_CONFIG_OVERRIDE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = config;
}

pub(super) fn search_config_override() -> Option<ZaSearchConfig> {
    SEARCH_CONFIG_OVERRIDE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn count_za_search_blob_read(account_id: u32) {
    ZA_SEARCH_COUNTERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(account_id)
        .or_default()
        .0 += 1;
}

pub fn count_za_search_message_open(account_id: u32) {
    ZA_SEARCH_COUNTERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(account_id)
        .or_default()
        .1 += 1;
}

pub fn note_za_search_item_aliases(account_id: u32, n: usize) {
    let mut counters = ZA_SEARCH_COUNTERS.lock().unwrap_or_else(|e| e.into_inner());
    let max_aliases = &mut counters.entry(account_id).or_default().2;
    *max_aliases = (*max_aliases).max(n);
}

pub fn za_search_blob_reads(account_id: u32) -> u64 {
    ZA_SEARCH_COUNTERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&account_id)
        .map_or(0, |counters| counters.0)
}

pub fn za_search_message_opens(account_id: u32) -> u64 {
    ZA_SEARCH_COUNTERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&account_id)
        .map_or(0, |counters| counters.1)
}

pub fn za_search_max_item_aliases(account_id: u32) -> usize {
    ZA_SEARCH_COUNTERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&account_id)
        .map_or(0, |counters| counters.2)
}

pub fn reset_za_search_counters(account_id: u32) {
    ZA_SEARCH_COUNTERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&account_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_blob_failure_is_taken_once_by_its_account() {
        set_search_blob_fail_once(Some(5));
        assert!(!take_search_blob_fail(6));
        assert!(take_search_blob_fail(5));
        assert!(!take_search_blob_fail(5));
    }
}

pub static SEARCH_COMPACTION_FORCE_ALLOWANCE: Mutex<Option<u32>> = Mutex::new(None);

pub fn search_compaction_force_allowance(account_id: u32) -> bool {
    *SEARCH_COMPACTION_FORCE_ALLOWANCE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        == Some(account_id)
}

/// Opt-in numeric accounting for the search compaction benchmark and fixtures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchContentWrite {
    Original(u64),
    StagedMemory(u64),
    RetirementMemory(u64),
    Encoded {
        tier: u8,
        bytes: u64,
    },
    Uploaded {
        tier: u8,
        bytes: u64,
    },
    Linked {
        tier: u8,
        bytes: u64,
    },
    Committed {
        chain_length: u32,
        full_merge: bool,
        thread_compaction: bool,
    },
}

#[derive(Default)]
struct SearchWriteObservers {
    events: Option<(u32, Vec<SearchContentWrite>)>,
    commits: Option<(u32, Arc<AtomicU64>)>,
}

static SEARCH_CONTENT_WRITES: Mutex<SearchWriteObservers> = Mutex::new(SearchWriteObservers {
    events: None,
    commits: None,
});

pub fn set_search_content_writes(account_id: Option<u32>) {
    SEARCH_CONTENT_WRITES.lock().unwrap().events = account_id.map(|id| (id, Vec::new()));
}

pub fn take_search_content_writes(account_id: u32) -> Vec<SearchContentWrite> {
    SEARCH_CONTENT_WRITES
        .lock()
        .unwrap()
        .events
        .as_mut()
        .filter(|(id, _)| *id == account_id)
        .map_or_else(Vec::new, |(_, events)| std::mem::take(events))
}

pub fn note_search_content_write(account_id: u32, event: SearchContentWrite) {
    let mut observers = SEARCH_CONTENT_WRITES.lock().unwrap();
    if let Some((id, commits)) = &observers.commits
        && *id == account_id
        && matches!(event, SearchContentWrite::Committed { .. })
    {
        commits.fetch_add(1, Ordering::Relaxed);
    }
    if let Some((_, events)) = observers
        .events
        .as_mut()
        .filter(|(id, _)| *id == account_id)
    {
        events.push(event);
    }
}

/// Counts all writer commits for this account, including commits outside the
/// measured request tasks. It retains no events and shares the existing hook.
pub struct SearchCommitCounter(Arc<AtomicU64>);

impl SearchCommitCounter {
    pub fn start(account_id: u32) -> Self {
        let mut observers = SEARCH_CONTENT_WRITES.lock().unwrap();
        if observers.commits.is_some() {
            drop(observers);
            panic!("a search commit counter is active");
        }
        let count = Arc::new(AtomicU64::new(0));
        observers.commits = Some((account_id, Arc::clone(&count)));
        Self(count)
    }

    pub fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

impl Drop for SearchCommitCounter {
    fn drop(&mut self) {
        SEARCH_CONTENT_WRITES.lock().unwrap().commits = None;
    }
}

/// Request-local inclusive timings. A parent stage may include other reported
/// stages, and cancellation drops the span at the point the wait is stopped.
#[derive(Clone, Copy, Debug)]
pub enum SearchStage {
    DispatcherPrePass,
    Method,
    MapFlightWait,
    MapIndexWait,
    ResidentIndexWait,
    ContentFlightWait,
    ContentIndexWait,
    PassLockWait,
    VerifiedBlobFetch,
    ThreadOpenDecode,
    ThreadModelBuild,
    ContentOpen,
    ContentDecode,
    Postings,
    TombstonePreparation,
    ExactGenerationInstall,
    EnsureRound,
    ContentRetry,
    ViewRetry,
    ContentAdmission,
}

impl SearchStage {
    const ALL: [Self; Self::ContentAdmission as usize + 1] = [
        Self::DispatcherPrePass,
        Self::Method,
        Self::MapFlightWait,
        Self::MapIndexWait,
        Self::ResidentIndexWait,
        Self::ContentFlightWait,
        Self::ContentIndexWait,
        Self::PassLockWait,
        Self::VerifiedBlobFetch,
        Self::ThreadOpenDecode,
        Self::ThreadModelBuild,
        Self::ContentOpen,
        Self::ContentDecode,
        Self::Postings,
        Self::TombstonePreparation,
        Self::ExactGenerationInstall,
        Self::EnsureRound,
        Self::ContentRetry,
        Self::ViewRetry,
        Self::ContentAdmission,
    ];
}

#[derive(Default)]
struct StageTotal {
    count: AtomicU64,
    nanos: AtomicU64,
}

#[derive(Debug)]
pub struct SearchAdmission {
    pub reason: &'static str,
    pub remaining: Duration,
}

pub struct SearchRequestMeasurement {
    account_id: u32,
    call_id: String,
    claimed: AtomicBool,
    totals: [StageTotal; SearchStage::ALL.len()],
    admission: OnceLock<SearchAdmission>,
}

static SEARCH_REQUEST_SERIAL: AtomicU64 = AtomicU64::new(0);
static PENDING_SEARCH_MEASUREMENT: Mutex<Option<Arc<SearchRequestMeasurement>>> = Mutex::new(None);

tokio::task_local! {
    static SEARCH_MEASUREMENT: Arc<SearchRequestMeasurement>;
}

/// Arm a single method call. The dispatcher can claim it only once and
/// checks both account and call ID. Dropping an unclaimed observation clears it.
pub struct SearchRequestObservation {
    measurement: Arc<SearchRequestMeasurement>,
}

impl SearchRequestObservation {
    pub fn arm(account_id: u32) -> Self {
        let serial = SEARCH_REQUEST_SERIAL.fetch_add(1, Ordering::Relaxed);
        let measurement = Arc::new(SearchRequestMeasurement {
            account_id,
            call_id: format!("search-cold-{serial}"),
            claimed: AtomicBool::new(false),
            totals: Default::default(),
            admission: OnceLock::new(),
        });
        let mut pending = PENDING_SEARCH_MEASUREMENT.lock().unwrap();
        if pending.is_some() {
            drop(pending);
            panic!("a search observation is already armed");
        }
        *pending = Some(Arc::clone(&measurement));
        Self { measurement }
    }

    pub fn call_id(&self) -> &str {
        &self.measurement.call_id
    }

    pub fn claimed(&self) -> bool {
        self.measurement.claimed.load(Ordering::Relaxed)
    }

    pub fn count(&self, stage: SearchStage) -> u64 {
        self.measurement.totals[stage as usize]
            .count
            .load(Ordering::Relaxed)
    }

    pub fn timings(&self) -> impl Iterator<Item = (SearchStage, u64, Duration)> + '_ {
        SearchStage::ALL.into_iter().map(|stage| {
            let total = &self.measurement.totals[stage as usize];
            (
                stage,
                self.count(stage),
                Duration::from_nanos(total.nanos.load(Ordering::Relaxed)),
            )
        })
    }

    pub fn admission(&self) -> Option<&SearchAdmission> {
        self.measurement.admission.get()
    }
    /// Clear the pending claim after the wire attempt, even when the response
    /// never reached the dispatcher. Keep already observed timings readable.
    pub fn disarm(&self) {
        let mut pending = PENDING_SEARCH_MEASUREMENT.lock().unwrap();
        if pending
            .as_ref()
            .is_some_and(|measurement| Arc::ptr_eq(measurement, &self.measurement))
        {
            *pending = None;
        }
    }
}

impl Drop for SearchRequestObservation {
    fn drop(&mut self) {
        self.disarm();
    }
}

pub fn claim_search_observation(
    account_id: u32,
    call_id: &str,
) -> Option<Arc<SearchRequestMeasurement>> {
    if !call_id.starts_with("search-cold-") {
        return None;
    }
    let mut pending = PENDING_SEARCH_MEASUREMENT.try_lock().ok()?;
    if pending.as_ref().is_some_and(|measurement| {
        measurement.account_id == account_id && measurement.call_id == call_id
    }) {
        let measurement = pending.take().unwrap();
        measurement.claimed.store(true, Ordering::Relaxed);
        Some(measurement)
    } else {
        None
    }
}

pub async fn with_search_observation<F: std::future::Future>(
    observation: Option<&Arc<SearchRequestMeasurement>>,
    future: F,
) -> F::Output {
    match observation {
        Some(observation) => {
            SEARCH_MEASUREMENT
                .scope(Arc::clone(observation), future)
                .await
        }
        None => future.await,
    }
}

fn search_measurement(account_id: u32) -> Option<Arc<SearchRequestMeasurement>> {
    SEARCH_MEASUREMENT
        .try_with(|measurement| {
            (measurement.account_id == account_id).then(|| Arc::clone(measurement))
        })
        .ok()
        .flatten()
}

pub struct SearchSpan {
    measurement: Arc<SearchRequestMeasurement>,
    stage: SearchStage,
    started: Instant,
}

impl Drop for SearchSpan {
    fn drop(&mut self) {
        let nanos = self.started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.measurement.totals[self.stage as usize]
            .nanos
            .fetch_add(nanos, Ordering::Relaxed);
    }
}

pub fn search_span(account_id: u32, stage: SearchStage) -> Option<SearchSpan> {
    let measurement = search_measurement(account_id)?;
    measurement.totals[stage as usize]
        .count
        .fetch_add(1, Ordering::Relaxed);
    Some(SearchSpan {
        measurement,
        stage,
        started: Instant::now(),
    })
}

pub async fn observe_search_wait<F: std::future::Future>(
    account_id: u32,
    stage: SearchStage,
    future: F,
) -> F::Output {
    let _span = search_span(account_id, stage);
    future.await
}

pub fn note_search_count(account_id: u32, stage: SearchStage) {
    if let Some(measurement) = search_measurement(account_id) {
        measurement.totals[stage as usize]
            .count
            .fetch_add(1, Ordering::Relaxed);
    }
}

pub fn note_search_admission(
    account_id: u32,
    reason: impl FnOnce() -> &'static str,
    deadline: Instant,
) {
    if let Some(measurement) = search_measurement(account_id) {
        let _ = measurement.admission.set(SearchAdmission {
            reason: reason(),
            remaining: deadline.saturating_duration_since(Instant::now()),
        });
    }
}
