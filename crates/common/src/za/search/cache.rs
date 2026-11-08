/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::config::ZaSearchConfig;
use crate::Server;

pub struct ZaSearchCache {
    config: ZaSearchConfig,
    refusal: Option<String>,
}

impl ZaSearchCache {
    pub fn new(config: Result<ZaSearchConfig, String>) -> Self {
        match config {
            Ok(config) => Self {
                config,
                refusal: None,
            },
            Err(reason) => Self {
                config: ZaSearchConfig::default(),
                refusal: Some(reason),
            },
        }
    }

    pub fn config(&self) -> &ZaSearchConfig {
        &self.config
    }

    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    fn effective_config(&self) -> ZaSearchConfig {
        #[cfg(feature = "test_mode")]
        if let Some(config) = crate::za::test_hooks::search_config_override() {
            return config;
        }

        self.config().clone()
    }
}

impl Server {
    pub fn za_search_config(&self) -> ZaSearchConfig {
        self.inner.cache.za_search.effective_config()
    }
}

#[cfg(all(test, feature = "test_mode"))]
mod tests {
    use super::*;
    use crate::za::test_hooks::set_search_config_override;

    #[test]
    fn the_override_replaces_the_boot_value_in_test_mode() {
        let boot = ZaSearchConfig {
            pass_cap: 3,
            ..ZaSearchConfig::default()
        };
        let cache = ZaSearchCache::new(Ok(boot.clone()));
        let overridden = ZaSearchConfig {
            pass_cap: 5,
            ..boot.clone()
        };

        assert_eq!(cache.effective_config(), boot);
        set_search_config_override(Some(overridden.clone()));
        let effective = cache.effective_config();
        set_search_config_override(None);

        assert_eq!(effective, overridden);
        assert_eq!(cache.effective_config(), boot);
    }
}
