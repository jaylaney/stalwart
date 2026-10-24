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
//! - `TASK_CONSUMER_PAUSE`: the task manager's consumer loop takes no task
//!   while it is held; global (`services::task_manager::manager`).
//! - The lease-duration override: stands in for `ZA_UID_LEASE_SECS`, which
//!   is read once per process.
//! - The redelivery batch override: stands in for `REDELIVER_BATCH`, the
//!   entries one `contacts/pin` call attempts (`http::api::contacts`,
//!   through `common::za::redeliver_batch_override`).
//! - `DRAIN_LOOKUP_PAUSE`: in the drain's import, after the first UID
//!   lookup found nothing and before the creation lease, keyed by the
//!   draining account (`groupware::calendar::drain`).
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
//! - `INDEX_WRITE_PAUSE`: in the index worker after its documents are built
//!   and immediately before the search store's write, keyed by the account
//!   of an `IndexDocument` task of the batch
//!   (`services::task_manager::index`, mail ruling R54).
//! - `TRAIN_PAUSE`: in `add_account_spam_sample` after the message's
//!   metadata read and before the sample is added to the batch, keyed by
//!   the sample's account (`email::message::ingest`, mail ruling R55).
//! - `INDEX_CHUNK_PAUSE`: in `Store::index` after a multi-chunk document's
//!   first commit chunk and before its second, keyed by the document's
//!   account (`store::search::index`, mail ruling R62); defined in
//!   `store::za_test` and re-exported here.
//! - `INDEX_GUARD_FAIL_ONCE`: the account's next later commit chunk of a
//!   guarded search document is not written and fails with
//!   `AssertValueFailed`, as FoundationDB reports a read error at the
//!   guard's assertion (`store::search::index`, mail ruling R63); defined in
//!   `store::za_test` and re-exported here.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering},
};
pub use store::za_test::{
    INDEX_GUARD_FAIL_ONCE, Pause, set_index_chunk_pause, set_index_guard_fail_once,
};

pub static FAN_OUT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static PUT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static COMMIT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static UPDATE_COMMIT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);
pub static DELETE_COMMIT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// Global: its `account_id` is ignored.
pub static TASK_CONSUMER_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// The account whose next key-account PUT create fails its commit.
pub static COMMIT_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

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
