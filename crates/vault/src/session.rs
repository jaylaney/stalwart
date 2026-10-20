/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::keys::{KeyError, Secret, derive_event_key, derive_mail_key};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// Keys resident for one request (spec 5, "in-flight copies"). Cloned by
/// `Arc`; the secrets are zeroed when the last reference drops.
pub struct SessionKeys {
    pub account_id: u32,
    pub generation: u64,
    mk: Secret,
    ewk: Secret,
    /// Mail spec 3.1: the mail wrapping key, derived from MK next to the EWK.
    mwk: Secret,
    /// Invitations spec 3.2: the X25519 private key, unwrapped from the
    /// vault record on first use. Holds the outcome, so a wrap that does not
    /// open is traced once per session.
    private_key: OnceLock<Result<Secret, KeyError>>,
    /// Invitations spec 6.2-6.3: what this session remembers about its
    /// drain. In memory only, dropped with the key-cache entry.
    drain: ZaDrainState,
}

/// The drain's per-session state (invitations spec 6.2, "The cursor", and
/// 6.3, "The pass"): the inbox change id the last complete pass observed
/// before enumerating, the time before which a pass that left an item
/// pending is not retried, and the lock that keeps to one pass per session
/// on a node. Never a value produced by the drain's own writes: the cursor
/// is the snapshot the pass started from.
pub struct ZaDrainState {
    /// `NO_CURSOR` until a pass completes.
    cursor: AtomicU64,
    /// Unix seconds; `0` when no deferral is in force.
    retry_after: AtomicU64,
    pass: tokio::sync::Mutex<PassState>,
}

/// Per-session bookkeeping of items that failed (invitations spec 6.2,
/// "Skipped items"), held under the pass lock. A UID kept here is never
/// traced.
#[derive(Default)]
pub struct PassState {
    /// Drain failures of each item (by document id) in this session.
    pub failures: HashMap<u32, u8>,
    /// The first UID a failure of each item named, kept across failures.
    pub uids: HashMap<u32, String>,
    /// Items skipped after `DRAIN_ITEM_FAILURE_LIMIT` failures, with the
    /// UID each named, if known: later items of that UID wait.
    pub skipped: HashMap<u32, Option<String>>,
    /// Items left pending behind a skipped item of their UID in this
    /// session: not re-opened until a fresh session retries them.
    pub waiting: HashSet<u32>,
}

impl PassState {
    /// Records a failure; `true` when it reached the limit and the item is
    /// now skipped, with the first UID any of its failures named.
    pub fn record_failure(&mut self, document_id: u32, uid: Option<String>, limit: u8) -> bool {
        if let Some(uid) = uid {
            self.uids.entry(document_id).or_insert(uid);
        }
        let count = self.failures.entry(document_id).or_insert(0);
        *count = count.saturating_add(1);
        if *count >= limit {
            self.skipped
                .insert(document_id, self.uids.get(&document_id).cloned());
            true
        } else {
            false
        }
    }

    pub fn is_skipped(&self, document_id: u32) -> bool {
        self.skipped.contains_key(&document_id)
    }

    pub fn is_waiting(&self, document_id: u32) -> bool {
        self.waiting.contains(&document_id)
    }

    /// Whether a skipped item named `uid`.
    pub fn blocks_uid(&self, uid: &str) -> bool {
        self.skipped
            .values()
            .flatten()
            .any(|skipped| skipped == uid)
    }
}

const NO_CURSOR: u64 = u64::MAX;

impl ZaDrainState {
    fn new() -> Self {
        ZaDrainState {
            cursor: AtomicU64::new(NO_CURSOR),
            retry_after: AtomicU64::new(0),
            pass: tokio::sync::Mutex::new(PassState::default()),
        }
    }

    /// The inbox change id the last complete pass started from; `None`
    /// until one completed (the session's first request drains).
    pub fn cursor(&self) -> Option<u64> {
        Some(self.cursor.load(Ordering::SeqCst)).filter(|cursor| *cursor != NO_CURSOR)
    }

    /// A pass found every pending item done: remember its snapshot and lift
    /// any deferral.
    pub fn complete(&self, snapshot: u64) {
        self.cursor
            .store(snapshot.min(NO_CURSOR - 1), Ordering::SeqCst);
        self.retry_after.store(0, Ordering::SeqCst);
    }

    pub fn retry_after(&self) -> u64 {
        self.retry_after.load(Ordering::SeqCst)
    }

    /// A pass left an item pending (spec 6.2): no pass before `secs`.
    pub fn defer_until(&self, secs: u64) {
        self.retry_after.store(secs, Ordering::SeqCst);
    }

    /// One pass per session on a node (spec 6.3): a second request of the
    /// session waits for the running pass and then finds its cursor.
    pub async fn lock(&self) -> tokio::sync::MutexGuard<'_, PassState> {
        self.pass.lock().await
    }
}

impl SessionKeys {
    pub fn new(account_id: u32, generation: u64, mk: Secret) -> Self {
        let ewk = derive_event_key(&mk);
        let mwk = derive_mail_key(&mk);
        #[cfg(feature = "test_mode")]
        live::created(account_id);
        SessionKeys {
            account_id,
            generation,
            mk,
            ewk,
            mwk,
            private_key: OnceLock::new(),
            drain: ZaDrainState::new(),
        }
    }

    pub fn mk(&self) -> &Secret {
        &self.mk
    }

    pub fn ewk(&self) -> &Secret {
        &self.ewk
    }

    pub fn mwk(&self) -> &Secret {
        &self.mwk
    }

    /// `None` until the first unwrap was attempted.
    pub fn private_key(&self) -> Option<Result<&Secret, KeyError>> {
        self.private_key
            .get()
            .map(|outcome| outcome.as_ref().map_err(|err| *err))
    }

    /// Records the unwrap outcome; `false` if one was already recorded (the
    /// first stands).
    pub fn set_private_key(&self, outcome: Result<Secret, KeyError>) -> bool {
        self.private_key.set(outcome).is_ok()
    }

    pub fn drain(&self) -> &ZaDrainState {
        &self.drain
    }
}

/// Counts the `SessionKeys` alive in this process, so a test can show that a
/// session's keys were dropped. The secrets zero themselves through
/// `Zeroizing`; this `Drop` only counts.
#[cfg(feature = "test_mode")]
impl Drop for SessionKeys {
    fn drop(&mut self) {
        live::dropped(self.account_id);
    }
}

#[cfg(feature = "test_mode")]
pub mod live {
    use std::{
        collections::HashMap,
        sync::{LazyLock, Mutex, MutexGuard, PoisonError},
    };

    static LIVE: LazyLock<Mutex<HashMap<u32, usize>>> = LazyLock::new(Default::default);

    fn live() -> MutexGuard<'static, HashMap<u32, usize>> {
        LIVE.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The number of `SessionKeys` of `account_id` alive in this process.
    pub fn session_keys(account_id: u32) -> usize {
        live().get(&account_id).copied().unwrap_or(0)
    }

    pub(super) fn created(account_id: u32) {
        *live().entry(account_id).or_insert(0) += 1;
    }

    pub(super) fn dropped(account_id: u32) {
        let mut live = live();
        if let Some(count) = live.get_mut(&account_id) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                live.remove(&account_id);
            }
        }
    }
}

impl std::fmt::Debug for SessionKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SessionKeys(account {}, generation {})",
            self.account_id, self.generation
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{KeyError, Secret, derive_event_key, derive_mail_key};

    #[test]
    fn session_keys_derive_ewk_and_print_nothing() {
        let mk = Secret::from_bytes([9; 32]);
        let keys = SessionKeys::new(3, 11, mk.clone());
        assert_eq!(keys.account_id, 3);
        assert_eq!(keys.generation, 11);
        assert_eq!(keys.mk().as_bytes(), mk.as_bytes());
        assert_eq!(keys.ewk().as_bytes(), derive_event_key(&mk).as_bytes());
        assert_eq!(format!("{keys:?}"), "SessionKeys(account 3, generation 11)");
    }

    #[test]
    fn private_key_is_recorded_once_and_never_printed() {
        let keys = SessionKeys::new(3, 11, Secret::from_bytes([9; 32]));
        assert!(keys.private_key().is_none(), "nothing before first use");
        assert!(keys.set_private_key(Ok(Secret::from_bytes([0xAB; 32]))));
        assert!(
            !keys.set_private_key(Err(KeyError::Aead)),
            "the first outcome stands"
        );
        assert_eq!(keys.private_key().unwrap().unwrap().as_bytes(), &[0xAB; 32]);
        assert_eq!(format!("{keys:?}"), "SessionKeys(account 3, generation 11)");
        let failed = SessionKeys::new(3, 11, Secret::random());
        assert!(failed.set_private_key(Err(KeyError::Aead)));
        assert_eq!(
            failed.private_key().map(|outcome| outcome.err()),
            Some(Some(KeyError::Aead))
        );
    }

    #[tokio::test]
    async fn drain_state_starts_empty_and_remembers_a_completed_pass() {
        let keys = SessionKeys::new(3, 11, Secret::from_bytes([9; 32]));
        let drain = keys.drain();
        assert_eq!(drain.cursor(), None, "no pass completed yet");
        assert_eq!(drain.retry_after(), 0);
        drain.defer_until(1_700_000_000);
        assert_eq!(drain.retry_after(), 1_700_000_000);
        drain.complete(42);
        assert_eq!(drain.cursor(), Some(42));
        assert_eq!(drain.retry_after(), 0, "a complete pass lifts the deferral");
        drain.complete(0);
        assert_eq!(
            drain.cursor(),
            Some(0),
            "an empty inbox's change id is a cursor too"
        );
        drain.complete(u64::MAX);
        assert_eq!(
            drain.cursor(),
            Some(u64::MAX - 1),
            "the sentinel is never stored"
        );
        // One pass at a time.
        let held = drain.lock().await;
        assert!(drain.pass.try_lock().is_err(), "a second pass waits");
        drop(held);
        assert!(drain.pass.try_lock().is_ok());
        assert_eq!(format!("{keys:?}"), "SessionKeys(account 3, generation 11)");
    }

    #[test]
    fn session_keys_carry_the_mail_key() {
        let mk = Secret::random();
        let keys = SessionKeys::new(7, 3, mk.clone());
        assert_eq!(keys.mwk().as_bytes(), derive_mail_key(&mk).as_bytes());
        assert_eq!(format!("{keys:?}"), "SessionKeys(account 7, generation 3)");
    }

    #[cfg(feature = "test_mode")]
    #[test]
    fn the_live_counter_follows_construction_and_drop() {
        let before = live::session_keys(99);
        let keys = std::sync::Arc::new(SessionKeys::new(99, 1, Secret::random()));
        let clone = keys.clone();
        assert_eq!(live::session_keys(99), before + 1);
        drop(keys);
        assert_eq!(
            live::session_keys(99),
            before + 1,
            "a clone keeps the keys alive"
        );
        drop(clone);
        assert_eq!(live::session_keys(99), before);
    }
}
