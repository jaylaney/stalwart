/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Invitations spec 6.3: the per-(account, UID) creation lease with fenced
//! commits. A key account's session that creates an event for a UID (a PUT
//! create here; the drain's import in plan 10) holds the lease from its
//! absence check through the insert, and the insert batch asserts that the
//! lease still holds this holder's exact value. An expired holder whose
//! lease was taken over therefore cannot commit; the duration only decides
//! how long a crashed holder blocks others.
//!
//! The key lives in the data store as an in-memory-class key, written
//! through `Server::store()` the way `try_lock` writes one when the
//! in-memory store is the data store, and never through the in-memory store
//! dispatch: the fenced commit needs the lease and the event in one
//! transaction. The generic `purge_in_memory_store` skips the
//! `KV_LOCK_ZA_UID` range, since its unconditional clear would delete a
//! replacement lease; plan 10's sweep (not yet present) is to clear
//! expired leases with an assertion on the value it read.

use crate::{KV_LOCK_ZA_UID, auth::vault::RawValue};
use sha2::{Digest, Sha256};
use std::{sync::LazyLock, time::Duration};
use store::{
    Store, U64_LEN, ValueKey, rand,
    write::{BatchBuilder, InMemoryClass, ValueClass, assert::AssertValue, now},
};
use trc::AddContext;
use xxhash_rust::xxh3::xxh3_64;

/// Lease duration when `ZA_UID_LEASE_SECS` is unset (spec 6.3).
pub const DEFAULT_LEASE_SECS: u64 = 30;
/// Upper bound of `ZA_UID_LEASE_SECS`; a larger value is clamped to it.
pub const MAX_LEASE_SECS: u64 = 3600;
/// Further attempts at a held lease, `LEASE_RETRY_WAIT` apart.
pub const LEASE_RETRIES: u32 = 20;
pub const LEASE_RETRY_WAIT: Duration = Duration::from_millis(50);
/// Length of the random fence token in a lease value.
pub const FENCE_LEN: usize = 16;
/// Prefix, account id (4 bytes), SHA-256 of the UID (32 bytes).
pub const LEASE_KEY_LEN: usize = 1 + 4 + 32;

static LEASE_SECS: LazyLock<u64> =
    LazyLock::new(|| lease_secs_from(std::env::var("ZA_UID_LEASE_SECS").ok().as_deref()));

/// `ZA_UID_LEASE_SECS`, in seconds, within `1..=MAX_LEASE_SECS`: unset,
/// malformed or zero uses the default; a value above `MAX_LEASE_SECS` is
/// clamped to it.
fn lease_secs_from(value: Option<&str>) -> u64 {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map_or(DEFAULT_LEASE_SECS, |secs| secs.min(MAX_LEASE_SECS))
}

/// How long a new lease lasts: `ZA_UID_LEASE_SECS`, read once per process,
/// or in test builds the override a test set.
pub fn lease_secs() -> u64 {
    #[cfg(feature = "test_mode")]
    if let Some(secs) = crate::za::test_hooks::lease_secs_override() {
        return secs;
    }
    *LEASE_SECS
}

/// `KV_LOCK_ZA_UID || account_id (u32 BE) || SHA-256(uid)`: fixed length
/// whatever the UID.
pub fn lease_key(account_id: u32, uid: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(LEASE_KEY_LEN);
    key.push(KV_LOCK_ZA_UID);
    key.extend_from_slice(&account_id.to_be_bytes());
    key.extend_from_slice(Sha256::digest(uid.as_bytes()).as_slice());
    key
}

/// The key range that holds every lease (plan 10's sweep iterates it).
pub fn lease_key_range() -> (ValueKey<ValueClass>, ValueKey<ValueClass>) {
    (
        ValueKey::from(lease_class(vec![KV_LOCK_ZA_UID])),
        ValueKey::from(lease_class(vec![KV_LOCK_ZA_UID + 1])),
    )
}

/// The leading expiry of a lease value; `None` for a value too short to
/// hold one.
pub fn lease_value_expiry(value: &[u8]) -> Option<u64> {
    value
        .get(..U64_LEN)
        .and_then(|bytes| <[u8; U64_LEN]>::try_from(bytes).ok())
        .map(u64::from_be_bytes)
}

fn lease_class(key: Vec<u8>) -> ValueClass {
    ValueClass::InMemory(InMemoryClass::Key(key))
}

/// A held lease: its key and this holder's exact value
/// (`expiry (u64 BE, seconds) || fence (16 random bytes)`).
pub struct ZaUidLease {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl ZaUidLease {
    /// Spec 6.3 step 1. Reads the key; while it holds an unexpired value,
    /// waits `LEASE_RETRY_WAIT` and tries again, `LEASE_RETRIES` times. Then
    /// writes `expiry || fence`, asserting the value read (absent, or the
    /// expired value's exact bytes); a write that loses its assertion to
    /// another holder counts as an attempt. `None` when every attempt found
    /// the lease held.
    pub async fn acquire(
        store: &Store,
        account_id: u32,
        uid: &str,
    ) -> trc::Result<Option<ZaUidLease>> {
        let key = lease_key(account_id, uid);
        for attempt in 0..=LEASE_RETRIES {
            if attempt > 0 {
                tokio::time::sleep(LEASE_RETRY_WAIT).await;
            }
            let current = store
                .get_value::<RawValue>(ValueKey::from(lease_class(key.clone())))
                .await
                .caused_by(trc::location!())?;
            let now = now();
            if current
                .as_ref()
                .and_then(|raw| lease_value_expiry(&raw.0))
                .is_some_and(|expiry| expiry > now)
            {
                continue;
            }
            let mut value = Vec::with_capacity(U64_LEN + FENCE_LEN);
            value.extend_from_slice(&now.saturating_add(lease_secs()).to_be_bytes());
            value.extend_from_slice(&rand::random::<[u8; FENCE_LEN]>());
            let mut batch = BatchBuilder::new();
            batch.assert_value(
                lease_class(key.clone()),
                match &current {
                    Some(raw) => AssertValue::Hash(xxh3_64(&raw.0)),
                    None => AssertValue::None,
                },
            );
            batch.set(lease_class(key.clone()), value.clone());
            match store.write(batch.build_all()).await {
                Ok(_) => return Ok(Some(ZaUidLease { key, value })),
                Err(err) if err.is_assertion_failure() => continue,
                Err(err) => return Err(err.caused_by(trc::location!())),
            }
        }
        Ok(None)
    }

    /// Spec 6.3 step 3: adds to `batch` the assertion that the key still
    /// holds this holder's exact value. Call it before the insert, so the
    /// assertion and the insert share the batch's first commit point.
    pub fn assert_in(&self, batch: &mut BatchBuilder) {
        batch.assert_value(
            lease_class(self.key.clone()),
            AssertValue::Hash(xxh3_64(&self.value)),
        );
    }

    /// Spec 6.3 step 4: clears the key if it still holds this holder's
    /// value, so a holder never removes a successor's lease. Returns whether
    /// the key is gone: cleared here, or already absent (plan 10's sweep
    /// removed the expired lease, which after a fenced commit is benign and
    /// not traced). A key holding another value, or a store
    /// error, is traced and returns `false`; the key then expires and plan
    /// 10's sweep removes it.
    pub async fn release(self, store: &Store) -> bool {
        let mut batch = BatchBuilder::new();
        batch.assert_value(
            lease_class(self.key.clone()),
            AssertValue::Hash(xxh3_64(&self.value)),
        );
        batch.clear(lease_class(self.key.clone()));
        let err = match store.write(batch.build_all()).await {
            Ok(_) => return true,
            Err(err) => err,
        };
        if err.is_assertion_failure()
            && matches!(
                store
                    .get_value::<RawValue>(ValueKey::from(lease_class(self.key)))
                    .await,
                Ok(None)
            )
        {
            return true;
        }
        trc::error!(
            err.details("zero-access: UID creation lease not released")
                .caused_by(trc::location!())
        );
        false
    }
}

/// A lease held across a request's early returns. `release` clears it after
/// the commit; `abandon` gives up a lease whose fence was lost (the key
/// holds another holder's value, so nothing is written); dropped otherwise,
/// it releases from a spawned task.
pub struct ZaUidLeaseGuard {
    store: Store,
    lease: Option<ZaUidLease>,
}

impl ZaUidLeaseGuard {
    pub fn new(store: Store, lease: ZaUidLease) -> Self {
        ZaUidLeaseGuard {
            store,
            lease: Some(lease),
        }
    }

    pub fn lease(&self) -> &ZaUidLease {
        self.lease
            .as_ref()
            .expect("a guard holds its lease until release or abandon")
    }

    pub async fn release(mut self) -> bool {
        match self.lease.take() {
            Some(lease) => lease.release(&self.store).await,
            None => true,
        }
    }

    pub fn abandon(mut self) {
        self.lease = None;
    }
}

impl Drop for ZaUidLeaseGuard {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take()
            && let Ok(handle) = tokio::runtime::Handle::try_current()
        {
            let store = self.store.clone();
            handle.spawn(async move {
                lease.release(&store).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use store::{InMemoryStore, backend::ephemeral::EphemeralStore, dispatch::lookup::KeyValue};

    async fn stored(store: &Store, account_id: u32, uid: &str) -> Option<Vec<u8>> {
        store
            .get_value::<RawValue>(ValueKey::from(lease_class(lease_key(account_id, uid))))
            .await
            .unwrap()
            .map(|raw| raw.0)
    }

    async fn write_raw(store: &Store, account_id: u32, uid: &str, value: Vec<u8>) {
        let mut batch = BatchBuilder::new();
        batch.set(lease_class(lease_key(account_id, uid)), value);
        store.write(batch.build_all()).await.unwrap();
    }

    fn expired_value(fence: u8) -> Vec<u8> {
        let mut value = (now() - 1).to_be_bytes().to_vec();
        value.extend_from_slice(&[fence; FENCE_LEN]);
        value
    }

    /// A batch fenced on `lease`, writing an unrelated key after the
    /// assertion, as the PUT create's insert does.
    async fn commit_fenced(store: &Store, lease: &ZaUidLease) -> trc::Result<()> {
        let mut batch = BatchBuilder::new();
        lease.assert_in(&mut batch);
        batch.set(
            lease_class(b"za-lease-test-payload".to_vec()),
            vec![0xFF; 8],
        );
        store.write(batch.build_all()).await.map(|_| ())
    }

    #[test]
    fn lease_key_is_fixed_length_and_binds_account_and_uid() {
        let key = lease_key(7, "uid-a");
        assert_eq!(key.len(), LEASE_KEY_LEN);
        assert_eq!(key[0], KV_LOCK_ZA_UID);
        assert_eq!(&key[1..5], &7u32.to_be_bytes());
        assert_eq!(&key[5..], Sha256::digest(b"uid-a").as_slice());
        assert_ne!(key, lease_key(7, "uid-b"));
        assert_ne!(key, lease_key(8, "uid-a"));
        assert_eq!(lease_key(7, &"u".repeat(2000)).len(), LEASE_KEY_LEN);
        let (from, to) = lease_key_range();
        assert!(from.class.serialize(0, 0, 0, 0) <= lease_class(key.clone()).serialize(0, 0, 0, 0));
        assert!(lease_class(key).serialize(0, 0, 0, 0) < to.class.serialize(0, 0, 0, 0));
    }

    #[test]
    fn lease_duration_knob_parsing() {
        assert_eq!(lease_secs_from(None), DEFAULT_LEASE_SECS);
        assert_eq!(lease_secs_from(Some("5")), 5);
        assert_eq!(lease_secs_from(Some(" 7 ")), 7);
        assert_eq!(lease_secs_from(Some("0")), DEFAULT_LEASE_SECS);
        assert_eq!(lease_secs_from(Some("soon")), DEFAULT_LEASE_SECS);
        assert_eq!(lease_secs_from(Some("3600")), MAX_LEASE_SECS);
        assert_eq!(lease_secs_from(Some("3601")), MAX_LEASE_SECS);
        assert_eq!(lease_secs_from(Some(&u64::MAX.to_string())), MAX_LEASE_SECS);
    }

    #[tokio::test]
    async fn a_purged_lease_counts_as_released() {
        let store = EphemeralStore::open();
        let lease = ZaUidLease::acquire(&store, 5, "purged")
            .await
            .unwrap()
            .unwrap();
        // Plan 10's sweep removed the expired key after the commit.
        let mut batch = BatchBuilder::new();
        batch.clear(lease_class(lease_key(5, "purged")));
        store.write(batch.build_all()).await.unwrap();
        assert!(
            lease.release(&store).await,
            "an absent key is already released"
        );
        assert!(stored(&store, 5, "purged").await.is_none());
    }

    /// The generic in-memory purge clears expired keys without an
    /// assertion, so it would delete a replacement lease taken between its
    /// scan and its clear; it skips the lease range (PR #35 review).
    #[tokio::test]
    async fn the_generic_purge_leaves_lease_keys() {
        let store = EphemeralStore::open();
        write_raw(&store, 9, "stale", expired_value(1)).await;
        let live = ZaUidLease::acquire(&store, 9, "live")
            .await
            .unwrap()
            .unwrap();
        let control = KeyValue::<()>::build_key(25, b"za-purge-control");
        let mut value = (now() - 1).to_be_bytes().to_vec();
        value.extend_from_slice(b"payload");
        let mut batch = BatchBuilder::new();
        batch.set(lease_class(control.clone()), value);
        store.write(batch.build_all()).await.unwrap();

        InMemoryStore::from(store.clone())
            .purge_in_memory_store()
            .await
            .unwrap();

        assert!(
            stored(&store, 9, "stale").await.is_some(),
            "the purge deleted an expired lease"
        );
        assert!(
            stored(&store, 9, "live").await.is_some(),
            "the purge deleted a live lease"
        );
        assert!(
            store
                .get_value::<RawValue>(ValueKey::from(lease_class(control)))
                .await
                .unwrap()
                .is_none(),
            "the purge kept an expired key of another prefix"
        );
        assert!(commit_fenced(&store, &live).await.is_ok());
        assert!(live.release(&store).await);
    }

    #[tokio::test]
    async fn acquire_writes_expiry_then_fence_and_refuses_a_held_lease() {
        let store = EphemeralStore::open();
        let before = now();
        let lease = ZaUidLease::acquire(&store, 1, "held")
            .await
            .unwrap()
            .expect("a free lease is taken");
        let value = stored(&store, 1, "held").await.expect("the key is written");
        assert_eq!(value.len(), U64_LEN + FENCE_LEN);
        let expiry = lease_value_expiry(&value).unwrap();
        assert!(expiry >= before + lease_secs() && expiry <= now() + lease_secs());
        assert!(
            ZaUidLease::acquire(&store, 1, "held")
                .await
                .unwrap()
                .is_none(),
            "a held lease is refused after the retries"
        );
        assert!(
            commit_fenced(&store, &lease).await.is_ok(),
            "the holder commits"
        );
        assert!(lease.release(&store).await, "the holder releases");
        assert!(stored(&store, 1, "held").await.is_none());
    }

    #[tokio::test]
    async fn an_expired_holder_is_fenced_out() {
        let store = EphemeralStore::open();
        // The old holder's lease expired; its value is still stored.
        write_raw(&store, 2, "taken", expired_value(7)).await;
        let old = ZaUidLease {
            key: lease_key(2, "taken"),
            value: expired_value(7),
        };
        let new = ZaUidLease::acquire(&store, 2, "taken")
            .await
            .unwrap()
            .expect("an expired lease is taken over");
        assert_ne!(stored(&store, 2, "taken").await.unwrap(), old.value);
        let err = commit_fenced(&store, &old).await.unwrap_err();
        assert!(err.is_assertion_failure(), "the old holder cannot commit");
        assert!(
            !old.release(&store).await,
            "nor remove its successor's lease"
        );
        assert!(commit_fenced(&store, &new).await.is_ok());
        assert!(new.release(&store).await);
        // A released lease fences out its own holder too.
        let gone = ZaUidLease::acquire(&store, 2, "taken")
            .await
            .unwrap()
            .unwrap();
        let copy = ZaUidLease {
            key: gone.key.clone(),
            value: gone.value.clone(),
        };
        assert!(gone.release(&store).await);
        assert!(
            commit_fenced(&store, &copy)
                .await
                .unwrap_err()
                .is_assertion_failure()
        );
    }

    #[tokio::test]
    async fn a_malformed_value_counts_as_expired_and_fences_differ() {
        let store = EphemeralStore::open();
        write_raw(&store, 3, "short", vec![1, 2, 3]).await;
        let a = ZaUidLease::acquire(&store, 3, "short").await.unwrap();
        assert!(a.is_some(), "a value with no expiry is replaced");
        let b = ZaUidLease::acquire(&store, 3, "other")
            .await
            .unwrap()
            .unwrap();
        assert_ne!(a.unwrap().value[U64_LEN..], b.value[U64_LEN..]);
    }

    #[tokio::test]
    async fn a_dropped_guard_releases_and_an_abandoned_one_does_not() {
        let store = EphemeralStore::open();
        let lease = ZaUidLease::acquire(&store, 4, "guard")
            .await
            .unwrap()
            .unwrap();
        drop(ZaUidLeaseGuard::new(store.clone(), lease));
        for _ in 0..50 {
            if stored(&store, 4, "guard").await.is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            stored(&store, 4, "guard").await.is_none(),
            "released on drop"
        );
        let lease = ZaUidLease::acquire(&store, 4, "guard")
            .await
            .unwrap()
            .unwrap();
        ZaUidLeaseGuard::new(store.clone(), lease).abandon();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            stored(&store, 4, "guard").await.is_some(),
            "an abandoned lease stays"
        );
    }
}
