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
//! - `COMMIT_FAIL_ONCE`: the commit of a key account's PUT create fails once
//!   with an internal error after its fan-out, keyed by the account
//!   (`dav::calendar::za_commit_create`).
//! - `TASK_CONSUMER_PAUSE`: the task manager's consumer loop takes no task
//!   while it is held; global (`services::task_manager::manager`).
//! - The lease-duration override: stands in for `ZA_UID_LEASE_SECS`, which
//!   is read once per process.
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
//! - The drain skip: while an account is listed (or `DRAIN_SKIP_ALL` is),
//!   its drain does not run; the za suite lists every account after setup
//!   and the drain modules clear it for their duration.
//! - The drain retry override: stands in for `DRAIN_RETRY_SECS`.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, AtomicU64, Ordering},
};
use tokio::sync::Notify;

pub struct Pause {
    pub account_id: u32,
    pub arrived: Notify,
    pub release: Notify,
}

impl Pause {
    pub fn new(account_id: u32) -> Self {
        Pause {
            account_id,
            arrived: Notify::new(),
            release: Notify::new(),
        }
    }
}

pub static FAN_OUT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static PUT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub static COMMIT_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

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

/// Accounts whose drain does not run; `DRAIN_SKIP_ALL` stands for every
/// account.
static DRAIN_SKIP: Mutex<Vec<u32>> = Mutex::new(Vec::new());

pub const DRAIN_SKIP_ALL: u32 = u32::MAX;

/// Seconds before a pass that left an item pending is retried; `-1` unset.
static DRAIN_RETRY_OVERRIDE: AtomicI64 = AtomicI64::new(-1);

/// Seconds a new creation lease lasts; 0 when unset.
static LEASE_SECS_OVERRIDE: AtomicU64 = AtomicU64::new(0);

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
