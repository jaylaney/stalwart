/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

//! Boot configuration for zero-access search (search spec 4.4).

use std::str::FromStr;
use store::za_search::reservation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZaSearchConfig {
    pub mem_mb: u64,
    pub thread_mem_mb: u64,
    pub account_max_mb: u64,
    pub idle_secs: u64,
    pub pass_cap: usize,
    pub pass_mb: u64,
    pub pass_hidden_kb: usize,
    pub pass_ms: u64,
    pub wait_secs: u64,
    pub segments: usize,
    pub checkpoint: usize,
    pub absent_retry_secs: u64,
    pub pad_min: usize,
}

impl Default for ZaSearchConfig {
    fn default() -> Self {
        Self {
            mem_mb: 1536,
            thread_mem_mb: 256,
            account_max_mb: 512,
            idle_secs: 900,
            pass_cap: 48,
            pass_mb: 32,
            pass_hidden_kb: 32,
            pass_ms: 2000,
            wait_secs: 30,
            segments: 16,
            checkpoint: 1000,
            absent_retry_secs: 10,
            pad_min: 65_536,
        }
    }
}

impl ZaSearchConfig {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        // ZA_SEARCH_SKIP_AFTER is retired and intentionally ignored: transient
        // failures remain pending until recovery, regardless of their count.
        let defaults = Self::default();
        let config = Self {
            mem_mb: read(&lookup, "ZA_SEARCH_MEM_MB", defaults.mem_mb),
            thread_mem_mb: read(&lookup, "ZA_SEARCH_THREAD_MEM_MB", defaults.thread_mem_mb),
            account_max_mb: read(&lookup, "ZA_SEARCH_ACCOUNT_MAX_MB", defaults.account_max_mb),
            idle_secs: read(&lookup, "ZA_SEARCH_IDLE_SECS", defaults.idle_secs),
            pass_cap: read(&lookup, "ZA_SEARCH_PASS_CAP", defaults.pass_cap),
            pass_mb: read(&lookup, "ZA_SEARCH_PASS_MB", defaults.pass_mb),
            pass_hidden_kb: read(&lookup, "ZA_SEARCH_PASS_HIDDEN_KB", defaults.pass_hidden_kb),
            pass_ms: read(&lookup, "ZA_SEARCH_PASS_MS", defaults.pass_ms),
            wait_secs: read(&lookup, "ZA_SEARCH_WAIT_SECS", defaults.wait_secs),
            segments: read(&lookup, "ZA_SEARCH_SEGMENTS", defaults.segments),
            checkpoint: read(&lookup, "ZA_SEARCH_CHECKPOINT", defaults.checkpoint),
            absent_retry_secs: read(
                &lookup,
                "ZA_SEARCH_ABSENT_RETRY_SECS",
                defaults.absent_retry_secs,
            ),
            pad_min: read(&lookup, "ZA_SEARCH_PAD_MIN", defaults.pad_min),
        };

        // Compute the bound before narrowing, so even a huge configured hidden
        // budget gets a boot refusal instead of overflowing.
        let min_pad = config.pass_hidden_kb as u128 * 1024 + reservation() as u128 + 4;
        if (config.pad_min as u128) < min_pad {
            return Err(format!(
                "ZA_SEARCH_PAD_MIN={} is below {min_pad} = ZA_SEARCH_PASS_HIDDEN_KB·1024 + R + (K − 1)·a + 4; refusing to start",
                config.pad_min,
            ));
        }

        let max_pass_cap = config.hidden_budget() / reservation();
        if config.pass_cap > max_pass_cap {
            return Err(format!(
                "ZA_SEARCH_PASS_CAP={} is above {max_pass_cap} = floor(ZA_SEARCH_PASS_HIDDEN_KB·1024 / (R + (K − 1)·a)); refusing to start",
                config.pass_cap,
            ));
        }

        let max_account_mb = config.mem_mb / 3;
        if config.account_max_mb > max_account_mb {
            return Err(format!(
                "ZA_SEARCH_ACCOUNT_MAX_MB={} is above {max_account_mb} = floor(ZA_SEARCH_MEM_MB / 3); refusing to start",
                config.account_max_mb,
            ));
        }

        if !(2..=255).contains(&config.segments) {
            return Err(format!(
                "ZA_SEARCH_SEGMENTS={} is outside 2..=255; refusing to start",
                config.segments,
            ));
        }

        Ok(config)
    }

    pub fn hidden_budget(&self) -> usize {
        self.pass_hidden_kb * 1024
    }
}

fn read<T: FromStr + Default + PartialEq>(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &str,
    fallback: T,
) -> T {
    lookup(name)
        .and_then(|value| value.trim().parse().ok())
        .filter(|value| *value != T::default())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(vars: &[(&str, &str)]) -> Result<ZaSearchConfig, String> {
        let map: HashMap<&str, &str> = vars.iter().copied().collect();
        ZaSearchConfig::from_lookup(|name| map.get(name).map(|v| v.to_string()))
    }

    #[test]
    fn defaults_when_unset_or_malformed() {
        let c = cfg(&[("ZA_SEARCH_PASS_CAP", "lots"), ("ZA_SEARCH_IDLE_SECS", "")]).unwrap();
        assert_eq!(c, ZaSearchConfig::default());
        assert_eq!(c.mem_mb, 1536);
        assert_eq!(c.thread_mem_mb, 256);
        assert_eq!(c.account_max_mb, 512);
        assert_eq!(c.idle_secs, 900);
        assert_eq!(c.pass_cap, 48);
        assert_eq!(c.pass_mb, 32);
        assert_eq!(c.pass_hidden_kb, 32);
        assert_eq!(c.pass_ms, 2000);
        assert_eq!(c.wait_secs, 30);
        assert_eq!(c.segments, 16);
        assert_eq!(c.checkpoint, 1000);
        assert_eq!(c.absent_retry_secs, 10);
        assert_eq!(c.pad_min, 65_536);
        assert_eq!(c.hidden_budget(), 32_768);
    }

    #[test]
    fn a_retired_skip_threshold_does_not_change_configuration() {
        for value in ["0", "1", "255", "invalid"] {
            assert_eq!(
                cfg(&[("ZA_SEARCH_SKIP_AFTER", value)]).unwrap(),
                ZaSearchConfig::default()
            );
        }
    }

    #[test]
    fn a_value_parses_with_surrounding_space() {
        assert_eq!(cfg(&[("ZA_SEARCH_WAIT_SECS", " 7 ")]).unwrap().wait_secs, 7);
    }

    #[test]
    fn the_pad_minimum_must_cover_the_budget_the_overshoot_and_the_frame() {
        assert!(
            cfg(&[("ZA_SEARCH_PAD_MIN", "33300")])
                .unwrap_err()
                .contains("ZA_SEARCH_PAD_MIN")
        );
        assert_eq!(
            cfg(&[("ZA_SEARCH_PAD_MIN", "33301")]).unwrap().pad_min,
            33_301
        );
        // A larger budget moves the bound with it: H = 64 KiB needs M ≥ 66069.
        assert!(
            cfg(&[("ZA_SEARCH_PASS_HIDDEN_KB", "64")])
                .unwrap_err()
                .contains("ZA_SEARCH_PAD_MIN")
        );
        assert!(
            cfg(&[
                ("ZA_SEARCH_PASS_HIDDEN_KB", "64"),
                ("ZA_SEARCH_PAD_MIN", "131072")
            ])
            .is_ok()
        );
    }

    #[test]
    fn the_pass_cap_must_bind_under_the_budget() {
        assert!(
            cfg(&[("ZA_SEARCH_PASS_CAP", "62")])
                .unwrap_err()
                .contains("ZA_SEARCH_PASS_CAP")
        );
        assert_eq!(cfg(&[("ZA_SEARCH_PASS_CAP", "61")]).unwrap().pass_cap, 61);
    }

    #[test]
    fn the_account_cap_is_at_most_a_third_of_the_budget() {
        assert!(
            cfg(&[("ZA_SEARCH_ACCOUNT_MAX_MB", "513")])
                .unwrap_err()
                .contains("ZA_SEARCH_ACCOUNT_MAX_MB")
        );
        assert_eq!(
            cfg(&[("ZA_SEARCH_ACCOUNT_MAX_MB", "512")])
                .unwrap()
                .account_max_mb,
            512
        );
        assert!(
            cfg(&[("ZA_SEARCH_MEM_MB", "1535")])
                .unwrap_err()
                .contains("ZA_SEARCH_ACCOUNT_MAX_MB")
        );
    }

    #[test]
    fn a_zero_is_malformed_where_zero_cannot_work() {
        // Zero means "unset" for the counts and intervals that must be positive.
        let c = cfg(&[("ZA_SEARCH_SEGMENTS", "0"), ("ZA_SEARCH_WAIT_SECS", "0")]).unwrap();
        assert_eq!((c.segments, c.wait_secs), (16, 30));
    }

    #[test]
    fn the_segment_count_fits_the_manifests_byte() {
        // PL12: the manifest's thread count is a u8
        assert!(
            cfg(&[("ZA_SEARCH_SEGMENTS", "256")])
                .unwrap_err()
                .contains("ZA_SEARCH_SEGMENTS")
        );
        assert_eq!(cfg(&[("ZA_SEARCH_SEGMENTS", "255")]).unwrap().segments, 255);
        assert!(
            cfg(&[("ZA_SEARCH_SEGMENTS", "1")])
                .unwrap_err()
                .contains("ZA_SEARCH_SEGMENTS")
        );
        assert_eq!(cfg(&[("ZA_SEARCH_SEGMENTS", "2")]).unwrap().segments, 2);
    }

    #[test]
    fn the_pad_bound_rejects_a_hidden_budget_that_cannot_fit_in_usize() {
        let hidden_kb = usize::MAX.to_string();
        assert!(
            cfg(&[("ZA_SEARCH_PASS_HIDDEN_KB", &hidden_kb)])
                .unwrap_err()
                .contains("ZA_SEARCH_PAD_MIN")
        );
    }
}
