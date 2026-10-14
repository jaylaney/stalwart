/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use crate::keys::{KeyError, Secret, derive_event_key};
use std::sync::OnceLock;

/// Keys resident for one request (spec 5, "in-flight copies"). Cloned by
/// `Arc`; the secrets are zeroed when the last reference drops.
pub struct SessionKeys {
    pub account_id: u32,
    pub generation: u64,
    mk: Secret,
    ewk: Secret,
    /// Invitations spec 3.2: the X25519 private key, unwrapped from the
    /// vault record on first use. Holds the outcome, so a wrap that does not
    /// open is traced once per session.
    private_key: OnceLock<Result<Secret, KeyError>>,
}

impl SessionKeys {
    pub fn new(account_id: u32, generation: u64, mk: Secret) -> Self {
        let ewk = derive_event_key(&mk);
        SessionKeys {
            account_id,
            generation,
            mk,
            ewk,
            private_key: OnceLock::new(),
        }
    }

    pub fn mk(&self) -> &Secret {
        &self.mk
    }

    pub fn ewk(&self) -> &Secret {
        &self.ewk
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
    use crate::keys::{KeyError, Secret, derive_event_key};

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
}
