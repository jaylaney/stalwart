/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Zero-access fork: the test-mode hook of the store crate, compiled only
//! under `test_mode`, with the `Pause` shape of `common::za::test_hooks`
//! (which re-exports both). None changes behaviour while unset. A pause is
//! one-shot and keyed by account id: only the first request for that
//! account reaching the point takes it; requests for other accounts pass
//! and leave it set.
//!
//! - `INDEX_CHUNK_PAUSE`: in `Store::index` after a multi-chunk document's
//!   first commit chunk and before its second, keyed by the document's
//!   account (`store::search::index`, mail ruling R62).

use std::sync::{Arc, Mutex};
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

pub static INDEX_CHUNK_PAUSE: Mutex<Option<Arc<Pause>>> = Mutex::new(None);

pub fn set_index_chunk_pause(pause: Option<Arc<Pause>>) {
    *INDEX_CHUNK_PAUSE.lock().unwrap_or_else(|e| e.into_inner()) = pause;
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

pub async fn index_chunk_pause_point(account_id: u32) {
    wait(take_for(&INDEX_CHUNK_PAUSE, account_id)).await;
}
