/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::session::SessionKeys;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Sliding idle timeout of a resident entry (spec 5: 15 minutes).
pub const DEFAULT_IDLE_SECS: u64 = 900;
/// Hard cap from insertion, however often the entry is used (spec 5: 60 minutes).
pub const DEFAULT_MAX_AGE_SECS: u64 = 3600;
/// Entry bound; the least recently used entry is evicted beyond it.
pub const DEFAULT_MAX_ENTRIES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCacheConfig {
    pub idle: Duration,
    pub max_age: Duration,
    pub max_entries: usize,
}

impl Default for KeyCacheConfig {
    fn default() -> Self {
        KeyCacheConfig {
            idle: Duration::from_secs(DEFAULT_IDLE_SECS),
            max_age: Duration::from_secs(DEFAULT_MAX_AGE_SECS),
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }
}

impl KeyCacheConfig {
    /// `ZA_KEY_IDLE_SECS` (900), `ZA_KEY_MAX_AGE_SECS` (3600), `ZA_KEY_MAX_ENTRIES` (10000).
    /// An unset or malformed value uses the default.
    pub fn from_env() -> Self {
        fn var<T: std::str::FromStr>(name: &str, default: T) -> T {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        }
        KeyCacheConfig {
            idle: Duration::from_secs(var("ZA_KEY_IDLE_SECS", DEFAULT_IDLE_SECS)),
            max_age: Duration::from_secs(var("ZA_KEY_MAX_AGE_SECS", DEFAULT_MAX_AGE_SECS)),
            max_entries: var("ZA_KEY_MAX_ENTRIES", DEFAULT_MAX_ENTRIES).max(1),
        }
    }
}

struct Entry {
    keys: Arc<SessionKeys>,
    inserted: Instant,
    last_used: Instant,
}

impl Entry {
    fn is_expired(&self, now: Instant, config: &KeyCacheConfig) -> bool {
        now.duration_since(self.last_used) >= config.idle
            || now.duration_since(self.inserted) >= config.max_age
    }
}

/// Process-local cache of resident master keys, keyed by the keyed fingerprint
/// of the Authorization header (spec 5). Never shared across nodes.
pub struct KeyCache {
    config: KeyCacheConfig,
    inner: Mutex<HashMap<[u8; 32], Entry>>,
}

impl KeyCache {
    pub fn new(config: KeyCacheConfig) -> Self {
        KeyCache {
            config,
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Deadline-driven expiry: an expired entry is refused and removed here.
    /// `None` means no entry is left under `fp`, so the caller drops the
    /// authentication it cached under the same fingerprint (spec 5: a
    /// fingerprint lives no longer than its keys).
    pub fn get(&self, fp: &[u8; 32], now: Instant) -> Option<Arc<SessionKeys>> {
        let mut map = self.inner.lock().unwrap();
        let expired = map.get(fp).is_some_and(|e| e.is_expired(now, &self.config));
        if expired {
            map.remove(fp);
            return None;
        }
        let entry = map.get_mut(fp)?;
        entry.last_used = now;
        Some(entry.keys.clone())
    }

    /// Returns the fingerprint evicted to stay within the bound, if any; the
    /// caller drops what it cached under it.
    #[must_use = "the evicted fingerprint's cached authentication must be dropped"]
    pub fn insert(&self, fp: [u8; 32], keys: Arc<SessionKeys>, now: Instant) -> Option<[u8; 32]> {
        let mut map = self.inner.lock().unwrap();
        let mut evicted = None;
        if !map.contains_key(&fp) && map.len() >= self.config.max_entries {
            // Least recently used eviction (spec 5).
            if let Some(victim) = map.iter().min_by_key(|(_, e)| e.last_used).map(|(k, _)| *k) {
                map.remove(&victim);
                evicted = Some(victim);
            }
        }
        map.insert(
            fp,
            Entry {
                keys,
                inserted: now,
                last_used: now,
            },
        );
        evicted
    }

    pub fn remove(&self, fp: &[u8; 32]) {
        self.inner.lock().unwrap().remove(fp);
    }

    pub fn remove_account(&self, account_id: u32) {
        self.inner
            .lock()
            .unwrap()
            .retain(|_, e| e.keys.account_id != account_id);
    }

    /// Whether any entry holds keys of `account_id`, expired or not.
    pub fn contains_account(&self, account_id: u32) -> bool {
        self.inner
            .lock()
            .unwrap()
            .values()
            .any(|e| e.keys.account_id == account_id)
    }

    /// Sweep-driven removal; returns the fingerprints removed, so the caller
    /// drops what it cached under them.
    #[must_use = "the removed fingerprints' cached authentication must be dropped"]
    pub fn sweep(&self, now: Instant) -> Vec<[u8; 32]> {
        let mut map = self.inner.lock().unwrap();
        let mut removed = Vec::new();
        map.retain(|fp, e| {
            let expired = e.is_expired(now, &self.config);
            if expired {
                removed.push(*fp);
            }
            !expired
        });
        removed
    }

    /// The effective configuration (logged once at startup, no secrets).
    pub fn config(&self) -> KeyCacheConfig {
        self.config
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        self.inner.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Secret;
    use std::time::Duration;

    fn cfg() -> KeyCacheConfig {
        KeyCacheConfig {
            idle: Duration::from_secs(900),
            max_age: Duration::from_secs(3600),
            max_entries: 3,
        }
    }

    fn keys(account_id: u32, generation: u64) -> Arc<SessionKeys> {
        Arc::new(SessionKeys::new(account_id, generation, Secret::random()))
    }

    #[test]
    fn idle_entry_is_refused_after_timeout_and_removed_by_sweep() {
        let cache = KeyCache::new(cfg());
        let t0 = Instant::now();
        assert_eq!(cache.insert([1; 32], keys(1, 1), t0), None);
        assert!(cache.get(&[1; 32], t0 + Duration::from_secs(899)).is_some());
        // The lookup above refreshed last_used; now go idle.
        let t_idle = t0 + Duration::from_secs(899) + Duration::from_secs(901);
        assert!(
            cache.get(&[1; 32], t_idle).is_none(),
            "expired entries are refused at lookup"
        );
        assert_eq!(cache.len(), 0, "a refused entry is removed immediately");
        assert_eq!(cache.insert([2; 32], keys(2, 1), t0), None);
        assert_eq!(
            cache.sweep(t0 + Duration::from_secs(901)),
            vec![[2; 32]],
            "sweep removes idle entries never looked up, and names them"
        );
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn hard_cap_evicts_a_continuously_used_entry() {
        let cache = KeyCache::new(cfg());
        let t0 = Instant::now();
        assert_eq!(cache.insert([1; 32], keys(1, 1), t0), None);
        for minute in 1..60 {
            assert!(
                cache
                    .get(&[1; 32], t0 + Duration::from_secs(minute * 60))
                    .is_some()
            );
        }
        assert!(
            cache
                .get(&[1; 32], t0 + Duration::from_secs(3601))
                .is_none()
        );
    }

    #[test]
    fn lru_eviction_under_pressure_and_account_removal() {
        let cache = KeyCache::new(cfg());
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        assert_eq!(cache.insert([1; 32], keys(1, 1), t0), None);
        assert_eq!(cache.insert([2; 32], keys(2, 1), at(1)), None);
        assert_eq!(cache.insert([3; 32], keys(3, 1), at(2)), None);
        assert!(cache.get(&[1; 32], at(3)).is_some()); // 1 is now most recent
        assert_eq!(
            cache.insert([4; 32], keys(4, 1), at(4)),
            Some([2; 32]),
            "the evicted fingerprint is returned"
        );
        assert_eq!(cache.len(), 3);
        assert!(
            cache.get(&[2; 32], at(5)).is_none(),
            "2 was least recently used"
        );
        // Touch 1 and 3 so that 4 (last used at t4) is the next LRU victim.
        assert!(cache.get(&[1; 32], at(6)).is_some());
        assert!(cache.get(&[3; 32], at(6)).is_some());
        assert_eq!(cache.insert([5; 32], keys(1, 2), at(7)), Some([4; 32]));
        assert_eq!(
            cache.insert([5; 32], keys(1, 2), at(7)),
            None,
            "replacing an entry evicts nothing"
        );
        assert!(
            cache.get(&[4; 32], at(8)).is_none(),
            "4 was least recently used"
        );
        cache.remove_account(1);
        assert!(cache.get(&[1; 32], at(9)).is_none());
        assert!(cache.get(&[5; 32], at(9)).is_none());
        assert!(cache.get(&[3; 32], at(9)).is_some());
    }

    #[test]
    fn eviction_drops_the_last_reference_so_keys_are_zeroed() {
        let cache = KeyCache::new(cfg());
        let t0 = Instant::now();
        let k = keys(1, 1);
        assert_eq!(cache.insert([1; 32], k.clone(), t0), None);
        assert_eq!(Arc::strong_count(&k), 2);
        cache.remove(&[1; 32]);
        assert_eq!(
            Arc::strong_count(&k),
            1,
            "the cache held exactly one reference"
        );
    }

    #[test]
    fn config_from_env_uses_defaults() {
        let c = KeyCacheConfig::from_env();
        assert_eq!(c.idle, Duration::from_secs(900));
        assert_eq!(c.max_age, Duration::from_secs(3600));
        assert_eq!(c.max_entries, 10_000);
    }
}
