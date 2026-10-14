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
//! - `COMMIT_FAIL_ONCE`: the commit of a key account's PUT create fails once
//!   with an internal error after its fan-out, keyed by the account
//!   (`dav::calendar::za_commit_create`).
//! - `TASK_CONSUMER_PAUSE`: the task manager's consumer loop takes no task
//!   while it is held; global (`services::task_manager::manager`).
//! - The lease-duration override: stands in for `ZA_UID_LEASE_SECS`, which
//!   is read once per process.
//!
//! Plan 10 adds the drain pause and the drain skip here.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
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

/// Global: its `account_id` is ignored.
pub static TASK_CONSUMER_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

/// The account whose next key-account PUT create fails its commit.
pub static COMMIT_FAIL_ONCE: Mutex<Option<u32>> = Mutex::new(None);

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

fn take_for(slot: &Mutex<Option<Arc<Pause>>>, account_id: u32) -> Option<Arc<Pause>> {
    let mut pause = slot.lock().unwrap_or_else(|e| e.into_inner());
    if pause.as_ref().is_some_and(|p| p.account_id == account_id) {
        pause.take()
    } else {
        None
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

pub async fn task_consumer_pause_point() {
    let pause = TASK_CONSUMER_PAUSE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    wait(pause).await;
}

/// Whether the commit of `account_id` must fail now; clears the flag.
pub fn take_commit_fail(account_id: u32) -> bool {
    let mut slot = COMMIT_FAIL_ONCE.lock().unwrap_or_else(|e| e.into_inner());
    if *slot == Some(account_id) {
        *slot = None;
        true
    } else {
        false
    }
}
