/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::keys::{Secret, derive_event_key};

/// Keys resident for one request (spec 5, "in-flight copies"). Cloned by
/// `Arc`; the secrets are zeroed when the last reference drops.
pub struct SessionKeys {
    pub account_id: u32,
    pub generation: u64,
    mk: Secret,
    ewk: Secret,
}

impl SessionKeys {
    pub fn new(account_id: u32, generation: u64, mk: Secret) -> Self {
        let ewk = derive_event_key(&mk);
        SessionKeys {
            account_id,
            generation,
            mk,
            ewk,
        }
    }

    pub fn mk(&self) -> &Secret {
        &self.mk
    }

    pub fn ewk(&self) -> &Secret {
        &self.ewk
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
    use crate::keys::{Secret, derive_event_key};

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
}
